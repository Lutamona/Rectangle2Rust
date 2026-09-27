//! Применение согласованного ресайза на подставных окнах: порядок установки, повторный
//! проход, история соседей, возврат соседей. Геометрия и чистые проверки перебора
//! сверяются с оригиналом оракулом `tools/coop-oracle`.

use std::cell::RefCell;
use std::rc::Rc;

use super::*;
use crate::config::CycleSize;
use crate::side_split_ratios;

/// Высота основного экрана: рамки хранятся в AX, как у настоящих окон.
const PRIMARY: f64 = 1001.0;

const SCREEN: Rect = Rect {
    x: 0.0,
    y: 0.0,
    w: 1728.0,
    h: 1001.0,
};

/// Стол с окнами: рамки (AX), минимальные размеры, которые знает только «приложение»,
/// и порядок установки.
#[derive(Default)]
struct Desk {
    frames: RefCell<HashMap<u32, Rect>>,
    hidden_minimum: RefCell<HashMap<u32, Size>>,
    moves: RefCell<Vec<u32>>,
}

#[derive(Clone)]
struct FakeWindow {
    id: u32,
    desk: Rc<Desk>,
}

impl CooperativeWindow for FakeWindow {
    fn frame(&self) -> Option<Rect> {
        self.desk.frames.borrow().get(&self.id).copied()
    }

    fn set_frame(&self, rect: &Rect) {
        let mut rect = *rect;
        if let Some(minimum) = self.desk.hidden_minimum.borrow().get(&self.id) {
            rect.w = rect.w.max(minimum.width);
            rect.h = rect.h.max(minimum.height);
        }
        self.desk.frames.borrow_mut().insert(self.id, rect);
        self.desk.moves.borrow_mut().push(self.id);
    }
}

impl Desk {
    /// Поставить окно (рамка — Cocoa).
    fn place(self: &Rc<Self>, id: u32, frame: Rect) -> FakeWindow {
        self.frames
            .borrow_mut()
            .insert(id, frame.screen_flipped(PRIMARY));
        FakeWindow {
            id,
            desk: self.clone(),
        }
    }

    /// Рамка окна, Cocoa.
    fn cocoa(&self, id: u32) -> Rect {
        self.frames.borrow()[&id].screen_flipped(PRIMARY)
    }

    fn neighbours(self: &Rc<Self>, ids: &[u32]) -> Vec<NeighborWindow<FakeWindow>> {
        ids.iter()
            .map(|&id| NeighborWindow {
                id,
                window: FakeWindow {
                    id,
                    desk: self.clone(),
                },
                frame: self.cocoa(id),
                minimum_size: None,
            })
            .collect()
    }
}

fn cooperative_config() -> Config {
    Config {
        cooperative_corner_resize: true,
        ..Config::default()
    }
}

fn last(action: Action, count: u32, frame: Rect) -> LastAction {
    LastAction {
        action,
        sub_action: None,
        rect: frame.screen_flipped(PRIMARY),
        count,
    }
}

fn request<'a>(
    config: &'a Config,
    last_action: Option<&'a LastAction>,
    old: Rect,
    new: Rect,
) -> PlanRequest<'a> {
    PlanRequest {
        focused_window_id: Some(1),
        focused_window_is_fixed_size: false,
        focused_window_minimum_size: None,
        action: Action::LeftHalf,
        source: ExecutionSource::DragToSnap,
        old_focused_frame: old,
        new_focused_frame: new,
        screen_frame: SCREEN,
        destination_screen_is_current_screen: true,
        last_action,
        config,
        primary_height: PRIMARY,
    }
}

/// Поставить окно в фокусе «цепочкой доводки» — просто рамкой.
fn mover(window: &FakeWindow) -> impl FnMut(&Rect) + '_ {
    move |rect: &Rect| window.set_frame(&rect.screen_flipped(PRIMARY))
}

#[test]
fn preconditions_match_the_original() {
    let config = cooperative_config();
    let frame = Rect::new(0.0, 0.0, 864.0, 1001.0);
    let base = request(&config, None, frame, frame);
    assert!(plan_preconditions(&base).is_some());

    let disabled = Config::default();
    assert!(plan_preconditions(&PlanRequest {
        config: &disabled,
        ..base
    })
    .is_none());
    for source in [
        ExecutionSource::MenuItem,
        ExecutionSource::Url,
        ExecutionSource::TitleBar,
    ] {
        assert!(plan_preconditions(&PlanRequest { source, ..base }).is_none());
    }
    assert!(plan_preconditions(&PlanRequest {
        focused_window_id: None,
        ..base
    })
    .is_none());
    assert!(plan_preconditions(&PlanRequest {
        focused_window_is_fixed_size: true,
        ..base
    })
    .is_none());
    assert!(plan_preconditions(&PlanRequest {
        destination_screen_is_current_screen: false,
        ..base
    })
    .is_none());
    assert!(plan_preconditions(&PlanRequest {
        action: Action::Maximize,
        ..base
    })
    .is_none());
}

#[test]
fn growing_window_makes_room_first_and_neighbour_continues_its_cycle() {
    side_split_ratios::reset_all(&cooperative_config());
    let desk = Rc::new(Desk::default());
    let old = Rect::new(0.0, 0.0, 864.0, 1001.0);
    let focused = desk.place(1, old);
    desk.place(2, Rect::new(864.0, 0.0, 864.0, 1001.0));

    let config = cooperative_config();
    let previous = last(Action::LeftHalf, 1, old);
    let new = Rect::new(0.0, 0.0, 1152.0, 1001.0);
    let plan = plan_with_windows(
        &request(&config, Some(&previous), old, new),
        Axis::Horizontal,
        MovedEdge::Right,
        desk.neighbours(&[2]),
    )
    .unwrap();
    assert_eq!(plan.action_description, REPEATED_DESCRIPTION);
    assert!(plan.needs_application(Some(&old)));

    let resulting = apply_cooperative_corner_resize(&focused, &mut mover(&focused), &plan);
    assert_eq!(resulting, Some(new.screen_flipped(PRIMARY)));
    // Сначала уступает сосед, потом растёт окно в фокусе.
    assert_eq!(*desk.moves.borrow(), vec![2, 1]);
    assert_eq!(desk.cocoa(2), Rect::new(1152.0, 0.0, 576.0, 1001.0));

    // Соседу за краем записана его сторона — следующий повтор продолжит перебор.
    let history = window_history::last_action(2).unwrap();
    assert_eq!(history.action, Action::RightHalf);
    assert_eq!(history.count, 1);
    assert_eq!(history.rect, desk.frames.borrow()[&2]);

    // Всё на месте — применять нечего.
    assert!(!plan.needs_application(Some(&new)));
}

#[test]
fn shrinking_window_goes_first() {
    let desk = Rc::new(Desk::default());
    let old = Rect::new(0.0, 0.0, 1152.0, 1001.0);
    let focused = desk.place(1, old);
    desk.place(2, Rect::new(1152.0, 0.0, 576.0, 1001.0));

    let config = cooperative_config();
    let new = Rect::new(0.0, 0.0, 576.0, 1001.0);
    // Первое нажатие ищет соседей у новой рамки — сосед у старого края не найден.
    assert!(plan_with_windows(
        &request(&config, None, old, new),
        Axis::Horizontal,
        MovedEdge::Right,
        desk.neighbours(&[2]),
    )
    .is_none());

    // Повтор ищет у прежней рамки.
    let previous = last(Action::LeftHalf, 2, old);
    let plan = plan_with_windows(
        &request(&config, Some(&previous), old, new),
        Axis::Horizontal,
        MovedEdge::Right,
        desk.neighbours(&[2]),
    )
    .unwrap();
    assert_eq!(plan.action_description, REPEATED_DESCRIPTION);

    apply_cooperative_corner_resize(&focused, &mut mover(&focused), &plan);
    assert_eq!(*desk.moves.borrow(), vec![1, 2]);
    assert_eq!(desk.cocoa(1), new);
    assert_eq!(desk.cocoa(2), Rect::new(576.0, 0.0, 1152.0, 1001.0));
}

#[test]
fn neighbour_that_refuses_to_shrink_gets_a_second_pass() {
    let desk = Rc::new(Desk::default());
    let old = Rect::new(0.0, 0.0, 864.0, 1001.0);
    let focused = desk.place(1, old);
    desk.place(2, Rect::new(864.0, 0.0, 864.0, 1001.0));
    // Минимум, о котором приложение не сообщило через AX.
    desk.hidden_minimum
        .borrow_mut()
        .insert(2, Size::new(700.0, 100.0));

    let config = cooperative_config();
    let previous = last(Action::LeftHalf, 2, old);
    window_history::with(|history| {
        history.record_action(
            2,
            Rect::new(0.0, 0.0, 1.0, 1.0),
            Action::RightHalf,
            None,
            true,
        )
    });
    let new = Rect::new(0.0, 0.0, 1152.0, 1001.0);
    let plan = plan_with_windows(
        &request(&config, Some(&previous), old, new),
        Axis::Horizontal,
        MovedEdge::Right,
        desk.neighbours(&[2]),
    )
    .unwrap();

    let resulting = apply_cooperative_corner_resize(&focused, &mut mover(&focused), &plan);
    // Сосед остался шириной 700 — окно в фокусе встало вплотную к нему.
    assert_eq!(desk.cocoa(1), Rect::new(0.0, 0.0, 1028.0, 1001.0));
    assert_eq!(desk.cocoa(2), Rect::new(1028.0, 0.0, 700.0, 1001.0));
    assert_eq!(resulting, Some(desk.frames.borrow()[&1]));
    // Первый проход продвинул счётчик соседа, повторный — нет.
    assert_eq!(window_history::last_action(2).unwrap().count, 2);
    assert_eq!(*desk.moves.borrow(), vec![2, 2, 1]);
}

#[test]
fn repeated_press_skips_a_size_the_neighbour_cannot_reach() {
    // Сосед справа упёрся в свой минимум между ⅓ и ½: рост левой половины до ⅔ ничего
    // бы не дал — повтор перескакивает на следующий размер перебора (⅓).
    let config = cooperative_config();
    side_split_ratios::reset_all(&config);
    let desk = Rc::new(Desk::default());
    let old = Rect::new(0.0, 0.0, 1128.0, 1001.0);
    desk.place(1, old);
    desk.place(2, Rect::new(1128.0, 0.0, 600.0, 1001.0));

    let previous = last(Action::LeftHalf, 2, old);
    let two_thirds = Rect::new(0.0, 0.0, 1152.0, 1001.0);
    let plan = plan_with_windows(
        &request(&config, Some(&previous), old, two_thirds),
        Axis::Horizontal,
        MovedEdge::Right,
        desk.neighbours(&[2]),
    )
    .unwrap();
    let one_third = Rect::new(0.0, 0.0, 576.0, 1001.0);
    assert_eq!(plan.requested_focused_frame, one_third);
    assert_eq!(plan.side_split_recording_frame, Some(one_third));
    assert_eq!(plan.focused_frame, one_third);
    assert_eq!(
        plan.adjustments[0].new_frame,
        Rect::new(576.0, 0.0, 1152.0, 1001.0)
    );
    assert_eq!(CycleSize::TwoThirds.title(), "⅔");
}

#[test]
fn cleanup_restores_a_window_left_at_the_old_size() {
    // Окна 1 и 3 стояли стопкой в левой части шириной 1037 (не размер перебора), сосед 2 —
    // справа. Окно 1 ушло в правую часть вплотную к окну 3: окно 3 возвращается к
    // половине, соседи справа занимают освободившееся место.
    let config = cooperative_config();
    side_split_ratios::reset_all(&config);
    let desk = Rc::new(Desk::default());
    let old = Rect::new(0.0, 0.0, 1037.0, 1001.0);
    desk.place(1, Rect::new(1037.0, 0.0, 691.0, 1001.0));
    desk.place(2, Rect::new(1037.0, 0.0, 691.0, 1001.0));
    desk.place(3, old);

    let previous = last(Action::LeftHalf, 1, old);
    let request = CleanupRequest {
        focused_window_id: Some(1),
        source: ExecutionSource::DragToSnap,
        old_focused_frame: old,
        new_focused_frame: Some(Rect::new(1037.0, 0.0, 691.0, 1001.0)),
        screen_frame: SCREEN,
        current_action: Action::RightHalf,
        last_action: Some(&previous),
        config: &config,
        primary_height: PRIMARY,
    };
    let cleanup = cleanup_preconditions(&request).unwrap();
    cleanup_with_windows(&request, cleanup, desk.neighbours(&[1, 2, 3]));

    assert_eq!(desk.cocoa(3), Rect::new(0.0, 0.0, 864.0, 1001.0));
    assert_eq!(desk.cocoa(1), Rect::new(864.0, 0.0, 864.0, 1001.0));
    assert_eq!(desk.cocoa(2), Rect::new(864.0, 0.0, 864.0, 1001.0));
    // Окно 3 растёт… нет, сжимается: сначала оно, потом соседи.
    assert_eq!(desk.moves.borrow()[0], 3);
    // История при возврате не продвигается: у окна 3 записано прошлое действие, счёт 1.
    let history = window_history::last_action(3).unwrap();
    assert_eq!((history.action, history.count), (Action::LeftHalf, 1));

    // То же действие ещё раз — возвращать нечего.
    assert!(cleanup_preconditions(&CleanupRequest {
        current_action: Action::LeftHalf,
        ..request
    })
    .is_none());
    // Из меню соседей не трогаем.
    assert!(cleanup_preconditions(&CleanupRequest {
        source: ExecutionSource::MenuItem,
        ..request
    })
    .is_none());
}

fn oracle_candidate(id: u32, x: f64, y: f64, w: f64, h: f64, minimum: Option<Size>) -> Candidate {
    Candidate {
        id,
        frame: Rect::new(x, y, w, h),
        minimum_size: minimum,
    }
}

#[test]
fn oracle_cleanup_checks() {
    // Оракул, случай k34: слева окно шире половины, у него справа — окно в фокусе.
    let config = cooperative_config();
    side_split_ratios::reset_all(&config);
    let screen = Rect::new(0.0, 25.0, 1440.0, 875.0);
    let context = CycleContext {
        action: Action::LeftHalf,
        screen_frame: screen,
        axis: Axis::Horizontal,
        moved_edge: MovedEdge::Right,
        tolerance: 13.125,
        gap_size: 30.0,
        config: &config,
    };
    let candidates = [
        oracle_candidate(24331, 535.950788592121, 58.0, 405.0, 816.0, None),
        oracle_candidate(
            3874,
            30.0,
            55.0,
            475.950788592121,
            815.0,
            Some(Size::new(815.0, 364.0)),
        ),
        oracle_candidate(
            1449,
            121.74029855892768,
            229.115921768725,
            46.0,
            348.0,
            Some(Size::new(15.0, 26.0)),
        ),
        oracle_candidate(
            22617,
            548.950788592121,
            53.0,
            83.36030067805369,
            815.0,
            None,
        ),
        oracle_candidate(
            7696,
            -260.68187756028084,
            899.7334148826811,
            730.0,
            524.0,
            Some(Size::new(175.0, 320.0)),
        ),
    ];
    let mut target_candidates = candidates.to_vec();
    target_candidates.push(oracle_candidate(
        18258,
        482.0,
        56.0,
        412.76081039702257,
        815.0,
        Some(Size::new(588.0, 198.0)),
    ));

    let old = Rect::new(30.0, 55.0, 435.0, 815.0);
    let source = cleanup_source_frame(&context, &old, &candidates, 96.0).unwrap();
    assert_eq!(source, Rect::new(30.0, 55.0, 475.950788592121, 815.0));
    let target = cleanup_target_frame(&context, &source, false, &target_candidates).unwrap();
    assert_eq!(target, Rect::new(30.0, 55.0, 435.0, 815.0));
    let destination = Rect::new(535.950788592121, 68.0, 127.61288703740252, 813.0);
    assert!(cleanup_destination_allows_source_resize(
        &context,
        &source,
        &target,
        &destination
    ));
}

#[test]
fn oracle_look_ahead_with_remembered_ratios() {
    // Оракул, случай l0: экран слева от основного, запомненные доли сторон, гэп 5.
    let config = Config {
        horizontal_split_ratio: 60.0,
        ..cooperative_config()
    };
    let screen = Rect::new(-1920.0, 0.0, 1920.0, 1055.0);
    side_split_ratios::reset_all(&config);
    side_split_ratios::record_side_action(
        Action::LeftHalf,
        &Rect::new(0.0, 0.0, 729.0, 1.0),
        &screen,
        &config,
    );
    side_split_ratios::record_side_action(
        Action::TopHalf,
        &Rect::new(0.0, 0.0, 1.0, 688.0),
        &screen,
        &config,
    );
    let context = CycleContext {
        action: Action::BottomHalf,
        screen_frame: screen,
        axis: Axis::Vertical,
        moved_edge: MovedEdge::Top,
        tolerance: 15.825,
        gap_size: 5.0,
        config: &config,
    };
    let candidates = [
        oracle_candidate(
            21343,
            -1915.0,
            529.5,
            1910.0,
            445.0,
            Some(Size::new(491.0, 6.0)),
        ),
        oracle_candidate(
            3766,
            -2098.0,
            766.0,
            1886.0,
            681.0,
            Some(Size::new(389.0, 120.0)),
        ),
    ];
    let target = cycle_look_ahead_target_for_minimum_restricted_adjacent(
        &context,
        &Rect::new(-1915.0, 5.0, 1910.0, 519.5),
        &Rect::new(-1915.0, 5.0, 1910.0, 695.5),
        &candidates,
    )
    .unwrap();
    assert_eq!(target.raw_frame, Rect::new(-1920.0, 0.0, 1920.0, 351.0));
    assert_eq!(target.gapped_frame, Rect::new(-1915.0, 5.0, 1910.0, 343.5));
    assert_eq!(target.skipped_cycle_size, CycleSize::TwoThirds);
    assert_eq!(target.target_cycle_size, CycleSize::OneThird);
    assert_eq!(target.restricted_adjacent_id, 21343);
}

#[test]
fn window_scope_is_off_by_default() {
    assert_eq!(WINDOW_SCOPE_PID.load(Ordering::SeqCst), 0);
}
