//! Шина событий приложения и отложенные вызовы на главном потоке.
//!
//! События — те же, что слушают модули оригинала: смена активного приложения
//! (`ApplicationToggle` и `prevActiveApp` в `AppDelegate`), запуск и
//! завершение приложений, смена экранов (`NSApplication.didChangeScreenParameters`),
//! смена Space (`NSWorkspace.activeSpaceDidChange`), смена настроек
//! (`config::subscribe`) и `applicationWillBecomeActive`. Источник — один
//! объект-наблюдатель на `NSWorkspace.notificationCenter` и
//! `NotificationCenter.default` (`install`).
//!
//! Подписка — замыканием на вид события (`on_front_app_changed`, …,
//! `subscribe`). Всё живёт на главном потоке: подписываться и рассылать можно
//! только там.
//!
//! `run_later` / `run_after` — `DispatchQueue.main.async` / `asyncAfter`
//! оригинала: через libdispatch, без таймеров AppKit. Задачи с модальными
//! окнами — через `run_in_run_loop` (разовый таймер цикла событий): блок
//! главной очереди, который крутит вложенный цикл, останавливает эту очередь.

use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::rc::Rc;
use std::time::Duration;

use core_foundation_sys::base::{kCFAllocatorDefault, CFRelease};
use core_foundation_sys::date::CFAbsoluteTimeGetCurrent;
use core_foundation_sys::runloop::{
    kCFRunLoopCommonModes, CFRunLoopAddTimer, CFRunLoopGetMain, CFRunLoopRef,
    CFRunLoopTimerContext, CFRunLoopTimerCreate, CFRunLoopTimerRef,
};
use core_foundation_sys::string::CFStringRef;
use objc2::rc::Retained;
use objc2::runtime::NSObject;
use objc2::{define_class, msg_send, sel, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSApplicationDidChangeScreenParametersNotification, NSRunningApplication, NSWorkspace,
    NSWorkspaceActiveSpaceDidChangeNotification, NSWorkspaceApplicationKey,
    NSWorkspaceDidActivateApplicationNotification, NSWorkspaceDidLaunchApplicationNotification,
    NSWorkspaceDidTerminateApplicationNotification,
};
use objc2_foundation::{NSNotification, NSNotificationCenter};

use crate::config::{self, Config};

// ---------------------------------------------------------------- события

/// Запущенное приложение — то, что оригинал берёт из `NSRunningApplication`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppInfo {
    pub pid: i32,
    /// `bundleIdentifier`; у приложений без бандла — `None`.
    pub bundle_id: Option<String>,
    /// `localizedName`.
    pub name: Option<String>,
}

impl AppInfo {
    fn from_running(application: &NSRunningApplication) -> AppInfo {
        AppInfo {
            pid: application.processIdentifier(),
            bundle_id: application.bundleIdentifier().map(|id| id.to_string()),
            name: application.localizedName().map(|name| name.to_string()),
        }
    }
}

/// Событие. Подписчик получает только события своего вида.
#[derive(Debug)]
pub enum Event<'a> {
    /// Активным стало другое приложение; `previous` — прежнее активное.
    FrontAppChanged {
        current: &'a AppInfo,
        previous: Option<&'a AppInfo>,
    },
    AppLaunched(&'a AppInfo),
    AppTerminated(&'a AppInfo),
    /// Экраны подключили, отключили или поменяли им разрешение и расположение.
    ScreensChanged,
    /// Пользователь перешёл на другой Space (рабочий стол).
    SpaceChanged,
    /// Настройки изменились (`config::update`): старые и новые.
    ConfigChanged {
        old: &'a Config,
        new: &'a Config,
    },
    /// Само приложение сейчас станет активным (`applicationWillBecomeActive`).
    AppWillBecomeActive,
}

/// Вид события — ключ подписки.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EventKind {
    FrontAppChanged,
    AppLaunched,
    AppTerminated,
    ScreensChanged,
    SpaceChanged,
    ConfigChanged,
    AppWillBecomeActive,
}

impl Event<'_> {
    pub fn kind(&self) -> EventKind {
        match self {
            Event::FrontAppChanged { .. } => EventKind::FrontAppChanged,
            Event::AppLaunched(_) => EventKind::AppLaunched,
            Event::AppTerminated(_) => EventKind::AppTerminated,
            Event::ScreensChanged => EventKind::ScreensChanged,
            Event::SpaceChanged => EventKind::SpaceChanged,
            Event::ConfigChanged { .. } => EventKind::ConfigChanged,
            Event::AppWillBecomeActive => EventKind::AppWillBecomeActive,
        }
    }
}

/// Номер подписки — чтобы отписаться.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SubscriptionId(u64);

type Handler = Rc<dyn Fn(&Event<'_>)>;

/// Подписчики по видам событий.
#[derive(Default)]
struct Bus {
    next_id: u64,
    handlers: Vec<(SubscriptionId, EventKind, Handler)>,
}

impl Bus {
    fn subscribe(&mut self, kind: EventKind, handler: Handler) -> SubscriptionId {
        self.next_id += 1;
        let id = SubscriptionId(self.next_id);
        self.handlers.push((id, kind, handler));
        id
    }

    fn unsubscribe(&mut self, id: SubscriptionId) -> bool {
        let before = self.handlers.len();
        self.handlers.retain(|(handler_id, _, _)| *handler_id != id);
        self.handlers.len() != before
    }

    /// Подписчики вида `kind` в порядке подписки. Это снимок: подписки и
    /// отписки во время рассылки действуют со следующего события.
    fn handlers(&self, kind: EventKind) -> Vec<Handler> {
        self.handlers
            .iter()
            .filter(|(_, handler_kind, _)| *handler_kind == kind)
            .map(|(_, _, handler)| handler.clone())
            .collect()
    }
}

/// Активное и прежнее активное приложение (`frontAppId`/`frontAppName` у
/// `ApplicationToggle`, `prevActiveApp` у `AppDelegate`).
#[derive(Default, Debug)]
struct FrontApps {
    current: Option<AppInfo>,
    previous: Option<AppInfo>,
}

impl FrontApps {
    /// Активировалось `app`; `true` — активное приложение сменилось.
    fn activated(&mut self, app: AppInfo) -> bool {
        if self
            .current
            .as_ref()
            .is_some_and(|current| current.pid == app.pid)
        {
            self.current = Some(app);
            return false;
        }
        self.previous = self.current.replace(app);
        true
    }

    /// Приложение завершилось: вернуть фокус ему уже нельзя.
    fn terminated(&mut self, pid: i32) {
        if self
            .previous
            .as_ref()
            .is_some_and(|previous| previous.pid == pid)
        {
            self.previous = None;
        }
    }

    /// Последнее активное приложение, кроме процесса `pid`.
    fn latest_except(&self, pid: i32) -> Option<&AppInfo> {
        [&self.current, &self.previous]
            .into_iter()
            .flatten()
            .find(|app| app.pid != pid)
    }
}

thread_local! {
    static BUS: RefCell<Bus> = RefCell::new(Bus::default());
    static FRONT_APPS: RefCell<FrontApps> = RefCell::new(FrontApps::default());
    /// Наблюдатель уведомлений: центры уведомлений его не удерживают.
    static OBSERVER: RefCell<Option<Retained<Observer>>> = const { RefCell::new(None) };
}

/// Подписаться на события вида `kind`.
pub fn subscribe(kind: EventKind, handler: impl Fn(&Event<'_>) + 'static) -> SubscriptionId {
    BUS.with(|bus| bus.borrow_mut().subscribe(kind, Rc::new(handler)))
}

/// Отписаться; `false` — такой подписки нет.
pub fn unsubscribe(id: SubscriptionId) -> bool {
    BUS.with(|bus| bus.borrow_mut().unsubscribe(id))
}

/// Разослать событие подписчикам его вида.
pub fn emit(event: &Event<'_>) {
    let handlers = BUS.with(|bus| bus.borrow().handlers(event.kind()));
    for handler in handlers {
        handler(event);
    }
}

/// Сменилось активное приложение: `(текущее, прежнее)`.
pub fn on_front_app_changed(
    handler: impl Fn(&AppInfo, Option<&AppInfo>) + 'static,
) -> SubscriptionId {
    subscribe(EventKind::FrontAppChanged, move |event| {
        if let Event::FrontAppChanged { current, previous } = event {
            handler(current, *previous);
        }
    })
}

/// Запустилось приложение.
pub fn on_app_launched(handler: impl Fn(&AppInfo) + 'static) -> SubscriptionId {
    subscribe(EventKind::AppLaunched, move |event| {
        if let Event::AppLaunched(app) = event {
            handler(app);
        }
    })
}

/// Завершилось приложение.
pub fn on_app_terminated(handler: impl Fn(&AppInfo) + 'static) -> SubscriptionId {
    subscribe(EventKind::AppTerminated, move |event| {
        if let Event::AppTerminated(app) = event {
            handler(app);
        }
    })
}

/// Сменились экраны.
pub fn on_screens_changed(handler: impl Fn() + 'static) -> SubscriptionId {
    subscribe(EventKind::ScreensChanged, move |_| handler())
}

/// Сменился Space.
pub fn on_space_changed(handler: impl Fn() + 'static) -> SubscriptionId {
    subscribe(EventKind::SpaceChanged, move |_| handler())
}

/// Сменились настройки: `(старые, новые)`.
pub fn on_config_changed(handler: impl Fn(&Config, &Config) + 'static) -> SubscriptionId {
    subscribe(EventKind::ConfigChanged, move |event| {
        if let Event::ConfigChanged { old, new } = event {
            handler(old, new);
        }
    })
}

/// Приложение сейчас станет активным.
pub fn on_app_will_become_active(handler: impl Fn() + 'static) -> SubscriptionId {
    subscribe(EventKind::AppWillBecomeActive, move |_| handler())
}

/// Активное приложение (до `install` — `None`).
pub fn front_app() -> Option<AppInfo> {
    FRONT_APPS.with(|apps| apps.borrow().current.clone())
}

/// Приложение, которое было активным до текущего.
pub fn previous_app() -> Option<AppInfo> {
    FRONT_APPS.with(|apps| apps.borrow().previous.clone())
}

/// Приложение, с которым работает пользователь: активное, а если активно само
/// это приложение (его вывела вперёд ссылка или меню) — прежнее.
pub fn front_app_except_self() -> Option<AppInfo> {
    let own_pid = std::process::id() as i32;
    FRONT_APPS.with(|apps| apps.borrow().latest_except(own_pid).cloned())
}

// ---------------------------------------------------------------- источники

define_class!(
    /// Принимает уведомления AppKit и пересылает их в шину.
    #[unsafe(super(NSObject))]
    #[name = "R2EventsObserver"]
    #[thread_kind = MainThreadOnly]
    #[ivars = ()]
    struct Observer;

    impl Observer {
        #[unsafe(method(applicationActivated:))]
        fn application_activated(&self, notification: &NSNotification) {
            let Some(app) = running_application(notification) else {
                return;
            };
            let changed = FRONT_APPS.with(|apps| apps.borrow_mut().activated(app));
            if !changed {
                return;
            }
            let (current, previous) = FRONT_APPS.with(|apps| {
                let apps = apps.borrow();
                (apps.current.clone(), apps.previous.clone())
            });
            if let Some(current) = current {
                emit(&Event::FrontAppChanged {
                    current: &current,
                    previous: previous.as_ref(),
                });
            }
        }

        #[unsafe(method(applicationLaunched:))]
        fn application_launched(&self, notification: &NSNotification) {
            if let Some(app) = running_application(notification) {
                emit(&Event::AppLaunched(&app));
            }
        }

        #[unsafe(method(applicationTerminated:))]
        fn application_terminated(&self, notification: &NSNotification) {
            if let Some(app) = running_application(notification) {
                FRONT_APPS.with(|apps| apps.borrow_mut().terminated(app.pid));
                emit(&Event::AppTerminated(&app));
            }
        }

        #[unsafe(method(screenParametersChanged:))]
        fn screen_parameters_changed(&self, _notification: &NSNotification) {
            emit(&Event::ScreensChanged);
        }

        #[unsafe(method(activeSpaceChanged:))]
        fn active_space_changed(&self, _notification: &NSNotification) {
            emit(&Event::SpaceChanged);
        }
    }
);

impl Observer {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm);
        unsafe { msg_send![this, init] }
    }
}

/// Приложение из `userInfo[NSWorkspaceApplicationKey]` уведомления NSWorkspace.
fn running_application(notification: &NSNotification) -> Option<AppInfo> {
    let info = notification.userInfo()?;
    let key = unsafe { NSWorkspaceApplicationKey };
    let application = info
        .objectForKey(key)?
        .downcast::<NSRunningApplication>()
        .ok()?;
    Some(AppInfo::from_running(&application))
}

/// Подключить шину к уведомлениям системы и к настройкам. Повторный вызов
/// ничего не делает.
pub fn install(mtm: MainThreadMarker) {
    if OBSERVER.with(|slot| slot.borrow().is_some()) {
        return;
    }
    let observer = Observer::new(mtm);
    let workspace = NSWorkspace::sharedWorkspace();
    let workspace_center = workspace.notificationCenter();
    let default_center = NSNotificationCenter::defaultCenter();
    // SAFETY: у наблюдателя есть все эти селекторы с аргументом NSNotification,
    // а живёт он до конца работы приложения (в OBSERVER).
    unsafe {
        workspace_center.addObserver_selector_name_object(
            &observer,
            sel!(applicationActivated:),
            Some(NSWorkspaceDidActivateApplicationNotification),
            None,
        );
        workspace_center.addObserver_selector_name_object(
            &observer,
            sel!(applicationLaunched:),
            Some(NSWorkspaceDidLaunchApplicationNotification),
            None,
        );
        workspace_center.addObserver_selector_name_object(
            &observer,
            sel!(applicationTerminated:),
            Some(NSWorkspaceDidTerminateApplicationNotification),
            None,
        );
        workspace_center.addObserver_selector_name_object(
            &observer,
            sel!(activeSpaceChanged:),
            Some(NSWorkspaceActiveSpaceDidChangeNotification),
            None,
        );
        default_center.addObserver_selector_name_object(
            &observer,
            sel!(screenParametersChanged:),
            Some(NSApplicationDidChangeScreenParametersNotification),
            None,
        );
    }
    OBSERVER.with(|slot| *slot.borrow_mut() = Some(observer));

    if let Some(front) = workspace.frontmostApplication() {
        FRONT_APPS.with(|apps| {
            apps.borrow_mut().activated(AppInfo::from_running(&front));
        });
    }

    config::subscribe(Box::new(|old, new| {
        emit(&Event::ConfigChanged { old, new });
    }));
}

// ---------------------------------------------------------------- главная очередь

#[repr(C)]
struct DispatchQueue {
    _opaque: [u8; 0],
}

type DispatchFunction = extern "C" fn(*mut c_void);

extern "C" {
    /// Главная очередь: `dispatch_get_main_queue()` — макрос над этим символом.
    static _dispatch_main_q: DispatchQueue;
    fn dispatch_async_f(queue: *const DispatchQueue, context: *mut c_void, work: DispatchFunction);
    fn dispatch_after_f(
        when: u64,
        queue: *const DispatchQueue,
        context: *mut c_void,
        work: DispatchFunction,
    );
    fn dispatch_time(when: u64, delta: i64) -> u64;
}

const DISPATCH_TIME_NOW: u64 = 0;

type Task = Box<dyn FnOnce()>;

extern "C" fn run_task(context: *mut c_void) {
    // SAFETY: `context` — `Box<Task>` из `enqueue`, вызывается ровно один раз.
    let task = unsafe { Box::from_raw(context as *mut Task) };
    task();
}

fn enqueue(delay: Duration, task: Task) {
    let context = Box::into_raw(Box::new(task)) as *mut c_void;
    // SAFETY: главная очередь существует всё время работы процесса; `run_task`
    // заберёт `context` обратно.
    unsafe {
        let queue = &raw const _dispatch_main_q;
        if delay.is_zero() {
            dispatch_async_f(queue, context, run_task);
        } else {
            let nanoseconds = i64::try_from(delay.as_nanos()).unwrap_or(i64::MAX);
            let when = dispatch_time(DISPATCH_TIME_NOW, nanoseconds);
            dispatch_after_f(when, queue, context, run_task);
        }
    }
}

/// Выполнить на главном потоке при следующем обороте цикла событий
/// (`DispatchQueue.main.async`). Вызывать с главного потока.
pub fn run_later(task: impl FnOnce() + 'static) {
    run_after(Duration::ZERO, task);
}

/// Выполнить на главном потоке через `delay` (`DispatchQueue.main.asyncAfter`).
/// Вызывать с главного потока: `task` не обязана быть `Send`.
pub fn run_after(delay: Duration, task: impl FnOnce() + 'static) {
    assert!(
        MainThreadMarker::new().is_some(),
        "events::run_after вызывают только с главного потока"
    );
    enqueue(delay, Box::new(task));
}

/// Выполнить на главном потоке с любого потока.
pub fn run_on_main(task: impl FnOnce() + Send + 'static) {
    enqueue(Duration::ZERO, Box::new(task));
}

// ---------------------------------------------------------------- таймер цикла событий

/// Выполнить на главном потоке при следующем обороте цикла событий — из
/// разового таймера цикла (`CFRunLoopTimer` в common modes главного цикла), а
/// не из блока главной очереди GCD. Вызывать с главного потока.
///
/// Для задач, которые крутят вложенный цикл событий, — прежде всего модальных
/// окон (`NSAlert.runModal`, `NSOpenPanel.runModal`). Вложенный цикл,
/// запущенный изнутри блока главной очереди (`run_later`, `run_after`,
/// `run_on_main`), эту очередь не обслуживает: пока окно на экране, стоят все
/// остальные блоки — обработчики мониторов мыши, действие зелёной кнопки,
/// отложенные вызовы менеджеров. Из таймера очередь работает и под модальным
/// окном (`examples/main_queue_modal_check.rs`). Остальным задачам хватает
/// `run_later`.
pub fn run_in_run_loop(task: impl FnOnce() + 'static) {
    assert!(
        MainThreadMarker::new().is_some(),
        "events::run_in_run_loop вызывают только с главного потока"
    );
    // SAFETY: главный цикл существует всё время работы процесса.
    unsafe { schedule_timer(CFRunLoopGetMain(), kCFRunLoopCommonModes, Box::new(task)) };
}

/// Задача разового таймера: забирается при срабатывании.
type TimerTask = Cell<Option<Task>>;

/// Разовый таймер «сейчас» на цикле `run_loop` в режиме `mode`: выполнит
/// `task` в потоке этого цикла и снимется сам. Если цикл так и не дойдёт до
/// таймера (его поток завершился), задача освобождается вместе с таймером, не
/// выполнившись.
///
/// # Safety
/// `run_loop` и `mode` — живые объекты CF; `task` выполнится в потоке
/// `run_loop`, поэтому цикл должен принадлежать вызывающему потоку (или
/// главному, если вызывают с главного).
unsafe fn schedule_timer(run_loop: CFRunLoopRef, mode: CFStringRef, task: Task) {
    let info = Box::into_raw(Box::new(TimerTask::new(Some(task)))) as *mut c_void;
    let mut context = CFRunLoopTimerContext {
        version: 0,
        info,
        retain: None,
        release: Some(release_timer_task),
        copyDescription: None,
    };
    let timer = CFRunLoopTimerCreate(
        kCFAllocatorDefault,
        CFAbsoluteTimeGetCurrent(),
        0.0,
        0,
        0,
        run_timer_task,
        &mut context,
    );
    if timer.is_null() {
        release_timer_task(info);
        return;
    }
    CFRunLoopAddTimer(run_loop, timer, mode);
    // Дальше таймер держит цикл: разовый таймер после срабатывания снимается
    // сам и освобождается (вместе с задачей — `release_timer_task`).
    CFRelease(timer as *const c_void);
}

extern "C" fn run_timer_task(_timer: CFRunLoopTimerRef, info: *mut c_void) {
    // SAFETY: `info` — `TimerTask` из `schedule_timer`, живёт, пока жив таймер.
    let task = unsafe { &*(info as *const TimerTask) }.take();
    if let Some(task) = task {
        task();
    }
}

extern "C" fn release_timer_task(info: *const c_void) {
    // SAFETY: CF зовёт это ровно один раз — когда таймер снят или освобождён.
    drop(unsafe { Box::from_raw(info as *mut TimerTask) });
}

#[cfg(test)]
mod tests {
    use super::*;
    use core_foundation_sys::runloop::{
        kCFRunLoopDefaultMode, kCFRunLoopRunFinished, CFRunLoopGetCurrent, CFRunLoopRunInMode,
    };
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    fn app(pid: i32, bundle_id: &str) -> AppInfo {
        AppInfo {
            pid,
            bundle_id: Some(bundle_id.to_string()),
            name: Some(bundle_id.to_string()),
        }
    }

    #[test]
    fn handlers_get_only_their_kind() {
        let screens = Rc::new(Cell::new(0));
        let spaces = Rc::new(Cell::new(0));
        let screens_seen = screens.clone();
        let spaces_seen = spaces.clone();
        on_screens_changed(move || screens_seen.set(screens_seen.get() + 1));
        on_space_changed(move || spaces_seen.set(spaces_seen.get() + 1));

        emit(&Event::ScreensChanged);
        emit(&Event::ScreensChanged);
        emit(&Event::SpaceChanged);
        emit(&Event::AppWillBecomeActive);

        assert_eq!(screens.get(), 2);
        assert_eq!(spaces.get(), 1);
    }

    #[test]
    fn typed_handlers_receive_payload() {
        let seen = Rc::new(RefCell::new(Vec::new()));
        let front_seen = seen.clone();
        on_front_app_changed(move |current, previous| {
            front_seen.borrow_mut().push(format!(
                "front {} после {:?}",
                current.pid,
                previous.map(|app| app.pid)
            ));
        });
        let launched_seen = seen.clone();
        on_app_launched(move |app| {
            launched_seen
                .borrow_mut()
                .push(format!("запуск {}", app.pid))
        });
        let terminated_seen = seen.clone();
        on_app_terminated(move |app| {
            terminated_seen
                .borrow_mut()
                .push(format!("выход {}", app.pid))
        });
        let config_seen = seen.clone();
        on_config_changed(move |old, new| {
            config_seen
                .borrow_mut()
                .push(format!("зазор {} → {}", old.gap_size, new.gap_size));
        });

        let terminal = app(10, "com.apple.Terminal");
        let safari = app(20, "com.apple.Safari");
        emit(&Event::FrontAppChanged {
            current: &safari,
            previous: Some(&terminal),
        });
        emit(&Event::AppLaunched(&terminal));
        emit(&Event::AppTerminated(&safari));
        let old = Config::default();
        let new = Config {
            gap_size: 8.0,
            ..Config::default()
        };
        emit(&Event::ConfigChanged {
            old: &old,
            new: &new,
        });

        assert_eq!(
            *seen.borrow(),
            vec![
                "front 20 после Some(10)".to_string(),
                "запуск 10".to_string(),
                "выход 20".to_string(),
                "зазор 0 → 8".to_string(),
            ]
        );
    }

    #[test]
    fn unsubscribe_stops_delivery() {
        let count = Rc::new(Cell::new(0));
        let counter = count.clone();
        let id = on_space_changed(move || counter.set(counter.get() + 1));
        emit(&Event::SpaceChanged);
        assert!(unsubscribe(id));
        assert!(!unsubscribe(id));
        emit(&Event::SpaceChanged);
        assert_eq!(count.get(), 1);
    }

    #[test]
    fn subscribing_inside_handler_takes_effect_next_time() {
        // Подписка изнутри рассылки не должна паниковать (RefCell) и не
        // получает текущее событие.
        let inner = Rc::new(Cell::new(0));
        let inner_outer = inner.clone();
        let subscribed = Rc::new(Cell::new(false));
        on_screens_changed(move || {
            if !subscribed.replace(true) {
                let inner_counter = inner_outer.clone();
                on_screens_changed(move || inner_counter.set(inner_counter.get() + 1));
            }
        });
        emit(&Event::ScreensChanged);
        assert_eq!(inner.get(), 0);
        emit(&Event::ScreensChanged);
        assert_eq!(inner.get(), 1);
    }

    #[test]
    fn handlers_run_in_subscription_order() {
        let order = Rc::new(RefCell::new(Vec::new()));
        for number in 1..=3 {
            let order = order.clone();
            subscribe(EventKind::AppWillBecomeActive, move |event| {
                assert_eq!(event.kind(), EventKind::AppWillBecomeActive);
                order.borrow_mut().push(number);
            });
        }
        emit(&Event::AppWillBecomeActive);
        assert_eq!(*order.borrow(), vec![1, 2, 3]);
    }

    #[test]
    fn front_apps_track_current_and_previous() {
        let mut apps = FrontApps::default();
        assert!(apps.activated(app(1, "local.rectangle2rust")));
        assert_eq!(apps.previous, None);

        assert!(apps.activated(app(2, "com.apple.Terminal")));
        assert_eq!(apps.current.as_ref().map(|app| app.pid), Some(2));
        assert_eq!(apps.previous.as_ref().map(|app| app.pid), Some(1));

        // Повторная активация того же процесса не сдвигает «прежнее».
        assert!(!apps.activated(app(2, "com.apple.Terminal")));
        assert_eq!(apps.previous.as_ref().map(|app| app.pid), Some(1));

        assert!(apps.activated(app(3, "com.apple.Safari")));
        assert_eq!(apps.previous.as_ref().map(|app| app.pid), Some(2));

        apps.terminated(2);
        assert_eq!(apps.previous, None);
        assert_eq!(apps.current.as_ref().map(|app| app.pid), Some(3));
    }

    /// Поднимает флаг, когда его роняют: задача таймера держит его, пока жива.
    struct DropFlag(Arc<AtomicBool>);

    impl Drop for DropFlag {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    #[test]
    fn run_loop_task_runs_once_and_its_timer_goes_away() {
        // Главный цикл в тестах никто не крутит: таймер ставится на цикл потока
        // теста — так же, как `run_in_run_loop` ставит его на главный.
        let runs = Rc::new(Cell::new(0));
        let dropped = Arc::new(AtomicBool::new(false));
        let counter = runs.clone();
        let flag = DropFlag(dropped.clone());
        unsafe {
            schedule_timer(
                CFRunLoopGetCurrent(),
                kCFRunLoopDefaultMode,
                Box::new(move || {
                    let _flag = &flag;
                    counter.set(counter.get() + 1);
                }),
            );
        }
        assert_eq!(runs.get(), 0, "задача ждёт оборота цикла");
        // Разовый таймер после срабатывания снимается сам: цикл выходит, потому
        // что в режиме больше ничего нет.
        let result = unsafe { CFRunLoopRunInMode(kCFRunLoopDefaultMode, 2.0, 0) };
        assert_eq!(result, kCFRunLoopRunFinished);
        assert_eq!(runs.get(), 1);
        assert!(dropped.load(Ordering::SeqCst), "задачу отпустили");
        let again = unsafe { CFRunLoopRunInMode(kCFRunLoopDefaultMode, 0.0, 0) };
        assert_eq!(again, kCFRunLoopRunFinished);
        assert_eq!(runs.get(), 1, "второй раз не срабатывает");
    }

    #[test]
    fn run_loop_task_is_released_if_its_loop_never_runs() {
        let ran = Arc::new(AtomicBool::new(false));
        let dropped = Arc::new(AtomicBool::new(false));
        let (ran_in_task, dropped_in_task) = (ran.clone(), dropped.clone());
        std::thread::spawn(move || {
            let flag = DropFlag(dropped_in_task);
            unsafe {
                schedule_timer(
                    CFRunLoopGetCurrent(),
                    kCFRunLoopDefaultMode,
                    Box::new(move || {
                        let _flag = &flag;
                        ran_in_task.store(true, Ordering::SeqCst);
                    }),
                );
            }
            // Поток завершается, так и не запустив свой цикл.
        })
        .join()
        .unwrap();
        assert!(!ran.load(Ordering::SeqCst));
        assert!(dropped.load(Ordering::SeqCst), "задача не утекла");
    }

    #[test]
    fn latest_app_skips_own_process() {
        let mut apps = FrontApps::default();
        assert_eq!(apps.latest_except(1), None);
        apps.activated(app(2, "com.apple.Terminal"));
        assert_eq!(apps.latest_except(1).map(|app| app.pid), Some(2));
        // Ссылка вывела вперёд само приложение (pid 1) — нужно прежнее.
        apps.activated(app(1, "local.rectangle2rust"));
        assert_eq!(apps.latest_except(1).map(|app| app.pid), Some(2));
        apps.activated(app(3, "com.apple.Safari"));
        assert_eq!(apps.latest_except(1).map(|app| app.pid), Some(3));
    }
}
