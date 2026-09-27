//! Подписка на AX-уведомления об окнах чужого приложения (AXObserver) и
//! перезаводимый таймер на главном run loop.
//!
//! `AppObserver::attach(pid, обработчик)` подписывается у приложения на
//! `kAXWindowCreatedNotification`, а у каждого его окна — на
//! `kAXUIElementDestroyedNotification`, `kAXWindowMiniaturizedNotification` и
//! `kAXWindowDeminiaturizedNotification` (новое окно подписывается само, как только
//! приходит уведомление о его создании). Перемещение и ресайз окон не слушаем.
//! Источник событий висит на главном run loop в `kCFRunLoopDefaultMode`, поэтому
//! и подключаться, и получать события нужно на главном потоке.
//!
//! Память CF: observer (+1 из `AXObserverCreate`) отпускается, когда `AppObserver`
//! роняют, — вместе со снятием всех подписок и источника с run loop; окна держатся
//! как `AxElement` (retain) и отпускаются при закрытии окна или вместе с observer.
//! Если `AppObserver` роняют прямо из его же обработчика, сам observer отпускается
//! чуть позже разовым таймером — уже вне колбэка AX, который ещё держит его.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ffi::c_void;
use std::fmt;
use std::ptr;
use std::rc::Rc;
use std::time::Duration;

use core_foundation::base::TCFType;
use core_foundation::string::{CFString, CFStringRef};
use core_foundation_sys::base::{kCFAllocatorDefault, CFEqual, CFRelease};
use core_foundation_sys::date::CFAbsoluteTimeGetCurrent;
use core_foundation_sys::runloop::{
    kCFRunLoopDefaultMode, CFRunLoopAddSource, CFRunLoopAddTimer, CFRunLoopGetMain,
    CFRunLoopRemoveSource, CFRunLoopSourceRef, CFRunLoopTimerContext, CFRunLoopTimerCreate,
    CFRunLoopTimerInvalidate, CFRunLoopTimerRef, CFRunLoopTimerSetNextFireDate,
};
use objc2::MainThreadMarker;

use crate::ax::AxElement;

// ---------------------------------------------------------------------------
// FFI: AXObserver (ApplicationServices)
// ---------------------------------------------------------------------------

/// `AXObserverRef` (CFTypeRef).
type ObserverRef = *const c_void;

type ObserverCallback = unsafe extern "C" fn(
    observer: ObserverRef,
    element: *const c_void,
    notification: CFStringRef,
    refcon: *mut c_void,
);

/// `AXError`, успех.
const K_AX_ERROR_SUCCESS: i32 = 0;
/// `kAXErrorNotificationAlreadyRegistered` — подписка уже есть, это не ошибка.
const K_AX_ERROR_ALREADY_REGISTERED: i32 = -25209;

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXObserverCreate(
        application: i32,
        callback: ObserverCallback,
        out_observer: *mut ObserverRef,
    ) -> i32;
    fn AXObserverAddNotification(
        observer: ObserverRef,
        element: *const c_void,
        notification: CFStringRef,
        refcon: *mut c_void,
    ) -> i32;
    fn AXObserverRemoveNotification(
        observer: ObserverRef,
        element: *const c_void,
        notification: CFStringRef,
    ) -> i32;
    fn AXObserverGetRunLoopSource(observer: ObserverRef) -> CFRunLoopSourceRef;
}

/// `kAXWindowCreatedNotification` — у элемента приложения.
const WINDOW_CREATED: &str = "AXWindowCreated";
/// `kAXUIElementDestroyedNotification`.
const ELEMENT_DESTROYED: &str = "AXUIElementDestroyed";
/// `kAXWindowMiniaturizedNotification`.
const WINDOW_MINIATURIZED: &str = "AXWindowMiniaturized";
/// `kAXWindowDeminiaturizedNotification`.
const WINDOW_DEMINIATURIZED: &str = "AXWindowDeminiaturized";
/// Уведомления, на которые подписывается каждое окно.
const WINDOW_NOTIFICATIONS: [&str; 3] = [
    ELEMENT_DESTROYED,
    WINDOW_MINIATURIZED,
    WINDOW_DEMINIATURIZED,
];

/// Страховка от зависшего приложения: дольше ответа AX не ждём (секунды).
const MESSAGING_TIMEOUT: f32 = 0.5;

// ---------------------------------------------------------------------------
// События и ошибки
// ---------------------------------------------------------------------------

/// Что случилось с окнами приложения.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowEvent {
    /// Появилось окно (`kAXWindowCreatedNotification`).
    Created,
    /// Окно закрыто (`kAXUIElementDestroyedNotification`).
    Destroyed,
    /// Окно свёрнуто в Dock (`kAXWindowMiniaturizedNotification`).
    Miniaturized,
    /// Окно развёрнуто из Dock (`kAXWindowDeminiaturizedNotification`).
    Deminiaturized,
}

impl WindowEvent {
    /// Событие по имени AX-уведомления; чужие уведомления — `None`.
    pub fn from_notification(name: &str) -> Option<WindowEvent> {
        match name {
            WINDOW_CREATED => Some(WindowEvent::Created),
            ELEMENT_DESTROYED => Some(WindowEvent::Destroyed),
            WINDOW_MINIATURIZED => Some(WindowEvent::Miniaturized),
            WINDOW_DEMINIATURIZED => Some(WindowEvent::Deminiaturized),
            _ => None,
        }
    }
}

/// Почему не удалось подписаться на окна приложения.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObserverError {
    /// Подписываться можно только на главном потоке: события приходят в его run loop.
    NotMainThread,
    /// `AXError` из `AXObserverCreate`/`AXObserverAddNotification`.
    Ax(i32),
}

impl fmt::Display for ObserverError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ObserverError::NotMainThread => {
                write!(f, "подписка на окна — только с главного потока")
            }
            ObserverError::Ax(-25211) => {
                write!(f, "нет доступа к управлению компьютером (AXError -25211)")
            }
            ObserverError::Ax(-25204) => write!(f, "приложение не ответило (AXError -25204)"),
            ObserverError::Ax(-25207) => {
                write!(
                    f,
                    "приложение не шлёт уведомления об окнах (AXError -25207)"
                )
            }
            ObserverError::Ax(code) => write!(f, "AXError {code}"),
        }
    }
}

// ---------------------------------------------------------------------------
// Реестр подписок
// ---------------------------------------------------------------------------

/// Живая подписка: observer, его источник на run loop и окна под наблюдением.
struct Registration {
    pid: i32,
    /// +1 из `AXObserverCreate`.
    observer: ObserverRef,
    /// Принадлежит observer'у (get rule): не отпускаем, только снимаем с run loop.
    source: CFRunLoopSourceRef,
    app: AxElement,
    windows: Vec<AxElement>,
    handler: Rc<dyn Fn(i32, WindowEvent)>,
}

thread_local! {
    /// Подписки по номеру; номер уходит в AX как `refcon` и не переиспользуется,
    /// поэтому запоздалое уведомление снятой подписки просто не находит адресата.
    static REGISTRY: RefCell<HashMap<usize, Registration>> = RefCell::new(HashMap::new());
    static NEXT_ID: Cell<usize> = const { Cell::new(1) };
    /// Глубина вложенности колбэков AX (отпускать observer внутри них нельзя).
    static IN_CALLBACK: Cell<u32> = const { Cell::new(0) };
    /// Observer'ы, которые отпустим разовым таймером, уже вне колбэка.
    static DEFERRED: RefCell<Vec<ObserverRef>> = const { RefCell::new(Vec::new()) };
}

fn add_notification(
    observer: ObserverRef,
    element: &AxElement,
    name: &str,
    id: usize,
) -> Result<(), ObserverError> {
    let name = CFString::new(name);
    let err = unsafe {
        AXObserverAddNotification(
            observer,
            element.as_raw(),
            name.as_concrete_TypeRef(),
            id as *mut c_void,
        )
    };
    if err == K_AX_ERROR_SUCCESS || err == K_AX_ERROR_ALREADY_REGISTERED {
        Ok(())
    } else {
        Err(ObserverError::Ax(err))
    }
}

fn remove_notification(observer: ObserverRef, element: &AxElement, name: &str) {
    let name = CFString::new(name);
    // У закрытого окна подписки уже нет — ошибка здесь ожидаема и безвредна.
    unsafe {
        AXObserverRemoveNotification(observer, element.as_raw(), name.as_concrete_TypeRef());
    }
}

fn same_element(a: &AxElement, b: *const c_void) -> bool {
    unsafe { CFEqual(a.as_raw(), b) != 0 }
}

impl Registration {
    fn is_tracked(&self, element: *const c_void) -> bool {
        self.windows
            .iter()
            .any(|window| same_element(window, element))
    }

    /// Подписаться на окно. Без подписки на закрытие окно не отследить — такое не берём.
    fn track(&mut self, id: usize, window: AxElement) {
        if self.is_tracked(window.as_raw()) {
            return;
        }
        window.set_messaging_timeout(MESSAGING_TIMEOUT);
        if add_notification(self.observer, &window, ELEMENT_DESTROYED, id).is_err() {
            return;
        }
        for name in [WINDOW_MINIATURIZED, WINDOW_DEMINIATURIZED] {
            let _ = add_notification(self.observer, &window, name, id);
        }
        self.windows.push(window);
    }

    /// Забыть окно и снять его подписки.
    fn untrack_at(&mut self, index: usize) {
        let window = self.windows.swap_remove(index);
        for name in WINDOW_NOTIFICATIONS {
            remove_notification(self.observer, &window, name);
        }
    }

    fn untrack(&mut self, element: *const c_void) {
        if let Some(index) = self
            .windows
            .iter()
            .position(|window| same_element(window, element))
        {
            self.untrack_at(index);
        }
    }
}

impl Drop for Registration {
    fn drop(&mut self) {
        while !self.windows.is_empty() {
            self.untrack_at(self.windows.len() - 1);
        }
        remove_notification(self.observer, &self.app, WINDOW_CREATED);
        unsafe {
            CFRunLoopRemoveSource(CFRunLoopGetMain(), self.source, kCFRunLoopDefaultMode);
        }
        release_observer(self.observer);
    }
}

/// Отпустить observer; внутри колбэка AX — позже, разовым таймером.
fn release_observer(observer: ObserverRef) {
    let in_callback = IN_CALLBACK
        .try_with(|depth| depth.get() > 0)
        .unwrap_or(false);
    let deferred = in_callback
        && DEFERRED
            .try_with(|list| list.borrow_mut().push(observer))
            .is_ok();
    if !deferred {
        unsafe { CFRelease(observer) };
        return;
    }
    let mut context = CFRunLoopTimerContext {
        version: 0,
        info: ptr::null_mut(),
        retain: None,
        release: None,
        copyDescription: None,
    };
    unsafe {
        let timer = CFRunLoopTimerCreate(
            kCFAllocatorDefault,
            CFAbsoluteTimeGetCurrent(),
            0.0,
            0,
            0,
            release_deferred,
            &mut context,
        );
        if timer.is_null() {
            return;
        }
        CFRunLoopAddTimer(CFRunLoopGetMain(), timer, kCFRunLoopDefaultMode);
        // Разовый таймер: run loop держит его до срабатывания и потом отпускает сам.
        CFRelease(timer as *const c_void);
    }
}

extern "C" fn release_deferred(_timer: CFRunLoopTimerRef, _info: *mut c_void) {
    let observers = DEFERRED
        .try_with(|list| std::mem::take(&mut *list.borrow_mut()))
        .unwrap_or_default();
    for observer in observers {
        unsafe { CFRelease(observer) };
    }
}

/// Колбэк AX: сначала свои подписки (новое окно, закрытое окно), потом обработчик.
unsafe extern "C" fn observer_callback(
    _observer: ObserverRef,
    element: *const c_void,
    notification: CFStringRef,
    refcon: *mut c_void,
) {
    if notification.is_null() {
        return;
    }
    let name = CFString::wrap_under_get_rule(notification).to_string();
    let Some(event) = WindowEvent::from_notification(&name) else {
        return;
    };
    let id = refcon as usize;
    let _ = IN_CALLBACK.try_with(|depth| depth.set(depth.get() + 1));

    let target = REGISTRY
        .try_with(|registry| {
            let mut registry = registry.try_borrow_mut().ok()?;
            let registration = registry.get_mut(&id)?;
            match event {
                WindowEvent::Created => {
                    if let Some(window) = AxElement::retain_raw(element) {
                        window.set_messaging_timeout(MESSAGING_TIMEOUT);
                        if window.role().as_deref() == Some("AXWindow") {
                            registration.track(id, window);
                        }
                    }
                }
                WindowEvent::Destroyed => registration.untrack(element),
                WindowEvent::Miniaturized | WindowEvent::Deminiaturized => {}
            }
            Some((registration.pid, registration.handler.clone()))
        })
        .ok()
        .flatten();
    if let Some((pid, handler)) = target {
        handler(pid, event);
    }

    let _ = IN_CALLBACK.try_with(|depth| depth.set(depth.get().saturating_sub(1)));
}

// ---------------------------------------------------------------------------
// AppObserver
// ---------------------------------------------------------------------------

/// Подписка на окна одного приложения. Пока значение живо, обработчик получает
/// события; `drop` снимает все подписки и отпускает observer.
pub struct AppObserver {
    id: usize,
    pid: i32,
}

impl AppObserver {
    /// Подписаться на окна приложения `pid`: новое окно, закрытие, сворачивание и
    /// разворачивание из Dock. Обработчик зовётся на главном потоке с pid и событием.
    /// Только с главного потока.
    pub fn attach(
        pid: i32,
        handler: impl Fn(i32, WindowEvent) + 'static,
    ) -> Result<AppObserver, ObserverError> {
        if MainThreadMarker::new().is_none() {
            return Err(ObserverError::NotMainThread);
        }
        let mut observer: ObserverRef = ptr::null();
        let err = unsafe { AXObserverCreate(pid, observer_callback, &mut observer) };
        if err != K_AX_ERROR_SUCCESS || observer.is_null() {
            return Err(ObserverError::Ax(err));
        }

        let id = NEXT_ID.with(|next| {
            let id = next.get();
            next.set(id + 1);
            id
        });
        let app = AxElement::application(pid);
        app.set_messaging_timeout(MESSAGING_TIMEOUT);
        if let Err(error) = add_notification(observer, &app, WINDOW_CREATED, id) {
            unsafe { CFRelease(observer) };
            return Err(error);
        }
        let source = unsafe { AXObserverGetRunLoopSource(observer) };
        if source.is_null() {
            remove_notification(observer, &app, WINDOW_CREATED);
            unsafe { CFRelease(observer) };
            return Err(ObserverError::Ax(-25200));
        }
        unsafe { CFRunLoopAddSource(CFRunLoopGetMain(), source, kCFRunLoopDefaultMode) };

        REGISTRY.with(|registry| {
            registry.borrow_mut().insert(
                id,
                Registration {
                    pid,
                    observer,
                    source,
                    app,
                    windows: Vec::new(),
                    handler: Rc::new(handler),
                },
            );
        });
        let observer = AppObserver { id, pid };
        observer.refresh_windows();
        Ok(observer)
    }

    /// pid наблюдаемого приложения.
    pub fn pid(&self) -> i32 {
        self.pid
    }

    /// Сверить подписки с `AXWindows`: подписаться на окна, о которых
    /// уведомления не было (появились до подписки или уведомление потерялось), и
    /// забыть закрытые окна, которых нет в списке и которые больше не отвечают.
    /// Возвращает число окон под наблюдением.
    pub fn refresh_windows(&self) -> usize {
        let app = REGISTRY.with(|registry| {
            registry
                .borrow()
                .get(&self.id)
                .map(|registration| registration.app.clone())
        });
        let Some(app) = app else {
            return 0;
        };
        let listed = app.windows();

        REGISTRY.with(|registry| {
            let mut registry = registry.borrow_mut();
            let Some(registration) = registry.get_mut(&self.id) else {
                return 0;
            };
            let mut index = 0;
            while index < registration.windows.len() {
                let window = &registration.windows[index];
                let alive = listed
                    .iter()
                    .any(|listed| same_element(listed, window.as_raw()))
                    || window.role().is_some();
                if alive {
                    index += 1;
                } else {
                    registration.untrack_at(index);
                }
            }
            for window in listed {
                registration.track(self.id, window);
            }
            registration.windows.len()
        })
    }
}

impl Drop for AppObserver {
    fn drop(&mut self) {
        // Регистрацию вынимаем из реестра и роняем уже вне заёма: её `Drop`
        // снимает подписки через AX.
        let registration = REGISTRY
            .try_with(|registry| {
                registry
                    .try_borrow_mut()
                    .ok()
                    .and_then(|mut registry| registry.remove(&self.id))
            })
            .ok()
            .flatten();
        drop(registration);
    }
}

// ---------------------------------------------------------------------------
// MainTimer
// ---------------------------------------------------------------------------

/// «Никогда» для таймера в покое: и дата, и интервал (около 31 года).
const DISTANT: f64 = 1.0e9;

/// Перезаводимый таймер на главном run loop (`kCFRunLoopDefaultMode`).
///
/// `fire_in(задержка)` назначает ближайшее срабатывание; после срабатывания таймер
/// спит до следующего `fire_in`. Колбэк зовётся на главном потоке. `drop` снимает
/// таймер с run loop и отпускает его.
pub struct MainTimer(CFRunLoopTimerRef);

impl MainTimer {
    pub fn new(callback: fn()) -> MainTimer {
        let mut context = CFRunLoopTimerContext {
            version: 0,
            info: callback as *mut c_void,
            retain: None,
            release: None,
            copyDescription: None,
        };
        unsafe {
            let timer = CFRunLoopTimerCreate(
                kCFAllocatorDefault,
                CFAbsoluteTimeGetCurrent() + DISTANT,
                DISTANT,
                0,
                0,
                main_timer_fired,
                &mut context,
            );
            CFRunLoopAddTimer(CFRunLoopGetMain(), timer, kCFRunLoopDefaultMode);
            MainTimer(timer)
        }
    }

    /// Сработать через `delay` (прежний завод отменяется).
    pub fn fire_in(&self, delay: Duration) {
        unsafe {
            CFRunLoopTimerSetNextFireDate(self.0, CFAbsoluteTimeGetCurrent() + delay.as_secs_f64());
        }
    }

    /// Отменить завод.
    pub fn cancel(&self) {
        unsafe {
            CFRunLoopTimerSetNextFireDate(self.0, CFAbsoluteTimeGetCurrent() + DISTANT);
        }
    }
}

impl Drop for MainTimer {
    fn drop(&mut self) {
        unsafe {
            CFRunLoopTimerInvalidate(self.0);
            CFRelease(self.0 as *const c_void);
        }
    }
}

extern "C" fn main_timer_fired(_timer: CFRunLoopTimerRef, info: *mut c_void) {
    // `info` — указатель на функцию из `MainTimer::new`.
    let callback = unsafe { std::mem::transmute::<*mut c_void, fn()>(info) };
    callback();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notifications_map_to_events() {
        assert_eq!(
            WindowEvent::from_notification("AXWindowCreated"),
            Some(WindowEvent::Created)
        );
        assert_eq!(
            WindowEvent::from_notification("AXUIElementDestroyed"),
            Some(WindowEvent::Destroyed)
        );
        assert_eq!(
            WindowEvent::from_notification("AXWindowMiniaturized"),
            Some(WindowEvent::Miniaturized)
        );
        assert_eq!(
            WindowEvent::from_notification("AXWindowDeminiaturized"),
            Some(WindowEvent::Deminiaturized)
        );
        // Перемещение и ресайз не слушаем.
        assert_eq!(WindowEvent::from_notification("AXWindowMoved"), None);
        assert_eq!(WindowEvent::from_notification("AXWindowResized"), None);
    }

    #[test]
    fn error_messages_are_russian() {
        assert!(ObserverError::Ax(-25211)
            .to_string()
            .contains("нет доступа"));
        assert_eq!(ObserverError::Ax(-1).to_string(), "AXError -1");
    }
}
