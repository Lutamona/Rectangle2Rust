//! Раскладки сразу нескольких окон — порт `MultiWindow/MultiWindowManager.swift`
//! и `MultiWindow/ReverseAllManager.swift`: «Плитка: все окна» (`tileAll`),
//! «Каскад: все окна» (`cascadeAll`), «Каскад: окна приложения»
//! (`cascadeActiveApp`), «Плитка: окна приложения» (`tileActiveApp`) и «Поменять
//! местами» (`reverseAll`).
//!
//! Как в оригинале, эти действия перехватываются раньше обычного расчёта
//! (`ShortcutManager.execute` → `MultiWindowManager.execute`, здесь —
//! `window_manager::execute`). Окна берутся с экрана окна в фокусе (экран — как у
//! обычных действий, `detectScreens(using:)`), место считается в рабочей области
//! этого экрана (`adjustedVisibleFrame()`), рамки ставятся в координатах AX.
//! Историю и рамку для «Восстановить» оригинал не пишет (`TODO: save previous
//! position in history`) — порт тоже.
//!
//! Логика написана над трейтами `Desktop` и `LayoutWindow`: настоящая система —
//! `AxDesktop` и `AxElement`, в тестах — макеты, в примере
//! `examples/multi_window_check.rs` — сухой прогон, который только записывает,
//! что было бы сделано.
//!
//! Отличия от Swift — только там, где оригинал падает или шлёт окну мусор:
//! плитка из нуля окон ничего не делает (в Swift `Int(ceil(0 / 0))` роняет
//! программу); окно, чью рамку не прочитать, каскад и «Поменять местами»
//! пропускают (в Swift оно получило бы размер 0×0 или бесконечные координаты),
//! а «Каскад: окна приложения» без рамки переднего окна не выполняется (в Swift —
//! падение на `first.size!`).

use std::cmp::Reverse;

use crate::actions::Action;
use crate::ax::{self, AxElement};
use crate::config::{self, Config};
use crate::geometry::Rect;
use crate::log;
use crate::screen_detection::{detect_screens, ZERO_RECT};
use crate::screens::{self, Screen};
use crate::window_manager::{self, ExecutionParameters};

// ---------------------------------------------------------------- окна и система

/// Окно для раскладок (`AccessibilityElement` в `MultiWindow/*`).
pub trait LayoutWindow {
    /// pid владельца окна.
    fn pid(&self) -> Option<i32>;
    /// Рамка в координатах AX; `None` — не читается.
    fn frame(&self) -> Option<Rect>;
    /// `isWindow == true`.
    fn is_window(&self) -> bool;
    /// `isSheet == true`.
    fn is_sheet(&self) -> bool;
    /// `isMinimized == true`.
    fn is_minimized(&self) -> bool;
    /// `isHidden == true`: приложение окна скрыто.
    fn is_hidden(&self) -> bool;
    /// `isSystemDialog == true`.
    fn is_system_dialog(&self) -> bool;
    /// Окно — боковая панель Todo (`TodoManager.isTodoWindow`).
    fn is_todo_window(&self) -> bool;
    /// `setFrame` — координаты AX.
    fn set_frame(&self, rect: &Rect);
    /// `bringToFront()`: сделать окно главным и активировать приложение, если
    /// оно не активно.
    fn bring_to_front(&self);
}

/// Всё, что раскладкам нужно знать о системе, кроме самих окон.
pub trait Desktop {
    type Window: LayoutWindow;
    /// Окно в фокусе (`getFrontWindowElement`).
    fn front_window(&self) -> Option<Self::Window>;
    /// Окна всех приложений, у которых есть окна на экране
    /// (`getAllWindowElements`), в порядке списка окон.
    fn all_windows(&self) -> Vec<Self::Window>;
    /// `NSScreen.screens`: `[0]` — основной экран.
    fn screens(&self) -> &[Screen];
    fn config(&self) -> &Config;
    /// Рабочая область экрана (`adjustedVisibleFrame()`), координаты Cocoa.
    fn work_area(&self, screen: &Screen) -> Rect;
    /// `NSSound.beep()`.
    fn beep(&self);
}

impl LayoutWindow for AxElement {
    fn pid(&self) -> Option<i32> {
        AxElement::pid(self)
    }

    fn frame(&self) -> Option<Rect> {
        AxElement::frame(self)
    }

    fn is_window(&self) -> bool {
        AxElement::is_window(self)
    }

    fn is_sheet(&self) -> bool {
        AxElement::is_sheet(self)
    }

    fn is_minimized(&self) -> bool {
        AxElement::is_minimized(self)
    }

    fn is_hidden(&self) -> bool {
        AxElement::is_hidden(self) == Some(true)
    }

    fn is_system_dialog(&self) -> bool {
        AxElement::is_system_dialog(self)
    }

    fn is_todo_window(&self) -> bool {
        // `isTodoWindow(_ windowElement:)` берёт номер окна без запасных путей.
        self.window_id().is_some_and(window_manager::is_todo_window)
    }

    fn set_frame(&self, rect: &Rect) {
        AxElement::set_frame(self, rect);
    }

    fn bring_to_front(&self) {
        AxElement::bring_to_front(self, false);
    }
}

/// Настоящая система: окна — через AX, экраны и настройки — снимок на момент
/// действия.
pub struct AxDesktop {
    screens: Vec<Screen>,
    config: Config,
}

impl AxDesktop {
    pub fn current() -> AxDesktop {
        AxDesktop {
            screens: screens::screens(),
            config: config::current(),
        }
    }
}

impl Desktop for AxDesktop {
    type Window = AxElement;

    fn front_window(&self) -> Option<AxElement> {
        ax::front_window()
    }

    fn all_windows(&self) -> Vec<AxElement> {
        ax::all_window_elements()
    }

    fn screens(&self) -> &[Screen] {
        &self.screens
    }

    fn config(&self) -> &Config {
        &self.config
    }

    fn work_area(&self, screen: &Screen) -> Rect {
        window_manager::adjusted_visible_frame(screen, false, false)
    }

    fn beep(&self) {
        window_manager::beep();
    }
}

// ---------------------------------------------------------------- выполнение

/// Действие — раскладка нескольких окон.
pub fn is_multi_window(action: Action) -> bool {
    matches!(
        action,
        Action::ReverseAll
            | Action::TileAll
            | Action::CascadeAll
            | Action::CascadeActiveApp
            | Action::TileActiveApp
    )
}

/// `MultiWindowManager.execute`: раскладку выполняет этот модуль — `true`, даже
/// если она кончилась сигналом. Прочие действия — `false`, систему при этом не
/// трогаем.
pub fn execute(params: &ExecutionParameters) -> bool {
    if !is_multi_window(params.action) {
        return false;
    }
    execute_on(
        &AxDesktop::current(),
        params.action,
        params.window_element.clone(),
    )
}

/// `MultiWindowManager.execute` над любой системой. `window` — явное окно
/// (двойной клик по заголовку); `None` — окно в фокусе.
pub fn execute_on<D: Desktop>(desktop: &D, action: Action, window: Option<D::Window>) -> bool {
    match action {
        Action::ReverseAll => reverse_all(desktop, window),
        Action::TileAll => tile_all(desktop, window),
        Action::CascadeAll => cascade_all(desktop, window),
        Action::CascadeActiveApp => cascade_active_app(desktop, window),
        Action::TileActiveApp => tile_active_app(desktop, window),
        _ => return false,
    }
    true
}

fn primary_height(screens: &[Screen]) -> f64 {
    screens
        .first()
        .map(|screen| screen.frame.max_y())
        .unwrap_or(0.0)
}

/// Экран окна с рамкой `frame` (`detectScreens(using:)?.currentScreen`).
fn screen_of<D: Desktop>(desktop: &D, frame: Option<Rect>) -> Option<Screen> {
    let screens = desktop.screens();
    detect_screens(frame, screens, desktop.config(), primary_height(screens))
        .map(|usable| usable.current)
}

/// Рабочая область экрана в координатах AX (`adjustedVisibleFrame().screenFlipped`).
fn work_area_ax<D: Desktop>(desktop: &D, screen: &Screen) -> Rect {
    desktop
        .work_area(screen)
        .screen_flipped(primary_height(desktop.screens()))
}

/// `allWindowsOnScreen(sortByPID: true)` — все четыре раскладки просят
/// сортировку: экран окна `window` (нет — окна в фокусе) и окна на нём.
/// Берутся настоящие окна: не листы, не свёрнутые, приложение не скрыто, не
/// системные диалоги и не панель Todo (если Todo включён). Порядок — по
/// убыванию pid; сортировка устойчивая, как `sort(by:)` в Swift, так что окна
/// одного приложения остаются в порядке `AXWindows` (переднее — первым).
fn all_windows_on_screen<D: Desktop>(
    desktop: &D,
    window: Option<D::Window>,
) -> Option<(Screen, Vec<D::Window>)> {
    let current = window
        .or_else(|| desktop.front_window())
        .and_then(|window| screen_of(desktop, window.frame()));
    let Some(current) = current else {
        desktop.beep();
        log!("Раскладка окон: не удалось определить экран");
        return None;
    };

    let mut windows = desktop.all_windows();
    windows.sort_by_key(|window| Reverse(window.pid().unwrap_or(0)));

    let todo = desktop.config().todo == Some(true);
    let windows = windows
        .into_iter()
        .filter(|window| {
            if todo && window.is_todo_window() {
                return false;
            }
            screen_of(desktop, window.frame()).is_some_and(|screen| screen.same_display(&current))
                && window.is_window()
                && !window.is_sheet()
                && !window.is_minimized()
                && !window.is_hidden()
                && !window.is_system_dialog()
        })
        .collect();
    Some((current, windows))
}

/// Окна приложения, чьё окно сейчас в фокусе (`w.pid == frontWindowElement.pid`).
fn front_app_windows<W: LayoutWindow>(windows: Vec<W>, front: &W) -> Vec<W> {
    let front_pid = front.pid();
    windows
        .into_iter()
        .filter(|window| window.pid() == front_pid)
        .collect()
}

// ---------------------------------------------------------------- плитка

/// Сетка плитки на `count` окон: столбцов ⌈√n⌉, строк ⌈n / столбцов⌉, окна —
/// по строкам слева направо, сверху вниз; последняя строка может быть
/// неполной. Ориентация экрана на сетку не влияет. `screen_frame` и результат —
/// координаты AX.
fn tile_frames(count: usize, screen_frame: &Rect) -> Vec<Rect> {
    if count == 0 {
        return Vec::new();
    }
    let columns = (count as f64).sqrt().ceil() as usize;
    let rows = (count as f64 / columns as f64).ceil() as usize;
    let width = (screen_frame.max_x() - screen_frame.min_x()) / columns as f64;
    let height = (screen_frame.max_y() - screen_frame.min_y()) / rows as f64;
    (0..count)
        .map(|index| {
            let column = index % columns;
            let row = index / columns;
            Rect::new(
                screen_frame.x + width * column as f64,
                screen_frame.y + height * row as f64,
                width,
                height,
            )
        })
        .collect()
}

/// Поставить окна плиткой (`tileWindow` по очереди).
fn tile<W: LayoutWindow>(windows: &[W], screen_frame: &Rect) {
    for (window, rect) in windows.iter().zip(tile_frames(windows.len(), screen_frame)) {
        window.set_frame(&rect);
    }
}

/// `tileAllWindowsOnScreen`.
fn tile_all<D: Desktop>(desktop: &D, window: Option<D::Window>) {
    let Some((screen, windows)) = all_windows_on_screen(desktop, window) else {
        return;
    };
    tile(&windows, &work_area_ax(desktop, &screen));
}

/// `tileActiveAppWindowsOnScreen`: плитка только из окон приложения в фокусе.
fn tile_active_app<D: Desktop>(desktop: &D, window: Option<D::Window>) {
    let Some((screen, windows)) = all_windows_on_screen(desktop, window) else {
        return;
    };
    let Some(front) = desktop.front_window() else {
        return;
    };
    let screen_frame = work_area_ax(desktop, &screen);
    tile(&front_app_windows(windows, &front), &screen_frame);
}

// ---------------------------------------------------------------- каскад

/// Место `index`-го окна в каскаде всех окон (`cascadeWindow` без параметров):
/// от верхнего левого угла рабочей области со сдвигом `delta` по обеим осям,
/// размер окна прежний. Координаты AX.
fn cascade_frame(frame: &Rect, screen_frame: &Rect, delta: f64, index: usize) -> Rect {
    Rect::new(
        screen_frame.x + delta * index as f64,
        screen_frame.y + delta * index as f64,
        frame.w,
        frame.h,
    )
}

/// `cascadeAllWindowsOnScreen`: окна по очереди встают лесенкой и выводятся
/// вперёд, последнее оказывается сверху.
fn cascade_all<D: Desktop>(desktop: &D, window: Option<D::Window>) {
    let Some((screen, windows)) = all_windows_on_screen(desktop, window) else {
        return;
    };
    let screen_frame = work_area_ax(desktop, &screen);
    let delta = desktop.config().cascade_all_delta_size as f64;
    for (index, window) in windows.iter().enumerate() {
        // Размер не прочитать — окно пропускаем, его ступенька остаётся пустой.
        let Some(frame) = window.frame() else {
            continue;
        };
        window.set_frame(&cascade_frame(&frame, &screen_frame, delta, index));
        window.bring_to_front();
    }
}

/// Лесенка окон приложения (`CascadeActiveAppParameters`): все окна — одного
/// размера, направление задаёт четверть экрана, где стоит переднее окно.
#[derive(Clone, Copy, Debug, PartialEq)]
struct CascadeActiveApp {
    /// Переднее окно правее середины рабочей области: лесенка идёт от правого края.
    right: bool,
    /// Переднее окно ниже середины (в AX y растёт вниз): последнее окно — у нижнего края.
    bottom: bool,
    num_windows: usize,
    /// Размер переднего окна, но не больше, чем помещается лесенка из
    /// `num_windows` ступенек.
    size: (f64, f64),
}

impl CascadeActiveApp {
    /// `window_frame` — переднее окно, `screen_frame` — рабочая область, обе в AX.
    fn new(
        window_frame: &Rect,
        screen_frame: &Rect,
        num_windows: usize,
        size: (f64, f64),
        delta: f64,
    ) -> CascadeActiveApp {
        let steps = num_windows.saturating_sub(1) as f64 * delta;
        CascadeActiveApp {
            right: window_frame.mid_x() > screen_frame.mid_x(),
            bottom: window_frame.mid_y() > screen_frame.mid_y(),
            num_windows,
            size: (
                size.0.min(screen_frame.w - steps),
                size.1.min(screen_frame.h - steps),
            ),
        }
    }

    /// Место `index`-го окна (`cascadeWindow` с параметрами). По вертикали окна
    /// всегда спускаются, по горизонтали — вправо, а от правого края — влево.
    fn frame(&self, screen_frame: &Rect, delta: f64, index: usize) -> Rect {
        let (width, height) = self.size;
        let mut x = screen_frame.x + delta * index as f64;
        let mut y = screen_frame.y + delta * index as f64;
        if self.right {
            x = screen_frame.x + screen_frame.w - width - delta * index as f64;
        }
        if self.bottom {
            y = screen_frame.y + screen_frame.h
                - height
                - delta * (self.num_windows - 1 - index) as f64;
        }
        Rect::new(x, y, width, height)
    }
}

/// `cascadeActiveAppWindowsOnScreen`: окна приложения в фокусе встают лесенкой
/// одного размера и выводятся вперёд по очереди. Переднее окно уходит в конец
/// очереди — оно окажется сверху.
fn cascade_active_app<D: Desktop>(desktop: &D, window: Option<D::Window>) {
    let Some((screen, windows)) = all_windows_on_screen(desktop, window) else {
        return;
    };
    let Some(front) = desktop.front_window() else {
        return;
    };
    let screen_frame = work_area_ax(desktop, &screen);
    let delta = desktop.config().cascade_all_delta_size as f64;

    let mut windows = front_app_windows(windows, &front);
    if windows.is_empty() {
        return;
    }
    let first = windows.remove(0);
    let Some(first_frame) = first.frame() else {
        return;
    };
    windows.push(first);
    let parameters = CascadeActiveApp::new(
        &first_frame,
        &screen_frame,
        windows.len(),
        (first_frame.w, first_frame.h),
        delta,
    );
    for (index, window) in windows.iter().enumerate() {
        window.set_frame(&parameters.frame(&screen_frame, delta, index));
        window.bring_to_front();
    }
}

// ---------------------------------------------------------------- «Поменять местами»

/// Место окна, отражённое по горизонтали внутри рабочей области
/// (`reverseWindowPosition`): отступ от левого края становится отступом от
/// правого, вертикаль и размер прежние. По горизонтали координаты Cocoa и AX
/// совпадают, поэтому рабочая область — без переворота, как в Swift.
fn reversed_frame(frame: &Rect, screen_frame: &Rect) -> Rect {
    let offset_from_left = frame.min_x() - screen_frame.min_x();
    Rect::new(
        screen_frame.max_x() - offset_from_left - frame.w,
        frame.y,
        frame.w,
        frame.h,
    )
}

/// `ReverseAllManager.reverseAll`: все окна экрана окна в фокусе отражаются по
/// горизонтали. В отличие от остальных раскладок окна не сортируются и не
/// отбираются по роли, свёрнутости и скрытости — как в оригинале.
fn reverse_all<D: Desktop>(desktop: &D, window: Option<D::Window>) {
    // Без окна экран ищется по пустой рамке в начале координат (`?? .zero`).
    let frame = match window.or_else(|| desktop.front_window()) {
        Some(window) => window.frame(),
        None => Some(ZERO_RECT),
    };
    let Some(current) = screen_of(desktop, frame) else {
        return;
    };
    let windows = desktop.all_windows();
    let screen_frame = desktop.work_area(&current);
    let todo = desktop.config().todo == Some(true);

    for window in &windows {
        let frame = window.frame();
        let window_screen = screen_of(desktop, frame);
        if todo && window.is_todo_window() {
            continue;
        }
        if !window_screen.is_some_and(|screen| screen.same_display(&current)) {
            continue;
        }
        // Рамку не прочитать — отражать нечего, окно не трогаем.
        if let Some(frame) = frame {
            window.set_frame(&reversed_frame(&frame, &screen_frame));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    use crate::actions::all_actions;
    use crate::screen_detection::{self, ScreenEnvironment};
    use crate::stage::StageState;

    /// Что раскладка сделала с окнами, по порядку.
    #[derive(Clone, Debug, PartialEq)]
    enum Call {
        SetFrame(&'static str, Rect),
        BringToFront(&'static str),
    }

    /// Описание окна-макета.
    #[derive(Clone)]
    struct Spec {
        name: &'static str,
        pid: Option<i32>,
        frame: Option<Rect>,
        role: &'static str,
        subrole: &'static str,
        minimized: bool,
        hidden: bool,
        todo: bool,
    }

    fn window(name: &'static str, pid: i32, frame: Rect) -> Spec {
        Spec {
            name,
            pid: Some(pid),
            frame: Some(frame),
            role: "AXWindow",
            subrole: "AXStandardWindow",
            minimized: false,
            hidden: false,
            todo: false,
        }
    }

    struct FakeState {
        spec: Spec,
        frame: Cell<Option<Rect>>,
        calls: Rc<RefCell<Vec<Call>>>,
    }

    /// Окно-макет: `set_frame` меняет его рамку, все вызовы пишутся в общий журнал.
    #[derive(Clone)]
    struct FakeWindow(Rc<FakeState>);

    impl LayoutWindow for FakeWindow {
        fn pid(&self) -> Option<i32> {
            self.0.spec.pid
        }
        fn frame(&self) -> Option<Rect> {
            self.0.frame.get()
        }
        fn is_window(&self) -> bool {
            self.0.spec.role == "AXWindow"
        }
        fn is_sheet(&self) -> bool {
            self.0.spec.role == "AXSheet"
        }
        fn is_minimized(&self) -> bool {
            self.0.spec.minimized
        }
        fn is_hidden(&self) -> bool {
            self.0.spec.hidden
        }
        fn is_system_dialog(&self) -> bool {
            self.0.spec.subrole == "AXSystemDialog"
        }
        fn is_todo_window(&self) -> bool {
            self.0.spec.todo
        }
        fn set_frame(&self, rect: &Rect) {
            self.0.frame.set(Some(*rect));
            self.0
                .calls
                .borrow_mut()
                .push(Call::SetFrame(self.0.spec.name, *rect));
        }
        fn bring_to_front(&self) {
            self.0
                .calls
                .borrow_mut()
                .push(Call::BringToFront(self.0.spec.name));
        }
    }

    struct FakeDesktop {
        windows: Vec<FakeWindow>,
        front: Option<FakeWindow>,
        screens: Vec<Screen>,
        config: Config,
        beeps: Cell<usize>,
        calls: Rc<RefCell<Vec<Call>>>,
    }

    impl FakeDesktop {
        fn new(screens: Vec<Screen>) -> FakeDesktop {
            FakeDesktop {
                windows: Vec::new(),
                front: None,
                screens,
                config: Config::default(),
                beeps: Cell::new(0),
                calls: Rc::new(RefCell::new(Vec::new())),
            }
        }

        /// Добавить окно в конец списка окон (`getAllWindowElements`).
        fn add(&mut self, spec: Spec) -> FakeWindow {
            let window = FakeWindow(Rc::new(FakeState {
                frame: Cell::new(spec.frame),
                spec,
                calls: self.calls.clone(),
            }));
            self.windows.push(window.clone());
            window
        }

        /// Добавить окно и сделать его окном в фокусе.
        fn add_front(&mut self, spec: Spec) -> FakeWindow {
            let window = self.add(spec);
            self.front = Some(window.clone());
            window
        }

        fn calls(&self) -> Vec<Call> {
            self.calls.borrow().clone()
        }

        /// Только установки рамок: (окно, рамка).
        fn moves(&self) -> Vec<(&'static str, Rect)> {
            self.calls()
                .into_iter()
                .filter_map(|call| match call {
                    Call::SetFrame(name, rect) => Some((name, rect)),
                    Call::BringToFront(_) => None,
                })
                .collect()
        }

        fn run(&self, action: Action) -> bool {
            execute_on(self, action, None)
        }
    }

    impl Desktop for FakeDesktop {
        type Window = FakeWindow;

        fn front_window(&self) -> Option<FakeWindow> {
            self.front.clone()
        }
        fn all_windows(&self) -> Vec<FakeWindow> {
            self.windows.clone()
        }
        fn screens(&self) -> &[Screen] {
            &self.screens
        }
        fn config(&self) -> &Config {
            &self.config
        }
        fn work_area(&self, screen: &Screen) -> Rect {
            let env = ScreenEnvironment {
                config: &self.config,
                screens: &self.screens,
                separate_spaces: true,
                stage: StageState::default(),
                todo_screen: None,
            };
            screen_detection::adjusted_visible_frame(screen, &env, false, false)
        }
        fn beep(&self) {
            self.beeps.set(self.beeps.get() + 1);
        }
    }

    fn screen(id: u32, frame: Rect, visible_frame: Rect) -> Screen {
        Screen {
            id,
            frame,
            visible_frame,
            name: format!("экран {id}"),
            is_main: false,
            scale: 2.0,
            safe_area_top: 0.0,
        }
    }

    /// Ноутбук 1728×1117, основной: меню-бар 32, док снизу 83.
    /// Рабочая область в AX — (0, 32, 1728, 1002).
    fn laptop() -> Screen {
        let mut laptop = screen(
            1,
            Rect::new(0.0, 0.0, 1728.0, 1117.0),
            Rect::new(0.0, 83.0, 1728.0, 1002.0),
        );
        laptop.is_main = true;
        laptop.safe_area_top = 32.0;
        laptop
    }

    /// Вертикальный монитор 1080×1920 справа, ниже верха ноутбука, меню-бар 25.
    /// Рабочая область в AX — (1728, -378, 1080, 1895).
    fn portrait_right() -> Screen {
        screen(
            2,
            Rect::new(1728.0, -400.0, 1080.0, 1920.0),
            Rect::new(1728.0, -400.0, 1080.0, 1895.0),
        )
    }

    /// Широкий монитор слева и выше ноутбука: в AX обе координаты отрицательные.
    /// Рабочая область в AX — (-2560, -1098, 2560, 1415).
    fn wide_left_above() -> Screen {
        screen(
            3,
            Rect::new(-2560.0, 800.0, 2560.0, 1440.0),
            Rect::new(-2560.0, 800.0, 2560.0, 1415.0),
        )
    }

    const LAPTOP_AREA: Rect = Rect {
        x: 0.0,
        y: 32.0,
        w: 1728.0,
        h: 1002.0,
    };
    const PORTRAIT_AREA: Rect = Rect {
        x: 1728.0,
        y: -378.0,
        w: 1080.0,
        h: 1895.0,
    };
    const WIDE_AREA: Rect = Rect {
        x: -2560.0,
        y: -1098.0,
        w: 2560.0,
        h: 1415.0,
    };

    fn desk() -> FakeDesktop {
        FakeDesktop::new(vec![laptop(), portrait_right(), wide_left_above()])
    }

    #[test]
    fn work_areas_of_test_screens_in_ax() {
        let desk = desk();
        assert_eq!(work_area_ax(&desk, &laptop()), LAPTOP_AREA);
        assert_eq!(work_area_ax(&desk, &portrait_right()), PORTRAIT_AREA);
        assert_eq!(work_area_ax(&desk, &wide_left_above()), WIDE_AREA);
    }

    #[test]
    fn tile_grid_for_one_to_twelve_windows() {
        // (столбцы, строки) для n = 1…12: ⌈√n⌉ и ⌈n / столбцы⌉.
        let expected = [
            (1, 1),
            (2, 1),
            (2, 2),
            (2, 2),
            (3, 2),
            (3, 2),
            (3, 3),
            (3, 3),
            (3, 3),
            (4, 3),
            (4, 3),
            (4, 3),
        ];
        for area in [LAPTOP_AREA, PORTRAIT_AREA, WIDE_AREA] {
            for (n, &(columns, rows)) in (1..=12).zip(expected.iter()) {
                let frames = tile_frames(n, &area);
                assert_eq!(frames.len(), n);
                let width = area.w / columns as f64;
                let height = area.h / rows as f64;
                for (index, frame) in frames.iter().enumerate() {
                    let (column, row) = (index % columns, index / columns);
                    assert_eq!(
                        *frame,
                        Rect::new(
                            area.x + width * column as f64,
                            area.y + height * row as f64,
                            width,
                            height
                        ),
                        "n={n}, окно {index}"
                    );
                    assert!(
                        frame.min_x() >= area.min_x() - 1e-9
                            && frame.max_x() <= area.max_x() + 1e-9
                            && frame.min_y() >= area.min_y() - 1e-9
                            && frame.max_y() <= area.max_y() + 1e-9,
                        "n={n}: окно {index} вне рабочей области"
                    );
                    for other in &frames[index + 1..] {
                        assert!(
                            frame.intersection(other).is_none_or(|i| i.area() < 1e-6),
                            "n={n}: окна перекрываются"
                        );
                    }
                }
                // Последняя строка не пустая: лишних строк нет.
                assert_eq!((n - 1) / columns, rows - 1, "n={n}");
                // Первая строка — во всю ширину, если окон хватает.
                if n >= columns {
                    let right = frames[columns - 1].max_x();
                    assert!((right - area.max_x()).abs() < 1e-9, "n={n}");
                }
            }
        }
        assert!(tile_frames(0, &LAPTOP_AREA).is_empty());
    }

    #[test]
    fn tile_grid_examples() {
        // 5 окон на ноутбуке: 3 × 2, нижняя строка неполная.
        assert_eq!(
            tile_frames(5, &LAPTOP_AREA),
            vec![
                Rect::new(0.0, 32.0, 576.0, 501.0),
                Rect::new(576.0, 32.0, 576.0, 501.0),
                Rect::new(1152.0, 32.0, 576.0, 501.0),
                Rect::new(0.0, 533.0, 576.0, 501.0),
                Rect::new(576.0, 533.0, 576.0, 501.0),
            ]
        );
        // Вертикальный экран: сетка та же (2 × 1), окна высокие и узкие.
        assert_eq!(
            tile_frames(2, &PORTRAIT_AREA),
            vec![
                Rect::new(1728.0, -378.0, 540.0, 1895.0),
                Rect::new(2268.0, -378.0, 540.0, 1895.0),
            ]
        );
        // Монитор с отрицательными координатами: 3 окна — 2 × 2.
        assert_eq!(
            tile_frames(3, &WIDE_AREA),
            vec![
                Rect::new(-2560.0, -1098.0, 1280.0, 707.5),
                Rect::new(-1280.0, -1098.0, 1280.0, 707.5),
                Rect::new(-2560.0, -390.5, 1280.0, 707.5),
            ]
        );
    }

    #[test]
    fn tile_all_takes_windows_of_the_front_screen_sorted_by_pid() {
        let mut desk = desk();
        desk.add(window(
            "терминал 1",
            100,
            Rect::new(10.0, 40.0, 600.0, 400.0),
        ));
        desk.add(window("сафари", 300, Rect::new(200.0, 100.0, 900.0, 700.0)));
        desk.add_front(window(
            "терминал 2",
            100,
            Rect::new(50.0, 80.0, 600.0, 400.0),
        ));
        desk.add(window(
            "терминал 3",
            100,
            Rect::new(1800.0, 0.0, 600.0, 400.0),
        ));
        desk.add(window(
            "заметки",
            200,
            Rect::new(300.0, 300.0, 500.0, 500.0),
        ));

        assert!(desk.run(Action::TileAll));
        // pid по убыванию, внутри приложения — порядок списка окон; «терминал 3» — на
        // другом экране. 4 окна → 2 × 2.
        assert_eq!(
            desk.moves(),
            vec![
                ("сафари", Rect::new(0.0, 32.0, 864.0, 501.0)),
                ("заметки", Rect::new(864.0, 32.0, 864.0, 501.0)),
                ("терминал 1", Rect::new(0.0, 533.0, 864.0, 501.0)),
                ("терминал 2", Rect::new(864.0, 533.0, 864.0, 501.0)),
            ]
        );
        // Плитка окна вперёд не выводит.
        assert_eq!(desk.calls().len(), 4);
        assert_eq!(desk.beeps.get(), 0);
    }

    #[test]
    fn windows_are_picked_like_swift() {
        for todo_enabled in [false, true] {
            let mut desk = desk();
            desk.config.todo = todo_enabled.then_some(true);
            let frame = Rect::new(100.0, 100.0, 400.0, 300.0);
            desk.add_front(window("обычное", 10, frame));
            desk.add(Spec {
                role: "AXSheet",
                ..window("лист", 10, frame)
            });
            desk.add(Spec {
                role: "AXGroup",
                ..window("не окно", 10, frame)
            });
            desk.add(Spec {
                role: "",
                ..window("без роли", 10, frame)
            });
            desk.add(Spec {
                minimized: true,
                ..window("свёрнутое", 10, frame)
            });
            desk.add(Spec {
                hidden: true,
                ..window("скрытое", 20, frame)
            });
            desk.add(Spec {
                subrole: "AXSystemDialog",
                ..window("диалог", 20, frame)
            });
            desk.add(Spec {
                subrole: "AXDialog",
                ..window("обычный диалог", 20, frame)
            });
            desk.add(Spec {
                subrole: "",
                ..window("без подроли", 20, frame)
            });
            desk.add(Spec {
                todo: true,
                ..window("todo", 30, frame)
            });

            desk.run(Action::TileAll);
            let names: Vec<&str> = desk.moves().into_iter().map(|(name, _)| name).collect();
            // Панель Todo пропускается, только если Todo включён пользователем.
            let mut expected = vec!["обычный диалог", "без подроли", "обычное"];
            if !todo_enabled {
                expected.insert(0, "todo");
            }
            assert_eq!(names, expected, "todo = {todo_enabled}");
        }
    }

    #[test]
    fn same_pid_keeps_window_list_order_and_unknown_pid_goes_last() {
        let mut desk = desk();
        let frame = Rect::new(100.0, 100.0, 400.0, 300.0);
        desk.add(Spec {
            pid: None,
            ..window("без pid", 0, frame)
        });
        desk.add(window("а1", 5, frame));
        desk.add_front(window("б1", 7, frame));
        desk.add(window("а2", 5, frame));
        desk.add(window("б2", 7, frame));
        desk.add(window("а3", 5, frame));
        desk.run(Action::TileAll);
        let names: Vec<&str> = desk.moves().into_iter().map(|(name, _)| name).collect();
        assert_eq!(names, vec!["б1", "б2", "а1", "а2", "а3", "без pid"]);
    }

    #[test]
    fn layouts_use_the_screen_of_the_front_window() {
        let mut desk = desk();
        desk.add(window("ноутбук", 1, Rect::new(100.0, 100.0, 400.0, 300.0)));
        desk.add_front(window("верт 1", 2, Rect::new(1800.0, 0.0, 500.0, 500.0)));
        desk.add(window("верт 2", 3, Rect::new(2000.0, 900.0, 600.0, 600.0)));
        desk.add(window("слева", 4, Rect::new(-2000.0, -900.0, 800.0, 600.0)));

        desk.run(Action::TileAll);
        assert_eq!(
            desk.moves(),
            vec![
                ("верт 2", Rect::new(1728.0, -378.0, 540.0, 1895.0)),
                ("верт 1", Rect::new(2268.0, -378.0, 540.0, 1895.0)),
            ]
        );
    }

    #[test]
    fn explicit_window_chooses_the_screen() {
        // Двойной клик по заголовку передаёт окно: экран — его, а не окна в фокусе.
        let mut desk = desk();
        desk.add_front(window("ноутбук", 1, Rect::new(100.0, 100.0, 400.0, 300.0)));
        let left = desk.add(window("слева", 4, Rect::new(-2000.0, -900.0, 800.0, 600.0)));
        assert!(execute_on(&desk, Action::TileAll, Some(left)));
        assert_eq!(desk.moves(), vec![("слева", WIDE_AREA)]);
    }

    #[test]
    fn no_front_window_beeps_and_moves_nothing() {
        for action in [
            Action::TileAll,
            Action::CascadeAll,
            Action::CascadeActiveApp,
            Action::TileActiveApp,
        ] {
            let mut desk = desk();
            desk.add(window("окно", 1, Rect::new(100.0, 100.0, 400.0, 300.0)));
            assert!(desk.run(action), "{action:?}");
            assert_eq!(desk.beeps.get(), 1, "{action:?}");
            assert!(desk.calls().is_empty(), "{action:?}");
        }
    }

    #[test]
    fn nothing_to_tile_is_not_a_crash() {
        // Окно в фокусе — системный диалог, других окон на экране нет: плитка из
        // нуля окон (Swift здесь падает на 0 / 0).
        for action in [
            Action::TileAll,
            Action::TileActiveApp,
            Action::CascadeActiveApp,
        ] {
            let mut desk = desk();
            desk.add_front(Spec {
                subrole: "AXSystemDialog",
                ..window("диалог", 1, Rect::new(100.0, 100.0, 400.0, 300.0))
            });
            desk.add(window(
                "другой экран",
                1,
                Rect::new(1800.0, 0.0, 500.0, 500.0),
            ));
            assert!(desk.run(action), "{action:?}");
            assert!(desk.calls().is_empty(), "{action:?}");
            assert_eq!(desk.beeps.get(), 0, "{action:?}");
        }
    }

    #[test]
    fn cascade_all_steps_each_window_and_raises_it() {
        let mut desk = desk();
        desk.add_front(window("а", 9, Rect::new(500.0, 500.0, 700.0, 400.0)));
        desk.add(window("б", 5, Rect::new(10.0, 40.0, 300.0, 200.0)));
        desk.add(window("в", 5, Rect::new(900.0, 300.0, 800.0, 600.0)));
        desk.add(window(
            "на другом экране",
            1,
            Rect::new(1800.0, 0.0, 500.0, 500.0),
        ));

        desk.run(Action::CascadeAll);
        assert_eq!(
            desk.calls(),
            vec![
                Call::SetFrame("а", Rect::new(0.0, 32.0, 700.0, 400.0)),
                Call::BringToFront("а"),
                Call::SetFrame("б", Rect::new(30.0, 62.0, 300.0, 200.0)),
                Call::BringToFront("б"),
                Call::SetFrame("в", Rect::new(60.0, 92.0, 800.0, 600.0)),
                Call::BringToFront("в"),
            ]
        );
    }

    #[test]
    fn cascade_step_comes_from_settings() {
        let mut desk = desk();
        desk.config.cascade_all_delta_size = 45.0;
        desk.add_front(window("а", 2, Rect::new(-2000.0, -900.0, 800.0, 600.0)));
        desk.add(window("б", 1, Rect::new(-1500.0, -500.0, 700.0, 500.0)));
        desk.run(Action::CascadeAll);
        assert_eq!(
            desk.moves(),
            vec![
                ("а", Rect::new(-2560.0, -1098.0, 800.0, 600.0)),
                ("б", Rect::new(-2515.0, -1053.0, 700.0, 500.0)),
            ]
        );
    }

    #[test]
    fn work_area_follows_screen_edge_gaps() {
        let mut desk = desk();
        desk.config.screen_edge_gap_left = 10.0;
        desk.config.screen_edge_gap_right = 20.0;
        desk.config.screen_edge_gap_top = 5.0;
        desk.config.screen_edge_gap_bottom = 15.0;
        desk.add_front(window("а", 1, Rect::new(100.0, 100.0, 400.0, 300.0)));
        desk.run(Action::TileAll);
        // Рабочая область: x 10…1708, сверху 32 + 5, снизу 83 + 15.
        assert_eq!(
            desk.moves(),
            vec![("а", Rect::new(10.0, 37.0, 1698.0, 982.0))]
        );
    }

    /// Три окна приложения 100 (первое — переднее) и одно чужое.
    fn app_desk(front_frame: Rect) -> FakeDesktop {
        let mut desk = desk();
        desk.add(window("чужое", 200, Rect::new(300.0, 300.0, 500.0, 400.0)));
        desk.add_front(window("переднее", 100, front_frame));
        desk.add(window("второе", 100, Rect::new(400.0, 200.0, 900.0, 700.0)));
        desk.add(window("третье", 100, Rect::new(20.0, 600.0, 300.0, 300.0)));
        desk
    }

    #[test]
    fn cascade_active_app_goes_from_the_front_window_quarter() {
        // Центр рабочей области ноутбука в AX — (864, 533).
        let cases = [
            // Слева сверху: вправо и вниз от угла.
            (
                Rect::new(100.0, 100.0, 800.0, 600.0),
                [(0.0, 32.0), (30.0, 62.0), (60.0, 92.0)],
            ),
            // Справа сверху: от правого края влево, вниз.
            (
                Rect::new(1000.0, 100.0, 600.0, 400.0),
                [(1128.0, 32.0), (1098.0, 62.0), (1068.0, 92.0)],
            ),
            // Слева снизу: вправо, последнее окно — у нижнего края.
            (
                Rect::new(100.0, 700.0, 600.0, 300.0),
                [(0.0, 674.0), (30.0, 704.0), (60.0, 734.0)],
            ),
            // Справа снизу.
            (
                Rect::new(1000.0, 700.0, 600.0, 300.0),
                [(1128.0, 674.0), (1098.0, 704.0), (1068.0, 734.0)],
            ),
        ];
        for (front_frame, origins) in cases {
            let desk = app_desk(front_frame);
            desk.run(Action::CascadeActiveApp);
            let (w, h) = (front_frame.w, front_frame.h);
            // Переднее окно — последним, чтобы оказаться сверху; все — его размера.
            assert_eq!(
                desk.calls(),
                vec![
                    Call::SetFrame("второе", Rect::new(origins[0].0, origins[0].1, w, h)),
                    Call::BringToFront("второе"),
                    Call::SetFrame("третье", Rect::new(origins[1].0, origins[1].1, w, h)),
                    Call::BringToFront("третье"),
                    Call::SetFrame("переднее", Rect::new(origins[2].0, origins[2].1, w, h)),
                    Call::BringToFront("переднее"),
                ],
                "{front_frame:?}"
            );
        }
    }

    #[test]
    fn cascade_active_app_shrinks_to_fit_the_steps() {
        // Переднее окно во весь экран: размер урезается на две ступеньки.
        let desk = app_desk(LAPTOP_AREA);
        desk.run(Action::CascadeActiveApp);
        assert_eq!(
            desk.moves(),
            vec![
                ("второе", Rect::new(0.0, 32.0, 1668.0, 942.0)),
                ("третье", Rect::new(30.0, 62.0, 1668.0, 942.0)),
                ("переднее", Rect::new(60.0, 92.0, 1668.0, 942.0)),
            ]
        );
        // Последняя ступенька ровно упирается в правый нижний угол.
        let last = desk.moves()[2].1;
        assert_eq!((last.max_x(), last.max_y()), (1728.0, 1034.0));
    }

    #[test]
    fn cascade_active_app_single_window_goes_to_its_corner() {
        let mut desk = desk();
        desk.add_front(window("одно", 1, Rect::new(1200.0, 700.0, 400.0, 250.0)));
        desk.run(Action::CascadeActiveApp);
        assert_eq!(
            desk.calls(),
            vec![
                Call::SetFrame("одно", Rect::new(1328.0, 784.0, 400.0, 250.0)),
                Call::BringToFront("одно"),
            ]
        );
    }

    #[test]
    fn cascade_active_app_needs_the_front_window_frame() {
        // В Swift — падение на `first.size!`; здесь раскладка просто не делается.
        let mut desk = desk();
        desk.add_front(Spec {
            frame: None,
            ..window("без рамки", 1, Rect::new(0.0, 0.0, 0.0, 0.0))
        });
        desk.add(window("второе", 1, Rect::new(100.0, 100.0, 400.0, 300.0)));
        desk.run(Action::CascadeActiveApp);
        assert!(desk.calls().is_empty());
    }

    #[test]
    fn tile_active_app_tiles_only_the_front_app_on_its_screen() {
        let mut desk = app_desk(Rect::new(100.0, 100.0, 800.0, 600.0));
        desk.add(window(
            "того же на другом экране",
            100,
            Rect::new(1800.0, 0.0, 500.0, 500.0),
        ));
        desk.run(Action::TileActiveApp);
        assert_eq!(
            desk.moves(),
            vec![
                ("переднее", Rect::new(0.0, 32.0, 864.0, 501.0)),
                ("второе", Rect::new(864.0, 32.0, 864.0, 501.0)),
                ("третье", Rect::new(0.0, 533.0, 864.0, 501.0)),
            ]
        );
        assert_eq!(desk.calls().len(), 3);
    }

    #[test]
    fn frameless_windows_belong_to_the_first_ordered_screen() {
        // Рамка не читается: в Swift она `CGRect.null`, `contains(.null)` истинно у
        // любого экрана, и экран такого окна — первый в пользовательском порядке
        // (здесь широкий монитор слева-сверху), а не `NSScreen.main` (ноутбук). В
        // раскладки ноутбука окно не попадает.
        let build = || {
            let mut desk = desk();
            desk.add_front(window("а", 3, Rect::new(100.0, 100.0, 400.0, 300.0)));
            desk.add(Spec {
                frame: None,
                ..window("без рамки", 2, Rect::new(0.0, 0.0, 0.0, 0.0))
            });
            desk.add(window("в", 1, Rect::new(200.0, 200.0, 500.0, 300.0)));
            desk
        };
        let desk = build();
        desk.run(Action::TileAll);
        assert_eq!(
            desk.moves(),
            vec![
                ("а", Rect::new(0.0, 32.0, 864.0, 1002.0)),
                ("в", Rect::new(864.0, 32.0, 864.0, 1002.0)),
            ]
        );

        let desk = build();
        desk.run(Action::CascadeAll);
        assert_eq!(
            desk.moves(),
            vec![
                ("а", Rect::new(0.0, 32.0, 400.0, 300.0)),
                ("в", Rect::new(30.0, 62.0, 500.0, 300.0)),
            ]
        );

        let desk = build();
        desk.run(Action::ReverseAll);
        assert_eq!(
            desk.moves(),
            vec![
                ("а", Rect::new(1228.0, 100.0, 400.0, 300.0)),
                ("в", Rect::new(1028.0, 200.0, 500.0, 300.0)),
            ]
        );
    }

    #[test]
    fn frameless_windows_join_layouts_of_the_first_ordered_screen() {
        // Окно в фокусе на первом в пользовательском порядке экране — окно без
        // рамки в его плитке (плитке рамка не нужна); каскад окно пропускает,
        // ступенька пустая. Окно на ноутбуке ни туда, ни туда не попадает.
        let build = || {
            let mut desk = desk();
            desk.add_front(window("а", 3, Rect::new(-2000.0, -800.0, 400.0, 300.0)));
            desk.add(Spec {
                frame: None,
                ..window("без рамки", 2, Rect::new(0.0, 0.0, 0.0, 0.0))
            });
            desk.add(window("б", 1, Rect::new(-1500.0, -700.0, 500.0, 300.0)));
            desk.add(window(
                "на ноутбуке",
                1,
                Rect::new(200.0, 200.0, 500.0, 300.0),
            ));
            desk
        };
        let desk = build();
        desk.run(Action::TileAll);
        let moves = desk.moves();
        assert_eq!(
            moves.iter().map(|(name, _)| *name).collect::<Vec<_>>(),
            vec!["а", "без рамки", "б"]
        );
        assert!(moves.iter().all(|(_, rect)| WIDE_AREA.contains(rect)));

        let desk = build();
        desk.run(Action::CascadeAll);
        assert_eq!(
            desk.moves(),
            vec![
                ("а", Rect::new(-2560.0, -1098.0, 400.0, 300.0)),
                ("б", Rect::new(-2500.0, -1038.0, 500.0, 300.0)),
            ]
        );
    }

    #[test]
    fn reverse_all_mirrors_every_window_of_the_screen() {
        let mut desk = desk();
        desk.add_front(window("слева", 1, Rect::new(100.0, 50.0, 400.0, 300.0)));
        desk.add(window(
            "справа впритык",
            2,
            Rect::new(1300.0, 200.0, 428.0, 500.0),
        ));
        // «Поменять местами» не отбирает окна по роли и свёрнутости — как оригинал.
        desk.add(Spec {
            minimized: true,
            ..window("свёрнутое", 3, Rect::new(0.0, 32.0, 800.0, 600.0))
        });
        desk.add(Spec {
            role: "AXSheet",
            ..window("лист", 3, Rect::new(864.0, 300.0, 200.0, 100.0))
        });
        desk.add(window(
            "другой экран",
            4,
            Rect::new(1800.0, 0.0, 500.0, 500.0),
        ));

        assert!(desk.run(Action::ReverseAll));
        assert_eq!(
            desk.moves(),
            vec![
                ("слева", Rect::new(1228.0, 50.0, 400.0, 300.0)),
                ("справа впритык", Rect::new(0.0, 200.0, 428.0, 500.0)),
                ("свёрнутое", Rect::new(928.0, 32.0, 800.0, 600.0)),
                ("лист", Rect::new(664.0, 300.0, 200.0, 100.0)),
            ]
        );
        assert_eq!(desk.calls().len(), 4, "вперёд окна не выводятся");

        // Второй раз — всё на старых местах.
        desk.calls.borrow_mut().clear();
        desk.run(Action::ReverseAll);
        let back: Vec<Rect> = desk.moves().into_iter().map(|(_, rect)| rect).collect();
        assert_eq!(
            back,
            vec![
                Rect::new(100.0, 50.0, 400.0, 300.0),
                Rect::new(1300.0, 200.0, 428.0, 500.0),
                Rect::new(0.0, 32.0, 800.0, 600.0),
                Rect::new(864.0, 300.0, 200.0, 100.0),
            ]
        );
    }

    #[test]
    fn reverse_all_on_offset_screens_and_with_gaps() {
        assert_eq!(
            reversed_frame(&Rect::new(-2500.0, -1000.0, 1000.0, 500.0), &WIDE_AREA),
            Rect::new(-1060.0, -1000.0, 1000.0, 500.0)
        );
        assert_eq!(
            reversed_frame(&Rect::new(1728.0, 0.0, 540.0, 900.0), &PORTRAIT_AREA),
            Rect::new(2268.0, 0.0, 540.0, 900.0)
        );

        let mut desk = desk();
        desk.config.screen_edge_gap_left = 10.0;
        desk.config.screen_edge_gap_right = 20.0;
        desk.add_front(window("а", 1, Rect::new(100.0, 50.0, 400.0, 300.0)));
        desk.run(Action::ReverseAll);
        // Рабочая область 10…1708: отступ 90 слева становится отступом 90 справа.
        assert_eq!(
            desk.moves(),
            vec![("а", Rect::new(1218.0, 50.0, 400.0, 300.0))]
        );
    }

    #[test]
    fn reverse_all_skips_todo_only_when_enabled() {
        for todo_enabled in [false, true] {
            let mut desk = desk();
            desk.config.todo = todo_enabled.then_some(true);
            desk.add_front(window("а", 1, Rect::new(100.0, 50.0, 400.0, 300.0)));
            desk.add(Spec {
                todo: true,
                ..window("todo", 2, Rect::new(1328.0, 32.0, 400.0, 1002.0))
            });
            desk.run(Action::ReverseAll);
            let names: Vec<&str> = desk.moves().into_iter().map(|(name, _)| name).collect();
            let expected = if todo_enabled {
                vec!["а"]
            } else {
                vec!["а", "todo"]
            };
            assert_eq!(names, expected);
        }
    }

    #[test]
    fn reverse_all_without_a_window_uses_the_screen_at_the_origin() {
        // Окна в фокусе нет — экран по пустой рамке в (0, 0) AX, то есть основной;
        // без сигнала. (Широкий монитор здесь не подключён: его угол касается
        // этой точки, и он стоит первым в порядке экранов — тогда взяли бы его.)
        let mut two = FakeDesktop::new(vec![laptop(), portrait_right()]);
        two.add(window("ноутбук", 1, Rect::new(100.0, 50.0, 400.0, 300.0)));
        two.add(window("справа", 2, Rect::new(1800.0, 0.0, 500.0, 500.0)));
        two.run(Action::ReverseAll);
        assert_eq!(
            two.moves(),
            vec![("ноутбук", Rect::new(1228.0, 50.0, 400.0, 300.0))]
        );
        assert_eq!(two.beeps.get(), 0);

        let mut three = desk();
        three.add(window("ноутбук", 1, Rect::new(100.0, 50.0, 400.0, 300.0)));
        three.add(window(
            "слева",
            2,
            Rect::new(-2500.0, -1000.0, 1000.0, 500.0),
        ));
        three.run(Action::ReverseAll);
        assert_eq!(
            three.moves(),
            vec![("слева", Rect::new(-1060.0, -1000.0, 1000.0, 500.0))]
        );
    }

    #[test]
    fn other_actions_are_not_handled_and_touch_nothing() {
        let multi: Vec<Action> = all_actions()
            .into_iter()
            .filter(|action| is_multi_window(*action))
            .collect();
        assert_eq!(
            multi.len(),
            5,
            "в оригинале пять раскладок нескольких окон: {multi:?}"
        );

        let mut desk = desk();
        desk.add_front(window("а", 1, Rect::new(100.0, 50.0, 400.0, 300.0)));
        for action in all_actions() {
            if is_multi_window(action) {
                continue;
            }
            assert!(!desk.run(action), "{action:?}");
            // Настоящую систему для таких действий модуль даже не спрашивает.
            assert!(!execute(&ExecutionParameters::menu(action)), "{action:?}");
        }
        assert!(desk.calls().is_empty());
        assert_eq!(desk.beeps.get(), 0);
    }
}
