//! Прилипание окон при перетаскивании (drag-to-snap) — порт
//! `Snapping/SnappingManager.swift`.
//!
//! Тащишь окно к краю или углу экрана — появляется подсветка будущего места
//! (`footprint`), отпускаешь — окно встаёт туда действием области
//! (`window_manager::execute`, источник `DragToSnap`). Какие области у какого
//! края — `area_model` (настройки `landscapeSnapAreas` / `portraitSnapAreas`),
//! составные области — `compound`, чистая логика — `zones` и `drag`.
//!
//! Мышь слушают глобальные мониторы (`event_monitor`), только пока прилипание
//! включено (`windowSnapping`), приложение впереди его не выключает
//! («Игнорировать», `ignoreDragSnapToo`, `fullIgnoreBundleIds`) и переднее
//! окно не во весь экран. При `missionControlDragging` = «нет» монитор
//! активный: он ещё и не даёт быстро утащить окно в Mission Control.
//! Настройки перечитываются на лету (`reload`). На время модальных диалогов
//! (импорт и экспорт настроек) слежение ставят на паузу (`pause`/`resume`).
//!
//! При запуске (после остальных подсистем) — предупреждения о проблемных
//! приложениях и о конфликте с прилипанием macOS (`problem_apps`, `mac_tiling`).

pub mod area_model;
pub mod compound;
mod drag;
pub mod footprint;
mod problem_apps;
pub mod zones;

use std::cell::{Cell, RefCell};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use objc2::rc::Retained;
use objc2::runtime::NSObject;
use objc2::{define_class, msg_send, sel, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSHapticFeedbackManager, NSHapticFeedbackPattern, NSHapticFeedbackPerformanceTime,
    NSHapticFeedbackPerformer, NSScreen, NSWorkspace,
    NSWorkspaceSessionDidBecomeActiveNotification,
};
use objc2_foundation::NSNotification;

use crate::actions::Action;
use crate::ax::{self, AxElement};
use crate::calc::LastAction;
use crate::config::{self, Config};
use crate::event_monitor::{
    self, ActiveEventMonitor, EventMask, MouseEvent, MouseEventKind, PassiveEventMonitor, TapEvent,
    TapFilter,
};
use crate::geometry::Rect;
use crate::screens::{self, Screen};
use crate::window_manager::{self, ExecutionParameters};
use crate::{events, log, mac_tiling, stage, window_history};

use drag::{DragState, SnapSystem};
use footprint::{Footprint, FootprintStyle};
use zones::MissionControlGuard;

pub use zones::SnapArea;

// ---------------------------------------------------------------- система

/// `StageUtil.stageCapable` (macOS 13+) — за время работы не меняется, а
/// спрашивают его на каждом событии перетаскивания.
fn stage_capable() -> bool {
    static CAPABLE: OnceLock<bool> = OnceLock::new();
    *CAPABLE.get_or_init(stage::stage_capable)
}

/// Настоящая система для автомата перетаскивания: AX, экраны, подсветка,
/// менеджер окон и история.
struct RealSystem<'a> {
    footprint: &'a mut Option<Footprint>,
    mtm: MainThreadMarker,
}

impl SnapSystem for RealSystem<'_> {
    type Window = AxElement;

    fn window_under_cursor(&mut self) -> Option<AxElement> {
        ax::window_element_under_cursor()
    }

    fn window_id(&mut self, window: &AxElement) -> Option<u32> {
        window.get_window_id()
    }

    fn frame(&mut self, window: &AxElement) -> Option<Rect> {
        window.frame()
    }

    fn set_frame(&mut self, window: &AxElement, frame: Rect) {
        window.set_frame_ordered(&frame, false);
    }

    fn cursor(&mut self) -> Option<(f64, f64)> {
        screens::cursor_position()
    }

    fn screens(&mut self) -> Vec<Screen> {
        screens::screens()
    }

    fn in_stage_strip(&mut self, window_id: u32) -> bool {
        if !(stage_capable() && stage::stage_enabled()) {
            return false;
        }
        screens::screens()
            .into_iter()
            .find(|screen| screen.is_main)
            .is_some_and(|main| stage::stage_strip_window_group(window_id, &main).is_some())
    }

    fn is_todo_window(&mut self, window_id: u32) -> bool {
        window_manager::is_todo_window(window_id)
    }

    fn footprint_rect(
        &mut self,
        area: &SnapArea,
        window_frame: Rect,
        window_id: Option<u32>,
        config: &Config,
    ) -> Option<Rect> {
        let ignore_todo = window_id.is_some_and(window_manager::is_todo_window);
        let visible = window_manager::adjusted_visible_frame(&area.screen, ignore_todo, false);
        let primary_height = screens::primary_screen_height();
        let window = window_frame.screen_flipped(primary_height);
        zones::footprint_rect(area.action, window, visible, config, primary_height)
    }

    fn show_footprint(&mut self, area: &SnapArea, rect: Rect, config: &Config) {
        let style = FootprintStyle::from_config(config);
        // Вид подсветки поменяли в настройках — новое окно (пока старое не на экране).
        if self
            .footprint
            .as_ref()
            .is_some_and(|footprint| *footprint.style() != style && !footprint.real_is_visible())
        {
            *self.footprint = None;
        }
        let mtm = self.mtm;
        let footprint = self
            .footprint
            .get_or_insert_with(|| Footprint::new(mtm, style));
        footprint.show(
            rect,
            area.directional,
            config.footprint_animation_duration_multiplier as f64,
        );
    }

    fn hide_footprint(&mut self) {
        if let Some(footprint) = self.footprint.as_ref() {
            footprint.order_out();
        }
    }

    fn haptic_feedback(&mut self) {
        NSHapticFeedbackManager::defaultPerformer().performFeedbackPattern_performanceTime(
            NSHapticFeedbackPattern::Alignment,
            NSHapticFeedbackPerformanceTime::Now,
        );
    }

    fn execute(
        &mut self,
        action: Action,
        window: Option<AxElement>,
        window_id: Option<u32>,
        screen: Screen,
    ) {
        window_manager::execute(ExecutionParameters::snap(action, window, window_id, screen));
    }

    fn last_action(&mut self, window_id: u32) -> Option<LastAction> {
        window_history::last_action(window_id)
    }

    fn remove_last_action(&mut self, window_id: u32) {
        window_history::remove_last_action(window_id);
    }

    fn restore_rect(&mut self, window_id: u32) -> Option<Rect> {
        window_history::restore_rect(window_id)
    }

    fn set_restore_rect(&mut self, window_id: u32, rect: Option<Rect>) {
        match rect {
            Some(rect) => window_history::set_restore_rect(window_id, rect),
            None => window_history::with(|history| {
                history.restore_rects.remove(&window_id);
            }),
        }
    }
}

// ---------------------------------------------------------------- мониторы

/// Какие события слушает прилипание.
const MASK: EventMask = EventMask(
    EventMask::LEFT_MOUSE_DOWN.0 | EventMask::LEFT_MOUSE_UP.0 | EventMask::LEFT_MOUSE_DRAGGED.0,
);

enum Monitor {
    Passive(PassiveEventMonitor),
    Active(ActiveEventMonitor),
}

impl Monitor {
    /// `startEventMonitor`.
    fn start(active: bool) -> Monitor {
        if active {
            let mut monitor = ActiveEventMonitor::new(
                MASK,
                mission_control_filter(),
                Arc::new(|event: MouseEvent| handle_mouse_event(&event)),
            );
            monitor.start();
            Monitor::Active(monitor)
        } else {
            let mut monitor = PassiveEventMonitor::new(MASK, handle_mouse_event);
            monitor.start();
            Monitor::Passive(monitor)
        }
    }

    fn is_active(&self) -> bool {
        matches!(self, Monitor::Active(_))
    }

    fn running(&self) -> bool {
        match self {
            Monitor::Passive(monitor) => monitor.running(),
            Monitor::Active(monitor) => monitor.running(),
        }
    }
}

/// Верх главного экрана (`NSScreen.main`) в координатах Quartz — для фильтра
/// в потоке тапа, где AppKit трогать нельзя (оригинал читает `NSScreen.main`
/// прямо в фильтре, на каждом перетаскивании).
///
/// Главный экран — тот, где ключевое окно, и меняется он не только со сменой
/// экранов: щелчок по окну на другом дисплее делает главным его, и может
/// случиться позже, чем обработчик нажатия. Поэтому кэш освежается на каждом
/// нажатии и перетаскивании (`refresh_on`, обработчик на главном потоке), а
/// ещё при смене экранов и активного приложения (`watch_main_screen`).
struct MainScreenTop(AtomicU64);

impl MainScreenTop {
    /// Главного экрана не знаем (NaN).
    const fn unknown() -> MainScreenTop {
        MainScreenTop(AtomicU64::new(f64::NAN.to_bits()))
    }

    fn set(&self, top: Option<f64>) {
        self.0
            .store(top.unwrap_or(f64::NAN).to_bits(), Ordering::Relaxed);
    }

    fn get(&self) -> Option<f64> {
        let top = f64::from_bits(self.0.load(Ordering::Relaxed));
        (!top.is_nan()).then_some(top)
    }

    /// Событие дошло до обработчика: на нажатии и на каждом перетаскивании
    /// взять свежий верх главного экрана (`read`). Следующие события жеста
    /// фильтр сверит уже с ним.
    fn refresh_on(&self, kind: MouseEventKind, read: impl FnOnce() -> Option<f64>) {
        if matches!(
            kind,
            MouseEventKind::LeftMouseDown | MouseEventKind::LeftMouseDragged
        ) {
            self.set(read());
        }
    }
}

static MAIN_SCREEN_TOP: MainScreenTop = MainScreenTop::unknown();

/// `NSScreen.main.frame.screenFlipped.minY`; вне главного потока — `None`.
/// Дёшево (без списка экранов целиком): зовётся на каждом перетаскивании.
fn current_main_screen_top() -> Option<f64> {
    let mtm = MainThreadMarker::new()?;
    let primary = NSScreen::screens(mtm).firstObject()?;
    let main = NSScreen::mainScreen(mtm)?;
    let (primary, main) = (primary.frame(), main.frame());
    Some((primary.origin.y + primary.size.height) - (main.origin.y + main.size.height))
}

fn update_main_screen_top() {
    MAIN_SCREEN_TOP.set(current_main_screen_top());
}

fn main_screen_top() -> Option<f64> {
    MAIN_SCREEN_TOP.get()
}

/// Освежать верх главного экрана при смене экранов и активного приложения.
fn watch_main_screen(refresh: fn()) {
    events::on_screens_changed(refresh);
    events::on_front_app_changed(move |_current, _previous| refresh());
}

/// Фильтр активного монитора (`SnappingManager.filter`): защита от Mission
/// Control. Событий не поглощает.
fn mission_control_filter() -> TapFilter {
    let guard = Mutex::new(MissionControlGuard::default());
    Arc::new(move |event: &mut TapEvent| {
        let kind = event.kind();
        if !matches!(
            kind,
            MouseEventKind::LeftMouseDragged | MouseEventKind::LeftMouseUp
        ) {
            return false;
        }
        let (x, y) = event.location();
        let (distance, duration) = config::with(|config| {
            (
                config.mission_control_dragging_allowed_offscreen_distance as f64,
                config.mission_control_dragging_disallowed_duration,
            )
        });
        let moved = guard
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .filter(
                kind,
                y,
                event.delta_y(),
                main_screen_top(),
                distance,
                duration,
                event_monitor::uptime_milliseconds(),
            );
        if let Some(new_y) = moved {
            event.set_location(x, new_y);
        }
        false
    })
}

// ---------------------------------------------------------------- менеджер

struct Manager {
    mtm: MainThreadMarker,
    monitor: Option<Monitor>,
    /// `box`: создаётся, когда прилипание включают, и уходит, когда выключают.
    footprint: Option<Footprint>,
    drag: DragState<AxElement>,
    /// Приложение впереди не выключает прилипание (`allowListening`).
    allowed_for_front_app: bool,
    /// Переднее окно во весь экран (`isFullScreen`).
    front_window_full_screen: bool,
}

impl Manager {
    /// `toggleListening` + `reloadFromDefaults`: включить или выключить
    /// слежение за мышью и сменить вид монитора, если его поменяли в настройках.
    fn apply(&mut self) {
        let paused = PAUSES.with(Cell::get) > 0;
        let (listen, active, style) = config::with(|config| {
            (
                zones::should_listen(
                    self.allowed_for_front_app,
                    self.front_window_full_screen,
                    paused,
                    config,
                ),
                zones::uses_active_monitor(config),
                FootprintStyle::from_config(config),
            )
        });
        if !listen {
            self.disable();
            return;
        }
        if self.footprint.is_none() {
            self.footprint = Some(Footprint::new(self.mtm, style));
        }
        let as_wanted = self
            .monitor
            .as_ref()
            .is_some_and(|monitor| monitor.is_active() == active && monitor.running());
        if as_wanted {
            return;
        }
        if self.monitor.take().is_some() && self.drag.reset() {
            if let Some(footprint) = &self.footprint {
                footprint.order_out();
            }
        }
        if active {
            update_main_screen_top();
        }
        let monitor = Monitor::start(active);
        log!(
            "Прилипание: слушаем мышь ({}){}",
            if active {
                "активный перехват"
            } else {
                "глобальный монитор"
            },
            if monitor.running() {
                ""
            } else {
                " — монитор не запустился"
            }
        );
        self.monitor = Some(monitor);
    }

    /// `disableSnapping`.
    fn disable(&mut self) {
        if self.monitor.is_none() && self.footprint.is_none() {
            return;
        }
        self.monitor = None;
        self.footprint = None;
        self.drag.reset();
        log!("Прилипание: мышь не слушаем");
    }
}

thread_local! {
    static MANAGER: RefCell<Option<Manager>> = const { RefCell::new(None) };
    static SESSION_OBSERVER: RefCell<Option<Retained<SessionObserver>>> = const { RefCell::new(None) };
    /// Сколько пауз сейчас действует (`pause` без парного `resume`).
    static PAUSES: Cell<u32> = const { Cell::new(0) };
}

/// Выполнить с менеджером; если он сейчас занят (вызов изнутри его же
/// обработчика), — на следующем обороте цикла событий.
fn with_manager(f: impl FnOnce(&mut Manager) + 'static) {
    let busy = MANAGER.with(|slot| match slot.try_borrow_mut() {
        Ok(mut manager) => {
            if let Some(manager) = manager.as_mut() {
                f(manager);
            }
            None
        }
        Err(_) => Some(f),
    });
    if let Some(f) = busy {
        events::run_later(move || with_manager(f));
    }
}

/// Мышиное событие от монитора (`handle(event:)`), главный поток.
fn handle_mouse_event(event: &MouseEvent) {
    MANAGER.with(|slot| {
        let Ok(mut guard) = slot.try_borrow_mut() else {
            return;
        };
        let Some(manager) = guard.as_mut() else {
            return;
        };
        // События, дошедшие после остановки монитора, не нужны.
        let Some(monitor) = &manager.monitor else {
            return;
        };
        if monitor.is_active() {
            MAIN_SCREEN_TOP.refresh_on(event.kind, current_main_screen_top);
        }
        let Manager {
            drag,
            footprint,
            mtm,
            ..
        } = manager;
        let mut system = RealSystem {
            footprint,
            mtm: *mtm,
        };
        config::with(|config| drag.handle(&mut system, event, config));
    });
}

/// Приложение впереди не выключает прилипание.
fn allowed_for_front_app() -> bool {
    let front = events::front_app();
    config::with(|config| {
        zones::drag_snap_allowed_for(
            front.as_ref().and_then(|app| app.bundle_id.as_deref()),
            config,
        )
    })
}

/// `checkFullScreen`: переднее окно во весь экран.
fn front_window_full_screen() -> bool {
    ax::front_window().and_then(|window| window.is_full_screen()) == Some(true)
}

fn check_full_screen() {
    with_manager(|manager| {
        manager.front_window_full_screen = front_window_full_screen();
        manager.apply();
    });
}

/// `frontAppChanged`: приложение из «Игнорировать» выключает прилипание,
/// любое другое — включает (и проверяется полный экран).
fn front_app_changed() {
    with_manager(|manager| {
        manager.allowed_for_front_app = allowed_for_front_app();
        manager.front_window_full_screen = front_window_full_screen();
        manager.apply();
    });
}

define_class!(
    /// Возврат в сессию (`NSWorkspace.sessionDidBecomeActiveNotification`):
    /// проверить полный экран заново.
    #[unsafe(super(NSObject))]
    #[name = "R2SnappingSessionObserver"]
    #[thread_kind = MainThreadOnly]
    #[ivars = ()]
    struct SessionObserver;

    impl SessionObserver {
        #[unsafe(method(sessionDidBecomeActive:))]
        fn session_did_become_active(&self, _notification: &NSNotification) {
            check_full_screen();
        }
    }
);

fn install_session_observer(mtm: MainThreadMarker) {
    let observer: Retained<SessionObserver> =
        unsafe { msg_send![SessionObserver::alloc(mtm), init] };
    // SAFETY: у наблюдателя есть этот селектор, живёт он до конца работы (в
    // SESSION_OBSERVER).
    unsafe {
        NSWorkspace::sharedWorkspace()
            .notificationCenter()
            .addObserver_selector_name_object(
                &observer,
                sel!(sessionDidBecomeActive:),
                Some(NSWorkspaceSessionDidBecomeActiveNotification),
                None,
            );
    }
    SESSION_OBSERVER.with(|slot| *slot.borrow_mut() = Some(observer));
}

/// Запустить прилипание (строка в `subsystems::STARTS`). Повторный вызов
/// ничего не делает.
pub fn install(mtm: MainThreadMarker) {
    if MANAGER.with(|slot| slot.borrow().is_some()) {
        return;
    }
    let manager = Manager {
        mtm,
        monitor: None,
        footprint: None,
        drag: DragState::default(),
        allowed_for_front_app: allowed_for_front_app(),
        front_window_full_screen: front_window_full_screen(),
    };
    MANAGER.with(|slot| *slot.borrow_mut() = Some(manager));
    with_manager(Manager::apply);

    events::on_front_app_changed(|_current, _previous| front_app_changed());
    events::on_space_changed(check_full_screen);
    watch_main_screen(update_main_screen_top);
    install_session_observer(mtm);

    // Как в конце `accessibilityTrusted()` оригинала — когда остальные
    // подсистемы уже запущены. Алерты модальные: из таймера цикла событий, а не
    // из блока главной очереди, иначе, пока алерт на экране, стоят обработчики
    // мышиных мониторов (прилипание, зелёная кнопка).
    events::run_in_run_loop(|| {
        problem_apps::check_for_problematic_apps();
        mac_tiling::check_for_built_in_tiling(true);
    });
}

/// Настройки изменились (строка в `subsystems::RELOADS`): «Игнорировать»,
/// выключатель прилипания, вид монитора.
pub fn reload() {
    with_manager(|manager| {
        manager.allowed_for_front_app = allowed_for_front_app();
        manager.apply();
    });
}

/// Приостановить прилипание на время модального диалога — как пост
/// `windowSnapping(false)` у оригинала перед панелями импорта и экспорта
/// настроек: мышь не слушается, настройка `windowSnapping` не меняется. Паузы
/// вкладываются; слежение вернёт последний парный `resume` (если настройки и
/// приложение впереди его разрешают). Без запущенного прилипания только
/// запоминается.
pub fn pause() {
    PAUSES.with(|pauses| pauses.set(pauses.get() + 1));
    with_manager(Manager::apply);
}

/// Снять паузу `pause` (у оригинала — пост `windowSnapping(true)`).
pub fn resume() {
    PAUSES.with(|pauses| pauses.set(pauses.get().saturating_sub(1)));
    with_manager(Manager::apply);
}

/// Выполнить `f` (модальный диалог) с прилипанием на паузе.
pub fn paused<R>(f: impl FnOnce() -> R) -> R {
    pause();
    let result = f();
    resume();
    result
}

/// Сейчас прилипание слушает мышь.
pub fn is_listening() -> bool {
    MANAGER.with(|slot| {
        slot.try_borrow()
            .ok()
            .and_then(|manager| manager.as_ref().map(|manager| manager.monitor.is_some()))
            .unwrap_or(false)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn is_paused() -> bool {
        PAUSES.with(Cell::get) > 0
    }

    #[test]
    fn pauses_nest_and_resume_does_not_go_below_zero() {
        // Без запущенного прилипания пауза только запоминается.
        assert!(!is_paused());
        pause();
        pause();
        resume();
        assert!(is_paused());
        resume();
        assert!(!is_paused());
        resume();
        assert!(!is_paused());
        pause();
        assert!(is_paused());
        resume();

        let value = paused(|| {
            assert!(is_paused());
            7
        });
        assert_eq!(value, 7);
        assert!(!is_paused());
    }

    #[test]
    fn main_screen_top_follows_the_main_screen_during_a_drag() {
        // Два дисплея: A — основной (верх в Quartz — 0), B справа и выше (−200).
        let cache = MainScreenTop::unknown();
        assert_eq!(cache.get(), None);
        let main_top = Cell::new(0.0);
        cache.refresh_on(MouseEventKind::LeftMouseDown, || Some(main_top.get()));
        assert_eq!(cache.get(), Some(0.0));
        // Схватили окно на B: главным B стал уже после обработчика нажатия.
        main_top.set(-200.0);
        cache.refresh_on(MouseEventKind::LeftMouseDragged, || Some(main_top.get()));
        assert_eq!(
            cache.get(),
            Some(-200.0),
            "первое же перетаскивание видит B"
        );
        // Отпускание кэш не трогает.
        cache.refresh_on(MouseEventKind::LeftMouseUp, || None);
        assert_eq!(cache.get(), Some(-200.0));

        // Защита сверяет с верхом B: рывок за верхний край B опускается на пиксель.
        let mut guard = MissionControlGuard::default();
        let mut drag = |y: f64, delta_y: f64| {
            guard.filter(
                MouseEventKind::LeftMouseDragged,
                y,
                delta_y,
                cache.get(),
                30.0,
                250,
                1_000,
            )
        };
        assert_eq!(drag(-200.0, -3.0), None);
        assert_eq!(drag(-200.0, -40.0), Some(-199.0));
    }

    #[test]
    fn main_screen_top_is_refreshed_on_screen_and_front_app_changes() {
        thread_local! {
            static REFRESHES: Cell<u32> = const { Cell::new(0) };
        }
        fn refresh() {
            REFRESHES.with(|count| count.set(count.get() + 1));
        }
        // Шина — своя у потока теста.
        watch_main_screen(refresh);
        events::emit(&events::Event::ScreensChanged);
        let app = events::AppInfo {
            pid: 42,
            bundle_id: None,
            name: None,
        };
        events::emit(&events::Event::FrontAppChanged {
            current: &app,
            previous: None,
        });
        events::emit(&events::Event::SpaceChanged);
        assert_eq!(REFRESHES.with(Cell::get), 2);
    }
}
