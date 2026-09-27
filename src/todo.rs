//! Todo-режим — порт `TodoMode/TodoManager.swift` и todo-частей
//! `ApplicationToggle` и `AppDelegate`: выбранное приложение держится боковой
//! панелью у края экрана своего окна, остальные окна этого экрана уходят с её
//! места, а раскладки считают рабочую область без панели.
//!
//! Когда окна расставляются (`moveAll`), как в оригинале:
//! - при запуске подсистем, то есть когда есть доступ к управлению
//!   компьютером (`initializeTodo()`, с выводом Todo-окна вперёд);
//! - пункты меню: «Включить режим Todo приложения», «Использовать <приложение>
//!   в качестве приложения Todo», «Использовать как окно Todo» (все — если
//!   режим включён), «Обновить положение Todo окна» (всегда);
//! - действия «Todo слева/справа» (`TodoManager.execute`, с любого источника);
//! - смена Todo-настроек из настроек или импорт (`todoMenuToggled`,
//!   `configImported`, поля ширины и стороны панели) — без вывода вперёд.
//!
//! На смену активного приложения, экранов, запуск и завершение приложений
//! `TodoManager` оригинала не подписан, и порт тоже: Todo-окно ищется заново
//! при каждом обращении (`getTodoWindowElement`). Приложение перезапустили —
//! Todo-окном станет его первое окно, завершили — окна нет, и место под панель
//! не оставляется; сама панель встанет на место при следующей расстановке.
//! Шорткатов «Toggle Todo» и «Reflow Todo» нет (решение D1).
//!
//! Логика — в `manager` над трейтами системы; здесь — состояние приложения,
//! настоящая система (`AxTodoSystem`) и API для меню, менеджера окон и окна
//! настроек. Всё — на главном потоке.

mod manager;
#[cfg(test)]
mod tests;

use std::cell::{OnceCell, RefCell};

use objc2::MainThreadMarker;

use crate::ax::{self, AxElement};
use crate::config::{self, Config, TodoSidebarSide, TodoSidebarWidthUnit};
use crate::events;
use crate::geometry::Rect;
use crate::screens::{self, Screen};
use crate::window_manager::{self, ExecutionParameters};

pub use manager::{
    convert_width, is_todo_action, shift_off_sidebar, sidebar_rect, TodoState, TodoSystem,
    TodoWindow,
};

// ---------------------------------------------------------------- система

/// Настоящая система: окна — через AX, настройки и экраны — снимок на момент
/// создания (экраны читаются при первом обращении).
pub struct AxTodoSystem {
    config: Config,
    screens: OnceCell<Vec<Screen>>,
    /// Отладочное ограничение для примеров: Todo-приложение — этот процесс, и
    /// видны (а значит, двигаются) только его окна. В приложении — `None`.
    only_pid: Option<i32>,
}

impl AxTodoSystem {
    /// Система с текущими настройками.
    pub fn current() -> AxTodoSystem {
        AxTodoSystem {
            config: config::current(),
            screens: OnceCell::new(),
            only_pid: None,
        }
    }

    /// Только для примеров и проверок: Todo-приложение — процесс `pid` (его
    /// bundle id не нужен), и Todo-режим видит и двигает только окна этого
    /// процесса. Настройки — `config`, общие настройки приложения не трогаются.
    pub fn only_process(pid: i32, config: Config) -> AxTodoSystem {
        AxTodoSystem {
            config,
            screens: OnceCell::new(),
            only_pid: Some(pid),
        }
    }

    fn allowed(&self, window: &AxElement) -> bool {
        self.only_pid.is_none() || window.pid() == self.only_pid
    }
}

impl TodoSystem for AxTodoSystem {
    type Window = AxElement;

    fn config(&self) -> &Config {
        &self.config
    }

    fn screens(&self) -> &[Screen] {
        self.screens.get_or_init(screens::screens)
    }

    fn app_windows(&self, bundle_id: &str) -> Option<Vec<AxElement>> {
        match self.only_pid {
            Some(pid) => AxElement::application(pid).window_elements(),
            None => AxElement::for_bundle_id(bundle_id)?.window_elements(),
        }
    }

    fn front_window(&self) -> Option<AxElement> {
        ax::front_window().filter(|window| self.allowed(window))
    }

    fn all_windows(&self) -> Vec<AxElement> {
        match self.only_pid {
            Some(pid) => AxElement::application(pid)
                .window_elements()
                .unwrap_or_default(),
            None => ax::all_window_elements(),
        }
    }

    fn own_pid(&self) -> i32 {
        std::process::id() as i32
    }

    fn adjusted_visible_frame(
        &self,
        screen: &Screen,
        sidebar_screen: Option<u32>,
        ignore_todo: bool,
    ) -> Rect {
        window_manager::adjusted_visible_frame_with_todo(
            &self.config,
            self.screens(),
            screen,
            sidebar_screen,
            ignore_todo,
        )
    }
}

impl TodoWindow for AxElement {
    fn pid(&self) -> Option<i32> {
        AxElement::pid(self)
    }

    fn window_id(&self) -> Option<u32> {
        AxElement::window_id(self)
    }

    fn get_window_id(&self) -> Option<u32> {
        AxElement::get_window_id(self)
    }

    fn frame(&self) -> Option<Rect> {
        AxElement::frame(self)
    }

    fn set_frame(&self, rect: &Rect) {
        AxElement::set_frame(self, rect);
    }

    fn bring_to_front(&self) {
        AxElement::bring_to_front(self, false);
    }
}

// ---------------------------------------------------------------- состояние

thread_local! {
    /// `todoWindowId` и `todoScreen` оригинала.
    static STATE: RefCell<TodoState> = RefCell::new(TodoState::default());
    /// Todo-настройки, которые подсистема уже учла (см. `reload`).
    static SEEN: RefCell<Option<TodoSettings>> = const { RefCell::new(None) };
}

/// Работа с состоянием на настоящей системе. Состояние берётся копией и
/// кладётся обратно: пока идут вызовы AX, заём `RefCell` не держится, и
/// вложенный вызов (например, `is_todo_window` из обработчика уведомления) не
/// уронит программу.
fn with_state<R>(f: impl FnOnce(&mut TodoState, &AxTodoSystem) -> R) -> R {
    let system = AxTodoSystem::current();
    let mut state = STATE.with(|state| state.borrow().clone());
    let result = f(&mut state, &system);
    STATE.with(|slot| *slot.borrow_mut() = state);
    result
}

/// Todo-настройки, на смену которых откликается `reload`.
#[derive(Clone, Debug, PartialEq)]
struct TodoSettings {
    todo: Option<bool>,
    todo_mode: bool,
    todo_application: Option<String>,
    width: f32,
    unit: TodoSidebarWidthUnit,
    side: TodoSidebarSide,
}

impl TodoSettings {
    fn of(config: &Config) -> TodoSettings {
        TodoSettings {
            todo: config.todo,
            todo_mode: config.todo_mode,
            todo_application: config.todo_application.clone(),
            width: config.todo_sidebar_width,
            unit: config.todo_sidebar_width_unit,
            side: config.todo_sidebar_side,
        }
    }
}

/// Запомнить Todo-настройки как учтённые; `true` — они изменились с прошлого раза.
fn remember_settings(config: &Config) -> bool {
    let settings = TodoSettings::of(config);
    SEEN.with(|seen| seen.borrow_mut().replace(settings.clone()) != Some(settings))
}

/// Изменить настройки из самого Todo-режима (меню): `reload` их уже учёл и
/// окна не двигает — это делает вызывающий, со своим `bring_to_front`.
fn update_config(f: impl Fn(&mut Config)) {
    let mut preview = config::current();
    f(&mut preview);
    remember_settings(&preview);
    config::update(f);
}

// ---------------------------------------------------------------- подсистема

/// Запуск подсистемы (`initializeTodo()` в `accessibilityTrusted`): если режим
/// включён — расставить окна и вывести Todo-окно вперёд. Вызывается из
/// `subsystems::start_all`.
pub fn install(_mtm: MainThreadMarker) {
    remember_settings(&config::current());
    initialize(true);
}

/// Настройки изменились (`subsystems::reload_all`): если поменялись
/// Todo-настройки — флажок «Показывать Todo режим в меню» (`todoMenuToggled`),
/// ширина, единица или сторона панели, импорт настроек (`configImported`) —
/// расставить окна без вывода вперёд (`initializeTodo(false)`,
/// `moveAllIfNeeded(false)`).
pub fn reload() {
    if remember_settings(&config::current()) {
        initialize(false);
    }
}

/// `AppDelegate.initializeTodo`: пункты меню Todo меню читает само при
/// открытии, шорткатов нет — остаётся `moveAllIfNeeded`.
pub fn initialize(bring_to_front: bool) {
    move_all_if_needed(bring_to_front);
}

// ---------------------------------------------------------------- меню

/// Пункты Todo есть в меню: включено «Показывать Todo режим в меню»
/// (`Defaults.todo.userEnabled`).
pub fn is_enabled() -> bool {
    config::with(|config| config.todo == Some(true))
}

/// Todo-режим включён (`Defaults.todoMode`).
pub fn is_mode_on() -> bool {
    config::with(|config| config.todo_mode)
}

/// «Включить режим Todo приложения»: `TodoManager.setTodoMode(!todoMode)`.
pub fn toggle_mode() {
    set_mode(!is_mode_on(), true);
}

/// `TodoManager.setTodoMode`: включить или выключить режим и, если он
/// включён, расставить окна.
pub fn set_mode(enabled: bool, bring_to_front: bool) {
    update_config(|config| config.todo_mode = enabled);
    move_all_if_needed(bring_to_front);
}

/// Активное приложение — Todo-приложение (`ApplicationToggle.todoAppIsActive`).
pub fn is_todo_app_active() -> bool {
    let front = events::front_app().and_then(|app| app.bundle_id);
    config::with(|config| config.todo_application == front)
}

/// Активное окно — Todo-окно (`TodoManager.isTodoWindowFront`).
pub fn is_todo_window_front() -> bool {
    with_state(|state, system| state.is_todo_window_front(system))
}

/// «Использовать <приложение> в качестве приложения Todo»:
/// `ApplicationToggle.setTodoApp` и `TodoManager.moveAllIfNeeded`.
pub fn set_todo_app_to_frontmost() {
    let front = events::front_app().and_then(|app| app.bundle_id);
    update_config(|config| config.todo_application = front.clone());
    move_all_if_needed(true);
}

/// «Использовать как окно Todo»: `TodoManager.resetTodoWindow` и
/// `moveAllIfNeeded` — Todo-окном становится первое окно Todo-приложения (пункт
/// виден, когда оно впереди, — это его переднее окно).
pub fn set_todo_window_to_front() {
    with_state(|state, system| state.reset_todo_window(system));
    move_all_if_needed(true);
}

/// «Обновить положение Todo окна»: `TodoManager.moveAll`.
pub fn reflow() {
    move_all(true);
}

// ---------------------------------------------------------------- расстановка

/// `TodoManager.moveAll`: Todo-окно — панелью, остальные окна его экрана — с
/// её места; `bring_to_front` — вывести Todo-окно вперёд.
pub fn move_all(bring_to_front: bool) {
    with_state(|state, system| state.move_all(system, bring_to_front));
}

/// `TodoManager.moveAllIfNeeded`: то же, если Todo и режим включены.
pub fn move_all_if_needed(bring_to_front: bool) {
    with_state(|state, system| state.move_all_if_needed(system, bring_to_front));
}

/// `TodoManager.execute`: «Todo слева/справа» выполняет Todo-режим —
/// расстановкой окон (`true`); прочие действия — `false`, систему не трогаем.
pub fn execute(params: &ExecutionParameters) -> bool {
    if !is_todo_action(params.action) {
        return false;
    }
    move_all(true);
    true
}

// ---------------------------------------------------------------- менеджер окон

/// `TodoManager.isTodoWindow(_ windowId:)`: окно — боковая панель Todo.
pub fn is_todo_window(window_id: u32) -> bool {
    with_state(|state, system| state.is_todo_window_id(system, window_id))
}

/// Экран, у которого стоит панель Todo, — для рабочей области действий
/// (`adjustedVisibleFrame`): Todo и режим включены и Todo-окно есть.
pub fn sidebar_screen() -> Option<u32> {
    // Без режима — не спрашиваем AX.
    if !config::with(|config| config.todo == Some(true) && config.todo_mode) {
        return None;
    }
    with_state(|state, system| state.sidebar_screen(system))
}

// ---------------------------------------------------------------- окно настроек

/// Попап единицы ширины панели в настройках (`setTodoWidthUnit`): ширина
/// пересчитывается px ↔ % по рабочей области экрана панели
/// (`refreshTodoScreen`, `changeSidebarWidthUnit(to:)`) и записывается вместе
/// с единицей; окна расставит `reload` (`moveAllIfNeeded(false)`). Возвращает
/// ширину для поля ввода.
///
/// Та же единица ещё раз ширину не пересчитывает: в оригинале повторный выбор
/// «px» превращал 400 px в 400 % ширины экрана.
pub fn set_sidebar_width_unit(unit: TodoSidebarWidthUnit) -> f32 {
    let (current_unit, current_width) =
        config::with(|config| (config.todo_sidebar_width_unit, config.todo_sidebar_width));
    if unit == current_unit {
        return current_width;
    }
    let width = with_state(|state, system| {
        state.refresh_todo_screen(system);
        state.converted_sidebar_width(system, unit)
    });
    config::update(|config| {
        config.todo_sidebar_width_unit = unit;
        if let Some(width) = width {
            config.todo_sidebar_width = width;
        }
    });
    config::with(|config| config.todo_sidebar_width)
}
