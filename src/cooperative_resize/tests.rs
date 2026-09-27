//! Геометрия согласованного ресайза на синтетических раскладках. Полная сверка с
//! оригиналом до бита — оракул `tools/coop-oracle`; здесь — сценарии, которые легко
//! проверить руками, и случаи из оракула (id в комментарии).

use super::*;

/// Рабочая область 1728 × 1001: допуск общих краёв 15,015, захвата 138,24 → 96 по X и
/// 80,08 по Y.
const SCREEN: Rect = Rect {
    x: 0.0,
    y: 0.0,
    w: 1728.0,
    h: 1001.0,
};

fn candidate(id: u32, x: f64, y: f64, w: f64, h: f64) -> Candidate {
    Candidate {
        id,
        frame: Rect::new(x, y, w, h),
        minimum_size: None,
    }
}

fn params<'a>(old: Rect, new: Rect, candidates: &'a [Candidate], gap: f64) -> PlanParams<'a> {
    PlanParams {
        old_focused_frame: old,
        new_focused_frame: new,
        screen_frame: SCREEN,
        candidates,
        axis: Axis::Horizontal,
        tolerance: detection_tolerance(&SCREEN, gap),
        minimum_size: Size::new(1.0, 1.0),
        focused_minimum_size: None,
        gap_size: gap,
        capture_tolerance: Some(capture_tolerance(&SCREEN, Axis::Horizontal)),
        moved_edge_override: Some(MovedEdge::Right),
        candidate_discovery_frame: None,
        action_description: "тест",
    }
}

fn frames(plan: &Plan) -> Vec<(u32, AdjustmentKind, Rect)> {
    plan.adjustments
        .iter()
        .map(|adjustment| (adjustment.id, adjustment.kind, adjustment.new_frame))
        .collect()
}

#[test]
fn tolerances_follow_screen_size() {
    assert_eq!(detection_tolerance(&SCREEN, 0.0), 1001.0 * 0.015);
    assert_eq!(
        detection_tolerance(&Rect::new(0.0, 0.0, 300.0, 300.0), 0.0),
        8.0
    );
    assert_eq!(
        detection_tolerance(&Rect::new(0.0, 0.0, 5000.0, 3000.0), 0.0),
        24.0
    );
    assert_eq!(capture_tolerance(&SCREEN, Axis::Horizontal), 96.0);
    assert_eq!(capture_tolerance(&SCREEN, Axis::Vertical), 1001.0 * 0.08);
    assert_eq!(
        capture_tolerance(&Rect::new(0.0, 0.0, 50.0, 50.0), Axis::Vertical),
        8.0
    );
}

#[test]
fn moved_edge_is_the_only_edge_that_moved() {
    let old = Rect::new(0.0, 0.0, 864.0, 1001.0);
    let wider = Rect::new(0.0, 0.0, 1152.0, 1001.0);
    assert_eq!(
        moved_edge(&old, &wider, Axis::Horizontal, 15.0),
        Some(MovedEdge::Right)
    );
    let from_left = Rect::new(-100.0, 0.0, 964.0, 1001.0);
    assert_eq!(
        moved_edge(&old, &from_left, Axis::Horizontal, 15.0),
        Some(MovedEdge::Left)
    );
    // Сдвинулись оба края или ни один — края нет.
    let shifted = Rect::new(100.0, 0.0, 864.0, 1001.0);
    assert_eq!(moved_edge(&old, &shifted, Axis::Horizontal, 15.0), None);
    assert_eq!(moved_edge(&old, &old, Axis::Horizontal, 15.0), None);
    // В пределах допуска — не сдвиг.
    let nudged = Rect::new(0.0, 0.0, 870.0, 1001.0);
    assert_eq!(moved_edge(&old, &nudged, Axis::Horizontal, 15.0), None);
    let taller = Rect::new(0.0, 0.0, 864.0, 1200.0);
    assert_eq!(
        moved_edge(&old, &taller, Axis::Vertical, 15.0),
        Some(MovedEdge::Top)
    );
}

#[test]
fn growing_half_pushes_the_adjacent_half() {
    let old = Rect::new(0.0, 0.0, 864.0, 1001.0);
    let new = Rect::new(0.0, 0.0, 1152.0, 1001.0);
    let candidates = [candidate(7, 864.0, 0.0, 864.0, 1001.0)];
    let plan = plan(&params(old, new, &candidates, 0.0)).unwrap();
    assert_eq!(plan.focused_frame, new);
    assert_eq!(
        frames(&plan),
        vec![(
            7,
            AdjustmentKind::Adjacent,
            Rect::new(1152.0, 0.0, 576.0, 1001.0)
        )]
    );
    assert_eq!(plan.adjustments[0].old_frame, candidates[0].frame);
}

#[test]
fn neighbour_minimum_width_limits_the_shared_edge() {
    let old = Rect::new(0.0, 0.0, 864.0, 1001.0);
    let new = Rect::new(0.0, 0.0, 1152.0, 1001.0);
    let candidates = [Candidate {
        minimum_size: Some(Size::new(700.0, 100.0)),
        ..candidate(7, 864.0, 0.0, 864.0, 1001.0)
    }];
    let plan = plan(&params(old, new, &candidates, 0.0)).unwrap();
    assert_eq!(plan.focused_frame, Rect::new(0.0, 0.0, 1028.0, 1001.0));
    assert_eq!(frames(&plan)[0].2, Rect::new(1028.0, 0.0, 700.0, 1001.0));
    assert!(plan
        .debug_log
        .iter()
        .any(|line| line.contains("урезан") && line.contains("окно 7")));
}

#[test]
fn gaps_are_kept_between_neighbours() {
    // Левая и правая половины с гэпом 10: 10 + 849 + 10 + 849 + 10.
    let old = Rect::new(10.0, 10.0, 849.0, 981.0);
    let new = Rect::new(10.0, 10.0, 1137.0, 981.0);
    let candidates = [candidate(3, 869.0, 10.0, 849.0, 981.0)];
    let plan = plan(&params(old, new, &candidates, 10.0)).unwrap();
    assert_eq!(plan.focused_frame, new);
    assert_eq!(frames(&plan)[0].2, Rect::new(1157.0, 10.0, 561.0, 981.0));
}

#[test]
fn stacked_neighbours_and_same_column_windows_move_together() {
    // Левая половина растёт вправо: справа два угла стопкой, в её колонке — угол сверху,
    // который растёт вместе с ней; окно посередине половины не задето.
    let old = Rect::new(0.0, 0.0, 864.0, 1001.0);
    let new = Rect::new(0.0, 0.0, 1152.0, 1001.0);
    let candidates = [
        candidate(1, 864.0, 501.0, 864.0, 500.0),
        candidate(2, 864.0, 0.0, 864.0, 501.0),
        candidate(3, 0.0, 501.0, 864.0, 500.0),
        candidate(4, 300.0, 200.0, 400.0, 300.0),
    ];
    let plan = plan(&params(old, new, &candidates, 0.0)).unwrap();
    assert_eq!(plan.focused_frame, new);
    assert_eq!(
        frames(&plan),
        vec![
            (
                3,
                AdjustmentKind::MatchingFocusedFrame,
                Rect::new(0.0, 501.0, 1152.0, 500.0)
            ),
            (
                1,
                AdjustmentKind::Adjacent,
                Rect::new(1152.0, 501.0, 576.0, 500.0)
            ),
            (
                2,
                AdjustmentKind::Adjacent,
                Rect::new(1152.0, 0.0, 576.0, 501.0)
            ),
        ]
    );
}

#[test]
fn capture_tolerance_takes_a_neighbour_that_does_not_touch() {
    let old = Rect::new(0.0, 0.0, 864.0, 1001.0);
    // Между окнами 40 пикселей: общим край не считается.
    let candidates = [candidate(9, 904.0, 0.0, 824.0, 1001.0)];

    // Окно в фокусе заходит на соседа — сосед пересекает новую границу и берётся и так.
    let wide = Rect::new(0.0, 0.0, 1152.0, 1001.0);
    let crossing = PlanParams {
        capture_tolerance: None,
        ..params(old, wide, &candidates, 0.0)
    };
    assert_eq!(
        frames(&plan(&crossing).unwrap()),
        vec![(
            9,
            AdjustmentKind::Adjacent,
            Rect::new(1152.0, 0.0, 576.0, 1001.0)
        )]
    );

    // Чуть шире, до соседа не достаёт: берёт только захват с допуском, и сосед
    // закрывает щель.
    let slightly = Rect::new(0.0, 0.0, 880.0, 1001.0);
    let captured = plan(&params(old, slightly, &candidates, 0.0)).unwrap();
    assert_eq!(
        frames(&captured),
        vec![(
            9,
            AdjustmentKind::Adjacent,
            Rect::new(880.0, 0.0, 848.0, 1001.0)
        )]
    );
    let without_capture = PlanParams {
        capture_tolerance: None,
        ..params(old, slightly, &candidates, 0.0)
    };
    assert_eq!(plan(&without_capture), None);
}

#[test]
fn windows_outside_the_screen_are_ignored() {
    let old = Rect::new(0.0, 0.0, 864.0, 1001.0);
    let new = Rect::new(0.0, 0.0, 1152.0, 1001.0);
    let candidates = [candidate(4, 1728.0, 0.0, 864.0, 1001.0)];
    assert_eq!(plan(&params(old, new, &candidates, 0.0)), None);
}

#[test]
fn correction_uses_the_size_the_neighbour_refused_to_leave() {
    let old = Rect::new(0.0, 0.0, 864.0, 1001.0);
    let new = Rect::new(0.0, 0.0, 1152.0, 1001.0);
    let candidates = [candidate(7, 864.0, 0.0, 864.0, 1001.0)];
    let request = params(old, new, &candidates, 0.0);
    let planned = plan(&request).unwrap();

    // Всё встало по плану — переделывать нечего.
    let exact: HashMap<u32, Option<Rect>> =
        HashMap::from([(7, Some(Rect::new(1152.0, 0.0, 576.0, 1001.0)))]);
    let correction = CorrectionParams {
        request,
        planned: &planned,
        actual_focused_frame: Some(planned.focused_frame),
        actual_candidate_frames: &exact,
        layout_tolerance: 4.0,
    };
    assert_eq!(correction_plan(&correction), None);

    // Сосед не ужался меньше 700 и вылез за экран — край отступает до 1028.
    let refused: HashMap<u32, Option<Rect>> =
        HashMap::from([(7, Some(Rect::new(1152.0, 0.0, 700.0, 1001.0)))]);
    let corrected = correction_plan(&CorrectionParams {
        actual_candidate_frames: &refused,
        ..correction
    })
    .unwrap();
    assert_eq!(corrected.focused_frame, Rect::new(0.0, 0.0, 1028.0, 1001.0));
    assert_eq!(
        frames(&corrected)[0].2,
        Rect::new(1028.0, 0.0, 700.0, 1001.0)
    );
    assert!(corrected.debug_log[0].contains("повторный проход"));

    // Рамку соседа не прочитать — на план это не влияет.
    let unreadable: HashMap<u32, Option<Rect>> = HashMap::from([(7, None)]);
    assert_eq!(
        correction_plan(&CorrectionParams {
            actual_candidate_frames: &unreadable,
            ..correction
        }),
        None
    );

    // Окно в фокусе не выросло до плана (у него свой минимум поменьше не сработал —
    // приложение оставило 1100): его минимум не меняется, план тот же.
    let short_focused = correction_plan(&CorrectionParams {
        actual_focused_frame: Some(Rect::new(0.0, 0.0, 1100.0, 1001.0)),
        ..correction
    })
    .unwrap();
    assert_eq!(short_focused.focused_frame, planned.focused_frame);
}

#[test]
fn corner_takes_the_boundary_its_neighbours_already_share() {
    // Угол сверху слева встаёт первым нажатием, справа уже стоит угол шириной 728:
    // граница 1000, а не половина экрана.
    let requested = Rect::new(0.0, 501.0, 864.0, 500.0);
    let tolerance = detection_tolerance(&SCREEN, 0.0);
    let neighbour = [candidate(5, 1000.0, 501.0, 728.0, 500.0)];
    let resolved = focused_frame_resolving_realized_corner_boundary(
        &requested,
        &SCREEN,
        &neighbour,
        Axis::Horizontal,
        MovedEdge::Right,
        tolerance,
        0.0,
    );
    assert_eq!(resolved, Rect::new(0.0, 501.0, 1000.0, 500.0));

    // Окно той же ячейки (тот же левый край и полоса) важнее соседа.
    let with_same_side = [
        candidate(5, 1000.0, 501.0, 728.0, 500.0),
        candidate(6, 0.0, 501.0, 700.0, 500.0),
    ];
    let resolved = focused_frame_resolving_realized_corner_boundary(
        &requested,
        &SCREEN,
        &with_same_side,
        Axis::Horizontal,
        MovedEdge::Right,
        tolerance,
        0.0,
    );
    assert_eq!(resolved, Rect::new(0.0, 501.0, 700.0, 500.0));

    // Граница в пределах допуска — остаётся запрошенная рамка.
    let close = [candidate(5, 870.0, 501.0, 858.0, 500.0)];
    assert_eq!(
        focused_frame_resolving_realized_corner_boundary(
            &requested,
            &SCREEN,
            &close,
            Axis::Horizontal,
            MovedEdge::Right,
            tolerance,
            0.0,
        ),
        requested
    );
}

#[test]
fn needs_application_ignores_unreadable_frames() {
    let planned = Rect::new(0.0, 0.0, 864.0, 1001.0);
    assert!(!frame_needs_application(None, &planned, &SCREEN, 4.0));
    assert!(!frame_needs_application(
        Some(&Rect::new(2.0, -3.0, 866.0, 1001.0)),
        &planned,
        &SCREEN,
        4.0
    ));
    assert!(frame_needs_application(
        Some(&Rect::new(0.0, 0.0, 870.0, 1001.0)),
        &planned,
        &SCREEN,
        4.0
    ));
    // Вылезло за рабочую область больше допуска — ставить заново, даже если «на месте».
    let outside = Rect::new(-10.0, 0.0, 864.0, 1001.0);
    assert!(frame_needs_application(
        Some(&outside),
        &outside,
        &SCREEN,
        4.0
    ));
}

#[test]
fn action_properties_follow_corner_axis_setting() {
    let horizontal = Config::default();
    let vertical = Config {
        corner_cycle_expansion_axis: Axis::Vertical,
        ..Config::default()
    };
    assert_eq!(
        resize_moved_edge(Action::TopLeft, &horizontal),
        Some(MovedEdge::Right)
    );
    assert_eq!(
        resize_moved_edge(Action::TopLeft, &vertical),
        Some(MovedEdge::Bottom)
    );
    assert_eq!(
        resize_moved_edge(Action::BottomRight, &vertical),
        Some(MovedEdge::Top)
    );
    assert_eq!(
        resize_moved_edge(Action::RightHalf, &vertical),
        Some(MovedEdge::Left)
    );
    assert_eq!(
        resize_axis(Action::TopHalf, &horizontal),
        Some(Axis::Vertical)
    );
    assert_eq!(
        resize_axis(Action::TopRight, &vertical),
        Some(Axis::Vertical)
    );
    assert_eq!(resize_axis(Action::Maximize, &horizontal), None);
    assert_eq!(resize_moved_edge(Action::Maximize, &horizontal), None);

    // Угол продолжает перебор только того же угла; половина — той же стороны и оси,
    // в том числе угла с той же стороной.
    assert!(is_compatible_repeated_resize_action(
        Action::TopLeft,
        Some(Action::TopLeft),
        &horizontal
    ));
    assert!(!is_compatible_repeated_resize_action(
        Action::TopLeft,
        Some(Action::BottomLeft),
        &horizontal
    ));
    assert!(is_compatible_repeated_resize_action(
        Action::LeftHalf,
        Some(Action::BottomLeft),
        &horizontal
    ));
    assert!(!is_compatible_repeated_resize_action(
        Action::LeftHalf,
        Some(Action::BottomLeft),
        &vertical
    ));
    assert!(!is_compatible_repeated_resize_action(
        Action::LeftHalf,
        None,
        &horizontal
    ));
    // Два действия без стороны и оси «совместимы» — как в оригинале.
    assert!(is_compatible_repeated_resize_action(
        Action::Maximize,
        Some(Action::Center),
        &horizontal
    ));

    assert!(ExecutionSource::DragToSnap.allows_cooperative_resize());
    assert!(!ExecutionSource::MenuItem.allows_cooperative_resize());
    assert!(!ExecutionSource::Url.allows_cooperative_resize());
    assert!(!ExecutionSource::TitleBar.allows_cooperative_resize());
}

#[test]
fn oracle_vertical_plan_on_fractional_screen() {
    // Оракул, случай p136: рабочая область с дробными краями, гэп 17,5, край по разнице
    // рамок (верхний), нулевой допуск общих краёв — соседей берёт захват.
    let screen = Rect::new(10.5, 20.25, 1280.5, 777.75);
    let candidates = [
        candidate(43048, 28.0, 39.75, 600.75, 235.75),
        candidate(51369, 951.8609896436946, 160.81921191423783, 104.0, 562.0),
        candidate(9201, 657.75, 301.0, 643.75, 492.5),
        candidate(35834, 668.0, 229.0, 569.0, 691.0),
    ];
    let new = Rect::new(659.75, 37.75, 613.75, 491.75);
    let plan = plan(&PlanParams {
        old_focused_frame: Rect::new(659.75, 37.75, 613.75, 232.75),
        new_focused_frame: new,
        screen_frame: screen,
        candidates: &candidates,
        axis: Axis::Vertical,
        tolerance: 0.0,
        minimum_size: Size::new(200.0, 150.0),
        focused_minimum_size: None,
        gap_size: 17.5,
        capture_tolerance: Some(62.22),
        moved_edge_override: None,
        candidate_discovery_frame: Some(new),
        action_description: "оракул",
    })
    .unwrap();
    assert_eq!(plan.focused_frame, Rect::new(660.0, 38.0, 614.0, 492.0));
    assert_eq!(
        frames(&plan),
        vec![
            (
                51369,
                AdjustmentKind::MatchingFocusedFrame,
                Rect::new(952.0, 38.0, 200.0, 492.0)
            ),
            (
                35834,
                AdjustmentKind::MatchingFocusedFrame,
                Rect::new(660.0, 38.0, 614.0, 492.0)
            ),
            (
                9201,
                AdjustmentKind::Adjacent,
                Rect::new(660.0, 548.0, 614.0, 250.0)
            ),
        ]
    );
}
