//! Todo-режим на макетах: геометрия панели и рабочей области, выбор
//! Todo-окна, расстановка окон.

use std::cell::RefCell;
use std::rc::Rc;

use super::*;
use crate::actions::Action;
use crate::screen_detection::{self, ScreenEnvironment};
use crate::stage::StageState;

// ---------------------------------------------------------------- макеты

type Journal = Rc<RefCell<Vec<String>>>;

/// Окно-макет: рамка меняется по `set_frame`, всё записывается в журнал.
#[derive(Clone)]
struct MockWindow(Rc<MockWindowData>);

struct MockWindowData {
    name: &'static str,
    pid: Option<i32>,
    /// `windowId`.
    id: Option<u32>,
    /// Номер из запасных путей `getWindowId()`, если `id` нет.
    fallback_id: Option<u32>,
    frame: RefCell<Option<Rect>>,
    journal: Journal,
}

impl TodoWindow for MockWindow {
    fn pid(&self) -> Option<i32> {
        self.0.pid
    }

    fn window_id(&self) -> Option<u32> {
        self.0.id
    }

    fn get_window_id(&self) -> Option<u32> {
        self.0.id.or(self.0.fallback_id)
    }

    fn frame(&self) -> Option<Rect> {
        *self.0.frame.borrow()
    }

    fn set_frame(&self, rect: &Rect) {
        *self.0.frame.borrow_mut() = Some(*rect);
        self.0.journal.borrow_mut().push(format!(
            "{} → {} {} {} {}",
            self.0.name, rect.x, rect.y, rect.w, rect.h
        ));
    }

    fn bring_to_front(&self) {
        self.0
            .journal
            .borrow_mut()
            .push(format!("{} вперёд", self.0.name));
    }
}

const TODO_APP: &str = "com.example.todo";
const OWN_PID: i32 = 99;

struct MockSystem {
    config: Config,
    screens: Vec<Screen>,
    /// Запущенные приложения: bundle id и окна, переднее — первым.
    apps: Vec<(&'static str, Vec<MockWindow>)>,
    /// `getAllWindowElements`.
    all: Vec<MockWindow>,
    front: Option<MockWindow>,
    journal: Journal,
}

impl MockSystem {
    fn new(config: Config) -> MockSystem {
        MockSystem {
            config,
            screens: vec![laptop(), monitor()],
            apps: Vec::new(),
            all: Vec::new(),
            front: None,
            journal: Rc::default(),
        }
    }

    /// Окно приложения `bundle_id` (`None` — окно не Todo-приложения) с
    /// рамкой AX `frame`; попадает и в список всех окон.
    fn window(
        &mut self,
        name: &'static str,
        bundle_id: Option<&'static str>,
        pid: i32,
        id: Option<u32>,
        frame: Option<Rect>,
    ) -> MockWindow {
        let window = MockWindow(Rc::new(MockWindowData {
            name,
            pid: Some(pid),
            id,
            fallback_id: None,
            frame: RefCell::new(frame),
            journal: self.journal.clone(),
        }));
        if let Some(bundle_id) = bundle_id {
            match self.apps.iter_mut().find(|(id, _)| *id == bundle_id) {
                Some((_, windows)) => windows.push(window.clone()),
                None => self.apps.push((bundle_id, vec![window.clone()])),
            }
        }
        self.all.push(window.clone());
        window
    }

    fn journal(&self) -> Vec<String> {
        self.journal.borrow().clone()
    }
}

impl TodoSystem for MockSystem {
    type Window = MockWindow;

    fn config(&self) -> &Config {
        &self.config
    }

    fn screens(&self) -> &[Screen] {
        &self.screens
    }

    fn app_windows(&self, bundle_id: &str) -> Option<Vec<MockWindow>> {
        self.apps
            .iter()
            .find(|(id, _)| *id == bundle_id)
            .map(|(_, windows)| windows.clone())
    }

    fn front_window(&self) -> Option<MockWindow> {
        self.front.clone()
    }

    fn all_windows(&self) -> Vec<MockWindow> {
        self.all.clone()
    }

    fn own_pid(&self) -> i32 {
        OWN_PID
    }

    fn adjusted_visible_frame(
        &self,
        screen: &Screen,
        sidebar_screen: Option<u32>,
        ignore_todo: bool,
    ) -> Rect {
        let env = ScreenEnvironment {
            config: &self.config,
            screens: &self.screens,
            separate_spaces: true,
            stage: StageState::default(),
            todo_screen: sidebar_screen,
        };
        screen_detection::adjusted_visible_frame(screen, &env, ignore_todo, false)
    }
}

/// Ноутбук 1728×1117, меню-бар 32 (в AX рабочая область начинается с y = 32).
fn laptop() -> Screen {
    Screen {
        id: 1,
        frame: Rect::new(0.0, 0.0, 1728.0, 1117.0),
        visible_frame: Rect::new(0.0, 0.0, 1728.0, 1085.0),
        name: "ноутбук".to_string(),
        is_main: true,
        scale: 2.0,
        safe_area_top: 32.0,
    }
}

/// Монитор справа, ниже верха ноутбука: в AX рабочая область — с y = 25.
fn monitor() -> Screen {
    Screen {
        id: 2,
        frame: Rect::new(1728.0, -323.0, 2560.0, 1440.0),
        visible_frame: Rect::new(1728.0, -323.0, 2560.0, 1415.0),
        name: "монитор".to_string(),
        is_main: false,
        scale: 1.0,
        safe_area_top: 0.0,
    }
}

const PRIMARY_HEIGHT: f64 = 1117.0;

/// Todo включён в меню, режим включён, Todo-приложение задано; панель справа
/// 400 px.
fn todo_config() -> Config {
    Config {
        todo: Some(true),
        todo_mode: true,
        todo_application: Some(TODO_APP.to_string()),
        ..Config::default()
    }
}

fn r(x: f64, y: f64, w: f64, h: f64) -> Rect {
    Rect::new(x, y, w, h)
}

// ---------------------------------------------------------------- геометрия

#[test]
fn sidebar_takes_its_side_of_the_screen() {
    let visible = laptop().visible_frame;
    let mut config = todo_config();
    // Справа 400 px во всю высоту рабочей области (AX: сверху y = 32).
    assert_eq!(
        sidebar_rect(&visible, &config, PRIMARY_HEIGHT),
        r(1328.0, 32.0, 400.0, 1085.0)
    );

    config.todo_sidebar_side = TodoSidebarSide::Left;
    assert_eq!(
        sidebar_rect(&visible, &config, PRIMARY_HEIGHT),
        r(0.0, 32.0, 400.0, 1085.0)
    );

    // Проценты: round(25 % от 1728) = 432.
    config.todo_sidebar_side = TodoSidebarSide::Right;
    config.todo_sidebar_width = 25.0;
    config.todo_sidebar_width_unit = TodoSidebarWidthUnit::Pct;
    assert_eq!(
        sidebar_rect(&visible, &config, PRIMARY_HEIGHT),
        r(1296.0, 32.0, 432.0, 1085.0)
    );

    // Ширина до 1 — доля ширины (Float, как `Defaults.todoSidebarWidth`).
    config.todo_sidebar_width = 0.3;
    let width = 0.3f32 as f64 * 1728.0;
    assert_eq!(
        sidebar_rect(&visible, &config, PRIMARY_HEIGHT),
        r(1728.0 - width, 32.0, width, 1085.0)
    );
}

#[test]
fn sidebar_gaps_are_halved_on_the_shared_edge() {
    let visible = laptop().visible_frame;
    let mut config = Config {
        gap_size: 10.0,
        ..todo_config()
    };
    // Гэп 10 со всех сторон, у левого (общего с окнами) края — 5.
    assert_eq!(
        sidebar_rect(&visible, &config, PRIMARY_HEIGHT),
        r(1333.0, 42.0, 385.0, 1065.0)
    );
    config.todo_sidebar_side = TodoSidebarSide::Left;
    assert_eq!(
        sidebar_rect(&visible, &config, PRIMARY_HEIGHT),
        r(10.0, 42.0, 385.0, 1065.0)
    );
}

#[test]
fn sidebar_on_a_secondary_screen_is_flipped_by_the_primary_height() {
    // Монитор ниже верха основного экрана: AX y = 1117 − (−323 + 1415) = 25.
    let visible = monitor().visible_frame;
    assert_eq!(
        sidebar_rect(&visible, &todo_config(), PRIMARY_HEIGHT),
        r(3888.0, 25.0, 400.0, 1415.0)
    );
}

#[test]
fn work_area_without_the_sidebar_px_and_pct() {
    let system = MockSystem::new(todo_config());
    let laptop = laptop();
    // Справа 400 px: область короче на 400, начало на месте.
    assert_eq!(
        system.adjusted_visible_frame(&laptop, Some(1), false),
        r(0.0, 0.0, 1328.0, 1085.0)
    );
    // Экран без панели и окно самой панели — вся область.
    assert_eq!(
        system.adjusted_visible_frame(&laptop, Some(2), false),
        laptop.visible_frame
    );
    assert_eq!(
        system.adjusted_visible_frame(&laptop, Some(1), true),
        laptop.visible_frame
    );

    // Слева 25 % (432 px): область сдвигается вправо.
    let system = MockSystem::new(Config {
        todo_sidebar_side: TodoSidebarSide::Left,
        todo_sidebar_width: 25.0,
        todo_sidebar_width_unit: TodoSidebarWidthUnit::Pct,
        ..todo_config()
    });
    assert_eq!(
        system.adjusted_visible_frame(&laptop, Some(1), false),
        r(432.0, 0.0, 1296.0, 1085.0)
    );
    // На мониторе 25 % — это 640 px.
    let monitor = monitor();
    assert_eq!(
        system.adjusted_visible_frame(&monitor, Some(2), false),
        r(1728.0 + 640.0, -323.0, 1920.0, 1415.0)
    );
}

#[test]
fn window_is_shifted_off_a_right_sidebar() {
    let config = todo_config();
    // Рабочая область без панели 400 px справа.
    let visible = r(0.0, 0.0, 1328.0, 1085.0);
    // Не заходит на панель — не трогаем, даже вплотную.
    assert_eq!(
        shift_off_sidebar(&r(100.0, 50.0, 500.0, 400.0), &visible, &config),
        None
    );
    assert_eq!(
        shift_off_sidebar(&r(828.0, 50.0, 500.0, 400.0), &visible, &config),
        None
    );
    // Заходит — сдвигается влево вплотную к панели.
    assert_eq!(
        shift_off_sidebar(&r(1000.0, 50.0, 500.0, 400.0), &visible, &config),
        Some(r(828.0, 50.0, 500.0, 400.0))
    );
    // Шире области — к левому краю и сужается.
    assert_eq!(
        shift_off_sidebar(&r(50.0, 50.0, 1600.0, 400.0), &visible, &config),
        Some(r(0.0, 50.0, 1328.0, 400.0))
    );
    // Уже вылезает за левый край — левый край остаётся, окно сужается.
    assert_eq!(
        shift_off_sidebar(&r(-100.0, 50.0, 1600.0, 400.0), &visible, &config),
        Some(r(-100.0, 50.0, 1428.0, 400.0))
    );

    // С гэпом 10 до панели и до края экрана — по половине гэпа.
    let config = Config {
        gap_size: 10.0,
        ..config
    };
    assert_eq!(
        shift_off_sidebar(&r(1000.0, 50.0, 500.0, 400.0), &visible, &config),
        Some(r(823.0, 50.0, 500.0, 400.0))
    );
    // У левого края экрана окно остаётся где было (x = 0 левее 5) и сужается.
    assert_eq!(
        shift_off_sidebar(&r(0.0, 50.0, 1600.0, 400.0), &visible, &config),
        Some(r(0.0, 50.0, 1323.0, 400.0))
    );
}

#[test]
fn window_is_shifted_off_a_left_sidebar() {
    let config = Config {
        todo_sidebar_side: TodoSidebarSide::Left,
        ..todo_config()
    };
    let visible = r(400.0, 0.0, 1328.0, 1085.0);
    assert_eq!(
        shift_off_sidebar(&r(500.0, 50.0, 500.0, 400.0), &visible, &config),
        None
    );
    assert_eq!(
        shift_off_sidebar(&r(100.0, 50.0, 500.0, 400.0), &visible, &config),
        Some(r(400.0, 50.0, 500.0, 400.0))
    );
    // Не помещается: вправо до края экрана, потом сужается слева.
    assert_eq!(
        shift_off_sidebar(&r(0.0, 50.0, 1600.0, 400.0), &visible, &config),
        Some(r(400.0, 50.0, 1328.0, 400.0))
    );
    // Правая панель окно слева не трогает, и наоборот.
    let right = todo_config();
    assert_eq!(
        shift_off_sidebar(&r(100.0, 50.0, 500.0, 400.0), &visible, &right),
        None
    );
}

#[test]
fn width_converts_between_pixels_and_percent() {
    assert_eq!(
        convert_width(400.0, TodoSidebarWidthUnit::Pct, 1728.0),
        23.0
    );
    assert_eq!(
        convert_width(25.0, TodoSidebarWidthUnit::Pixels, 1728.0),
        432.0
    );
    // Округление — до ближайшего, половина — от нуля (`rounded()`).
    assert_eq!(
        convert_width(50.0, TodoSidebarWidthUnit::Pixels, 1001.0),
        501.0
    );
}

#[test]
fn only_todo_actions_are_taken() {
    assert!(is_todo_action(Action::LeftTodo));
    assert!(is_todo_action(Action::RightTodo));
    assert!(!is_todo_action(Action::LeftHalf));
    assert!(!is_todo_action(Action::TileAll));
    // Прочие действия Todo не трогает и систему не спрашивает.
    assert!(!execute(&ExecutionParameters::menu(Action::LeftHalf)));
}

// ---------------------------------------------------------------- Todo-окно

#[test]
fn todo_window_is_the_first_window_until_another_is_chosen() {
    let mut system = MockSystem::new(todo_config());
    system.window("первое", Some(TODO_APP), 20, Some(11), None);
    system.window("второе", Some(TODO_APP), 20, Some(12), None);
    let mut state = TodoState::default();

    let window = state.todo_window(&system).unwrap();
    assert_eq!(window.0.name, "первое");
    assert_eq!(state.window_id, Some(11));

    // Запомненное окно остаётся Todo-окном, даже если оно не первое.
    state.window_id = Some(12);
    assert_eq!(state.todo_window(&system).unwrap().0.name, "второе");
    assert_eq!(state.window_id, Some(12));

    // Запомненное окно закрыли — снова первое.
    state.window_id = Some(99);
    assert_eq!(state.todo_window(&system).unwrap().0.name, "первое");
    assert_eq!(state.window_id, Some(11));

    // «Использовать как окно Todo» — заново первое окно.
    state.window_id = Some(12);
    state.reset_todo_window(&system);
    assert_eq!(state.window_id, Some(11));
}

#[test]
fn no_todo_window_without_a_running_todo_app() {
    let mut system = MockSystem::new(todo_config());
    system.window("чужое", Some("com.example.other"), 30, Some(21), None);
    let mut state = TodoState {
        window_id: Some(21),
        screen: None,
    };
    // Приложение не запущено — окна нет, запомненное забыто.
    assert!(state.todo_window(&system).is_none());
    assert_eq!(state.window_id, None);

    // Приложение не задано.
    system.config.todo_application = None;
    state.window_id = Some(21);
    assert!(!state.has_todo_window(&system));
    assert_eq!(state.window_id, None);

    // Запущено, но без окон.
    system.config.todo_application = Some(TODO_APP.to_string());
    system.apps.push((TODO_APP, Vec::new()));
    assert!(!state.has_todo_window(&system));
    assert_eq!(state.window_id, None);
}

#[test]
fn first_window_without_a_number_means_no_todo_window() {
    let mut system = MockSystem::new(todo_config());
    system.window("без номера", Some(TODO_APP), 20, None, None);
    system.window("с номером", Some(TODO_APP), 20, Some(12), None);
    let mut state = TodoState::default();
    // Как в оригинале: берётся номер первого окна, а его нет.
    assert!(state.todo_window(&system).is_none());
    assert_eq!(state.window_id, None);
    // Но выбранное раньше окно с номером находится.
    state.window_id = Some(12);
    assert_eq!(state.todo_window(&system).unwrap().0.name, "с номером");
}

#[test]
fn todo_window_is_recognised_by_its_number() {
    let mut system = MockSystem::new(todo_config());
    let todo = system.window("todo", Some(TODO_APP), 20, Some(11), None);
    let other = system.window("другое", Some(TODO_APP), 20, Some(12), None);
    let foreign = system.window("чужое", None, 30, Some(21), None);
    let nameless = system.window("без номера", None, 30, None, None);
    let mut state = TodoState::default();

    assert!(state.is_todo_window_id(&system, 11));
    assert!(!state.is_todo_window_id(&system, 12));
    assert!(state.is_todo_window(&system, &todo));
    assert!(!state.is_todo_window(&system, &other));
    assert!(!state.is_todo_window(&system, &nameless));

    assert!(!state.is_todo_window_front(&system));
    for (front, expected) in [(todo, true), (other, false), (foreign, false)] {
        system.front = Some(front);
        assert_eq!(state.is_todo_window_front(&system), expected);
    }
}

#[test]
fn sidebar_screen_needs_the_mode_a_screen_and_a_todo_window() {
    let mut system = MockSystem::new(todo_config());
    system.window(
        "todo",
        Some(TODO_APP),
        20,
        Some(11),
        Some(r(100.0, 100.0, 500.0, 600.0)),
    );
    let mut state = TodoState::default();
    // Экран панели ещё не известен (расстановки не было).
    assert_eq!(state.sidebar_screen(&system), None);

    state.refresh_todo_screen(&system);
    assert_eq!(state.screen.as_ref().map(|screen| screen.id), Some(1));
    assert_eq!(state.sidebar_screen(&system), Some(1));

    system.config.todo_mode = false;
    assert_eq!(state.sidebar_screen(&system), None);
    system.config.todo_mode = true;
    system.config.todo = None;
    assert_eq!(state.sidebar_screen(&system), None);
    system.config.todo = Some(true);
    // Todo-приложение завершилось — места под панель нет.
    system.apps.clear();
    assert_eq!(state.sidebar_screen(&system), None);
}

#[test]
fn sidebar_screen_is_the_screen_of_the_todo_window() {
    let mut system = MockSystem::new(todo_config());
    let todo = system.window(
        "todo",
        Some(TODO_APP),
        20,
        Some(11),
        Some(r(3000.0, 100.0, 500.0, 600.0)),
    );
    let mut state = TodoState::default();
    state.refresh_todo_screen(&system);
    assert_eq!(state.screen.as_ref().map(|screen| screen.id), Some(2));

    // Без Todo-окна — экран окна в начале координат, то есть основной.
    *todo.0.frame.borrow_mut() = None;
    system.apps.clear();
    state.refresh_todo_screen(&system);
    assert_eq!(state.screen.as_ref().map(|screen| screen.id), Some(1));
}

// ---------------------------------------------------------------- расстановка

#[test]
fn move_all_puts_the_todo_window_right_and_clears_its_place() {
    let mut system = MockSystem::new(todo_config());
    system.window(
        "todo",
        Some(TODO_APP),
        20,
        Some(11),
        Some(r(100.0, 100.0, 500.0, 600.0)),
    );
    let beside = system.window(
        "рядом",
        None,
        30,
        Some(21),
        Some(r(100.0, 300.0, 400.0, 300.0)),
    );
    system.window(
        "на краю",
        None,
        30,
        Some(22),
        Some(r(1000.0, 100.0, 500.0, 400.0)),
    );
    system.window(
        "широкое",
        None,
        30,
        Some(23),
        Some(r(50.0, 200.0, 1600.0, 400.0)),
    );
    // Другие окна Todo-приложения — как все.
    system.window(
        "todo 2",
        Some(TODO_APP),
        20,
        Some(12),
        Some(r(1400.0, 100.0, 300.0, 300.0)),
    );
    // Другой экран, свои окна и окна без рамки не трогаем.
    let on_monitor = system.window(
        "на мониторе",
        None,
        30,
        Some(24),
        Some(r(3500.0, 100.0, 700.0, 400.0)),
    );
    let own = system.window(
        "своё",
        None,
        OWN_PID,
        Some(25),
        Some(r(1500.0, 100.0, 200.0, 200.0)),
    );
    system.window("без рамки", None, 30, Some(26), None);

    let mut state = TodoState::default();
    state.move_all(&system, true);

    assert_eq!(
        system.journal(),
        vec![
            "на краю → 828 100 500 400",
            "широкое → 0 200 1328 400",
            "todo 2 → 1028 100 300 300",
            "todo → 1328 32 400 1085",
            "todo вперёд",
        ]
    );
    assert_eq!(beside.frame(), Some(r(100.0, 300.0, 400.0, 300.0)));
    assert_eq!(on_monitor.frame(), Some(r(3500.0, 100.0, 700.0, 400.0)));
    assert_eq!(own.frame(), Some(r(1500.0, 100.0, 200.0, 200.0)));
    assert_eq!(state.window_id, Some(11));
    assert_eq!(state.screen.as_ref().map(|screen| screen.id), Some(1));
}

#[test]
fn move_all_left_in_percent_with_gaps() {
    let mut system = MockSystem::new(Config {
        todo_sidebar_side: TodoSidebarSide::Left,
        todo_sidebar_width: 25.0,
        todo_sidebar_width_unit: TodoSidebarWidthUnit::Pct,
        gap_size: 10.0,
        ..todo_config()
    });
    system.window(
        "todo",
        Some(TODO_APP),
        20,
        Some(11),
        Some(r(900.0, 100.0, 500.0, 600.0)),
    );
    system.window(
        "слева",
        None,
        30,
        Some(21),
        Some(r(100.0, 100.0, 500.0, 400.0)),
    );
    system.window(
        "справа",
        None,
        30,
        Some(22),
        Some(r(800.0, 100.0, 500.0, 400.0)),
    );

    let mut state = TodoState::default();
    state.move_all(&system, false);

    // Панель 432 px: окна — не левее 432 + 5, Todo-окно — с гэпами.
    assert_eq!(
        system.journal(),
        vec!["слева → 437 100 500 400", "todo → 10 42 417 1065"]
    );
}

#[test]
fn todo_window_on_a_secondary_screen_takes_that_screen() {
    let mut system = MockSystem::new(todo_config());
    system.window(
        "todo",
        Some(TODO_APP),
        20,
        Some(11),
        Some(r(2000.0, 100.0, 500.0, 600.0)),
    );
    // На ноутбуке, у правого края — но панель не там.
    system.window(
        "ноутбук",
        None,
        30,
        Some(21),
        Some(r(1300.0, 100.0, 400.0, 400.0)),
    );
    system.window(
        "монитор",
        None,
        30,
        Some(22),
        Some(r(3900.0, 100.0, 300.0, 400.0)),
    );

    let mut state = TodoState::default();
    state.move_all(&system, false);

    // Рабочая область монитора без панели кончается на 1728 + 2560 − 400 = 3888.
    assert_eq!(
        system.journal(),
        vec!["монитор → 3588 100 300 400", "todo → 3888 25 400 1415"]
    );
    assert_eq!(state.screen.as_ref().map(|screen| screen.id), Some(2));
}

#[test]
fn reflow_without_the_mode_still_places_the_todo_window() {
    // «Обновить положение Todo окна» и «Todo слева/справа» работают и при
    // выключенном режиме: место под панель тогда не оставляется, окна не
    // сдвигаются, а Todo-окно всё равно встаёт панелью.
    let mut system = MockSystem::new(Config {
        todo_mode: false,
        ..todo_config()
    });
    system.window(
        "todo",
        Some(TODO_APP),
        20,
        Some(11),
        Some(r(100.0, 100.0, 500.0, 600.0)),
    );
    system.window(
        "на краю",
        None,
        30,
        Some(21),
        Some(r(1000.0, 100.0, 500.0, 400.0)),
    );

    let mut state = TodoState::default();
    state.move_all_if_needed(&system, true);
    assert!(system.journal().is_empty());

    state.move_all(&system, true);
    assert_eq!(
        system.journal(),
        vec!["todo → 1328 32 400 1085", "todo вперёд"]
    );
}

#[test]
fn move_all_if_needed_needs_the_menu_items_too() {
    let mut system = MockSystem::new(Config {
        todo: Some(false),
        ..todo_config()
    });
    system.window(
        "todo",
        Some(TODO_APP),
        20,
        Some(11),
        Some(r(100.0, 100.0, 500.0, 600.0)),
    );
    let mut state = TodoState::default();
    state.move_all_if_needed(&system, false);
    assert!(system.journal().is_empty());

    system.config.todo = Some(true);
    state.move_all_if_needed(&system, false);
    assert_eq!(system.journal(), vec!["todo → 1328 32 400 1085"]);
}

#[test]
fn nothing_moves_without_a_todo_window() {
    let mut system = MockSystem::new(todo_config());
    system.window(
        "на краю",
        None,
        30,
        Some(21),
        Some(r(1000.0, 100.0, 500.0, 400.0)),
    );
    let mut state = TodoState::default();
    state.move_all(&system, true);
    assert!(system.journal().is_empty());
    // Экран панели всё равно обновился — основной.
    assert_eq!(state.screen.as_ref().map(|screen| screen.id), Some(1));
}

#[test]
fn window_with_the_todo_number_from_fallbacks_is_the_todo_window() {
    // `getWindowId()` с запасными путями у окна из списка совпал с номером
    // Todo-окна — это оно и есть, его не сдвигают.
    let mut system = MockSystem::new(todo_config());
    system.window(
        "todo",
        Some(TODO_APP),
        20,
        Some(11),
        Some(r(1300.0, 100.0, 500.0, 600.0)),
    );
    let twin = MockWindow(Rc::new(MockWindowData {
        name: "двойник",
        pid: Some(20),
        id: None,
        fallback_id: Some(11),
        frame: RefCell::new(Some(r(1300.0, 100.0, 500.0, 600.0))),
        journal: system.journal.clone(),
    }));
    system.all.push(twin);
    let mut state = TodoState::default();
    state.move_all(&system, false);
    assert_eq!(system.journal(), vec!["todo → 1328 32 400 1085"]);
}

// ---------------------------------------------------------------- настройки

#[test]
fn width_is_converted_by_the_sidebar_screen() {
    let mut system = MockSystem::new(todo_config());
    let mut state = TodoState::default();
    // Экрана панели нет — не пересчитываем.
    assert_eq!(
        state.converted_sidebar_width(&system, TodoSidebarWidthUnit::Pct),
        None
    );

    state.screen = Some(laptop());
    // 400 из 1728 px (без панели не вычитается) — 23 %.
    assert_eq!(
        state.converted_sidebar_width(&system, TodoSidebarWidthUnit::Pct),
        Some(23.0)
    );
    system.config.todo_sidebar_width = 25.0;
    system.config.todo_sidebar_width_unit = TodoSidebarWidthUnit::Pct;
    assert_eq!(
        state.converted_sidebar_width(&system, TodoSidebarWidthUnit::Pixels),
        Some(432.0)
    );
    state.screen = Some(monitor());
    assert_eq!(
        state.converted_sidebar_width(&system, TodoSidebarWidthUnit::Pixels),
        Some(640.0)
    );
}

#[test]
fn only_todo_settings_trigger_a_reflow() {
    let config = todo_config();
    // Первый раз — всё новое.
    assert!(remember_settings(&config));
    assert!(!remember_settings(&config));

    // Гэпы и прочее Todo не касаются.
    let other = Config {
        gap_size: 8.0,
        ..config.clone()
    };
    assert!(!remember_settings(&other));

    for changed in [
        Config {
            todo: Some(false),
            ..config.clone()
        },
        Config {
            todo_mode: false,
            ..config.clone()
        },
        Config {
            todo_application: Some("com.example.other".to_string()),
            ..config.clone()
        },
        Config {
            todo_sidebar_width: 500.0,
            ..config.clone()
        },
        Config {
            todo_sidebar_width_unit: TodoSidebarWidthUnit::Pct,
            ..config.clone()
        },
        Config {
            todo_sidebar_side: TodoSidebarSide::Left,
            ..config.clone()
        },
    ] {
        assert!(remember_settings(&changed));
        assert!(remember_settings(&config));
    }
}
