//! Зелёная кнопка окна разворачивает его вместо полного экрана — порт
//! `GreenButtonManager.swift`.
//!
//! При `greenButtonOverride` активный монитор (`event_monitor::ActiveEventMonitor`,
//! тап сессии в своём потоке) ловит нажатие левой кнопки на зелёной кнопке окна
//! (подроль `AXFullScreenButton`) и глотает его вместе с отпусканием: приложение
//! не видит щелчка и не уходит в полный экран. Отпустили над кнопкой — окно
//! разворачивается («Развернуть»), а если оно стоит ровно там, куда его
//! последним развернул Rectangle, — возвращается прежняя рамка
//! («Восстановить»). Отпустили мимо кнопки — ничего, как у обычной кнопки.
//! С любым модификатором (⌥, ⇧, ⌃, ⌘, fn, даже Caps Lock) щелчок уходит
//! приложению как есть — обычное поведение macOS; меню зелёной кнопки при
//! наведении тоже системное.
//!
//! Решает `GreenButtonState::filter` над `GreenButtonSystem`: настоящая система —
//! `AxGreenButtonSystem`, в тестах — подставная. Два отличия от оригинала:
//! - нажатие мимо кнопки забывает прежнее нажатие на ней. В Swift запомненное
//!   нажатие живёт до первого отпускания, и если отпускание после него до тапа
//!   не дошло (система выключала тап по таймауту), оригинал проглотил бы
//!   отпускание следующего, чужого щелчка;
//! - (сознательное улучшение) AX-вопросы из потока тапа — с таймаутом
//!   `TAP_AX_TIMEOUT` (0,25 с). Фильтр задаёт их на каждое нажатие левой кнопки
//!   по всей системе, а пока он ждёт ответа, WindowServer держит мышь всей
//!   системы. Оригинал спрашивает системный элемент с системным таймаутом
//!   (секунды): щелчок в окно зависшего приложения замораживал мышь до
//!   таймаута — и так на каждом щелчке. Здесь приложение не ответило — событие
//!   уходит дальше без изменений, как обычно в macOS. Как спрашивать с
//!   таймаутом, — `AxGreenButtonSystem` (у события без окна под курсором первый
//!   вопрос — как в оригинале); проверка на зависшем приложении —
//!   `examples/green_button_hang_check.rs`, сверка с оригиналом на живых окнах —
//!   `examples/green_button_hit_check.rs`, номер окна в настоящих нажатиях —
//!   `examples/green_button_tap_field_check.rs`.

use std::cell::RefCell;
use std::ffi::c_void;
use std::ptr;
use std::sync::{Arc, Mutex};

use core_foundation_sys::base::CFRelease;
use objc2::MainThreadMarker;

use crate::actions::Action;
use crate::ax::{self, AxElement};
use crate::calc::LastAction;
use crate::config;
use crate::event_monitor::{
    ActiveEventMonitor, EventMask, MouseEvent, MouseEventKind, TapEvent, TapFilter, TapHandler,
};
use crate::geometry::Rect;
use crate::title_bar::repeats_last_action;
use crate::window_manager::{self, ExecutionParameters};
use crate::{events, log, window_history};

/// Какие события перехватываются: `[.leftMouseDown, .leftMouseUp]`.
pub const MASK: EventMask = EventMask(EventMask::LEFT_MOUSE_DOWN.0 | EventMask::LEFT_MOUSE_UP.0);

/// Сколько фильтр в потоке тапа ждёт ответа приложения на каждый AX-вопрос,
/// секунды. Столько же — у значка стопки окон.
pub const TAP_AX_TIMEOUT: f32 = 0.25;

// ---------------------------------------------------------------- решение

/// Что фильтру нужно от системы. Вызывается в потоке тапа.
pub trait GreenButtonSystem {
    type Element;

    /// `AccessibilityElement(location)`: элемент в точке (координаты AX), без
    /// спуска к детям.
    fn element_at(&self, location: (f64, f64)) -> Option<Self::Element>;
    /// `isFullScreenButton == true`.
    fn is_full_screen_button(&self, element: &Self::Element) -> bool;
    /// `frame`, AX.
    fn frame(&self, element: &Self::Element) -> Option<Rect>;
    /// `windowElement`.
    fn window_element(&self, element: &Self::Element) -> Option<Self::Element>;
}

/// Что сделать с событием.
#[derive(Clone, Debug, PartialEq)]
pub enum Filtered<E> {
    /// Отдать дальше, приложению.
    Pass,
    /// Поглотить.
    Consume,
    /// Поглотить и выполнить действие над окном (`executeAction`, на главном потоке).
    ConsumeAndExecute(E),
}

/// Нажатая зелёная кнопка: её окно и рамка (`windowElement`, `buttonFrame`).
#[derive(Debug)]
pub struct GreenButtonState<E> {
    pressed: Option<(E, Rect)>,
}

impl<E> Default for GreenButtonState<E> {
    fn default() -> Self {
        GreenButtonState { pressed: None }
    }
}

impl<E> GreenButtonState<E> {
    /// `filter(_ event:)`.
    pub fn filter<S>(&mut self, system: &S, event: &MouseEvent) -> Filtered<E>
    where
        S: GreenButtonSystem<Element = E>,
    {
        match event.kind {
            MouseEventKind::LeftMouseDown => {
                self.pressed = None;
                match self.press(system, event) {
                    Some(pressed) => {
                        self.pressed = Some(pressed);
                        Filtered::Consume
                    }
                    None => Filtered::Pass,
                }
            }
            MouseEventKind::LeftMouseUp => {
                let Some((window, button_frame)) = self.pressed.take() else {
                    return Filtered::Pass;
                };
                // Отпустили мимо кнопки — щелчок отменён, как у обычной кнопки.
                match event.location {
                    Some((x, y)) if ax::rect_contains_point(&button_frame, x, y) => {
                        Filtered::ConsumeAndExecute(window)
                    }
                    _ => Filtered::Consume,
                }
            }
            _ => Filtered::Pass,
        }
    }

    /// Нажатие без модификаторов на зелёной кнопке окна: её окно и рамка.
    fn press<S>(&self, system: &S, event: &MouseEvent) -> Option<(E, Rect)>
    where
        S: GreenButtonSystem<Element = E>,
    {
        if event.device_independent_flags() != 0 {
            return None;
        }
        let element = system.element_at(event.location?)?;
        if !system.is_full_screen_button(&element) {
            return None;
        }
        let button_frame = system.frame(&element)?;
        let window = system.window_element(&element)?;
        Some((window, button_frame))
    }
}

/// Действие зелёной кнопки: «Развернуть», а у окна, которое стоит ровно там,
/// куда его последним развернул Rectangle, — «Восстановить».
pub fn button_action(frame: Option<Rect>, last: Option<&LastAction>) -> Action {
    if repeats_last_action(Action::Maximize, frame, last) {
        Action::Restore
    } else {
        Action::Maximize
    }
}

// ---------------------------------------------------------------- система

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXUIElementCopyElementAtPosition(
        element: *const c_void,
        x: f32,
        y: f32,
        out: *mut *const c_void,
    ) -> i32;
}

/// Настоящая система: AX (потокобезопасен, зовётся из потока тапа).
///
/// Элемент в точке спрашивается не у системного элемента, как в оригинале, а у
/// элемента приложения, которому достанется щелчок, — с таймаутом
/// `TAP_AX_TIMEOUT`: таймаут системного элемента один на весь процесс, короткий
/// только для тапа ему не поставить. Приложение — владелец окна, которое, по
/// словам WindowServer, примет событие (`window_under_pointer`). Список окон
/// тут не помощник: верхние окна в любой точке — прозрачные во весь экран окна
/// Dock и Центра уведомлений. Событие без окна (например, созданное
/// программой) — как в оригинале, через системный элемент.
#[derive(Clone, Copy, Debug, Default)]
pub struct AxGreenButtonSystem {
    /// Окно, которое примет щелчок (`TapEvent::window_under_pointer`).
    pub window_under_pointer: Option<u32>,
}

/// Элемент в точке (координаты AX) у приложения `pid`. И этот вопрос, и
/// следующие вопросы к найденному элементу ждут ответа не дольше
/// `TAP_AX_TIMEOUT`; не ответило — `None`.
pub fn app_element_at(pid: i32, (x, y): (f64, f64)) -> Option<AxElement> {
    let app = AxElement::application(pid);
    app.set_messaging_timeout(TAP_AX_TIMEOUT);
    let mut out: *const c_void = ptr::null();
    // SAFETY: `app` — живой элемент; при успехе `out` — +1 объект, он наш.
    let error =
        unsafe { AXUIElementCopyElementAtPosition(app.as_raw(), x as f32, y as f32, &mut out) };
    if error != 0 || out.is_null() {
        return None;
    }
    let element = unsafe { AxElement::retain_raw(out) };
    unsafe { CFRelease(out) };
    let element = element?;
    element.set_messaging_timeout(TAP_AX_TIMEOUT);
    Some(element)
}

impl GreenButtonSystem for AxGreenButtonSystem {
    type Element = AxElement;

    fn element_at(&self, (x, y): (f64, f64)) -> Option<AxElement> {
        let owner = self.window_under_pointer.and_then(|window_id| {
            ax::window_list_for(&[window_id])
                .first()
                .map(|info| info.pid)
        });
        match owner {
            Some(pid) => app_element_at(pid, (x, y)),
            None => {
                let element = ax::element_at_position(x, y)?;
                element.set_messaging_timeout(TAP_AX_TIMEOUT);
                Some(element)
            }
        }
    }

    fn is_full_screen_button(&self, element: &AxElement) -> bool {
        element.is_full_screen_button()
    }

    fn frame(&self, element: &AxElement) -> Option<Rect> {
        element.frame()
    }

    fn window_element(&self, element: &AxElement) -> Option<AxElement> {
        element.window_element()
    }
}

/// `executeAction`: развернуть окно или вернуть развёрнутое. Главный поток.
/// Возвращает выполненное действие.
pub fn execute(window: AxElement) -> Action {
    let action = match window.get_window_id() {
        Some(window_id) => button_action(
            window.frame(),
            window_history::last_action(window_id).as_ref(),
        ),
        None => Action::Maximize,
    };
    log!("Зелёная кнопка: {}", action.name());
    window_manager::execute(ExecutionParameters::title_bar(action, Some(window)));
    action
}

/// Фильтр тапа: решение — в потоке тапа, действие — на главном потоке.
fn tap_filter() -> TapFilter {
    let state = Mutex::new(GreenButtonState::default());
    Arc::new(move |event: &mut TapEvent| {
        let system = AxGreenButtonSystem {
            window_under_pointer: event.window_under_pointer(),
        };
        let event = event.to_mouse_event();
        let filtered = state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .filter(&system, &event);
        match filtered {
            Filtered::Pass => false,
            Filtered::Consume => true,
            Filtered::ConsumeAndExecute(window) => {
                events::run_on_main(move || {
                    execute(window);
                });
                true
            }
        }
    })
}

// ---------------------------------------------------------------- менеджер

#[derive(Default)]
struct Manager {
    monitor: Option<ActiveEventMonitor>,
}

impl Manager {
    /// `toggleListening`: перехват работает, пока включена настройка.
    fn apply(&mut self) {
        if !config::with(|config| config.green_button_override) {
            if self.monitor.take().is_some() {
                log!("Зелёная кнопка: не перехватываем");
            }
            return;
        }
        if self
            .monitor
            .as_ref()
            .is_some_and(ActiveEventMonitor::running)
        {
            return;
        }
        // Обработчик оригинала пустой: всё делает фильтр.
        let handler: TapHandler = Arc::new(|_event: MouseEvent| {});
        let mut monitor = ActiveEventMonitor::new(MASK, tap_filter(), handler);
        monitor.start();
        log!(
            "Зелёная кнопка: перехватываем{}",
            if monitor.running() {
                ""
            } else {
                " — монитор не запустился"
            }
        );
        self.monitor = Some(monitor);
    }
}

thread_local! {
    static MANAGER: RefCell<Option<Manager>> = const { RefCell::new(None) };
}

/// Запустить (строка в `subsystems::STARTS`). Повторный вызов ничего не делает.
pub fn install(_mtm: MainThreadMarker) {
    MANAGER.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.is_none() {
            slot.insert(Manager::default()).apply();
        }
    });
}

/// Настройки изменились (строка в `subsystems::RELOADS`): галочку включили или
/// сняли.
pub fn reload() {
    MANAGER.with(|slot| {
        if let Some(manager) = slot.borrow_mut().as_mut() {
            manager.apply();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use objc2_app_kit::NSEventMask;

    const BUTTON: Rect = Rect {
        x: 152.0,
        y: 206.0,
        w: 14.0,
        h: 16.0,
    };
    const ON_BUTTON: (f64, f64) = (158.0, 214.0);
    const ON_CLOSE: (f64, f64) = (118.0, 214.0);
    const IN_WINDOW: (f64, f64) = (400.0, 400.0);

    /// ⌥ и Caps Lock — `NSEvent.ModifierFlags`.
    const OPTION: u64 = 1 << 19;
    const CAPS_LOCK: u64 = 1 << 16;

    /// Элемент подставной системы.
    #[derive(Clone, Debug, PartialEq)]
    enum Element {
        GreenButton,
        CloseButton,
        Window,
        /// Кнопка без окна (окно уже закрывается) или без рамки.
        Orphan,
        Unframed,
    }

    struct Fake {
        under: Vec<((f64, f64), Element)>,
    }

    impl Fake {
        fn new() -> Fake {
            Fake {
                under: vec![
                    (ON_BUTTON, Element::GreenButton),
                    (ON_CLOSE, Element::CloseButton),
                    (IN_WINDOW, Element::Window),
                ],
            }
        }

        fn with(element: Element) -> Fake {
            Fake {
                under: vec![(ON_BUTTON, element)],
            }
        }
    }

    impl GreenButtonSystem for Fake {
        type Element = Element;

        fn element_at(&self, location: (f64, f64)) -> Option<Element> {
            self.under
                .iter()
                .find(|(point, _)| *point == location)
                .map(|(_, element)| element.clone())
        }
        fn is_full_screen_button(&self, element: &Element) -> bool {
            matches!(
                element,
                Element::GreenButton | Element::Orphan | Element::Unframed
            )
        }
        fn frame(&self, element: &Element) -> Option<Rect> {
            match element {
                Element::Unframed => None,
                _ => Some(BUTTON),
            }
        }
        fn window_element(&self, element: &Element) -> Option<Element> {
            match element {
                Element::Orphan => None,
                _ => Some(Element::Window),
            }
        }
    }

    fn event(kind: MouseEventKind, location: (f64, f64), flags: u64) -> MouseEvent {
        MouseEvent {
            kind,
            timestamp: 1.0,
            modifier_flags: flags,
            location: Some(location),
            delta_y: 0.0,
            click_count: 1,
        }
    }

    fn down(location: (f64, f64)) -> MouseEvent {
        event(MouseEventKind::LeftMouseDown, location, 0)
    }

    fn up(location: (f64, f64)) -> MouseEvent {
        event(MouseEventKind::LeftMouseUp, location, 0)
    }

    #[test]
    fn mask_matches_the_original() {
        assert_eq!(
            MASK.0,
            (NSEventMask::LeftMouseDown | NSEventMask::LeftMouseUp).0
        );
    }

    #[test]
    fn click_on_green_button_is_swallowed_and_executes() {
        let fake = Fake::new();
        let mut state = GreenButtonState::default();
        assert_eq!(state.filter(&fake, &down(ON_BUTTON)), Filtered::Consume);
        // Отпустили в другой точке, но над кнопкой.
        assert_eq!(
            state.filter(&fake, &up((BUTTON.x, BUTTON.max_y() - 1.0))),
            Filtered::ConsumeAndExecute(Element::Window)
        );
        // Щелчок кончился: следующее отпускание — не наше.
        assert_eq!(state.filter(&fake, &up(ON_BUTTON)), Filtered::Pass);
    }

    #[test]
    fn releasing_outside_the_button_cancels() {
        let fake = Fake::new();
        let mut state = GreenButtonState::default();
        assert_eq!(state.filter(&fake, &down(ON_BUTTON)), Filtered::Consume);
        // Отпускание всё равно глотается: нажатие приложение не видело.
        assert_eq!(state.filter(&fake, &up(IN_WINDOW)), Filtered::Consume);
        assert_eq!(state.filter(&fake, &up(ON_BUTTON)), Filtered::Pass);

        // Правая и нижняя границы в рамку не входят (`CGRect.contains`).
        assert_eq!(state.filter(&fake, &down(ON_BUTTON)), Filtered::Consume);
        assert_eq!(
            state.filter(&fake, &up((BUTTON.max_x(), BUTTON.mid_y()))),
            Filtered::Consume
        );
    }

    #[test]
    fn any_modifier_keeps_the_macos_behavior() {
        let fake = Fake::new();
        let mut state = GreenButtonState::default();
        for flags in [OPTION, CAPS_LOCK, 1 << 17, 1 << 18, 1 << 20, 1 << 23] {
            let pressed = event(MouseEventKind::LeftMouseDown, ON_BUTTON, flags);
            assert_eq!(state.filter(&fake, &pressed), Filtered::Pass, "{flags:#x}");
            assert_eq!(state.filter(&fake, &up(ON_BUTTON)), Filtered::Pass);
        }
        // Биты конкретной клавиши и «не склеено» (0x100) — не модификаторы.
        let pressed = event(MouseEventKind::LeftMouseDown, ON_BUTTON, 0x100 | 0x20);
        assert_eq!(state.filter(&fake, &pressed), Filtered::Consume);
    }

    #[test]
    fn other_elements_and_events_pass() {
        let fake = Fake::new();
        let mut state = GreenButtonState::default();
        assert_eq!(state.filter(&fake, &down(ON_CLOSE)), Filtered::Pass);
        assert_eq!(state.filter(&fake, &up(ON_CLOSE)), Filtered::Pass);
        assert_eq!(state.filter(&fake, &down(IN_WINDOW)), Filtered::Pass);
        assert_eq!(state.filter(&fake, &up(IN_WINDOW)), Filtered::Pass);
        assert_eq!(state.filter(&fake, &down((1.0, 1.0))), Filtered::Pass);

        let no_location = MouseEvent {
            location: None,
            ..down(ON_BUTTON)
        };
        assert_eq!(state.filter(&fake, &no_location), Filtered::Pass);

        // Пока кнопку держат, прочие события её не отпускают.
        assert_eq!(state.filter(&fake, &down(ON_BUTTON)), Filtered::Consume);
        let dragged = event(MouseEventKind::LeftMouseDragged, IN_WINDOW, 0);
        assert_eq!(state.filter(&fake, &dragged), Filtered::Pass);
        assert_eq!(
            state.filter(&fake, &up(ON_BUTTON)),
            Filtered::ConsumeAndExecute(Element::Window)
        );
    }

    #[test]
    fn button_needs_a_frame_and_a_window() {
        for element in [Element::Orphan, Element::Unframed] {
            let fake = Fake::with(element.clone());
            let mut state = GreenButtonState::default();
            assert_eq!(
                state.filter(&fake, &down(ON_BUTTON)),
                Filtered::Pass,
                "{element:?}"
            );
            assert_eq!(state.filter(&fake, &up(ON_BUTTON)), Filtered::Pass);
        }
    }

    #[test]
    fn a_lost_release_does_not_swallow_the_next_click() {
        let fake = Fake::new();
        let mut state = GreenButtonState::default();
        assert_eq!(state.filter(&fake, &down(ON_BUTTON)), Filtered::Consume);
        // Отпускание до тапа не дошло; следующий щелчок — в другом месте.
        assert_eq!(state.filter(&fake, &down(IN_WINDOW)), Filtered::Pass);
        assert_eq!(state.filter(&fake, &up(ON_BUTTON)), Filtered::Pass);
    }

    #[test]
    fn unknown_app_gives_no_element_without_waiting() {
        let started = std::time::Instant::now();
        assert!(app_element_at(i32::MAX, (10.0, 10.0)).is_none());
        assert!(started.elapsed() < std::time::Duration::from_secs(1));
    }

    #[test]
    fn maximized_window_is_restored() {
        let frame = Rect::new(0.0, 25.0, 1512.0, 957.0);
        let last = |action: Action, rect: Rect| LastAction {
            action,
            sub_action: None,
            rect,
            count: 1,
        };
        assert_eq!(button_action(Some(frame), None), Action::Maximize);
        assert_eq!(
            button_action(Some(frame), Some(&last(Action::Maximize, frame))),
            Action::Restore
        );
        assert_eq!(
            button_action(
                Some(frame),
                Some(&last(Action::Maximize, frame.offset_by(0.0, 1.0)))
            ),
            Action::Maximize
        );
        assert_eq!(
            button_action(Some(frame), Some(&last(Action::LeftHalf, frame))),
            Action::Maximize
        );
        assert_eq!(
            button_action(None, Some(&last(Action::Maximize, frame))),
            Action::Maximize
        );
    }
}
