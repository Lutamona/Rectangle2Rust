//! Перебор размеров при повторных нажатиях — порт `RepeatedExecutionsCalculation.swift`,
//! `RepeatedExecutionsInThirdsCalculation.swift` и расчётов, построенных на нём:
//! половины (`LeftRightHalfCalculation`, `TopHalfCalculation`, `BottomHalfCalculation`),
//! центр-половина (`CenterHalfCalculation`), углы (`UpperLeftCalculation` и соседи,
//! `CornerCycleExpansionCalculation`, `QuartersRepeated`) и «к краю»
//! (`MoveLeftRightCalculation`, `MoveUpDownCalculation`).
//!
//! Какой из вариантов перебора зовёт каждый расчёт — как в оригинале, где это решает
//! статическая диспетчеризация расширений протоколов: половины — `repeated_side_rect`,
//! центр-половина и «к краю» — `repeated_rect_in_thirds`, углы — `Corner::cycled_rect`.
//! Без кооперативного ресайза все три сводятся к `Repeated::repeated_rect`.

use super::cooperative::{self, MovedEdge};
use super::gaps;
use super::{apply_gaps_raw, swift_max, CalcParams, CalcResult, RectParams, RectResult};
use crate::actions::{Action, Dimension, SubAction};
use crate::config::{Config, CornerCycleExpansionAxis, CycleSize, SubsequentExecutionMode};
use crate::geometry::{self, horizontal_rect, vertical_rect, Rect, Side};
use crate::side_split_ratios;

/// Размеры перебора по порядку (`sortedCycleSizes`): пока набор не меняли — ½, ⅔, ⅓.
fn sorted_cycle_sizes(config: &Config) -> Vec<CycleSize> {
    config.effective_cycle_sizes().sorted_sizes()
}

/// Какой размер у `count`-го повтора (`cycleIndex(forExecutionCount:in:)`): если в наборе
/// есть ½ (размер первого нажатия), счёт идёт с нуля, иначе с единицы.
fn cycle_index(count: u32, sorted: &[CycleSize]) -> usize {
    let count = count as usize;
    if sorted.contains(&CycleSize::OneHalf) {
        count % sorted.len()
    } else {
        count.saturating_sub(1) % sorted.len()
    }
}

/// Расчёт с перебором размеров (`RepeatedExecutionsCalculation`): первое нажатие и
/// прямоугольник для размера из набора.
trait Repeated {
    fn first_rect(&self, p: &RectParams) -> RectResult;
    fn rect_for(&self, size: CycleSize, p: &RectParams) -> RectResult;

    /// `calculateRepeatedRect` протокола: то же действие ещё раз — размер по счётчику
    /// нажатий, другое действие — первое нажатие.
    fn repeated_rect(&self, p: &RectParams) -> RectResult {
        let last = match p.last {
            Some(last) if last.action == p.action => last,
            _ => return self.first_rect(p),
        };
        let sorted = sorted_cycle_sizes(p.config);
        if sorted.is_empty() {
            return self.first_rect(p);
        }
        self.rect_for(sorted[cycle_index(last.count, &sorted)], p)
    }
}

/// Прошлое действие продолжает перебор с кооперативным ресайзом.
fn continues_cooperative_cycle(p: &RectParams) -> bool {
    cooperative::is_compatible_repeated_resize_action(
        p.action,
        p.last.map(|last| last.action),
        p.config,
    )
}

/// `RepeatedExecutionsInThirdsCalculation.calculateRepeatedRect`: с кооперативным
/// ресайзом следующий размер ищется по нынешней рамке окна, а не по счётчику.
fn repeated_rect_in_thirds(calc: &impl Repeated, p: &RectParams) -> RectResult {
    if !p.config.cooperative_corner_resize {
        return calc.repeated_rect(p);
    }
    if !continues_cooperative_cycle(p) {
        return calc.first_rect(p);
    }
    let sorted = sorted_cycle_sizes(p.config);
    if sorted.is_empty() {
        return calc.first_rect(p);
    }
    let current = sorted
        .iter()
        .position(|size| calc.rect_for(*size, p).rect == p.window);
    if let Some(current) = current {
        return calc.rect_for(sorted[(current + 1) % sorted.len()], p);
    }
    let Some(last) = p.last else {
        return calc.first_rect(p);
    };
    calc.rect_for(sorted[cycle_index(last.count, &sorted)], p)
}

/// `calculateRepeatedSideRect`: с кооперативным ресайзом размер, в котором окно уже
/// стоит, пропускается.
fn repeated_side_rect(calc: &impl Repeated, p: &RectParams) -> RectResult {
    if !p.config.cooperative_corner_resize {
        return calc.repeated_rect(p);
    }
    if !continues_cooperative_cycle(p) {
        return calc.first_rect(p);
    }
    let sorted = sorted_cycle_sizes(p.config);
    if sorted.is_empty() {
        return calc.first_rect(p);
    }
    let Some(last) = p.last else {
        return calc.first_rect(p);
    };
    let mut position = cycle_index(last.count, &sorted);
    for _ in 0..sorted.len() {
        let result = calc.rect_for(sorted[position], p);
        if result.rect != p.window {
            return result;
        }
        position = (position + 1) % sorted.len();
    }
    calc.first_rect(p)
}

// ---------------------------------------------------------------- половины

/// Половина экрана: левая/правая или верхняя/нижняя.
struct Half {
    horizontal: bool,
    side: Side,
}

impl Half {
    /// Половина для действия; вызывается только для четырёх половин.
    fn of(action: Action) -> Half {
        match action {
            Action::RightHalf => Half {
                horizontal: true,
                side: Side::Trailing,
            },
            Action::TopHalf => Half {
                horizontal: false,
                side: Side::Leading,
            },
            Action::BottomHalf => Half {
                horizontal: false,
                side: Side::Trailing,
            },
            _ => Half {
                horizontal: true,
                side: Side::Leading,
            },
        }
    }

    /// `calculateFractionalRect`: половина заданной доли, прижатая к своей стороне.
    fn fractional_rect(&self, p: &RectParams, fraction: f32) -> RectResult {
        let rect = if self.horizontal {
            horizontal_rect(&p.visible, self.side, fraction)
        } else {
            vertical_rect(&p.visible, self.side, fraction)
        };
        RectResult::new(rect)
    }
}

impl Repeated for Half {
    /// Первое нажатие делит экран по доле сторон (`ActiveSideSplitRatios`).
    fn first_rect(&self, p: &RectParams) -> RectResult {
        let ratio = if self.horizontal {
            side_split_ratios::horizontal_ratio(&p.visible, p.config)
        } else {
            side_split_ratios::vertical_ratio(&p.visible, p.config)
        };
        let fraction = if self.side == Side::Trailing {
            1.0 - ratio
        } else {
            ratio
        };
        self.fractional_rect(p, fraction)
    }

    fn rect_for(&self, size: CycleSize, p: &RectParams) -> RectResult {
        self.fractional_rect(p, size.fraction())
    }
}

/// «Левая/правая половина» (`LeftRightHalfCalculation.calculate`): перебор размеров —
/// только в режимах с ресайзом; итоговое действие — нажатое, без под-действия.
pub(super) fn left_right_half(p: &CalcParams) -> CalcResult {
    let half = Half::of(p.action);
    let rect = match p.config.subsequent_execution_mode {
        SubsequentExecutionMode::AcrossMonitor => return across_displays(p),
        SubsequentExecutionMode::AcrossAndResize if p.num_screens != 1 => {
            return across_displays(p)
        }
        SubsequentExecutionMode::AcrossAndResize
        | SubsequentExecutionMode::Resize
        | SubsequentExecutionMode::ResizeAndCycleQuadrants => {
            repeated_side_rect(&half, &p.rect_params()).rect
        }
        SubsequentExecutionMode::None | SubsequentExecutionMode::CycleMonitor => {
            half.first_rect(&p.rect_params()).rect
        }
    };
    CalcResult {
        rect,
        action: p.action,
        sub_action: None,
        screen_frame: None,
    }
}

/// Половины с переездом между экранами (`calculateAcrossDisplays`). Соседние экраны в
/// параметры не передаются, поэтому это ветка оригинала без соседей: половина на
/// текущем экране.
fn across_displays(p: &CalcParams) -> CalcResult {
    let action = if p.action == Action::RightHalf {
        Action::RightHalf
    } else {
        Action::LeftHalf
    };
    let mut params = p.rect_params();
    params.action = action;
    CalcResult {
        rect: Half::of(action).first_rect(&params).rect,
        action,
        sub_action: None,
        screen_frame: None,
    }
}

/// `LeftRightHalfCalculation.calculateRect` — половина первого нажатия (подсветка
/// drag-to-snap, повтор действия на другом экране).
pub(super) fn left_right_half_rect(p: &RectParams) -> RectResult {
    Half::of(p.action).first_rect(p)
}

/// `TopHalfCalculation`/`BottomHalfCalculation.calculateRect`.
pub(super) fn vertical_half_rect(p: &RectParams) -> RectResult {
    let half = Half::of(p.action);
    if p.last.is_none() || !p.config.subsequent_execution_mode.resizes() {
        return half.first_rect(p);
    }
    repeated_side_rect(&half, p)
}

// ---------------------------------------------------------------- центр-половина

/// Центр-половина (`CenterHalfCalculation`): по ширине на горизонтальном экране,
/// по высоте на вертикальном.
struct CenterHalf;

impl CenterHalf {
    fn fractional_rect(p: &RectParams, fraction: f32) -> RectResult {
        if p.visible.is_landscape() {
            center_half_landscape(&p.visible, fraction)
        } else {
            center_half_portrait(&p.visible, fraction)
        }
    }
}

impl Repeated for CenterHalf {
    fn first_rect(&self, p: &RectParams) -> RectResult {
        Self::fractional_rect(p, 1.0 / 2.0)
    }

    fn rect_for(&self, size: CycleSize, p: &RectParams) -> RectResult {
        Self::fractional_rect(p, size.fraction())
    }
}

/// `CenterHalfCalculation.landscapeRect(_:fraction:)`.
pub(super) fn center_half_landscape(visible: &Rect, fraction: f32) -> RectResult {
    let mut rect = *visible;
    rect.h = visible.h;
    rect.w = (visible.w * fraction as f64).round();
    rect.x = ((visible.w - rect.w) / 2.0).round() + visible.min_x();
    rect.y = ((visible.h - rect.h) / 2.0).round() + visible.min_y();
    RectResult::with_sub(rect, SubAction::CenterVerticalHalf)
}

/// `CenterHalfCalculation.portraitRect(_:fraction:)`.
pub(super) fn center_half_portrait(visible: &Rect, fraction: f32) -> RectResult {
    let mut rect = *visible;
    rect.w = visible.w;
    rect.h = (visible.h * fraction as f64).round();
    rect.x = ((visible.w - rect.w) / 2.0).round() + visible.min_x();
    rect.y = ((visible.h - rect.h) / 2.0).round() + visible.min_y();
    RectResult::with_sub(rect, SubAction::CenterHorizontalHalf)
}

/// `CenterHalfCalculation.calculateRect`: с `centerHalfCycles` перебирает размеры при
/// любом нажатии и в любом режиме повторов.
pub(super) fn center_half_rect(p: &RectParams) -> RectResult {
    if (p.last.is_some() && p.config.subsequent_execution_mode.resizes())
        || p.config.center_half_cycles == Some(true)
    {
        return repeated_rect_in_thirds(&CenterHalf, p);
    }
    CenterHalf::fractional_rect(p, 0.5)
}

// ---------------------------------------------------------------- углы

/// Угол экрана (`UpperLeftCalculation` и соседи).
#[derive(Clone, Copy)]
struct Corner {
    action: Action,
    horizontal_side: Side,
    vertical_side: Side,
    /// Под-действие четверти при переборе четвертей.
    quarter: SubAction,
}

impl Corner {
    /// Угол для действия; вызывается только для четырёх углов.
    fn of(action: Action) -> Corner {
        let (action, horizontal_side, vertical_side, quarter) = match action {
            Action::TopRight => (
                Action::TopRight,
                Side::Trailing,
                Side::Leading,
                SubAction::TopRightQuarter,
            ),
            Action::BottomLeft => (
                Action::BottomLeft,
                Side::Leading,
                Side::Trailing,
                SubAction::BottomLeftQuarter,
            ),
            Action::BottomRight => (
                Action::BottomRight,
                Side::Trailing,
                Side::Trailing,
                SubAction::BottomRightQuarter,
            ),
            _ => (
                Action::TopLeft,
                Side::Leading,
                Side::Leading,
                SubAction::TopLeftQuarter,
            ),
        };
        Corner {
            action,
            horizontal_side,
            vertical_side,
            quarter,
        }
    }

    fn horizontal_split_fraction(&self, p: &RectParams) -> f32 {
        let ratio = side_split_ratios::horizontal_ratio(&p.visible, p.config);
        if self.horizontal_side == Side::Trailing {
            1.0 - ratio
        } else {
            ratio
        }
    }

    fn vertical_split_fraction(&self, p: &RectParams) -> f32 {
        let ratio = side_split_ratios::vertical_ratio(&p.visible, p.config);
        if self.vertical_side == Side::Trailing {
            1.0 - ratio
        } else {
            ratio
        }
    }

    fn corner_rect(
        &self,
        p: &RectParams,
        horizontal_fraction: f32,
        vertical_fraction: f32,
    ) -> Rect {
        geometry::corner_rect(
            &p.visible,
            self.horizontal_side,
            self.vertical_side,
            horizontal_fraction,
            vertical_fraction,
        )
    }

    /// `calculateFractionalRect`: по оси перебора — доля размера, по другой оси — как у
    /// первого нажатия.
    fn fractional_rect(&self, p: &RectParams, fraction: f32) -> RectResult {
        let normal = self.first_rect(p).rect;
        let rect = match p.config.corner_cycle_expansion_axis {
            CornerCycleExpansionAxis::Horizontal => {
                let cycled = self.corner_rect(p, fraction, self.vertical_split_fraction(p));
                Rect::new(cycled.x, normal.y, cycled.w, normal.h)
            }
            CornerCycleExpansionAxis::Vertical => {
                let cycled = self.corner_rect(p, self.horizontal_split_fraction(p), fraction);
                Rect::new(normal.x, cycled.y, normal.w, cycled.h)
            }
        };
        RectResult::new(rect)
    }

    /// `quarterRect`: угол с под-действием четверти — для перебора четвертей.
    fn quarter_rect(&self, p: &RectParams) -> RectResult {
        let rect = self.corner_rect(
            p,
            self.horizontal_split_fraction(p),
            self.vertical_split_fraction(p),
        );
        RectResult::with_sub(rect, self.quarter)
    }

    /// `CornerCycleExpansionCalculation.calculateRepeatedRect`: с кооперативным ресайзом
    /// следующий размер ищется по рамке окна — с гэпами или сдвинутой соседями в пределах
    /// допуска.
    fn cycled_rect(&self, p: &RectParams) -> RectResult {
        if !p.config.cooperative_corner_resize {
            return self.repeated_rect(p);
        }
        if !continues_cooperative_cycle(p) {
            return self.first_rect(p);
        }
        let sorted = sorted_cycle_sizes(p.config);
        if sorted.is_empty() {
            return self.first_rect(p);
        }
        let current = sorted
            .iter()
            .position(|size| current_frame_matches(&p.window, &self.rect_for(*size, p).rect, p));
        if let Some(current) = current {
            return self.rect_for(sorted[(current + 1) % sorted.len()], p);
        }
        let Some(last) = p.last else {
            return self.first_rect(p);
        };
        self.rect_for(sorted[cycle_index(last.count, &sorted)], p)
    }
}

impl Repeated for Corner {
    fn first_rect(&self, p: &RectParams) -> RectResult {
        RectResult::new(self.corner_rect(
            p,
            self.horizontal_split_fraction(p),
            self.vertical_split_fraction(p),
        ))
    }

    fn rect_for(&self, size: CycleSize, p: &RectParams) -> RectResult {
        self.fractional_rect(p, size.fraction())
    }
}

/// Следующая четверть по кругу (`QuartersRepeated.nextCalculation(direction: .right)`).
fn next_quarter(sub_action: SubAction) -> Option<Corner> {
    let action = match sub_action {
        SubAction::TopLeftQuarter => Action::TopRight,
        SubAction::TopRightQuarter => Action::BottomLeft,
        SubAction::BottomLeftQuarter => Action::BottomRight,
        SubAction::BottomRightQuarter => Action::TopLeft,
        _ => return None,
    };
    Some(Corner::of(action))
}

/// Углы (`UpperLeftCalculation.calculateRect` и соседи). В режиме «размеры и четверти»
/// повтор переставляет окно в следующую четверть по кругу — и тогда, когда прошлым
/// действием был другой угол, но окно стоит в этой четверти.
pub(super) fn corner_rect(p: &RectParams) -> RectResult {
    let corner = Corner::of(p.action);
    let mode = p.config.subsequent_execution_mode;

    if mode.cycles_quadrant_positions() {
        if let Some(last) = p.last {
            if let Some(sub_action) = last.sub_action {
                if last.action == corner.action || sub_action == corner.quarter {
                    if let Some(next) = next_quarter(sub_action) {
                        return next.quarter_rect(p);
                    }
                }
            }
        }
        return corner.quarter_rect(p);
    }

    if p.last.is_none() || !mode.resizes() {
        return corner.first_rect(p);
    }
    corner.cycled_rect(p)
}

/// Окно стоит в размере перебора: рамка совпадает с ним или с ним же с гэпами, а у углов
/// ещё и с допуском — соседи при кооперативном ресайзе могли сдвинуть край
/// (`currentFrame(_:matchesCycleFrame:params:)`).
fn current_frame_matches(current: &Rect, cycle_frame: &Rect, p: &RectParams) -> bool {
    if cycle_frame == current {
        return true;
    }
    let gapped = gap_adjusted_cycle_frame(cycle_frame, p);
    if gapped == *current {
        return true;
    }
    if !cooperative::is_corner_action(p.action) {
        return false;
    }

    let axis = p.config.corner_cycle_expansion_axis;
    let tolerance = swift_max(4.0, p.config.gap_size as f64 * 2.0 + 4.0);
    let current_axis_size = axis_size(current, axis);
    let expected_axis_sizes = [axis_size(cycle_frame, axis), axis_size(&gapped, axis)];
    if !expected_axis_sizes
        .iter()
        .any(|size| (current_axis_size - size).abs() <= tolerance)
    {
        return false;
    }

    [*cycle_frame, gapped].iter().any(|expected| {
        matches_fixed_edge(current, expected, p, axis, tolerance)
            && matches_perpendicular_span(current, expected, axis, tolerance)
    })
}

/// Размер перебора с гэпами действия (`gapAdjustedCycleFrame`).
fn gap_adjusted_cycle_frame(cycle_frame: &Rect, p: &RectParams) -> Rect {
    let config = p.config;
    let gaps_applicable = p.action.gaps_applicable(
        config.resize_on_directional_move,
        config.apply_gaps_to_maximize != Some(false),
        config.apply_gaps_to_maximize_height != Some(false),
    );
    if !gaps::gaps_enabled(config) || gaps_applicable == Dimension::NONE {
        return *cycle_frame;
    }
    apply_gaps_raw(
        *cycle_frame,
        gaps_applicable,
        p.action.gap_shared_edge(config.resize_on_directional_move),
        config.gap_size,
        config.skip_gap_top_edge,
    )
}

/// Неподвижный край окна на месте (`matchesFixedEdge`).
fn matches_fixed_edge(
    current: &Rect,
    expected: &Rect,
    p: &RectParams,
    axis: CornerCycleExpansionAxis,
    tolerance: f64,
) -> bool {
    match cooperative::moved_edge(p.action, p.config) {
        Some(MovedEdge::Right | MovedEdge::Top) => {
            (axis_min(current, axis) - axis_min(expected, axis)).abs() <= tolerance
        }
        Some(MovedEdge::Left | MovedEdge::Bottom) => {
            (axis_max(current, axis) - axis_max(expected, axis)).abs() <= tolerance
        }
        None => false,
    }
}

/// Поперёк оси перебора окно занимает ту же полосу (`matchesPerpendicularSpan`).
fn matches_perpendicular_span(
    current: &Rect,
    expected: &Rect,
    axis: CornerCycleExpansionAxis,
    tolerance: f64,
) -> bool {
    match axis {
        CornerCycleExpansionAxis::Horizontal => {
            (current.min_y() - expected.min_y()).abs() <= tolerance
                && (current.max_y() - expected.max_y()).abs() <= tolerance
        }
        CornerCycleExpansionAxis::Vertical => {
            (current.min_x() - expected.min_x()).abs() <= tolerance
                && (current.max_x() - expected.max_x()).abs() <= tolerance
        }
    }
}

fn axis_min(frame: &Rect, axis: CornerCycleExpansionAxis) -> f64 {
    match axis {
        CornerCycleExpansionAxis::Horizontal => frame.min_x(),
        CornerCycleExpansionAxis::Vertical => frame.min_y(),
    }
}

fn axis_max(frame: &Rect, axis: CornerCycleExpansionAxis) -> f64 {
    match axis {
        CornerCycleExpansionAxis::Horizontal => frame.max_x(),
        CornerCycleExpansionAxis::Vertical => frame.max_y(),
    }
}

fn axis_size(frame: &Rect, axis: CornerCycleExpansionAxis) -> f64 {
    match axis {
        CornerCycleExpansionAxis::Horizontal => frame.w,
        CornerCycleExpansionAxis::Vertical => frame.h,
    }
}

// ---------------------------------------------------------------- «к краю»

/// «К краю» по горизонтали или вертикали: окно прижимается к краю, а с
/// `resizeOnDirectionalMove` ещё и перебирает размеры.
struct Move {
    horizontal: bool,
}

impl Move {
    /// `calculateGenericRect`: окно (с новой шириной или высотой, если задана доля)
    /// у своего края.
    fn generic_rect(&self, p: &RectParams, fraction: Option<f32>) -> RectResult {
        let visible = p.visible;
        let mut rect = p.window;
        if self.horizontal {
            if let Some(fraction) = fraction {
                rect.w = (visible.w * fraction as f64).floor();
            }
            rect.x = if p.action == Action::MoveRight {
                visible.max_x() - rect.w
            } else {
                visible.min_x()
            };
        } else {
            if let Some(fraction) = fraction {
                rect.h = (visible.h * fraction as f64).floor();
            }
            rect.y = if p.action == Action::MoveUp {
                visible.max_y() - rect.h
            } else {
                visible.min_y()
            };
        }
        RectResult::new(rect)
    }
}

impl Repeated for Move {
    fn first_rect(&self, p: &RectParams) -> RectResult {
        self.generic_rect(p, Some(1.0 / 2.0))
    }

    fn rect_for(&self, size: CycleSize, p: &RectParams) -> RectResult {
        self.generic_rect(p, Some(size.fraction()))
    }
}

/// `MoveLeftRightCalculation.calculateRect(_:newDisplay: false)`. Перебор размеров идёт
/// при любом режиме повторов: оригинал смотрит только на `resizeOnDirectionalMove`.
pub(super) fn move_left_right_rect(p: &RectParams) -> RectResult {
    let visible = p.visible;
    let calculation = Move { horizontal: true };
    let mut rect = if p.config.resize_on_directional_move {
        repeated_rect_in_thirds(&calculation, p).rect
    } else {
        calculation.generic_rect(p, None).rect
    };
    if p.config.centered_directional_move != Some(false) {
        rect.y = ((visible.h - rect.h) / 2.0).round() + visible.min_y();
    }
    if p.window.h >= visible.h {
        rect.h = visible.h;
        rect.y = visible.min_y();
    }
    RectResult::new(rect)
}

/// `MoveUpDownCalculation.calculateRect`.
pub(super) fn move_up_down_rect(p: &RectParams) -> RectResult {
    let visible = p.visible;
    let calculation = Move { horizontal: false };
    let mut rect = if p.config.resize_on_directional_move {
        repeated_rect_in_thirds(&calculation, p).rect
    } else {
        calculation.generic_rect(p, None).rect
    };
    if p.config.centered_directional_move != Some(false) {
        rect.x = ((visible.w - rect.w) / 2.0).round() + visible.min_x();
    }
    if p.window.w >= visible.w {
        rect.w = visible.w;
        rect.x = visible.min_x();
    }
    RectResult::new(rect)
}
