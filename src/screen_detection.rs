//! Выбор экранов для действия — порт `ScreenDetection.swift` (включая
//! `NSScreen.adjustedVisibleFrame`).
//!
//! Чистая логика без обращения к системе: экраны и всё, что про них известно
//! (Stage Manager, общие Spaces, Todo), передаются снимком `ScreenEnvironment`.
//! Координаты экранов — Cocoa, рамки окон — AX (переворот — по высоте основного
//! экрана `screens[0]`).

use crate::config::{Config, ScreenOrdering, TodoSidebarSide, TodoSidebarWidthUnit};
use crate::geometry::Rect;
use crate::screens::Screen;
use crate::stage::{StageState, StageStripPosition};

/// Соседние экраны в порядке `screensOrderedByX` (по кругу).
#[derive(Clone, Debug, PartialEq)]
pub struct AdjacentScreens {
    pub prev: Screen,
    pub next: Screen,
}

/// Экраны для действия (`UsableScreens`).
#[derive(Clone, Debug, PartialEq)]
pub struct UsableScreens {
    /// Экран окна (или курсора, или явно заданный).
    pub current: Screen,
    pub adjacent: Option<AdjacentScreens>,
    /// Сколько всего экранов (`NSScreen.screens.count`); у явно заданного экрана — 1.
    pub num_screens: usize,
    /// Экраны в порядке `screensOrderedByX` — для «Дисплей N».
    pub ordered: Vec<Screen>,
}

impl UsableScreens {
    /// Явно заданный экран (drag-to-snap, перебор экранов): соседей нет,
    /// экран считается единственным.
    pub fn single(screen: Screen) -> UsableScreens {
        UsableScreens {
            ordered: vec![screen.clone()],
            current: screen,
            adjacent: None,
            num_screens: 1,
        }
    }

    /// `frameOfCurrentScreen`.
    pub fn frame_of_current_screen(&self) -> Rect {
        self.current.frame
    }
}

/// Пустая рамка в начале координат — `CGRect.zero`, которую Swift подставляет,
/// когда окна нет (`frontmostWindowElement?.frame ?? .zero`).
pub const ZERO_RECT: Rect = Rect {
    x: 0.0,
    y: 0.0,
    w: 0.0,
    h: 0.0,
};

/// Экраны для окна (`detectScreens(using:)`). `window_frame_ax` — рамка окна в
/// координатах AX; `None` — окно есть, но рамку прочитать не удалось: в Swift
/// такая рамка — `CGRect.null`, а `contains(.null)` истинно у любого экрана, так
/// что экраном становится первый в пользовательском порядке (`screensOrdered[0]`),
/// а не `NSScreen.main`. Для «окна нет» передавайте `Some(ZERO_RECT)`.
pub fn detect_screens(
    window_frame_ax: Option<Rect>,
    screens: &[Screen],
    config: &Config,
    primary_height: f64,
) -> Option<UsableScreens> {
    let first = screens.first()?;

    if screens.len() == 1 {
        let adjacent = (config.traverse_single_screen == Some(true)).then(|| AdjacentScreens {
            prev: first.clone(),
            next: first.clone(),
        });
        return Some(UsableScreens {
            current: first.clone(),
            adjacent,
            num_screens: 1,
            ordered: vec![first.clone()],
        });
    }

    let ordered = order(screens, config.screens_ordered_by_x);
    let main = screens.iter().find(|screen| screen.is_main);
    let source = match window_frame_ax {
        Some(frame) => screen_containing(&frame.screen_flipped(primary_height), &ordered, main),
        None => ordered.first().cloned(),
    };
    let Some(source) = source else {
        return Some(UsableScreens {
            current: first.clone(),
            adjacent: Some(AdjacentScreens {
                prev: first.clone(),
                next: first.clone(),
            }),
            num_screens: screens.len(),
            ordered,
        });
    };

    let adjacent = adjacent(&source.frame, &ordered);
    Some(UsableScreens {
        current: source,
        adjacent,
        num_screens: screens.len(),
        ordered,
    })
}

/// Экраны по курсору (`detectScreensAtCursor`), `cursor` — координаты Cocoa.
/// Курсор ищется в системном порядке экранов, соседи — в пользовательском.
pub fn detect_screens_at_cursor(
    cursor: (f64, f64),
    screens: &[Screen],
    config: &Config,
    primary_height: f64,
) -> Option<UsableScreens> {
    if screens.len() == 1 {
        return detect_screens(Some(ZERO_RECT), screens, config, primary_height);
    }

    let ordered = order(screens, config.screens_ordered_by_x);
    let Some(cursor_screen) = screens
        .iter()
        .find(|screen| contains_point(&screen.frame, cursor))
    else {
        return detect_screens(Some(ZERO_RECT), screens, config, primary_height);
    };

    let adjacent = adjacent(&cursor_screen.frame, &ordered);
    Some(UsableScreens {
        current: cursor_screen.clone(),
        adjacent,
        num_screens: screens.len(),
        ordered,
    })
}

/// `CGRect.contains(CGPoint)`: правая и верхняя границы не входят.
fn contains_point(rect: &Rect, (x, y): (f64, f64)) -> bool {
    x >= rect.min_x() && x < rect.max_x() && y >= rect.min_y() && y < rect.max_y()
}

/// Экран, в котором окно целиком, иначе — на котором его больше всего
/// (`screenContaining`). Окно вне всех экранов — `main` (`NSScreen.main`).
/// `rect` — координаты Cocoa.
pub fn screen_containing(rect: &Rect, screens: &[Screen], main: Option<&Screen>) -> Option<Screen> {
    let mut result = main.cloned();
    let mut largest_percentage = 0.0;
    for screen in screens {
        if screen.frame.contains(rect) {
            return Some(screen.clone());
        }
        let percentage = rect.percentage_within(&screen.frame);
        if percentage > largest_percentage {
            largest_percentage = percentage;
            result = Some(screen.clone());
        }
    }
    result
}

/// Соседи экрана с рамкой `frame` (`adjacent(toFrameOfScreen:screens:)`):
/// из двух экранов сосед с обеих сторон — другой, из трёх и больше — по кругу.
pub fn adjacent(frame: &Rect, screens: &[Screen]) -> Option<AdjacentScreens> {
    if screens.len() == 2 {
        let other = screens.iter().find(|screen| screen.frame != *frame)?;
        return Some(AdjacentScreens {
            prev: other.clone(),
            next: other.clone(),
        });
    }
    if screens.len() > 2 {
        let index = screens.iter().position(|screen| screen.frame == *frame)?;
        let next = if index == screens.len() - 1 {
            0
        } else {
            index + 1
        };
        let prev = if index == 0 {
            screens.len() - 1
        } else {
            index - 1
        };
        return Some(AdjacentScreens {
            prev: screens[prev].clone(),
            next: screens[next].clone(),
        });
    }
    None
}

/// Порядок экранов (`order(screens:)`).
///
/// Сортировка — вставками, ровно как `sorted(by:)` в Swift на таких маленьких
/// массивах: сравнение «сверху, затем слева» нетранзитивно для «лесенки» из
/// трёх мониторов, и тогда результат зависит от алгоритма. Так он совпадает со
/// Swift и не может уронить программу (`sort_by` вправе паниковать на
/// нетранзитивном сравнении).
pub fn order(screens: &[Screen], ordering: ScreenOrdering) -> Vec<Screen> {
    let mut ordered = screens.to_vec();
    match ordering {
        ScreenOrdering::MidX => {
            insertion_sort(&mut ordered, |a, b| a.frame.mid_x() < b.frame.mid_x())
        }
        ScreenOrdering::MinX => {
            insertion_sort(&mut ordered, |a, b| a.frame.min_x() < b.frame.min_x())
        }
        ScreenOrdering::YThenMinX => insertion_sort(&mut ordered, |a, b| {
            if b.frame.max_y() <= a.frame.min_y() {
                return true;
            }
            if a.frame.max_y() <= b.frame.min_y() {
                return false;
            }
            a.frame.min_x() < b.frame.min_x()
        }),
    }
    ordered
}

/// `_insertionSort` из стандартной библиотеки Swift: элемент уезжает назад,
/// пока он «меньше» соседа слева.
fn insertion_sort<T>(items: &mut [T], are_in_increasing_order: impl Fn(&T, &T) -> bool) {
    for sorted_end in 1..items.len() {
        let mut i = sorted_end;
        while i > 0 && are_in_increasing_order(&items[i], &items[i - 1]) {
            items.swap(i, i - 1);
            i -= 1;
        }
    }
}

// ---------------------------------------------------------------- рабочая область

/// Всё, что нужно знать о системе, чтобы посчитать рабочую область экрана.
/// Снимается один раз на действие.
#[derive(Clone, Debug)]
pub struct ScreenEnvironment<'a> {
    pub config: &'a Config,
    /// `NSScreen.screens` в системном порядке: `[0]` — основной экран.
    pub screens: &'a [Screen],
    /// `NSScreen.screensHaveSeparateSpaces`.
    pub separate_spaces: bool,
    pub stage: StageState,
    /// Экран, у которого сейчас стоит боковая панель Todo (режим включён и окно
    /// Todo есть) — `TodoManager.todoScreen`, см. `todo_screen()` в менеджере окон.
    pub todo_screen: Option<u32>,
}

impl ScreenEnvironment<'_> {
    /// Высота основного экрана (`NSScreen.screens[0].frame.maxY`) — база
    /// переворота Cocoa ↔ AX.
    pub fn primary_height(&self) -> f64 {
        self.screens
            .first()
            .map(|screen| screen.frame.max_y())
            .unwrap_or(0.0)
    }
}

/// Ширина панели Todo (`TodoManager.getSidebarWidth`): до 1 — доля ширины,
/// в процентах — пересчёт в пиксели с округлением.
pub fn todo_sidebar_width(config: &Config, visible_frame_width: f64) -> f64 {
    let width = config.todo_sidebar_width as f64;
    if width > 0.0 && width <= 1.0 {
        width * visible_frame_width
    } else if config.todo_sidebar_width_unit == TodoSidebarWidthUnit::Pct {
        ((width * 0.01) * visible_frame_width).round()
    } else {
        width
    }
}

/// Рабочая область экрана для раскладок (`NSScreen.adjustedVisibleFrame`):
/// `visibleFrame` (или все экраны вместе в режиме общего экрана), минус полоса
/// Stage Manager, минус панель Todo, минус отступы от краёв экрана.
///
/// Отступы «только на основном экране» — это `screens[0]` (экран с меню-баром),
/// а не экран с ключевым окном. На экране с вырезом камеры верхний отступ —
/// `screenEdgeGapTopNotch`, если он задан.
pub fn adjusted_visible_frame(
    screen: &Screen,
    env: &ScreenEnvironment,
    ignore_todo: bool,
    ignore_stage: bool,
) -> Rect {
    let config = env.config;
    let mut frame = screen.visible_frame;

    if !env.separate_spaces && config.combined_display_mode == Some(true) {
        let mut union: Option<Rect> = None;
        for other in env.screens {
            union = Some(match union {
                Some(rect) => rect.union(&other.visible_frame),
                None => other.visible_frame,
            });
        }
        if let Some(union) = union {
            frame = union;
        }
    }

    if !ignore_stage && config.stage_size > 0.0 {
        if let Some(position) = env.stage.strip_on(screen) {
            let stage_size = if config.stage_size < 1.0 {
                frame.w * config.stage_size as f64
            } else {
                config.stage_size as f64
            };
            if position == StageStripPosition::Left {
                frame.x += stage_size;
            }
            frame.w -= stage_size;
        }
    }

    if !ignore_todo && env.todo_screen.is_some_and(|id| id == screen.id) {
        let sidebar_width = todo_sidebar_width(config, frame.w);
        frame.w -= sidebar_width;
        if config.todo_sidebar_side == TodoSidebarSide::Left {
            frame.x += sidebar_width;
        }
    }

    let is_primary = env
        .screens
        .first()
        .is_some_and(|primary| primary.same_display(screen));
    if config.screen_edge_gaps_on_main_screen_only && !is_primary {
        return frame;
    }

    let left = config.screen_edge_gap_left as f64;
    let right = config.screen_edge_gap_right as f64;
    let bottom = config.screen_edge_gap_bottom as f64;
    frame.x += left;
    frame.y += bottom;
    frame.w -= left + right;

    if screen.safe_area_top != 0.0 && config.screen_edge_gap_top_notch != 0.0 {
        frame.h -= config.screen_edge_gap_top_notch as f64 + bottom;
    } else {
        frame.h -= config.screen_edge_gap_top as f64 + bottom;
    }

    frame
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen(id: u32, frame: Rect) -> Screen {
        Screen {
            id,
            frame,
            visible_frame: frame,
            name: format!("экран {id}"),
            is_main: false,
            scale: 2.0,
            safe_area_top: 0.0,
        }
    }

    /// Ноутбук 1728×1117 (меню-бар 32, док снизу 83) и монитор справа.
    fn laptop() -> Screen {
        let mut laptop = screen(1, Rect::new(0.0, 0.0, 1728.0, 1117.0));
        laptop.visible_frame = Rect::new(0.0, 83.0, 1728.0, 1002.0);
        laptop.safe_area_top = 32.0;
        laptop
    }

    fn monitor_right() -> Screen {
        let mut monitor = screen(2, Rect::new(1728.0, -323.0, 2560.0, 1440.0));
        monitor.visible_frame = Rect::new(1728.0, -323.0, 2560.0, 1415.0);
        monitor
    }

    fn env<'a>(config: &'a Config, screens: &'a [Screen]) -> ScreenEnvironment<'a> {
        ScreenEnvironment {
            config,
            screens,
            separate_spaces: true,
            stage: StageState::default(),
            todo_screen: None,
        }
    }

    #[test]
    fn single_screen_has_no_neighbours_unless_traversal_is_on() {
        let mut config = Config::default();
        let screens = vec![laptop()];
        let usable = detect_screens(
            Some(Rect::new(10.0, 40.0, 500.0, 400.0)),
            &screens,
            &config,
            1117.0,
        )
        .unwrap();
        assert_eq!(usable.current.id, 1);
        assert!(usable.adjacent.is_none());
        assert_eq!(usable.num_screens, 1);

        config.traverse_single_screen = Some(true);
        let usable = detect_screens(None, &screens, &config, 1117.0).unwrap();
        let adjacent = usable.adjacent.unwrap();
        assert_eq!((adjacent.prev.id, adjacent.next.id), (1, 1));
    }

    #[test]
    fn window_goes_to_screen_with_largest_share() {
        let config = Config::default();
        let screens = vec![laptop(), monitor_right()];
        // Окно на мониторе целиком (AX: y от верха основного экрана).
        let on_monitor = Rect::new(2000.0, 100.0, 800.0, 600.0);
        let usable = detect_screens(Some(on_monitor), &screens, &config, 1117.0).unwrap();
        assert_eq!(usable.current.id, 2);
        assert_eq!(usable.num_screens, 2);
        // Из двух экранов сосед с обеих сторон — другой.
        let adjacent = usable.adjacent.unwrap();
        assert_eq!((adjacent.prev.id, adjacent.next.id), (1, 1));

        // Окно на стыке, большая часть — на ноутбуке.
        let straddling = Rect::new(1200.0, 100.0, 800.0, 600.0);
        let usable = detect_screens(Some(straddling), &screens, &config, 1117.0).unwrap();
        assert_eq!(usable.current.id, 1);
    }

    #[test]
    fn window_outside_all_screens_falls_back_to_main_screen() {
        let config = Config::default();
        let mut monitor = monitor_right();
        monitor.is_main = true;
        let screens = vec![laptop(), monitor];
        // Остались координаты отключённого монитора слева.
        let lost = Rect::new(-3000.0, 100.0, 800.0, 600.0);
        let found = screen_containing(
            &lost.screen_flipped(1117.0),
            &order(&screens, config.screens_ordered_by_x),
            screens.iter().find(|s| s.is_main),
        );
        assert_eq!(found.unwrap().id, 2);

        // Нет ни экрана с окном, ни main — первый экран, соседи — он же.
        let screens = vec![laptop(), monitor_right()];
        let usable = detect_screens(Some(lost), &screens, &config, 1117.0).unwrap();
        assert_eq!(usable.current.id, 1);
        let adjacent = usable.adjacent.unwrap();
        assert_eq!((adjacent.prev.id, adjacent.next.id), (1, 1));
    }

    #[test]
    fn unreadable_window_frame_picks_first_ordered_screen_not_main() {
        // Swift: рамка `.null`, `contains(.null)` истинно у первого же экрана в
        // `screensOrdered` — `NSScreen.main` тут ни при чём.
        let config = Config::default();
        let mut monitor = monitor_right();
        monitor.is_main = true;
        let screens = vec![laptop(), monitor];
        let usable = detect_screens(None, &screens, &config, 1117.0).unwrap();
        assert_eq!(usable.current.id, 1);
        let adjacent = usable.adjacent.unwrap();
        assert_eq!((adjacent.prev.id, adjacent.next.id), (2, 2));

        // Первый в пользовательском порядке, а не в системном: слева направо.
        let a = screen(1, Rect::new(0.0, 0.0, 1000.0, 800.0));
        let b = screen(2, Rect::new(1000.0, 0.0, 1000.0, 800.0));
        let mut c = screen(3, Rect::new(2000.0, 0.0, 1000.0, 800.0));
        c.is_main = true;
        let screens = vec![b, c, a];
        let usable = detect_screens(None, &screens, &config, 800.0).unwrap();
        assert_eq!(usable.current.id, 1);
        assert_eq!(usable.num_screens, 3);
        let adjacent = usable.adjacent.unwrap();
        assert_eq!((adjacent.prev.id, adjacent.next.id), (3, 2));
    }

    #[test]
    fn three_screens_wrap_around() {
        let config = Config::default();
        let a = screen(1, Rect::new(0.0, 0.0, 1000.0, 800.0));
        let b = screen(2, Rect::new(1000.0, 0.0, 1000.0, 800.0));
        let c = screen(3, Rect::new(2000.0, 0.0, 1000.0, 800.0));
        let screens = vec![b.clone(), c.clone(), a.clone()];
        let ordered = order(&screens, config.screens_ordered_by_x);
        assert_eq!(
            ordered.iter().map(|s| s.id).collect::<Vec<_>>(),
            vec![1, 2, 3]
        );

        let first = adjacent(&a.frame, &ordered).unwrap();
        assert_eq!((first.prev.id, first.next.id), (3, 2));
        let last = adjacent(&c.frame, &ordered).unwrap();
        assert_eq!((last.prev.id, last.next.id), (2, 1));
        assert!(adjacent(&Rect::new(9.0, 9.0, 9.0, 9.0), &ordered).is_none());
    }

    #[test]
    fn screen_orderings() {
        // Монитор сверху, под ним два рядом.
        let top = screen(1, Rect::new(500.0, 1000.0, 1000.0, 800.0));
        let bottom_left = screen(2, Rect::new(0.0, 0.0, 1000.0, 1000.0));
        let bottom_right = screen(3, Rect::new(1000.0, 0.0, 1200.0, 1000.0));
        let screens = vec![bottom_right.clone(), bottom_left.clone(), top.clone()];
        let ids = |ordering| {
            order(&screens, ordering)
                .iter()
                .map(|s| s.id)
                .collect::<Vec<_>>()
        };
        assert_eq!(ids(ScreenOrdering::YThenMinX), vec![1, 2, 3]);
        assert_eq!(ids(ScreenOrdering::MinX), vec![2, 1, 3]);
        // midX: 500, 1000, 1600.
        assert_eq!(ids(ScreenOrdering::MidX), vec![2, 1, 3]);
    }

    #[test]
    fn staircase_ordering_matches_swift_insertion_sort() {
        // «Лесенка»: сравнение нетранзитивно. Результат — как у Swift на трёх
        // элементах (сортировка вставками), без паники.
        let a = screen(1, Rect::new(0.0, 0.0, 1000.0, 1000.0));
        let b = screen(2, Rect::new(1000.0, 600.0, 1000.0, 1000.0));
        let c = screen(3, Rect::new(2000.0, 1200.0, 1000.0, 1000.0));
        let ordered = order(&[a, b, c], ScreenOrdering::YThenMinX);
        // Проход: [1,2,3] → 2 не выше 1 по правилу (пересекаются по Y) и правее → стоит;
        // 3 выше 1 целиком, но с 2 пересекается и правее → останавливается на месте.
        assert_eq!(
            ordered.iter().map(|s| s.id).collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
    }

    #[test]
    fn cursor_screen_is_found_in_system_order() {
        let config = Config::default();
        let screens = vec![laptop(), monitor_right()];
        let usable = detect_screens_at_cursor((2000.0, 500.0), &screens, &config, 1117.0).unwrap();
        assert_eq!(usable.current.id, 2);
        let usable = detect_screens_at_cursor((100.0, 100.0), &screens, &config, 1117.0).unwrap();
        assert_eq!(usable.current.id, 1);
        // Правая граница экрана не входит в него.
        let usable = detect_screens_at_cursor((1728.0, 500.0), &screens, &config, 1117.0).unwrap();
        assert_eq!(usable.current.id, 2);
    }

    #[test]
    fn explicit_screen_is_the_only_one() {
        let usable = UsableScreens::single(monitor_right());
        assert_eq!(usable.num_screens, 1);
        assert!(usable.adjacent.is_none());
        assert_eq!(usable.ordered.len(), 1);
        assert_eq!(usable.frame_of_current_screen(), monitor_right().frame);
    }

    #[test]
    fn edge_gaps_on_main_screen_only_means_primary_screen() {
        let config = Config {
            screen_edge_gap_left: 10.0,
            screen_edge_gap_right: 20.0,
            screen_edge_gap_top: 5.0,
            screen_edge_gap_bottom: 15.0,
            screen_edge_gaps_on_main_screen_only: true,
            ..Config::default()
        };

        // Ключевое окно на мониторе (он `main`), но отступы — только у основного экрана.
        let mut monitor = monitor_right();
        monitor.is_main = true;
        let screens = vec![laptop(), monitor.clone()];
        let env = env(&config, &screens);

        let primary = adjusted_visible_frame(&screens[0], &env, false, false);
        assert_eq!(primary, Rect::new(10.0, 98.0, 1698.0, 982.0));
        let secondary = adjusted_visible_frame(&monitor, &env, false, false);
        assert_eq!(secondary, monitor.visible_frame);
    }

    #[test]
    fn notch_gap_replaces_top_gap_on_screens_with_a_notch() {
        let mut config = Config {
            screen_edge_gap_top: 5.0,
            screen_edge_gap_bottom: 15.0,
            ..Config::default()
        };
        let screens = vec![laptop(), monitor_right()];
        let env_without = env(&config, &screens);
        // Отступ для выреза не задан — везде обычный верхний.
        assert_eq!(
            adjusted_visible_frame(&screens[0], &env_without, false, false).h,
            1002.0 - 20.0
        );

        config.screen_edge_gap_top_notch = 40.0;
        let env_notch = env(&config, &screens);
        // У ноутбука вырез: вместо 5 сверху — 40.
        assert_eq!(
            adjusted_visible_frame(&screens[0], &env_notch, false, false).h,
            1002.0 - 55.0
        );
        // У монитора выреза нет — обычный отступ.
        assert_eq!(
            adjusted_visible_frame(&screens[1], &env_notch, false, false).h,
            1415.0 - 20.0
        );
    }

    #[test]
    fn stage_strip_takes_its_side() {
        let config = Config::default();
        let screens = vec![laptop(), monitor_right()];
        let mut environment = env(&config, &screens);
        environment.stage = StageState {
            active: true,
            position: Some(StageStripPosition::Left),
            visible_on: vec![1],
        };
        let frame = adjusted_visible_frame(&screens[0], &environment, false, false);
        assert_eq!(frame, Rect::new(190.0, 83.0, 1538.0, 1002.0));
        // «Центр» считает без полосы.
        assert_eq!(
            adjusted_visible_frame(&screens[0], &environment, false, true),
            screens[0].visible_frame
        );
        // На мониторе полосы нет.
        assert_eq!(
            adjusted_visible_frame(&screens[1], &environment, false, false),
            screens[1].visible_frame
        );

        // Полоса справа и размер долей ширины.
        let config = Config {
            stage_size: 0.1,
            ..Config::default()
        };
        let mut environment = env(&config, &screens);
        environment.stage = StageState {
            active: true,
            position: Some(StageStripPosition::Right),
            visible_on: vec![1],
        };
        let frame = adjusted_visible_frame(&screens[0], &environment, false, false);
        assert_eq!(frame.x, 0.0);
        // Доля — Float, как `Defaults.stageSize.cgFloat` в Swift.
        assert_eq!(frame.w, 1728.0 - 1728.0 * (0.1f32 as f64));

        // stageSize 0 — полосу не учитываем.
        let config = Config {
            stage_size: 0.0,
            ..Config::default()
        };
        let mut environment = env(&config, &screens);
        environment.stage = StageState {
            active: true,
            position: Some(StageStripPosition::Left),
            visible_on: vec![1],
        };
        assert_eq!(
            adjusted_visible_frame(&screens[0], &environment, false, false),
            screens[0].visible_frame
        );
    }

    #[test]
    fn combined_display_mode_joins_screens_without_separate_spaces() {
        let config = Config {
            combined_display_mode: Some(true),
            ..Config::default()
        };
        let screens = vec![laptop(), monitor_right()];
        let mut environment = env(&config, &screens);
        // Пока у мониторов отдельные Spaces, режим не действует.
        assert_eq!(
            adjusted_visible_frame(&screens[0], &environment, false, false),
            screens[0].visible_frame
        );
        environment.separate_spaces = false;
        let union = screens[0].visible_frame.union(&screens[1].visible_frame);
        assert_eq!(
            adjusted_visible_frame(&screens[0], &environment, false, false),
            union
        );
    }

    #[test]
    fn todo_sidebar_takes_space_on_its_screen() {
        let config = Config {
            todo_sidebar_width: 400.0,
            ..Config::default()
        };
        let screens = vec![laptop(), monitor_right()];
        let mut environment = env(&config, &screens);
        environment.todo_screen = Some(1);
        let frame = adjusted_visible_frame(&screens[0], &environment, false, false);
        assert_eq!(frame, Rect::new(0.0, 83.0, 1328.0, 1002.0));
        // Окно самой панели считает без неё.
        assert_eq!(
            adjusted_visible_frame(&screens[0], &environment, true, false),
            screens[0].visible_frame
        );

        let config = Config {
            todo_sidebar_side: TodoSidebarSide::Left,
            todo_sidebar_width: 25.0,
            todo_sidebar_width_unit: TodoSidebarWidthUnit::Pct,
            ..Config::default()
        };
        let mut environment = env(&config, &screens);
        environment.todo_screen = Some(1);
        let frame = adjusted_visible_frame(&screens[0], &environment, false, false);
        assert_eq!(frame, Rect::new(432.0, 83.0, 1296.0, 1002.0));
    }

    #[test]
    fn todo_sidebar_width_units() {
        let mut config = Config {
            todo_sidebar_width: 0.25,
            ..Config::default()
        };
        assert_eq!(todo_sidebar_width(&config, 1000.0), 250.0);
        config.todo_sidebar_width = 30.0;
        config.todo_sidebar_width_unit = TodoSidebarWidthUnit::Pct;
        assert_eq!(todo_sidebar_width(&config, 1001.0), 300.0);
        config.todo_sidebar_width_unit = TodoSidebarWidthUnit::Pixels;
        assert_eq!(todo_sidebar_width(&config, 1001.0), 30.0);
    }
}
