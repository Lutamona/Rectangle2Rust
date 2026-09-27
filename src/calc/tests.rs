//! Регрессионные тесты расчётов: каждое число — из Swift-оракула
//! (`tools/swift-oracle`, id случая в комментарии), по классу расхождений, которые
//! были у порта до сверки.

use super::*;
use crate::config::{
    CornerCycleExpansionAxis, CycleSize, CycleSizes, SubsequentExecutionMode, TodoSidebarWidthUnit,
};
use crate::side_split_ratios;

/// `NSScreen.screens[0].frame.maxY` машины, на которой снимался оракул.
const PRIMARY_MAX_Y: f64 = 1117.0;

/// Рабочая область `v1` оракула — горизонтальная.
const LANDSCAPE: Rect = Rect {
    x: 0.0,
    y: 0.0,
    w: 1728.0,
    h: 1001.0,
};

/// Рабочая область `v4` — вертикальная.
const PORTRAIT: Rect = Rect {
    x: 0.0,
    y: 0.0,
    w: 1001.0,
    h: 1728.0,
};

/// Обычное окно `wn`: +300, +200 от начала области, 800 × 600.
fn normal_window(visible: Rect) -> Rect {
    Rect::new(visible.x + 300.0, visible.y + 200.0, 800.0, 600.0)
}

/// Итог нажатия, как его печатает оракул.
#[derive(Debug, PartialEq)]
struct Pressed {
    action: Action,
    sub_action: Option<SubAction>,
    rect: Rect,
}

/// Нажатия подряд — тем же конвейером, что оракул (`WindowManager.execute`): расчёт,
/// гэпы, доля сторон, история. Окно встаёт ровно в посчитанную рамку.
fn press(config: &Config, visible: Rect, window: Rect, actions: &[Action]) -> Vec<Option<Pressed>> {
    side_split_ratios::reset_all(config);
    let mut window = window;
    let mut last: Option<LastAction> = None;
    actions
        .iter()
        .map(|&action| {
            let params = CalcParams {
                window,
                visible,
                visible_ignoring_stage: None,
                action,
                last: last.as_ref(),
                config,
                source_visible: None,
                num_screens: 1,
                primary_max_y: PRIMARY_MAX_Y,
            };
            let result = calculate(&params)?;
            let rect = apply_gaps(result.rect, result.action, result.sub_action, config);
            side_split_ratios::record_side_action(
                result.action,
                &result.rect,
                &result.screen_frame.unwrap_or(visible),
                config,
            );
            let count = match last {
                Some(last) if last.action == result.action => last.count + 1,
                _ => 1,
            };
            last = Some(LastAction {
                action: result.action,
                sub_action: result.sub_action,
                rect: rect.screen_flipped(PRIMARY_MAX_Y),
                count,
            });
            window = rect;
            Some(Pressed {
                action: result.action,
                sub_action: result.sub_action,
                rect,
            })
        })
        .collect()
}

fn pressed(
    action: Action,
    sub_action: Option<SubAction>,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
) -> Option<Pressed> {
    Some(Pressed {
        action,
        sub_action,
        rect: Rect::new(x, y, w, h),
    })
}

fn gaps(gap_size: f32) -> Config {
    Config {
        gap_size,
        ..Config::default()
    }
}

#[test]
fn halves_cycle_through_default_sizes() {
    // c0-v1-wn-leftHalf: ½ → ⅔ → ⅓ → ½.
    let config = Config::default();
    let rects = press(
        &config,
        LANDSCAPE,
        normal_window(LANDSCAPE),
        &[Action::LeftHalf; 4],
    );
    let widths: Vec<f64> = rects
        .iter()
        .map(|item| item.as_ref().unwrap().rect.w)
        .collect();
    assert_eq!(widths, [864.0, 1152.0, 576.0, 864.0]);
}

#[test]
fn grid_cells_carry_sub_actions_for_gaps() {
    // Под-действие ячейки задаёт общие с соседями края: у них гэп половинный.
    let config = gaps(10.0);
    // c1-v1-wn-topLeftSixth, шаг 1.
    assert_eq!(
        press(
            &config,
            LANDSCAPE,
            normal_window(LANDSCAPE),
            &[Action::TopLeftSixth]
        )[0],
        pressed(
            Action::TopLeftSixth,
            Some(SubAction::TopLeftSixthLandscape),
            10.0,
            506.0,
            561.0,
            485.0
        )
    );
    // c1-v1-wn-middleCenterNinth, шаг 1.
    assert_eq!(
        press(
            &config,
            LANDSCAPE,
            normal_window(LANDSCAPE),
            &[Action::MiddleCenterNinth]
        )[0],
        pressed(
            Action::MiddleCenterNinth,
            Some(SubAction::MiddleCenterNinth),
            581.0,
            338.0,
            566.0,
            323.0
        )
    );
}

#[test]
fn corners_have_no_sub_action_outside_quadrant_mode() {
    // c1-v1-wn-topLeft, шаг 1: гэпы по общим краям самого угла (снизу и справа).
    assert_eq!(
        press(
            &gaps(10.0),
            LANDSCAPE,
            normal_window(LANDSCAPE),
            &[Action::TopLeft]
        )[0],
        pressed(Action::TopLeft, None, 10.0, 506.0, 849.0, 485.0)
    );
}

#[test]
fn maximize_is_recorded_as_maximize() {
    // c0-v1-wn-chain-maximizeLeftLeft: после «развернуть» левая половина — первое нажатие.
    let rects = press(
        &Config::default(),
        LANDSCAPE,
        normal_window(LANDSCAPE),
        &[Action::Maximize, Action::LeftHalf, Action::LeftHalf],
    );
    assert_eq!(
        rects[0],
        pressed(Action::Maximize, None, 0.0, 0.0, 1728.0, 1001.0)
    );
    assert_eq!(
        rects[1],
        pressed(Action::LeftHalf, None, 0.0, 0.0, 864.0, 1001.0)
    );
    assert_eq!(
        rects[2],
        pressed(Action::LeftHalf, None, 0.0, 0.0, 1152.0, 1001.0)
    );

    // c3-v1-wn-maximize: applyGapsToMaximize=false — без гэпов.
    let config = Config {
        gap_size: 7.0,
        apply_gaps_to_maximize: Some(false),
        ..Config::default()
    };
    assert_eq!(
        press(
            &config,
            LANDSCAPE,
            normal_window(LANDSCAPE),
            &[Action::Maximize]
        )[0],
        pressed(Action::Maximize, None, 0.0, 0.0, 1728.0, 1001.0)
    );
}

#[test]
fn display_actions_need_a_second_screen() {
    // c0-v1-wn-nextDisplay, c0-v1-wn-displayTwo: при одном экране расчёта нет (бип).
    let rects = press(
        &Config::default(),
        LANDSCAPE,
        normal_window(LANDSCAPE),
        &[
            Action::NextDisplay,
            Action::PreviousDisplay,
            Action::Display(2),
        ],
    );
    assert_eq!(rects, [None, None, None]);
}

#[test]
fn todo_sidebar_width() {
    // c0-v1-wn-rightTodo: 400 px у правого края.
    assert_eq!(
        press(
            &Config::default(),
            LANDSCAPE,
            normal_window(LANDSCAPE),
            &[Action::RightTodo]
        )[0],
        pressed(
            Action::RightTodo,
            Some(SubAction::RightTodo),
            1328.0,
            0.0,
            400.0,
            1001.0
        )
    );
    // c24-v1-wn-leftTodo: доля 0,3 — Float, переведённый в CGFloat.
    let config = Config {
        todo_sidebar_width: 0.3,
        ..Config::default()
    };
    assert_eq!(
        press(
            &config,
            LANDSCAPE,
            normal_window(LANDSCAPE),
            &[Action::LeftTodo]
        )[0],
        pressed(
            Action::LeftTodo,
            Some(SubAction::LeftTodo),
            0.0,
            0.0,
            0.3f32 as f64 * 1728.0,
            1001.0
        )
    );
    // c23-v1-wn-leftTodo: 30 % → round(518,4) = 518, гэп 2,5.
    let config = Config {
        todo_sidebar_width: 30.0,
        todo_sidebar_width_unit: TodoSidebarWidthUnit::Pct,
        gap_size: 2.5,
        ..Config::default()
    };
    assert_eq!(
        press(
            &config,
            LANDSCAPE,
            normal_window(LANDSCAPE),
            &[Action::LeftTodo]
        )[0],
        pressed(
            Action::LeftTodo,
            Some(SubAction::LeftTodo),
            2.5,
            2.5,
            514.25,
            996.0
        )
    );
}

#[test]
fn repeated_press_continues_from_neighbouring_action() {
    let config = Config::default();
    let window = normal_window(LANDSCAPE);
    use Action::*;

    // c0-v1-wn-chain-lastThirdx3FirstThird, шаг 4: окно в левой трети после «последней
    // трети» — первая треть ведёт в центр.
    let rects = press(
        &config,
        LANDSCAPE,
        window,
        &[LastThird, LastThird, LastThird, FirstThird],
    );
    assert_eq!(
        rects[3],
        pressed(
            FirstThird,
            Some(SubAction::CenterVerticalThird),
            576.0,
            0.0,
            576.0,
            1001.0
        )
    );

    // c0-v1-wn-chain-lastFourthx4FirstFourth, шаг 5.
    let rects = press(
        &config,
        LANDSCAPE,
        window,
        &[LastFourth, LastFourth, LastFourth, LastFourth, FirstFourth],
    );
    assert_eq!(
        rects[4],
        pressed(
            FirstFourth,
            Some(SubAction::CenterLeftFourth),
            432.0,
            0.0,
            432.0,
            1001.0
        )
    );

    // c0-v1-wn-chain-firstTwoThirdsx2LastTwoThirds, шаг 3: «последние две трети» смотрят
    // только на под-действие.
    let rects = press(
        &config,
        LANDSCAPE,
        window,
        &[FirstTwoThirds, FirstTwoThirds, LastTwoThirds],
    );
    assert_eq!(
        rects[2],
        pressed(
            LastTwoThirds,
            Some(SubAction::LeftTwoThirds),
            0.0,
            0.0,
            1152.0,
            1001.0
        )
    );

    // c0-v4-wn-chain-firstTwoThirdsTopBottomVerticalTwoThirds, шаг 2.
    let rects = press(
        &config,
        PORTRAIT,
        normal_window(PORTRAIT),
        &[FirstTwoThirds, TopVerticalTwoThirds],
    );
    assert_eq!(
        rects[1],
        pressed(
            TopVerticalTwoThirds,
            Some(SubAction::BottomTwoThirds),
            0.0,
            0.0,
            1001.0,
            1152.0
        )
    );

    // c0-v1-wn-chain-topRightSixthx3TopLeftSixth, шаг 4.
    let rects = press(
        &config,
        LANDSCAPE,
        window,
        &[TopRightSixth, TopRightSixth, TopRightSixth, TopLeftSixth],
    );
    assert_eq!(
        rects[3],
        pressed(
            TopLeftSixth,
            Some(SubAction::TopCenterSixthLandscape),
            576.0,
            501.0,
            576.0,
            500.0
        )
    );
}

#[test]
fn middle_vertical_third_does_not_cycle() {
    // c0-v1-wn-middleVerticalThird, шаг 2: та же треть; высота — без floor.
    let rects = press(
        &Config::default(),
        LANDSCAPE,
        normal_window(LANDSCAPE),
        &[Action::MiddleVerticalThird; 2],
    );
    assert_eq!(
        rects[1],
        pressed(
            Action::MiddleVerticalThird,
            Some(SubAction::CenterVerticalThird),
            0.0,
            333.0,
            1728.0,
            1001.0 / 3.0
        )
    );
}

#[test]
fn cycling_depends_on_mode_not_on_resizing() {
    use Action::*;
    // Режим «на соседний экран» (acrossMonitor): столбики и четверти всё равно
    // перебираются — оригинал проверяет только «не none».
    let across = Config {
        subsequent_execution_mode: SubsequentExecutionMode::AcrossMonitor,
        ..Config::default()
    };
    let window = normal_window(LANDSCAPE);
    // c26-v1-wn-columnFive3, шаг 2.
    let column = Column { count: 5, index: 3 };
    assert_eq!(
        press(&across, LANDSCAPE, window, &[column; 2])[1],
        pressed(column, None, 1037.0, 0.0, 345.0, 1001.0)
    );
    // c26-v1-wn-secondFourth, шаги 2–3.
    let rects = press(&across, LANDSCAPE, window, &[SecondFourth; 3]);
    assert_eq!(
        rects[1],
        pressed(
            SecondFourth,
            Some(SubAction::RightThreeFourths),
            432.0,
            0.0,
            1296.0,
            1001.0
        )
    );
    assert_eq!(
        rects[2],
        pressed(
            SecondFourth,
            Some(SubAction::CenterVerticalHalf),
            432.0,
            0.0,
            864.0,
            1001.0
        )
    );

    // Режим none: «к краю» с ресайзом и центр-половина с centerHalfCycles перебирают.
    let none = Config {
        subsequent_execution_mode: SubsequentExecutionMode::None,
        resize_on_directional_move: true,
        center_half_cycles: Some(true),
        ..Config::default()
    };
    // c19-v1-wn-moveLeft, шаг 2.
    assert_eq!(
        press(&none, LANDSCAPE, window, &[MoveLeft; 2])[1],
        pressed(MoveLeft, None, 0.0, 201.0, 1152.0, 600.0)
    );
    // c19-v1-wn-centerHalf, шаг 2.
    assert_eq!(
        press(&none, LANDSCAPE, window, &[CenterHalf; 2])[1],
        pressed(
            CenterHalf,
            Some(SubAction::CenterVerticalHalf),
            288.0,
            0.0,
            1152.0,
            1001.0
        )
    );
}

#[test]
fn center_sixths_cycle_into_two_sixths_blocks() {
    use Action::*;
    let config = Config::default();
    // c0-v1-wn-topCenterSixth: шестая → две шестых справа → две слева.
    let rects = press(
        &config,
        LANDSCAPE,
        normal_window(LANDSCAPE),
        &[TopCenterSixth; 3],
    );
    assert_eq!(
        rects[1],
        pressed(
            TopCenterSixth,
            Some(SubAction::TopRightTwoSixthsLandscape),
            576.0,
            501.0,
            1152.0,
            500.0
        )
    );
    assert_eq!(
        rects[2],
        pressed(
            TopCenterSixth,
            Some(SubAction::TopLeftTwoSixthsLandscape),
            0.0,
            501.0,
            1152.0,
            500.0
        )
    );
    // c0-v4-wn-topCenterSixth: на вертикальном экране свой путь.
    let rects = press(
        &config,
        PORTRAIT,
        normal_window(PORTRAIT),
        &[TopCenterSixth; 3],
    );
    assert_eq!(
        rects[1],
        pressed(
            TopCenterSixth,
            Some(SubAction::BottomLeftTwoSixthsPortrait),
            0.0,
            0.0,
            500.0,
            1152.0
        )
    );
    assert_eq!(
        rects[2],
        pressed(
            TopCenterSixth,
            Some(SubAction::TopLeftTwoSixthsPortrait),
            0.0,
            576.0,
            500.0,
            1152.0
        )
    );
    // c0-v4-wn-bottomCenterSixth, шаг 3.
    let rects = press(
        &config,
        PORTRAIT,
        normal_window(PORTRAIT),
        &[BottomCenterSixth; 3],
    );
    assert_eq!(
        rects[2],
        pressed(
            BottomCenterSixth,
            Some(SubAction::TopRightTwoSixthsPortrait),
            501.0,
            576.0,
            500.0,
            1152.0
        )
    );
}

#[test]
fn grids_walk_their_cells_and_wrap() {
    use Action::*;
    let config = Config::default();
    let window = normal_window(LANDSCAPE);
    // c0-v1-wn-chain-topLeftEighthx9: восьмая ячейка → первая.
    let rects = press(&config, LANDSCAPE, window, &[TopLeftEighth; 9]);
    assert_eq!(
        rects[4],
        pressed(
            TopLeftEighth,
            Some(SubAction::BottomLeftEighth),
            0.0,
            0.0,
            432.0,
            500.0
        )
    );
    assert_eq!(rects[8], rects[0]);
    // c0-v1-wn-bottomRightSixteenth, шаг 2: последняя ячейка → первая.
    assert_eq!(
        press(&config, LANDSCAPE, window, &[BottomRightSixteenth; 2])[1],
        pressed(
            BottomRightSixteenth,
            Some(SubAction::TopLeftSixteenth),
            0.0,
            751.0,
            432.0,
            250.0
        )
    );
    // c4-v1-wn-topLeftSixth: режим none — перебора нет.
    let none = Config {
        subsequent_execution_mode: SubsequentExecutionMode::None,
        ..Config::default()
    };
    let rects = press(&none, LANDSCAPE, window, &[TopLeftSixth; 2]);
    assert_eq!(rects[1], rects[0]);
}

#[test]
fn minimum_window_size_is_floored_and_guarded() {
    // c10-v1-wn-halveHeightUp: минимум floor(1001 × 0,3) = 300, окно 300 — не меньше.
    let config = Config {
        minimum_window_width: 0.3,
        minimum_window_height: 0.3,
        ..Config::default()
    };
    assert_eq!(
        press(
            &config,
            LANDSCAPE,
            normal_window(LANDSCAPE),
            &[Action::HalveHeightUp]
        )[0],
        pressed(Action::HalveHeightUp, None, 300.0, 500.0, 800.0, 300.0)
    );
    // c24-v1-wn-smaller: доля вне 0…1 заменяется на ¼, шаг ≤ 0 — на 30.
    let config = Config {
        minimum_window_width: 1.5,
        minimum_window_height: 0.0,
        size_offset: -5.0,
        ..Config::default()
    };
    assert_eq!(
        press(
            &config,
            LANDSCAPE,
            normal_window(LANDSCAPE),
            &[Action::Smaller]
        )[0],
        pressed(Action::Smaller, None, 315.0, 215.0, 770.0, 570.0)
    );
}

#[test]
fn smaller_on_window_against_all_edges_shrinks_from_center() {
    // c1-v1-wn-chain-maximizeSmallerx2SmallerHeight, шаг 2: развёрнутое с гэпами окно
    // прижато ко всем краям — «меньше» уменьшает его на шаг с обеих сторон.
    let rects = press(
        &gaps(10.0),
        LANDSCAPE,
        normal_window(LANDSCAPE),
        &[Action::Maximize, Action::Smaller],
    );
    assert_eq!(
        rects[1],
        pressed(Action::Smaller, None, 25.0, 25.0, 1678.0, 951.0)
    );
}

#[test]
fn side_split_ratios_follow_last_half_with_cooperative_resize() {
    use Action::*;
    let config = Config {
        cooperative_corner_resize: true,
        ..Config::default()
    };
    // c21-v1-wn-chain-bottomHalfx2TopHalfTopRight: нижняя половина в ⅔ сдвигает границу,
    // верхняя половина и угол делят экран по ней.
    let rects = press(
        &config,
        LANDSCAPE,
        normal_window(LANDSCAPE),
        &[BottomHalf, BottomHalf, TopHalf, TopRight],
    );
    assert_eq!(rects[2], pressed(TopHalf, None, 0.0, 667.0, 1728.0, 334.0));
    assert_eq!(
        rects[3],
        pressed(TopRight, None, 864.0, 667.0, 864.0, 334.0)
    );

    // c27-v4-wn-chain-rightHalfx3LeftHalfBottomRight: доля правой половины — Float:
    // 1 − 500/1001 даёт левой половине 501.
    let config = Config {
        cooperative_corner_resize: true,
        subsequent_execution_mode: SubsequentExecutionMode::CycleMonitor,
        ..Config::default()
    };
    let rects = press(
        &config,
        PORTRAIT,
        normal_window(PORTRAIT),
        &[RightHalf, RightHalf, RightHalf, LeftHalf],
    );
    assert_eq!(rects[3], pressed(LeftHalf, None, 0.0, 0.0, 501.0, 1728.0));
}

#[test]
fn cooperative_repeat_continues_after_compatible_action() {
    // c21-v1-wn-chain-maximizeCenterHalfx3: у «развернуть» и центр-половины нет стороны
    // и оси, для кооперативного перебора это одно и то же действие — перебор идёт
    // по счётчику «развернуть».
    let config = Config {
        cooperative_corner_resize: true,
        ..Config::default()
    };
    let rects = press(
        &config,
        LANDSCAPE,
        normal_window(LANDSCAPE),
        &[
            Action::Maximize,
            Action::CenterHalf,
            Action::CenterHalf,
            Action::CenterHalf,
        ],
    );
    let center = |x: f64, w: f64| {
        pressed(
            Action::CenterHalf,
            Some(SubAction::CenterVerticalHalf),
            x,
            0.0,
            w,
            1001.0,
        )
    };
    assert_eq!(rects[1], center(288.0, 1152.0));
    assert_eq!(rects[2], center(576.0, 576.0));
    assert_eq!(rects[3], center(432.0, 864.0));
}

#[test]
fn quadrant_mode_cycles_quarters() {
    use Action::*;
    // c18-v1-wn-chain-topLeftx4BottomRight: четверти по кругу; «снизу справа» после
    // того, как окно встало в эту четверть, продолжает перебор.
    let config = Config {
        subsequent_execution_mode: SubsequentExecutionMode::ResizeAndCycleQuadrants,
        gap_size: 8.0,
        horizontal_split_ratio: 35.0,
        ..Config::default()
    };
    let rects = press(
        &config,
        LANDSCAPE,
        normal_window(LANDSCAPE),
        &[TopLeft, TopLeft, TopLeft, TopLeft, BottomRight],
    );
    assert_eq!(
        rects[1],
        pressed(
            TopLeft,
            Some(SubAction::TopRightQuarter),
            609.0,
            505.0,
            1111.0,
            488.0
        )
    );
    assert_eq!(
        rects[3],
        pressed(
            TopLeft,
            Some(SubAction::BottomRightQuarter),
            609.0,
            8.0,
            1111.0,
            488.0
        )
    );
    assert_eq!(
        rects[4],
        pressed(
            BottomRight,
            Some(SubAction::TopLeftQuarter),
            8.0,
            505.0,
            592.0,
            488.0
        )
    );
}

#[test]
fn split_ratio_and_cycle_sizes_are_float() {
    // c23-v1-wn-leftHalf: 33,3 % и ⅓ считаются во Float, гэп 2,5.
    let config = Config {
        horizontal_split_ratio: 33.3,
        vertical_split_ratio: 66.7,
        gap_size: 2.5,
        ..Config::default()
    };
    let rects = press(
        &config,
        LANDSCAPE,
        normal_window(LANDSCAPE),
        &[Action::LeftHalf; 4],
    );
    let widths: Vec<f64> = rects
        .iter()
        .map(|item| item.as_ref().unwrap().rect.w)
        .collect();
    assert_eq!(widths, [571.25, 1148.25, 572.25, 860.25]);
    // c23-v1-wn-topLeft, шаг 1.
    assert_eq!(
        press(
            &config,
            LANDSCAPE,
            normal_window(LANDSCAPE),
            &[Action::TopLeft]
        )[0],
        pressed(Action::TopLeft, None, 2.5, 335.25, 571.25, 663.25)
    );
}

#[test]
fn selected_cycle_sizes_and_vertical_corner_axis() {
    use Action::*;
    // c11-v1-wn-leftHalf: набор ½, ¾, ¼ — после ½ идёт ¾.
    let config = Config {
        cycle_sizes_is_changed: true,
        selected_cycle_sizes: [
            CycleSize::OneHalf,
            CycleSize::ThreeQuarters,
            CycleSize::OneQuarter,
        ]
        .into_iter()
        .collect(),
        ..Config::default()
    };
    let rects = press(&config, LANDSCAPE, normal_window(LANDSCAPE), &[LeftHalf; 3]);
    assert_eq!(rects[1], pressed(LeftHalf, None, 0.0, 0.0, 1296.0, 1001.0));
    assert_eq!(rects[2], pressed(LeftHalf, None, 0.0, 0.0, 432.0, 1001.0));

    // c14-v1-wn-leftHalf: пустой набор — перебора нет.
    let config = Config {
        cycle_sizes_is_changed: true,
        selected_cycle_sizes: CycleSizes::EMPTY,
        ..Config::default()
    };
    let rects = press(&config, LANDSCAPE, normal_window(LANDSCAPE), &[LeftHalf; 2]);
    assert_eq!(rects[1], rects[0]);

    // c13-v1-wn-topLeft, шаг 2: угол растёт по вертикали до ⅔.
    let config = Config {
        cycle_sizes_is_changed: true,
        selected_cycle_sizes: CycleSize::SORTED.into_iter().collect(),
        corner_cycle_expansion_axis: CornerCycleExpansionAxis::Vertical,
        ..Config::default()
    };
    let rects = press(&config, LANDSCAPE, normal_window(LANDSCAPE), &[TopLeft; 2]);
    assert_eq!(rects[1], pressed(TopLeft, None, 0.0, 334.0, 864.0, 667.0));
}

#[test]
fn columns_cycle_only_while_window_stays() {
    let config = Config::default();
    let column = Action::Column { count: 5, index: 1 };
    // c0-v1-wn-columnFive1: 346/345/346/345/346 — повтор переставляет в следующий.
    let rects = press(&config, LANDSCAPE, normal_window(LANDSCAPE), &[column; 2]);
    assert_eq!(rects[0], pressed(column, None, 0.0, 0.0, 346.0, 1001.0));
    assert_eq!(rects[1], pressed(column, None, 346.0, 0.0, 345.0, 1001.0));

    // Окно подвинули после нажатия: перебор начинается заново.
    let last = LastAction {
        action: column,
        sub_action: None,
        rect: Rect::new(0.0, 0.0, 346.0, 1001.0).screen_flipped(PRIMARY_MAX_Y),
        count: 1,
    };
    let params = CalcParams {
        window: Rect::new(40.0, 0.0, 346.0, 1001.0),
        visible: LANDSCAPE,
        visible_ignoring_stage: None,
        action: column,
        last: Some(&last),
        config: &config,
        source_visible: None,
        num_screens: 1,
        primary_max_y: PRIMARY_MAX_Y,
    };
    assert_eq!(
        calculate(&params).unwrap().rect,
        Rect::new(0.0, 0.0, 346.0, 1001.0)
    );
}

#[test]
fn center_keeps_size_and_maximizes_oversized_window() {
    let config = Config::default();
    // c0-v1-wn-center.
    assert_eq!(
        press(
            &config,
            LANDSCAPE,
            normal_window(LANDSCAPE),
            &[Action::Center]
        )[0],
        pressed(Action::Center, None, 464.0, 201.0, 800.0, 600.0)
    );
    // c0-v1-wh-centerProminently: окно больше области — «развернуть».
    let huge = Rect::new(-100.0, -100.0, 2028.0, 1301.0);
    assert_eq!(
        press(&config, LANDSCAPE, huge, &[Action::CenterProminently])[0],
        pressed(Action::Maximize, None, 0.0, 0.0, 1728.0, 1001.0)
    );
}
