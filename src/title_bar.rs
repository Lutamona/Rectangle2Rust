//! Двойной клик по заголовку окна — порт `TitleBarManager.swift`.
//!
//! Пассивный монитор (`event_monitor::PassiveEventMonitor`) слушает отпускание
//! левой кнопки. Отпускание второго щелчка (`clickCount == 2`) над заголовком
//! окна выполняет над этим окном действие из настройки `doubleClickTitleBar`
//! (rawValue действия + 1; галочка в настройках пишет «Развернуть»). Если окно
//! стоит ровно там, куда его последним поставило это же действие, двойной клик
//! возвращает прежнюю рамку («Восстановить») — пока не выключен
//! `doubleClickTitleBarRestore`.
//!
//! Заголовок — полоса по кнопке закрытия (`AxElement::title_bar_frame`) вместе
//! с панелью инструментов окна (кроме приложений из
//! `Config::double_click_tool_bar_ignored_apps`, по умолчанию — Eclipse), а
//! элемент под курсором — само окно, панель инструментов, группа, вкладки или
//! текст, но не кнопка или поле. Двойной клик не трогаем, если в macOS он
//! чем-то занят (`AppleActionOnDoubleClick` ≠ «None»: иначе окно развернёт или
//! свернёт сама система) и если приложение окна — в
//! `doubleClickTitleBarIgnoredApps` или (решение D4) в «Игнорировать» — с
//! `ignoreDragSnapToo`, как у прилипания.
//!
//! Мышь слушается, только пока действие задано; настройки перечитываются на лету
//! (`reload`). Решение принимает `TitleBarState::handle` над `TitleBarSystem`:
//! настоящая система — `AxTitleBarSystem`, в тестах — подставная, в примере
//! `examples/title_bar_check.rs` — система, которая видит только окна помощника.

use std::cell::RefCell;

use objc2::MainThreadMarker;
use objc2_app_kit::NSRunningApplication;

use crate::actions::Action;
use crate::ax::{self, AxElement};
use crate::calc::LastAction;
use crate::config::{self, Config};
use crate::defaults_store::{Store, UserDefaultsStore};
use crate::event_monitor::{EventMask, MouseEvent, MouseEventKind, PassiveEventMonitor};
use crate::geometry::Rect;
use crate::snapping::zones;
use crate::window_manager::{self, ExecutionParameters};
use crate::{events, log, screens, window_history};

/// Роли элемента под курсором, при которых двойной клик считается кликом по
/// заголовку (`isWindow`, `isToolbar`, `isGroup`, `isTabGroup`, `isStaticText`).
const TITLE_BAR_ROLES: [&str; 5] = [
    "AXWindow",
    "AXToolbar",
    "AXGroup",
    "AXTabGroup",
    "AXStaticText",
];

// ---------------------------------------------------------------- решение

/// Действие двойного клика: `WindowAction(rawValue: doubleClickTitleBar - 1)`.
/// `None` — двойной клик выключен (0) или число — не действие.
pub fn configured_action(config: &Config) -> Option<Action> {
    let raw = config.double_click_title_bar.checked_sub(1)?;
    Action::from_raw(i32::try_from(raw).ok()?)
}

/// Двойной клик по заголовкам окон этого приложения не трогаем: оно в
/// `doubleClickTitleBarIgnoredApps` или (решение D4) «Игнорировать» выключает
/// для него двойной клик по тому же правилу, что и прилипание
/// (`snapping::zones::drag_snap_allowed_for`): если `ignoreDragSnapToo` снят
/// явно, игнор выключает оба только у приложений из `fullIgnoreBundleIds`.
pub fn is_ignored_app(bundle_id: &str, config: &Config) -> bool {
    config
        .double_click_title_bar_ignored_apps
        .as_ref()
        .is_some_and(|apps| apps.iter().any(|app| app == bundle_id))
        || !zones::drag_snap_allowed_for(Some(bundle_id), config)
}

/// Окно стоит ровно там, куда его последним поставил Rectangle этим же
/// действием: повтор возвращает прежнюю рамку. Так решают и двойной клик по
/// заголовку, и зелёная кнопка.
pub fn repeats_last_action(action: Action, frame: Option<Rect>, last: Option<&LastAction>) -> bool {
    match (frame, last) {
        (Some(frame), Some(last)) => last.action == action && last.rect == frame,
        _ => false,
    }
}

/// `CGRect.contains(CGPoint)`.
fn contains(rect: &Rect, (x, y): (f64, f64)) -> bool {
    ax::rect_contains_point(rect, x, y)
}

/// Всё, что обработчику двойного клика нужно от системы.
pub trait TitleBarSystem {
    type Element;

    /// `TitleBarManager.systemSettingDisabled`.
    fn system_setting_disabled(&mut self) -> bool;
    /// `NSEvent.mouseLocation.screenFlipped` — координаты AX.
    fn mouse_location(&mut self) -> Option<(f64, f64)>;
    /// `AccessibilityElement(location)?.getSelfOrChildElementRecursively(location)`.
    fn element_at(&mut self, location: (f64, f64)) -> Option<Self::Element>;
    /// `windowElement`: само окно или окно, которому принадлежит элемент.
    fn window_element(&mut self, element: &Self::Element) -> Option<Self::Element>;
    /// `titleBarFrame` окна, AX.
    fn title_bar_frame(&mut self, window: &Self::Element) -> Option<Rect>;
    /// `getChildElement(.toolbar)?.frame` окна, AX.
    fn toolbar_frame(&mut self, window: &Self::Element) -> Option<Rect>;
    /// `AXRole` элемента.
    fn role(&mut self, element: &Self::Element) -> Option<String>;
    /// Bundle id приложения элемента (`NSRunningApplication(processIdentifier: pid)`).
    fn bundle_id(&mut self, element: &Self::Element) -> Option<String>;
    /// `windowId` — номер окна без запасных путей `getWindowId()`.
    fn window_id(&mut self, window: &Self::Element) -> Option<u32>;
    /// `frame` окна, AX.
    fn frame(&mut self, window: &Self::Element) -> Option<Rect>;
    /// `windowHistory.lastRectangleActions[windowId]`.
    fn last_action(&mut self, window_id: u32) -> Option<LastAction>;
}

/// Состояние обработчика (`lastEventNumber`).
#[derive(Debug, Default)]
pub struct TitleBarState {
    /// Отпускание, которое уже разобрано: одно событие не обрабатывается дважды.
    /// `MouseEvent` не несёт `eventNumber`, но то же событие узнаётся и по
    /// времени (`timestamp`).
    last_event: Option<f64>,
}

impl TitleBarState {
    /// `handle(_ event:)`: что выполнить и над каким окном (`postTitleBar`);
    /// `None` — ничего.
    pub fn handle<S: TitleBarSystem>(
        &mut self,
        system: &mut S,
        event: &MouseEvent,
        config: &Config,
    ) -> Option<(Action, S::Element)> {
        if event.kind != MouseEventKind::LeftMouseUp
            || event.click_count != 2
            || self.last_event == Some(event.timestamp)
            || !system.system_setting_disabled()
        {
            return None;
        }
        let action = configured_action(config)?;
        let location = system.mouse_location()?;
        let element = system.element_at(location)?;
        let window = system.window_element(&element)?;
        let mut title_bar = system.title_bar_frame(&window)?;
        self.last_event = Some(event.timestamp);

        let bundle_id = system.bundle_id(&element);
        if let Some(toolbar) = system.toolbar_frame(&window) {
            let toolbar_ignored = bundle_id
                .as_deref()
                .is_some_and(|id| config.double_click_tool_bar_ignored_apps().contains(id));
            if !toolbar_ignored {
                title_bar = title_bar.union(&toolbar);
            }
        }
        let on_title_bar = contains(&title_bar, location)
            && system
                .role(&element)
                .is_some_and(|role| TITLE_BAR_ROLES.contains(&role.as_str()));
        if !on_title_bar {
            return None;
        }
        if bundle_id
            .as_deref()
            .is_some_and(|id| is_ignored_app(id, config))
        {
            return None;
        }
        if config.double_click_title_bar_restore != Some(false) {
            if let Some(window_id) = system.window_id(&window) {
                let frame = system.frame(&window);
                if repeats_last_action(action, frame, system.last_action(window_id).as_ref()) {
                    return Some((Action::Restore, window));
                }
            }
        }
        Some((action, window))
    }
}

// ---------------------------------------------------------------- система

/// `TitleBarManager.systemSettingDisabled`: в macOS двойной клик по заголовку
/// окна ничего не делает («Рабочий стол и Dock» → «Не выполнять действие»,
/// `AppleActionOnDoubleClick = None`). Окну настроек — для предупреждения о
/// конфликте, когда включают галочку.
pub fn system_setting_disabled() -> bool {
    system_double_click_action().as_deref() == Some("None")
}

/// Системная настройка «Двойной щелчок по заголовку окна» (`AppleActionOnDoubleClick`
/// в `.GlobalPreferences`); `None` — не задана (macOS разворачивает окно сама).
pub fn system_double_click_action() -> Option<String> {
    UserDefaultsStore::suite(".GlobalPreferences")?.string("AppleActionOnDoubleClick")
}

fn bundle_id_of(pid: i32) -> Option<String> {
    NSRunningApplication::runningApplicationWithProcessIdentifier(pid)?
        .bundleIdentifier()
        .map(|bundle_id| bundle_id.to_string())
}

/// Настоящая система: AX, курсор, история окон. Главный поток.
pub struct AxTitleBarSystem;

impl TitleBarSystem for AxTitleBarSystem {
    type Element = AxElement;

    fn system_setting_disabled(&mut self) -> bool {
        system_setting_disabled()
    }

    fn mouse_location(&mut self) -> Option<(f64, f64)> {
        let (x, y) = screens::cursor_position()?;
        Some((x, screens::primary_screen_height() - y))
    }

    fn element_at(&mut self, (x, y): (f64, f64)) -> Option<AxElement> {
        Some(ax::element_at_position(x, y)?.self_or_child_at(x, y))
    }

    fn window_element(&mut self, element: &AxElement) -> Option<AxElement> {
        element.window_element()
    }

    fn title_bar_frame(&mut self, window: &AxElement) -> Option<Rect> {
        window.title_bar_frame()
    }

    fn toolbar_frame(&mut self, window: &AxElement) -> Option<Rect> {
        window.child_with_role("AXToolbar")?.frame()
    }

    fn role(&mut self, element: &AxElement) -> Option<String> {
        element.role()
    }

    fn bundle_id(&mut self, element: &AxElement) -> Option<String> {
        bundle_id_of(element.pid()?)
    }

    fn window_id(&mut self, window: &AxElement) -> Option<u32> {
        window.window_id()
    }

    fn frame(&mut self, window: &AxElement) -> Option<Rect> {
        window.frame()
    }

    fn last_action(&mut self, window_id: u32) -> Option<LastAction> {
        window_history::last_action(window_id)
    }
}

/// `postTitleBar(windowElement:)`: выполнить действие над окном заголовка.
pub fn execute(action: Action, window: AxElement) {
    log!("Двойной клик по заголовку: {}", action.name());
    window_manager::execute(ExecutionParameters::title_bar(action, Some(window)));
}

// ---------------------------------------------------------------- менеджер

#[derive(Default)]
struct Manager {
    monitor: Option<PassiveEventMonitor>,
    state: TitleBarState,
}

impl Manager {
    /// `toggleListening`: слушать мышь, только пока действие задано.
    fn apply(&mut self) {
        let listen = config::with(|config| configured_action(config).is_some());
        if listen == self.monitor.is_some() {
            return;
        }
        if listen {
            let mut monitor = PassiveEventMonitor::new(EventMask::LEFT_MOUSE_UP, handle_mouse_up);
            monitor.start();
            self.monitor = Some(monitor);
            log!("Двойной клик по заголовку: слушаем мышь");
        } else {
            self.monitor = None;
            log!("Двойной клик по заголовку: мышь не слушаем");
        }
    }
}

thread_local! {
    static MANAGER: RefCell<Option<Manager>> = const { RefCell::new(None) };
}

/// Выполнить с менеджером; если он сейчас занят, — на следующем обороте цикла
/// событий.
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

/// Отпускание левой кнопки (`handle(_ event:)`), главный поток. Действие
/// выполняется, когда менеджер уже отпущен.
fn handle_mouse_up(event: &MouseEvent) {
    let click = MANAGER.with(|slot| {
        let mut guard = slot.try_borrow_mut().ok()?;
        let manager = guard.as_mut()?;
        config::with(|config| manager.state.handle(&mut AxTitleBarSystem, event, config))
    });
    if let Some((action, window)) = click {
        execute(action, window);
    }
}

/// Запустить (строка в `subsystems::STARTS`). Повторный вызов ничего не делает.
pub fn install(_mtm: MainThreadMarker) {
    if MANAGER.with(|slot| slot.borrow().is_some()) {
        return;
    }
    MANAGER.with(|slot| *slot.borrow_mut() = Some(Manager::default()));
    with_manager(Manager::apply);
}

/// Настройки изменились (строка в `subsystems::RELOADS`): действие двойного
/// клика задали или сняли.
pub fn reload() {
    with_manager(Manager::apply);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// Окно 7 (AX): заголовок 28 px по кнопке закрытия, под ним панель
    /// инструментов 52 px; дальше содержимое.
    const WINDOW: Rect = Rect {
        x: 100.0,
        y: 200.0,
        w: 800.0,
        h: 600.0,
    };
    const TITLE_BAR: Rect = Rect {
        x: 100.0,
        y: 200.0,
        w: 800.0,
        h: 28.0,
    };
    const TOOLBAR: Rect = Rect {
        x: 100.0,
        y: 228.0,
        w: 800.0,
        h: 52.0,
    };
    const IN_TITLE: (f64, f64) = (500.0, 210.0);
    const IN_TOOLBAR: (f64, f64) = (500.0, 250.0);
    const IN_CONTENT: (f64, f64) = (500.0, 500.0);

    /// Элемент подставной системы: номер и роль.
    #[derive(Clone, Debug, PartialEq)]
    struct Element {
        id: u32,
        role: &'static str,
    }

    fn element(id: u32, role: &'static str) -> Element {
        Element { id, role }
    }

    /// Подставная система: одно окно 7 и то, что лежит в точках.
    struct Fake {
        setting_disabled: bool,
        mouse: (f64, f64),
        elements: HashMap<(i64, i64), Element>,
        toolbar: Option<Rect>,
        bundle_id: Option<String>,
        window_id: Option<u32>,
        frame: Option<Rect>,
        history: HashMap<u32, LastAction>,
    }

    impl Fake {
        fn new() -> Fake {
            let key = |(x, y): (f64, f64)| (x as i64, y as i64);
            Fake {
                setting_disabled: true,
                mouse: IN_TITLE,
                elements: HashMap::from([
                    (key(IN_TITLE), element(7, "AXWindow")),
                    (key(IN_TOOLBAR), element(8, "AXToolbar")),
                    (key(IN_CONTENT), element(9, "AXGroup")),
                ]),
                toolbar: Some(TOOLBAR),
                bundle_id: Some("com.apple.finder".to_string()),
                window_id: Some(7),
                frame: Some(WINDOW),
                history: HashMap::new(),
            }
        }

        fn put(&mut self, location: (f64, f64), role: &'static str) {
            self.elements
                .insert((location.0 as i64, location.1 as i64), element(20, role));
        }

        fn remember(&mut self, action: Action, rect: Rect) {
            self.history.insert(
                7,
                LastAction {
                    action,
                    sub_action: None,
                    rect,
                    count: 1,
                },
            );
        }
    }

    impl TitleBarSystem for Fake {
        type Element = Element;

        fn system_setting_disabled(&mut self) -> bool {
            self.setting_disabled
        }
        fn mouse_location(&mut self) -> Option<(f64, f64)> {
            Some(self.mouse)
        }
        fn element_at(&mut self, (x, y): (f64, f64)) -> Option<Element> {
            self.elements.get(&(x as i64, y as i64)).cloned()
        }
        fn window_element(&mut self, _element: &Element) -> Option<Element> {
            Some(element(7, "AXWindow"))
        }
        fn title_bar_frame(&mut self, window: &Element) -> Option<Rect> {
            assert_eq!(window.id, 7);
            Some(TITLE_BAR)
        }
        fn toolbar_frame(&mut self, _window: &Element) -> Option<Rect> {
            self.toolbar
        }
        fn role(&mut self, element: &Element) -> Option<String> {
            Some(element.role.to_string())
        }
        fn bundle_id(&mut self, _element: &Element) -> Option<String> {
            self.bundle_id.clone()
        }
        fn window_id(&mut self, _window: &Element) -> Option<u32> {
            self.window_id
        }
        fn frame(&mut self, _window: &Element) -> Option<Rect> {
            self.frame
        }
        fn last_action(&mut self, window_id: u32) -> Option<LastAction> {
            self.history.get(&window_id).copied()
        }
    }

    fn mouse_up(timestamp: f64, click_count: i64) -> MouseEvent {
        MouseEvent {
            kind: MouseEventKind::LeftMouseUp,
            timestamp,
            modifier_flags: 0,
            location: Some(IN_TITLE),
            delta_y: 0.0,
            click_count,
        }
    }

    /// Двойной клик «Развернуть» (как пишет галочка в настройках).
    fn maximize_config() -> Config {
        Config {
            double_click_title_bar: Action::Maximize.raw() as i64 + 1,
            ..Config::default()
        }
    }

    /// Прогон: подставная система, состояние, настройки, часы событий.
    struct Run {
        fake: Fake,
        state: TitleBarState,
        config: Config,
        time: f64,
    }

    impl Run {
        fn new() -> Run {
            Run {
                fake: Fake::new(),
                state: TitleBarState::default(),
                config: maximize_config(),
                time: 100.0,
            }
        }

        /// Двойной клик в точке: что выполнилось бы.
        fn double_click_at(&mut self, location: (f64, f64)) -> Option<Action> {
            self.fake.mouse = location;
            self.time += 1.0;
            let event = mouse_up(self.time, 2);
            self.state
                .handle(&mut self.fake, &event, &self.config)
                .map(|(action, window)| {
                    assert_eq!(window, element(7, "AXWindow"), "действие — над окном");
                    action
                })
        }

        fn double_click(&mut self) -> Option<Action> {
            self.double_click_at(IN_TITLE)
        }
    }

    #[test]
    fn action_is_raw_value_plus_one() {
        let with = |value: i64| {
            configured_action(&Config {
                double_click_title_bar: value,
                ..Config::default()
            })
        };
        assert_eq!(with(0), None, "0 — выключено");
        assert_eq!(with(3), Some(Action::Maximize), "галочка пишет 3");
        assert_eq!(with(1), Some(Action::LeftHalf));
        assert_eq!(with(20), Some(Action::Restore));
        assert_eq!(with(121), Some(Action::Display(1)));
        assert_eq!(with(7), None, "6 — не действие");
        assert_eq!(with(-4), None);
        assert_eq!(with(i64::MIN), None);
        assert_eq!(with(i64::MAX), None);
        assert_eq!(Config::default().double_click_title_bar, 0);

        // Галочка в окне настроек включена ровно тогда, когда мышь слушается.
        for value in -2..=200 {
            assert_eq!(
                with(value).is_some(),
                crate::config::is_swift_window_action_raw(value - 1),
                "{value}"
            );
        }
    }

    #[test]
    fn double_click_on_title_bar_runs_the_action_on_that_window() {
        let mut run = Run::new();
        assert_eq!(run.double_click(), Some(Action::Maximize));

        run.config.double_click_title_bar = Action::LeftHalf.raw() as i64 + 1;
        assert_eq!(run.double_click(), Some(Action::LeftHalf));
    }

    #[test]
    fn only_the_release_of_a_double_click_counts() {
        let mut run = Run::new();
        for click_count in [0, 1, 3, 4] {
            let event = mouse_up(1.0 + click_count as f64, click_count);
            assert!(run
                .state
                .handle(&mut run.fake, &event, &run.config)
                .is_none());
        }
        let down = MouseEvent {
            kind: MouseEventKind::LeftMouseDown,
            ..mouse_up(10.0, 2)
        };
        assert!(run
            .state
            .handle(&mut run.fake, &down, &run.config)
            .is_none());
        assert_eq!(run.double_click(), Some(Action::Maximize));
    }

    #[test]
    fn the_same_event_is_handled_once() {
        let mut run = Run::new();
        let event = mouse_up(5.0, 2);
        assert!(run
            .state
            .handle(&mut run.fake, &event, &run.config)
            .is_some());
        assert!(run
            .state
            .handle(&mut run.fake, &event, &run.config)
            .is_none());
        let next = mouse_up(5.5, 2);
        assert!(run
            .state
            .handle(&mut run.fake, &next, &run.config)
            .is_some());
    }

    #[test]
    fn nothing_happens_while_macos_owns_the_double_click() {
        let mut run = Run::new();
        run.fake.setting_disabled = false;
        assert_eq!(run.double_click(), None);
        // Событие, которое не разобрали, не считается разобранным.
        run.fake.setting_disabled = true;
        let event = mouse_up(run.time, 2);
        assert!(run
            .state
            .handle(&mut run.fake, &event, &run.config)
            .is_some());
    }

    #[test]
    fn nothing_happens_without_an_action() {
        let mut run = Run::new();
        run.config.double_click_title_bar = 0;
        assert_eq!(run.double_click(), None);
    }

    #[test]
    fn title_bar_includes_the_toolbar() {
        let mut run = Run::new();
        assert_eq!(run.double_click_at(IN_TOOLBAR), Some(Action::Maximize));
        assert_eq!(run.double_click_at(IN_CONTENT), None, "содержимое окна");

        run.fake.toolbar = None;
        assert_eq!(run.double_click_at(IN_TOOLBAR), None, "панели нет");
    }

    #[test]
    fn toolbar_is_skipped_for_toolbar_ignored_apps() {
        let mut run = Run::new();
        // По умолчанию панель инструментов не считается заголовком у Eclipse.
        run.fake.bundle_id = Some("epp.package.java".to_string());
        assert_eq!(run.double_click_at(IN_TOOLBAR), None);
        assert_eq!(run.double_click(), Some(Action::Maximize), "сам заголовок");

        // Тот же ключ, что у списка игнорируемых: заданный список заменяет Eclipse.
        run.config.double_click_title_bar_ignored_apps = Some(Vec::new());
        assert_eq!(run.double_click_at(IN_TOOLBAR), Some(Action::Maximize));
    }

    #[test]
    fn element_under_cursor_must_be_part_of_the_title_bar() {
        let mut run = Run::new();
        for role in [
            "AXWindow",
            "AXToolbar",
            "AXGroup",
            "AXTabGroup",
            "AXStaticText",
        ] {
            run.fake.put(IN_TITLE, role);
            assert_eq!(run.double_click(), Some(Action::Maximize), "{role}");
        }
        for role in ["AXButton", "AXTextField", "AXPopUpButton", "AXImage"] {
            run.fake.put(IN_TITLE, role);
            assert_eq!(run.double_click(), None, "{role}");
        }
    }

    #[test]
    fn ignored_apps_are_left_alone() {
        let mut run = Run::new();
        run.config.double_click_title_bar_ignored_apps = Some(vec!["com.apple.finder".to_string()]);
        assert_eq!(run.double_click(), None);

        // Решение D4: «Игнорировать <приложение>» выключает и двойной клик.
        let mut run = Run::new();
        run.config.disabled_apps = Some(["com.apple.finder".to_string()].into());
        assert_eq!(run.double_click(), None);
        run.config.disabled_apps = Some(["com.apple.Terminal".to_string()].into());
        assert_eq!(run.double_click(), Some(Action::Maximize));

        // Приложение без bundle id не игнорируется.
        run.fake.bundle_id = None;
        run.config.disabled_apps = Some(["".to_string()].into());
        assert_eq!(run.double_click(), Some(Action::Maximize));
    }

    #[test]
    fn ignore_follows_ignore_drag_snap_too_like_snapping() {
        let mut run = Run::new();
        run.config.disabled_apps = Some(["com.apple.finder".to_string()].into());
        run.config.ignore_drag_snap_too = Some(true);
        assert_eq!(run.double_click(), None);
        // Сняли «выключать и прилипание» явно — игнор не выключает ни
        // прилипание, ни двойной клик.
        run.config.ignore_drag_snap_too = Some(false);
        assert_eq!(run.double_click(), Some(Action::Maximize));
        // Кроме приложений, у которых прилипание выключается всегда
        // (`fullIgnoreBundleIds`, сравнение по началу).
        run.config.full_ignore_bundle_ids = Some(vec!["com.apple.fin".to_string()]);
        assert_eq!(run.double_click(), None);
        // Свой список двойного клика действует независимо от игнора.
        run.config.full_ignore_bundle_ids = None;
        run.config.double_click_title_bar_ignored_apps = Some(vec!["com.apple.finder".to_string()]);
        assert_eq!(run.double_click(), None);
    }

    #[test]
    fn double_click_and_snapping_share_the_ignore_rule() {
        use crate::snapping::zones::drag_snap_allowed_for;
        let ids = [
            "com.apple.finder",
            "com.apple.Safari",
            "com.mathworks.matlab",
            "com.install4j.1234",
        ];
        for ignore_drag_snap_too in [None, Some(true), Some(false)] {
            for full_ignore in [None, Some(vec!["com.apple.Saf".to_string()])] {
                let config = Config {
                    disabled_apps: Some(
                        [
                            "com.apple.Safari",
                            "com.mathworks.matlab",
                            "com.install4j.1234",
                        ]
                        .map(str::to_string)
                        .into(),
                    ),
                    ignore_drag_snap_too,
                    full_ignore_bundle_ids: full_ignore.clone(),
                    ..Config::default()
                };
                for id in ids {
                    assert_eq!(
                        is_ignored_app(id, &config),
                        !drag_snap_allowed_for(Some(id), &config),
                        "{id} {ignore_drag_snap_too:?} {full_ignore:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn repeating_on_the_placed_window_restores_it() {
        let mut run = Run::new();
        run.fake.remember(Action::Maximize, WINDOW);
        assert_eq!(run.double_click(), Some(Action::Restore));

        // Окно сдвинули после действия — снова «Развернуть».
        run.fake
            .remember(Action::Maximize, WINDOW.offset_by(1.0, 0.0));
        assert_eq!(run.double_click(), Some(Action::Maximize));

        // Последним было другое действие.
        run.fake.remember(Action::LeftHalf, WINDOW);
        assert_eq!(run.double_click(), Some(Action::Maximize));

        // Своё действие — своё «Восстановить».
        run.config.double_click_title_bar = Action::LeftHalf.raw() as i64 + 1;
        assert_eq!(run.double_click(), Some(Action::Restore));
    }

    #[test]
    fn restore_needs_the_setting_window_number_and_frame() {
        let mut run = Run::new();
        run.fake.remember(Action::Maximize, WINDOW);

        run.config.double_click_title_bar_restore = Some(false);
        assert_eq!(run.double_click(), Some(Action::Maximize));
        run.config.double_click_title_bar_restore = Some(true);
        assert_eq!(run.double_click(), Some(Action::Restore));

        run.fake.window_id = None;
        assert_eq!(run.double_click(), Some(Action::Maximize));
        run.fake.window_id = Some(7);
        run.fake.frame = None;
        assert_eq!(run.double_click(), Some(Action::Maximize));
    }

    #[test]
    fn repeat_check_compares_frames_exactly() {
        let last = LastAction {
            action: Action::Maximize,
            sub_action: None,
            rect: WINDOW,
            count: 3,
        };
        assert!(repeats_last_action(
            Action::Maximize,
            Some(WINDOW),
            Some(&last)
        ));
        assert!(!repeats_last_action(
            Action::Maximize,
            Some(WINDOW.with_size(800.0, 599.5)),
            Some(&last)
        ));
        assert!(!repeats_last_action(
            Action::Center,
            Some(WINDOW),
            Some(&last)
        ));
        assert!(!repeats_last_action(Action::Maximize, None, Some(&last)));
        assert!(!repeats_last_action(Action::Maximize, Some(WINDOW), None));
    }

    #[test]
    fn ignored_app_lists() {
        let config = Config {
            double_click_title_bar_ignored_apps: Some(vec!["com.a".to_string()]),
            disabled_apps: Some(["com.b".to_string()].into()),
            ..Config::default()
        };
        assert!(is_ignored_app("com.a", &config));
        assert!(is_ignored_app("com.b", &config));
        assert!(!is_ignored_app("com.c", &config));
        assert!(!is_ignored_app("com.a", &Config::default()));
    }
}
