//! Логика `TodoManager.swift` над трейтами `TodoSystem` и `TodoWindow`: выбор
//! Todo-окна, экран панели, расстановка окон (`moveAll`), пересчёт ширины
//! панели px ↔ %. Настоящая система — `AxTodoSystem` (`todo.rs`), в тестах —
//! макеты, в примере `todo_check` — настоящая система, ограниченная окнами
//! одного процесса.
//!
//! Координаты: рабочие области экранов — Cocoa (начало снизу слева), рамки
//! окон — AX (начало сверху слева), как в оригинале.

use crate::actions::{Action, Dimension};
use crate::calc::apply_gaps_raw;
use crate::config::{Config, TodoSidebarSide, TodoSidebarWidthUnit};
use crate::geometry::{Edge, Rect};
use crate::screen_detection::{detect_screens, todo_sidebar_width, ZERO_RECT};
use crate::screens::Screen;

// ---------------------------------------------------------------- окна и система

/// Окно для Todo-режима (`AccessibilityElement` в `TodoManager`).
pub trait TodoWindow {
    /// pid владельца окна.
    fn pid(&self) -> Option<i32>;
    /// `windowId`: номер окна без запасных путей.
    fn window_id(&self) -> Option<u32>;
    /// `getWindowId()`: номер окна с запасными путями (#640).
    fn get_window_id(&self) -> Option<u32>;
    /// Рамка в координатах AX; `None` — не читается.
    fn frame(&self) -> Option<Rect>;
    /// `setFrame` — координаты AX.
    fn set_frame(&self, rect: &Rect);
    /// `bringToFront()`: сделать окно главным и активировать приложение, если
    /// оно не активно.
    fn bring_to_front(&self);
}

/// Всё, что Todo-режиму нужно знать о системе.
pub trait TodoSystem {
    type Window: TodoWindow;
    fn config(&self) -> &Config;
    /// `NSScreen.screens`: `[0]` — основной экран.
    fn screens(&self) -> &[Screen];
    /// Окна запущенного приложения `bundle_id`
    /// (`AccessibilityElement(bundleId)?.windowElements`), переднее — первым.
    /// `None` — приложение не запущено или его окна не узнать.
    fn app_windows(&self, bundle_id: &str) -> Option<Vec<Self::Window>>;
    /// Окно в фокусе (`getFrontWindowElement`).
    fn front_window(&self) -> Option<Self::Window>;
    /// Окна всех приложений, у которых есть окна на экране
    /// (`getAllWindowElements`).
    fn all_windows(&self) -> Vec<Self::Window>;
    /// pid этого процесса (`ProcessInfo.processIdentifier`): свои окна Todo
    /// не двигает.
    fn own_pid(&self) -> i32;
    /// Рабочая область экрана (`adjustedVisibleFrame(ignoreTodo)`), Cocoa.
    /// `sidebar_screen` — экран, у которого стоит панель Todo
    /// (`TodoState::sidebar_screen`): у него из области вычитается панель,
    /// если `ignore_todo` не задан.
    fn adjusted_visible_frame(
        &self,
        screen: &Screen,
        sidebar_screen: Option<u32>,
        ignore_todo: bool,
    ) -> Rect;
}

// ---------------------------------------------------------------- состояние

/// Состояние `TodoManager`: запомненное Todo-окно и экран панели.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TodoState {
    /// `todoWindowId`: номер Todo-окна. `None` — при следующем обращении им
    /// станет первое окно Todo-приложения.
    pub window_id: Option<u32>,
    /// `todoScreen`: экран Todo-окна на момент последней расстановки.
    pub screen: Option<Screen>,
}

impl TodoState {
    /// Todo-окно (`getTodoWindowElement`): запомненное окно Todo-приложения, а
    /// если его нет среди окон приложения — первое из них (оно и запоминается).
    /// Приложение не задано или не запущено — окна нет, запомненное забывается.
    pub fn todo_window<S: TodoSystem>(&mut self, system: &S) -> Option<S::Window> {
        let windows = system
            .config()
            .todo_application
            .as_deref()
            .and_then(|bundle_id| system.app_windows(bundle_id));
        let Some(windows) = windows else {
            self.window_id = None;
            return None;
        };
        let ids: Vec<Option<u32>> = windows.iter().map(TodoWindow::window_id).collect();
        if self
            .window_id
            .is_some_and(|window_id| !ids.contains(&Some(window_id)))
        {
            self.window_id = None;
        }
        if self.window_id.is_none() {
            // Номер первого окна без запасных путей: нет номера — нет и Todo-окна.
            self.window_id = ids.first().copied().flatten();
        }
        let found = self.window_id.and_then(|window_id| {
            let index = ids.iter().position(|id| *id == Some(window_id))?;
            windows.into_iter().nth(index)
        });
        if found.is_none() {
            self.window_id = None;
        }
        found
    }

    /// `hasTodoWindow`.
    pub fn has_todo_window<S: TodoSystem>(&mut self, system: &S) -> bool {
        self.todo_window(system).is_some()
    }

    /// `isTodoWindow(_ windowId:)`: окно с этим номером — Todo-окно.
    pub fn is_todo_window_id<S: TodoSystem>(&mut self, system: &S, window_id: u32) -> bool {
        // У найденного окна номер — запомненный.
        self.todo_window(system).is_some() && self.window_id == Some(window_id)
    }

    /// `isTodoWindow(_ windowElement:)`: номер окна — без запасных путей.
    pub fn is_todo_window<S: TodoSystem>(&mut self, system: &S, window: &S::Window) -> bool {
        window
            .window_id()
            .is_some_and(|window_id| self.is_todo_window_id(system, window_id))
    }

    /// `isTodoWindowFront`: окно в фокусе — Todo-окно.
    pub fn is_todo_window_front<S: TodoSystem>(&mut self, system: &S) -> bool {
        system
            .front_window()
            .is_some_and(|window| self.is_todo_window(system, &window))
    }

    /// `resetTodoWindow`: забыть Todo-окно и выбрать заново — первое окно
    /// Todo-приложения.
    pub fn reset_todo_window<S: TodoSystem>(&mut self, system: &S) {
        self.window_id = None;
        let _ = self.todo_window(system);
    }

    /// `refreshTodoScreen`: экран панели — экран Todo-окна
    /// (`detectScreens(using:)`), а без окна — как для окна в начале координат.
    pub fn refresh_todo_screen<S: TodoSystem>(&mut self, system: &S) {
        let frame = match self.todo_window(system) {
            Some(window) => window.frame(),
            None => Some(ZERO_RECT),
        };
        let screens = system.screens();
        self.screen = detect_screens(frame, screens, system.config(), primary_height(screens))
            .map(|usable| usable.current);
    }

    /// Экран, у которого сейчас стоит панель: для рабочей области действий
    /// (`adjustedVisibleFrame`: Todo включён в меню, режим включён, экран панели
    /// известен и Todo-окно есть).
    pub fn sidebar_screen<S: TodoSystem>(&mut self, system: &S) -> Option<u32> {
        let config = system.config();
        if config.todo != Some(true) || !config.todo_mode {
            return None;
        }
        let screen_id = self.screen.as_ref()?.id;
        self.has_todo_window(system).then_some(screen_id)
    }

    /// `moveAll`: Todo-окно — панелью у края своего экрана, остальные окна
    /// этого экрана уходят с места панели (`shiftWindowOffSidebar`), кроме
    /// окон этого процесса. `bring_to_front` — вывести Todo-окно вперёд.
    pub fn move_all<S: TodoSystem>(&mut self, system: &S, bring_to_front: bool) {
        self.refresh_todo_screen(system);
        let Some(todo_window) = self.todo_window(system) else {
            return;
        };

        if let Some(screen) = self.screen.clone() {
            let config = system.config();
            let screens = system.screens();
            let primary_height = primary_height(screens);

            // `adjustedVisibleFrame()`: Todo-окно только что нашлось, так что
            // панель вычитается, если Todo и режим включены.
            let sidebar_screen =
                (config.todo == Some(true) && config.todo_mode).then_some(screen.id);
            let visible = system.adjusted_visible_frame(&screen, sidebar_screen, false);
            let todo_window_id = todo_window.get_window_id();
            let own_pid = system.own_pid();
            for window in system.all_windows() {
                if window.pid() == Some(own_pid) || window.get_window_id() == todo_window_id {
                    continue;
                }
                let frame = window.frame();
                let on_sidebar_screen = detect_screens(frame, screens, config, primary_height)
                    .is_some_and(|usable| usable.current.same_display(&screen));
                if !on_sidebar_screen {
                    continue;
                }
                // Окно без рамки пропускаем: Swift отправил бы ему мусор (`CGRect.null`).
                if let Some(rect) =
                    frame.and_then(|frame| shift_off_sidebar(&frame, &visible, config))
                {
                    window.set_frame(&rect);
                }
            }

            let visible = system.adjusted_visible_frame(&screen, sidebar_screen, true);
            todo_window.set_frame(&sidebar_rect(&visible, config, primary_height));
        }

        if bring_to_front {
            todo_window.bring_to_front();
        }
    }

    /// `moveAllIfNeeded`: расставить окна, только если Todo включён в меню и
    /// режим включён.
    pub fn move_all_if_needed<S: TodoSystem>(&mut self, system: &S, bring_to_front: bool) {
        let config = system.config();
        if config.todo == Some(true) && config.todo_mode {
            self.move_all(system, bring_to_front);
        }
    }

    /// `changeSidebarWidthUnit(to:)`: ширина панели из настроек, пересчитанная
    /// в единицу `unit` по ширине рабочей области экрана панели без неё самой.
    /// Экрана панели нет — `None`, ширина не меняется.
    pub fn converted_sidebar_width<S: TodoSystem>(
        &self,
        system: &S,
        unit: TodoSidebarWidthUnit,
    ) -> Option<f32> {
        let screen = self.screen.as_ref()?;
        let visible_width = system.adjusted_visible_frame(screen, None, true).w;
        let width = convert_width(
            system.config().todo_sidebar_width as f64,
            unit,
            visible_width,
        );
        Some(width as f32)
    }
}

// ---------------------------------------------------------------- геометрия

/// Действия, которые выполняет Todo-режим (`TodoManager.execute`).
pub fn is_todo_action(action: Action) -> bool {
    matches!(action, Action::LeftTodo | Action::RightTodo)
}

/// `NSScreen.screens[0].frame.maxY`.
fn primary_height(screens: &[Screen]) -> f64 {
    screens
        .first()
        .map(|screen| screen.frame.max_y())
        .unwrap_or(0.0)
}

/// Рамка Todo-окна (AX): полоса шириной панели у её стороны рабочей области
/// `visible` (Cocoa, без самой панели) во всю высоту; с гэпами — гэп со всех
/// сторон, у края, общего с остальными окнами, — половина.
pub fn sidebar_rect(visible: &Rect, config: &Config, primary_height: f64) -> Rect {
    let sidebar_width = todo_sidebar_width(config, visible.w);
    let is_right_side = config.todo_sidebar_side == TodoSidebarSide::Right;
    let shared_edge = if is_right_side {
        Edge::LEFT
    } else {
        Edge::RIGHT
    };

    let mut rect = *visible;
    if is_right_side {
        rect.x = visible.max_x() - sidebar_width;
    }
    rect.w = sidebar_width;
    let rect = rect.screen_flipped(primary_height);

    if config.gap_size > 0.0 {
        return apply_gaps_raw(rect, Dimension::BOTH, shared_edge, config.gap_size, false);
    }
    rect
}

/// `shiftWindowOffSidebar`: новая рамка окна (AX), если оно заходит на место
/// панели. `visible` — рабочая область экрана без панели (Cocoa; по X системы
/// координат совпадают). Окно сдвигается от панели, а если и так не
/// помещается — ещё и сужается. `None` — окно не мешает панели.
pub fn shift_off_sidebar(frame: &Rect, visible: &Rect, config: &Config) -> Option<Rect> {
    let mut rect = *frame;
    let half_gap_width = config.gap_size as f64 / 2.0;
    let min_x = visible.min_x() + half_gap_width;
    let max_x = visible.max_x() - half_gap_width;

    match config.todo_sidebar_side {
        TodoSidebarSide::Left if rect.min_x() < min_x => {
            // Вправо.
            rect.x = (max_x - rect.w).min(min_x);
            // Всё ещё не помещается — сузить.
            if rect.min_x() < min_x {
                let width_diff = min_x - rect.min_x();
                rect.x += width_diff;
                rect.w -= width_diff;
            }
            Some(rect)
        }
        TodoSidebarSide::Right if rect.max_x() > max_x => {
            // Влево.
            rect.x = rect.min_x().min(min_x.max(max_x - rect.w));
            // Всё ещё не помещается — сузить.
            if rect.max_x() > max_x {
                rect.w -= rect.max_x() - max_x;
            }
            Some(rect)
        }
        _ => None,
    }
}

/// `TodoManager.convert(width:toUnit:visibleFrameWidth:)`: проценты → пиксели
/// или пиксели → проценты, с округлением до целого.
pub fn convert_width(width: f64, unit: TodoSidebarWidthUnit, visible_frame_width: f64) -> f64 {
    match unit {
        TodoSidebarWidthUnit::Pixels => ((width * 0.01) * visible_frame_width).round(),
        TodoSidebarWidthUnit::Pct => ((width / visible_frame_width) * 100.0).round(),
    }
}
