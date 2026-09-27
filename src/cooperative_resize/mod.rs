//! Согласованный ресайз соседних окон — порт `CooperativeCornerResize.swift` (чистая
//! геометрия) и свойств действий из `WindowActionCooperativeResize.swift`.
//!
//! Половина или угол меняет размер повторным нажатием (или встаёт первым нажатием рядом
//! с уже поставленными окнами) — соседи подстраиваются: окна за двигающимся краем
//! уступают место, окна по эту сторону растут и сжимаются вместе с окном в фокусе.
//! `plan` находит затронутые окна (та же рамка, та же полоса, сосед через край или
//! захват с допуском), сужает допустимый диапазон общего края минимальными размерами и
//! рабочей областью, округляет край до целого пикселя и раздаёт рамки. `correction_plan`
//! переделывает план, когда окна встали не туда (приложение не дало ужать окно).
//! Поиск окон и применение плана — `cooperative_resize_manager`.
//!
//! Координаты — Cocoa, числа — `CGFloat` (`f64`). Рамки читаются по правилам `CGRect`
//! (модуль `cg`), `min`/`max` — как у Swift, поэтому итог совпадает с оригиналом до бита
//! (оракул `tools/coop-oracle`). Не перенесены неиспользуемые в оригинале обёртки
//! `adjustments(...)` и `focusedFramePreservingOccupiedCell` и мёртвая
//! `isContainedInSameSideRegion`.

pub(crate) mod cg;

#[cfg(test)]
mod tests;

use std::collections::HashMap;

use crate::actions::Action;
use crate::config::{Config, CornerCycleExpansionAxis as Axis};
use crate::geometry::Rect;
use crate::window_manager::ExecutionSource;

use cg::{height, intersects, max_x, max_y, min_x, min_y, swift_max, swift_min, width};

/// Размер (`CGSize`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Size {
    pub width: f64,
    pub height: f64,
}

impl Size {
    pub fn new(width: f64, height: f64) -> Size {
        Size { width, height }
    }
}

impl From<(f64, f64)> for Size {
    fn from((width, height): (f64, f64)) -> Size {
        Size { width, height }
    }
}

/// Окно-кандидат в соседи (`Candidate`): номер, рамка (Cocoa) и минимальный размер,
/// если приложение его сообщает.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Candidate {
    pub id: u32,
    pub frame: Rect,
    pub minimum_size: Option<Size>,
}

/// Как затронуто окно (`Adjustment.Kind`): растёт вместе с окном в фокусе или уступает
/// ему место за общим краем.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdjustmentKind {
    MatchingFocusedFrame,
    Adjacent,
}

/// Новая рамка соседа (`Adjustment`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Adjustment {
    pub id: u32,
    pub old_frame: Rect,
    pub new_frame: Rect,
    pub kind: AdjustmentKind,
}

/// План (`Plan`): рамка окна в фокусе, рамки соседей и журнал решений.
#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    pub focused_frame: Rect,
    pub adjustments: Vec<Adjustment>,
    pub debug_log: Vec<String>,
}

/// Край окна в фокусе, который двигается (`MovedEdge`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MovedEdge {
    Left,
    Right,
    Top,
    Bottom,
}

/// Параметры плана (`plan(...)`). Необязательные — как у Swift: `None` — значение по
/// умолчанию.
#[derive(Clone, Copy, Debug)]
pub struct PlanParams<'a> {
    pub old_focused_frame: Rect,
    pub new_focused_frame: Rect,
    /// Рабочая область экрана.
    pub screen_frame: Rect,
    pub candidates: &'a [Candidate],
    pub axis: Axis,
    /// Допуск, с которым края считаются общими (`detection_tolerance`).
    pub tolerance: f64,
    /// Минимальный размер соседа, который его не сообщает.
    pub minimum_size: Size,
    /// Минимальный размер окна в фокусе; нет — `minimum_size`.
    pub focused_minimum_size: Option<Size>,
    pub gap_size: f64,
    /// Допуск захвата окон, чей край не совпал с краем окна в фокусе (`capture_tolerance`);
    /// нет — захвата с допуском нет, только `tolerance`.
    pub capture_tolerance: Option<f64>,
    /// Какой край двигается; нет — по разнице рамок.
    pub moved_edge_override: Option<MovedEdge>,
    /// Рамка, по которой ищутся соседи; нет — `old_focused_frame`.
    pub candidate_discovery_frame: Option<Rect>,
    /// Для журнала: что за проход.
    pub action_description: &'a str,
}

/// Параметры переделки плана по фактическим рамкам (`correctionPlan(...)`).
#[derive(Clone, Copy, Debug)]
pub struct CorrectionParams<'a> {
    /// Параметры исходного плана: `new_focused_frame` — запрошенная рамка окна в фокусе.
    pub request: PlanParams<'a>,
    /// План, который применяли.
    pub planned: &'a Plan,
    /// Где окно в фокусе оказалось (`None` — рамка не читается, `CGRect.null`).
    pub actual_focused_frame: Option<Rect>,
    /// Где оказались соседи из плана (`None` у значения — рамка не читается).
    pub actual_candidate_frames: &'a HashMap<u32, Option<Rect>>,
    /// Насколько фактическая рамка может отличаться от плана (`layoutTolerance`).
    pub layout_tolerance: f64,
}

/// Роль затронутого окна (`AffectedRole`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AffectedRole {
    MatchingFocusedFrame,
    MatchingMovingSpan,
    Adjacent,
}

impl AffectedRole {
    /// Для журнала.
    fn title(self) -> &'static str {
        match self {
            AffectedRole::MatchingFocusedFrame => "та же рамка",
            AffectedRole::MatchingMovingSpan => "та же полоса",
            AffectedRole::Adjacent => "сосед",
        }
    }
}

/// Затронутое окно (`AffectedWindow`).
#[derive(Clone, Debug)]
struct AffectedWindow {
    candidate: Candidate,
    /// Рамка, от которой считается новая: у соседей по полосе и через край — выровненная
    /// по окну в фокусе и краям экрана.
    layout_frame: Rect,
    role: AffectedRole,
    kind: AdjustmentKind,
    minimum_size: Size,
    inclusion_reason: String,
}

/// Затронутые окна и журнал отбора (`AffectedWindowsResult`).
struct AffectedWindows {
    windows: Vec<AffectedWindow>,
    debug_log: Vec<String>,
}

/// Допустимый диапазон общего края и почему он такой (`EdgeRange`).
struct EdgeRange {
    min: f64,
    max: f64,
    lower_reasons: Vec<String>,
    upper_reasons: Vec<String>,
}

impl EdgeRange {
    fn new(min: f64, max: f64) -> EdgeRange {
        EdgeRange {
            min,
            max,
            lower_reasons: Vec::new(),
            upper_reasons: Vec::new(),
        }
    }

    fn require_min(&mut self, value: f64, reason: String) {
        if value > self.min + 0.001 {
            self.min = value;
            self.lower_reasons = vec![reason];
        } else if (value - self.min).abs() <= 0.001 {
            self.lower_reasons.push(reason);
        }
    }

    fn require_max(&mut self, value: f64, reason: String) {
        if value < self.max - 0.001 {
            self.max = value;
            self.upper_reasons = vec![reason];
        } else if (value - self.max).abs() <= 0.001 {
            self.upper_reasons.push(reason);
        }
    }
}

/// Граница ячейки, которую уже занимает соседнее окно (`ObservedCornerBoundary`).
#[derive(Clone, Copy, Debug)]
struct ObservedCornerBoundary {
    edge: f64,
    priority: i32,
    distance_from_requested_edge: f64,
    id: u32,
}

// ---------------------------------------------------------------- допуски

/// Допуск, с которым края окон считаются общими (`detectionTolerance`): 1,5% меньшей
/// стороны рабочей области, от 8 до 24 пикселей. Гэп оригинал не учитывает.
pub fn detection_tolerance(screen_frame: &Rect, _configured_gap: f64) -> f64 {
    let proportional_tolerance = swift_min(width(screen_frame), height(screen_frame)) * 0.015;
    swift_min(24.0, swift_max(8.0, proportional_tolerance))
}

/// Допуск захвата (`captureTolerance`): 8% стороны рабочей области по оси, от 8 до 96.
pub fn capture_tolerance(screen_frame: &Rect, axis: Axis) -> f64 {
    let dimension = match axis {
        Axis::Horizontal => width(screen_frame),
        Axis::Vertical => height(screen_frame),
    };
    swift_min(96.0, swift_max(8.0, dimension * 0.08))
}

/// Рамки различаются больше чем на допуск (`framesDiffer`).
pub fn frames_differ(lhs: &Rect, rhs: &Rect, tolerance: f64) -> bool {
    (min_x(lhs) - min_x(rhs)).abs() > tolerance
        || (min_y(lhs) - min_y(rhs)).abs() > tolerance
        || (width(lhs) - width(rhs)).abs() > tolerance
        || (height(lhs) - height(rhs)).abs() > tolerance
}

/// Окно надо ставить в посчитанную рамку (`frameNeedsApplication`): оно не там или
/// вылезло за рабочую область. Рамку не прочитать (`None`) — не надо.
pub fn frame_needs_application(
    current_frame: Option<&Rect>,
    solved_frame: &Rect,
    screen_frame: &Rect,
    layout_tolerance: f64,
) -> bool {
    frame_needs_correction(solved_frame, current_frame, screen_frame, layout_tolerance)
}

/// Окно в фокусе растёт по оси (`focusedWindowIsExpanding`).
pub fn focused_window_is_expanding(old_frame: &Rect, new_frame: &Rect, axis: Axis) -> bool {
    match axis {
        Axis::Horizontal => width(new_frame) > width(old_frame),
        Axis::Vertical => height(new_frame) > height(old_frame),
    }
}

/// Какой край сдвинулся (`movedEdge(from:to:axis:tolerance:)`): ровно один из двух по оси.
pub fn moved_edge(
    old_focused_frame: &Rect,
    new_focused_frame: &Rect,
    axis: Axis,
    tolerance: f64,
) -> Option<MovedEdge> {
    match axis {
        Axis::Horizontal => {
            let left_moved =
                (min_x(old_focused_frame) - min_x(new_focused_frame)).abs() > tolerance;
            let right_moved =
                (max_x(old_focused_frame) - max_x(new_focused_frame)).abs() > tolerance;
            if left_moved == right_moved {
                return None;
            }
            Some(if left_moved {
                MovedEdge::Left
            } else {
                MovedEdge::Right
            })
        }
        Axis::Vertical => {
            let bottom_moved =
                (min_y(old_focused_frame) - min_y(new_focused_frame)).abs() > tolerance;
            let top_moved = (max_y(old_focused_frame) - max_y(new_focused_frame)).abs() > tolerance;
            if bottom_moved == top_moved {
                return None;
            }
            Some(if bottom_moved {
                MovedEdge::Bottom
            } else {
                MovedEdge::Top
            })
        }
    }
}

// ---------------------------------------------------------------- первое нажатие угла

/// Угол встаёт в ячейку, которую уже делят соседи (`focusedFrameResolvingRealizedCornerBoundary`):
/// если в той же полосе стоит окно, чей край (или край соседа через гэп) отличается от
/// запрошенного больше допуска, угол берёт этот край.
pub fn focused_frame_resolving_realized_corner_boundary(
    requested_focused_frame: &Rect,
    screen_frame: &Rect,
    candidates: &[Candidate],
    axis: Axis,
    moved_edge: MovedEdge,
    tolerance: f64,
    gap_size: f64,
) -> Rect {
    let desired_edge = edge_coordinate(requested_focused_frame, moved_edge);
    let Some(observed_boundary) = observed_corner_boundary(
        requested_focused_frame,
        screen_frame,
        candidates,
        axis,
        moved_edge,
        desired_edge,
        tolerance,
        gap_size,
    )
    .filter(|boundary| (boundary.edge - desired_edge).abs() > tolerance) else {
        return *requested_focused_frame;
    };

    rounded_frame_inside_visible_frame(
        &same_side_frame(
            requested_focused_frame,
            moved_edge,
            axis,
            observed_boundary.edge,
            requested_focused_frame,
            screen_frame,
        ),
        screen_frame,
    )
}

#[allow(clippy::too_many_arguments)]
fn observed_corner_boundary(
    requested_focused_frame: &Rect,
    screen_frame: &Rect,
    candidates: &[Candidate],
    axis: Axis,
    moved_edge: MovedEdge,
    desired_edge: f64,
    tolerance: f64,
    gap_size: f64,
) -> Option<ObservedCornerBoundary> {
    let observed_boundaries = candidates.iter().filter_map(|candidate| {
        let frame = &candidate.frame;
        if !intersects(screen_frame, frame)
            || !matches_perpendicular_span(frame, requested_focused_frame, moved_edge, tolerance)
        {
            return None;
        }

        if shares_same_side_fixed_edge(frame, requested_focused_frame, moved_edge, axis, tolerance)
        {
            let edge = edge_coordinate(frame, moved_edge);
            if !is_valid_observed_boundary(
                edge,
                requested_focused_frame,
                screen_frame,
                moved_edge,
                axis,
                tolerance,
            ) {
                return None;
            }
            return Some(ObservedCornerBoundary {
                edge,
                priority: 0,
                distance_from_requested_edge: (edge - desired_edge).abs(),
                id: candidate.id,
            });
        }

        let adjacent_edge = observed_boundary_from_adjacent_frame(
            frame,
            screen_frame,
            moved_edge,
            axis,
            tolerance,
            gap_size,
        )?;
        if !is_valid_observed_boundary(
            adjacent_edge,
            requested_focused_frame,
            screen_frame,
            moved_edge,
            axis,
            tolerance,
        ) {
            return None;
        }
        Some(ObservedCornerBoundary {
            edge: adjacent_edge,
            priority: 1,
            distance_from_requested_edge: (adjacent_edge - desired_edge).abs(),
            id: candidate.id,
        })
    });

    // Сравнение — как в оригинале: «больше» — меньший приоритет, дальше от запрошенного
    // края, меньший номер окна.
    cg::swift_max_by(observed_boundaries, |lhs, rhs| {
        if lhs.priority != rhs.priority {
            return lhs.priority > rhs.priority;
        }
        if (lhs.distance_from_requested_edge - rhs.distance_from_requested_edge).abs() > 0.001 {
            return lhs.distance_from_requested_edge < rhs.distance_from_requested_edge;
        }
        lhs.id > rhs.id
    })
}

fn shares_same_side_fixed_edge(
    candidate: &Rect,
    focused: &Rect,
    moved_edge: MovedEdge,
    axis: Axis,
    tolerance: f64,
) -> bool {
    match moved_edge {
        MovedEdge::Right | MovedEdge::Top => {
            (axis_min(candidate, axis) - axis_min(focused, axis)).abs() <= tolerance
        }
        MovedEdge::Left | MovedEdge::Bottom => {
            (axis_max(candidate, axis) - axis_max(focused, axis)).abs() <= tolerance
        }
    }
}

fn observed_boundary_from_adjacent_frame(
    candidate: &Rect,
    screen_frame: &Rect,
    moved_edge: MovedEdge,
    axis: Axis,
    tolerance: f64,
    gap_size: f64,
) -> Option<f64> {
    match moved_edge {
        MovedEdge::Right | MovedEdge::Top => {
            if !matches_outer_max(
                axis_max(candidate, axis),
                screen_frame,
                axis,
                tolerance,
                gap_size,
            ) {
                return None;
            }
            Some(axis_min(candidate, axis) - gap_size)
        }
        MovedEdge::Left | MovedEdge::Bottom => {
            if !matches_outer_min(
                axis_min(candidate, axis),
                screen_frame,
                axis,
                tolerance,
                gap_size,
            ) {
                return None;
            }
            Some(axis_max(candidate, axis) + gap_size)
        }
    }
}

fn is_valid_observed_boundary(
    edge: f64,
    requested_focused_frame: &Rect,
    screen_frame: &Rect,
    moved_edge: MovedEdge,
    axis: Axis,
    tolerance: f64,
) -> bool {
    let visible_min = axis_min(screen_frame, axis);
    let visible_max = axis_max(screen_frame, axis);
    if !(edge > visible_min + tolerance && edge < visible_max - tolerance) {
        return false;
    }

    match moved_edge {
        MovedEdge::Right | MovedEdge::Top => {
            edge > axis_min(requested_focused_frame, axis) + tolerance
        }
        MovedEdge::Left | MovedEdge::Bottom => {
            edge < axis_max(requested_focused_frame, axis) - tolerance
        }
    }
}

// ---------------------------------------------------------------- план

/// План согласованного ресайза (`plan(...)`). `None` — край не определить или
/// затронутых окон нет.
pub fn plan(p: &PlanParams) -> Option<Plan> {
    let moved_edge = match p.moved_edge_override {
        Some(edge) => edge,
        None => moved_edge(
            &p.old_focused_frame,
            &p.new_focused_frame,
            p.axis,
            p.tolerance,
        )?,
    };
    let axis = p.axis;
    let screen_frame = &p.screen_frame;
    let new_focused_frame = &p.new_focused_frame;
    let gap_size = p.gap_size;

    let fallback_minimum_size = normalized_minimum_size(p.minimum_size);
    let resolved_focused_minimum_size =
        normalized_minimum_size(p.focused_minimum_size.unwrap_or(fallback_minimum_size));
    let resolved_capture_tolerance = p.capture_tolerance.unwrap_or(p.tolerance);
    let resolved_perpendicular_capture_tolerance = match p.capture_tolerance {
        None => p.tolerance,
        Some(_) => capture_tolerance(screen_frame, perpendicular_axis(axis)),
    };
    let discovery_frame = p.candidate_discovery_frame.unwrap_or(p.old_focused_frame);
    let affected = affected_windows(
        &discovery_frame,
        new_focused_frame,
        screen_frame,
        p.candidates,
        moved_edge,
        p.tolerance,
        resolved_capture_tolerance,
        resolved_perpendicular_capture_tolerance,
        fallback_minimum_size,
        gap_size,
    );
    if affected.windows.is_empty() {
        return None;
    }

    let old_edge = edge_coordinate(&p.old_focused_frame, moved_edge);
    let desired_edge = edge_coordinate(new_focused_frame, moved_edge);
    let requested_delta = desired_edge - old_edge;
    let included: Vec<String> = affected
        .windows
        .iter()
        .map(|window| format!("{} ({})", window.candidate.id, window.inclusion_reason))
        .collect();
    let mut debug_log = vec![
        format!("Согласованный ресайз, проход: {}", p.action_description),
        format!(
            "Согласованный ресайз, рабочая область: {}",
            describe(screen_frame)
        ),
        format!("Согласованный ресайз, гэп: {gap_size}"),
        format!("Согласованный ресайз, допуск общих краёв: {}", p.tolerance),
        format!(
            "Согласованный ресайз, допуск захвата: по оси {resolved_capture_tolerance}, поперёк {resolved_perpendicular_capture_tolerance}"
        ),
        format!("Согласованный ресайз, запрошенный сдвиг края: {requested_delta}"),
        format!(
            "Согласованный ресайз, затронутые окна: {}",
            included.join(", ")
        ),
    ];
    debug_log.extend(affected.debug_log);

    let mut edge_range = EdgeRange::new(axis_min(screen_frame, axis), axis_max(screen_frame, axis));
    constrain_same_side_window(
        &mut edge_range,
        "окно в фокусе",
        moved_edge,
        axis,
        new_focused_frame,
        screen_frame,
        resolved_focused_minimum_size,
    );
    for affected_window in &affected.windows {
        constrain_affected_window(
            &mut edge_range,
            affected_window,
            moved_edge,
            axis,
            new_focused_frame,
            screen_frame,
            gap_size,
        );
    }

    let clamped_edge = rounded_edge(
        clamp(desired_edge, edge_range.min, edge_range.max),
        edge_range.min,
        edge_range.max,
    );
    let focused_frame = rounded_frame_inside_visible_frame(
        &frame_with_minimum_size(
            &same_side_frame(
                new_focused_frame,
                moved_edge,
                axis,
                clamped_edge,
                new_focused_frame,
                screen_frame,
            ),
            resolved_focused_minimum_size,
            axis,
            screen_frame,
        ),
        screen_frame,
    );
    let proposed_focused_frame = same_side_frame(
        new_focused_frame,
        moved_edge,
        axis,
        desired_edge,
        new_focused_frame,
        screen_frame,
    );

    debug_log.push(format!(
        "Согласованный ресайз, предложенная рамка окна в фокусе: {}",
        describe(&proposed_focused_frame)
    ));
    debug_log.push(format!(
        "Согласованный ресайз, рамка окна в фокусе после ограничений: {}",
        describe(&focused_frame)
    ));

    let adjustments = affected
        .windows
        .iter()
        .map(|affected_window| {
            let proposed_frame = frame_for_affected_window(
                affected_window,
                desired_edge,
                moved_edge,
                axis,
                new_focused_frame,
                screen_frame,
                gap_size,
            );
            let clamped_frame = rounded_frame_inside_visible_frame(
                &frame_for_affected_window(
                    affected_window,
                    clamped_edge,
                    moved_edge,
                    axis,
                    new_focused_frame,
                    screen_frame,
                    gap_size,
                ),
                screen_frame,
            );
            let id = affected_window.candidate.id;
            debug_log.push(format!(
                "Согласованный ресайз, предложенная рамка окна {id}: {}",
                describe(&proposed_frame)
            ));
            debug_log.push(format!(
                "Согласованный ресайз, рамка окна {id} после ограничений: {}",
                describe(&clamped_frame)
            ));
            Adjustment {
                id,
                old_frame: affected_window.candidate.frame,
                new_frame: clamped_frame,
                kind: affected_window.kind,
            }
        })
        .collect();

    let applied_delta = clamped_edge - old_edge;
    if (applied_delta - requested_delta).abs() > 0.001 {
        let reasons = reduction_reasons(desired_edge, &edge_range);
        debug_log.push(format!(
            "Согласованный ресайз, сдвиг края урезан с {requested_delta} до {applied_delta}: {}",
            reasons.join("; ")
        ));
    }
    if edge_range.min > edge_range.max {
        debug_log.push(format!(
            "Согласованный ресайз, ограничения несовместны: допустимый диапазон края {}…{}",
            edge_range.min, edge_range.max
        ));
    }

    Some(Plan {
        focused_frame,
        adjustments,
        debug_log,
    })
}

/// Переделать план по тому, где окна встали на самом деле (`correctionPlan(...)`):
/// окно, которое не ужалось до плана, получает минимальный размер по факту. `None` —
/// всё встало как задумано (или новый план не сложился).
pub fn correction_plan(c: &CorrectionParams) -> Option<Plan> {
    let request = &c.request;
    let screen_frame = &request.screen_frame;
    let planned_adjustments_by_id: HashMap<u32, &Adjustment> = c
        .planned
        .adjustments
        .iter()
        .map(|adjustment| (adjustment.id, adjustment))
        .collect();
    let focused_needs_correction = frame_needs_correction(
        &c.planned.focused_frame,
        c.actual_focused_frame.as_ref(),
        screen_frame,
        c.layout_tolerance,
    );
    let cooperating_needs_correction = c.planned.adjustments.iter().any(|adjustment| {
        match c.actual_candidate_frames.get(&adjustment.id) {
            None => false,
            Some(actual_frame) => frame_needs_correction(
                &adjustment.new_frame,
                actual_frame.as_ref(),
                screen_frame,
                c.layout_tolerance,
            ),
        }
    });

    if !(focused_needs_correction || cooperating_needs_correction) {
        return None;
    }

    let fallback_minimum_size = normalized_minimum_size(request.minimum_size);
    let effective_focused_minimum_size = effective_minimum_size(
        request
            .focused_minimum_size
            .unwrap_or(fallback_minimum_size),
        &c.planned.focused_frame,
        c.actual_focused_frame.as_ref(),
        c.layout_tolerance,
    );
    let effective_candidates: Vec<Candidate> = request
        .candidates
        .iter()
        .map(|candidate| {
            let (Some(planned_adjustment), Some(actual_frame)) = (
                planned_adjustments_by_id.get(&candidate.id),
                c.actual_candidate_frames.get(&candidate.id),
            ) else {
                return *candidate;
            };
            let base_minimum_size = candidate.minimum_size.unwrap_or(fallback_minimum_size);
            let effective_minimum = effective_minimum_size(
                base_minimum_size,
                &planned_adjustment.new_frame,
                actual_frame.as_ref(),
                c.layout_tolerance,
            );
            Candidate {
                minimum_size: Some(effective_minimum),
                ..*candidate
            }
        })
        .collect();

    let corrected_plan = plan(&PlanParams {
        candidates: &effective_candidates,
        focused_minimum_size: Some(effective_focused_minimum_size),
        ..*request
    })?;

    let mut debug_log =
        vec!["Согласованный ресайз, повторный проход: окна встали не так, как в плане".to_string()];
    debug_log.extend(corrected_plan.debug_log);
    Some(Plan {
        focused_frame: corrected_plan.focused_frame,
        adjustments: corrected_plan.adjustments,
        debug_log,
    })
}

// ---------------------------------------------------------------- затронутые окна

#[allow(clippy::too_many_arguments)]
fn affected_windows(
    discovery_focused_frame: &Rect,
    new_focused_frame: &Rect,
    screen_frame: &Rect,
    candidates: &[Candidate],
    moved_edge: MovedEdge,
    tolerance: f64,
    capture_tolerance: f64,
    perpendicular_capture_tolerance: f64,
    fallback_minimum_size: Size,
    gap_size: f64,
) -> AffectedWindows {
    let mut matching_focused_frame = Vec::new();
    let mut matching_moving_span = Vec::new();
    let mut adjacent = Vec::new();
    let mut debug_log = Vec::new();

    for candidate in candidates {
        // Рамка у кандидата всегда есть: окна без рамки отсеивает поиск окон.
        if !intersects(screen_frame, &candidate.frame) {
            debug_log.push(format!(
                "Согласованный ресайз, окно {} не берём: вне рабочей области",
                candidate.id
            ));
            continue;
        }

        let minimum_size =
            normalized_minimum_size(candidate.minimum_size.unwrap_or(fallback_minimum_size));
        let strict_layout_frame = normalized_full_span_frame(
            &candidate.frame,
            discovery_focused_frame,
            moved_edge,
            tolerance,
        );
        let affected = |role, kind, reason: &str| AffectedWindow {
            candidate: *candidate,
            layout_frame: strict_layout_frame,
            role,
            kind,
            minimum_size,
            inclusion_reason: reason.to_string(),
        };

        if approximately_matches_frame(&candidate.frame, discovery_focused_frame, tolerance) {
            matching_focused_frame.push(affected(
                AffectedRole::MatchingFocusedFrame,
                AdjustmentKind::MatchingFocusedFrame,
                "та же рамка, что у окна в фокусе",
            ));
            debug_log.push(format!(
                "Согласованный ресайз, окно {} берём: та же рамка, что у окна в фокусе",
                candidate.id
            ));
            continue;
        }

        if matches_moving_span(
            &candidate.frame,
            discovery_focused_frame,
            moved_edge,
            tolerance,
        ) && is_supported_perpendicular_span(
            &candidate.frame,
            discovery_focused_frame,
            moved_edge,
            tolerance,
        ) {
            matching_moving_span.push(affected(
                AffectedRole::MatchingMovingSpan,
                AdjustmentKind::MatchingFocusedFrame,
                "та же полоса по оси",
            ));
            debug_log.push(format!(
                "Согласованный ресайз, окно {} берём: та же полоса по оси",
                candidate.id
            ));
            continue;
        }

        if is_supported_perpendicular_span(
            &candidate.frame,
            discovery_focused_frame,
            moved_edge,
            tolerance,
        ) && touches_old_moving_edge(
            &candidate.frame,
            discovery_focused_frame,
            moved_edge,
            tolerance,
            gap_size,
        ) {
            adjacent.push(affected(
                AffectedRole::Adjacent,
                AdjustmentKind::Adjacent,
                "общий край",
            ));
            debug_log.push(format!(
                "Согласованный ресайз, окно {} берём: общий край",
                candidate.id
            ));
            continue;
        }

        let Some(captured) = captured_window(
            candidate,
            discovery_focused_frame,
            new_focused_frame,
            screen_frame,
            moved_edge,
            capture_tolerance,
            perpendicular_capture_tolerance,
            gap_size,
            minimum_size,
        ) else {
            debug_log.push(format!(
                "Согласованный ресайз, окно {} не берём: нет общего края, пересечения границы и захвата с допуском",
                candidate.id
            ));
            continue;
        };

        debug_log.push(format!(
            "Согласованный ресайз, окно {} берём: {}",
            candidate.id, captured.inclusion_reason
        ));
        match captured.role {
            AffectedRole::MatchingFocusedFrame => matching_focused_frame.push(captured),
            AffectedRole::MatchingMovingSpan => matching_moving_span.push(captured),
            AffectedRole::Adjacent => adjacent.push(captured),
        }
    }

    let mut windows = matching_focused_frame;
    windows.extend(matching_moving_span);
    windows.extend(adjacent);
    AffectedWindows { windows, debug_log }
}

#[allow(clippy::too_many_arguments)]
fn captured_window(
    candidate: &Candidate,
    discovery_focused_frame: &Rect,
    new_focused_frame: &Rect,
    screen_frame: &Rect,
    moved_edge: MovedEdge,
    capture_tolerance: f64,
    perpendicular_capture_tolerance: f64,
    gap_size: f64,
    minimum_size: Size,
) -> Option<AffectedWindow> {
    let frame = &candidate.frame;
    let perpendicular_supported = is_supported_perpendicular_span(
        frame,
        discovery_focused_frame,
        moved_edge,
        perpendicular_capture_tolerance,
    );
    let perpendicular_overlaps =
        perpendicular_overlap(frame, discovery_focused_frame, moved_edge) > 0.0;

    let touches_discovery_edge = touches_old_moving_edge(
        frame,
        discovery_focused_frame,
        moved_edge,
        capture_tolerance,
        gap_size,
    );
    let touches_target_edge = touches_old_moving_edge(
        frame,
        new_focused_frame,
        moved_edge,
        capture_tolerance,
        gap_size,
    );
    let crosses_discovery_boundary = crosses_shared_boundary(
        frame,
        edge_coordinate(discovery_focused_frame, moved_edge),
        moved_edge,
        gap_size,
    );
    let crosses_target_boundary = crosses_shared_boundary(
        frame,
        edge_coordinate(new_focused_frame, moved_edge),
        moved_edge,
        gap_size,
    );
    let overlaps_target_boundary = overlaps_boundary_band(
        frame,
        edge_coordinate(new_focused_frame, moved_edge),
        axis_for(moved_edge),
        capture_tolerance,
    );

    let edge_capture = perpendicular_supported
        && (touches_discovery_edge
            || touches_target_edge
            || (intersects(new_focused_frame, frame) && overlaps_target_boundary));
    let boundary_capture =
        perpendicular_overlaps && (crosses_discovery_boundary || crosses_target_boundary);

    if !(edge_capture || boundary_capture) {
        return None;
    }

    let assignment = nearest_role(frame, new_focused_frame, screen_frame, moved_edge, gap_size);
    let layout_frame = normalized_captured_frame(
        frame,
        discovery_focused_frame,
        screen_frame,
        moved_edge,
        assignment,
        perpendicular_capture_tolerance,
        gap_size,
    );
    let nearest_region = format!("ближайшая область: {}", assignment.title());
    let reason_parts: Vec<&str> = [
        touches_discovery_edge.then_some("общий край с допуском захвата"),
        touches_target_edge.then_some("край цели с допуском захвата"),
        (crosses_discovery_boundary || crosses_target_boundary).then_some("пересекает границу"),
        overlaps_target_boundary.then_some("накрывает границу цели"),
        Some(nearest_region.as_str()),
    ]
    .into_iter()
    .flatten()
    .collect();
    let kind = if assignment == AffectedRole::Adjacent {
        AdjustmentKind::Adjacent
    } else {
        AdjustmentKind::MatchingFocusedFrame
    };

    Some(AffectedWindow {
        candidate: *candidate,
        layout_frame,
        role: assignment,
        kind,
        minimum_size,
        inclusion_reason: reason_parts.join(", "),
    })
}

/// Куда ближе окно, захваченное с допуском (`nearestRole`): к полосе окна в фокусе или
/// к области за двигающимся краем.
fn nearest_role(
    candidate: &Rect,
    focused_frame: &Rect,
    screen_frame: &Rect,
    moved_edge: MovedEdge,
    gap_size: f64,
) -> AffectedRole {
    let axis = axis_for(moved_edge);
    let boundary = edge_coordinate(focused_frame, moved_edge);
    let (same_interval, adjacent_interval) = match moved_edge {
        MovedEdge::Right | MovedEdge::Top => (
            (axis_min(focused_frame, axis), boundary),
            (boundary + gap_size, axis_max(screen_frame, axis)),
        ),
        MovedEdge::Left | MovedEdge::Bottom => (
            (boundary, axis_max(focused_frame, axis)),
            (axis_min(screen_frame, axis), boundary - gap_size),
        ),
    };

    let candidate_interval = (axis_min(candidate, axis), axis_max(candidate, axis));
    let same_overlap = interval_overlap(candidate_interval, same_interval);
    let adjacent_overlap = interval_overlap(candidate_interval, adjacent_interval);

    if (adjacent_overlap - same_overlap).abs() > 0.001 {
        return if adjacent_overlap > same_overlap {
            AffectedRole::Adjacent
        } else {
            AffectedRole::MatchingMovingSpan
        };
    }

    let center = (candidate_interval.0 + candidate_interval.1) / 2.0;
    let adjacent = match moved_edge {
        MovedEdge::Right | MovedEdge::Top => center >= boundary,
        MovedEdge::Left | MovedEdge::Bottom => center <= boundary,
    };
    if adjacent {
        AffectedRole::Adjacent
    } else {
        AffectedRole::MatchingMovingSpan
    }
}

fn crosses_shared_boundary(
    candidate: &Rect,
    boundary: f64,
    moved_edge: MovedEdge,
    gap_size: f64,
) -> bool {
    let axis = axis_for(moved_edge);
    let candidate_min = axis_min(candidate, axis);
    let candidate_max = axis_max(candidate, axis);

    match moved_edge {
        MovedEdge::Right | MovedEdge::Top => {
            candidate_min < boundary + gap_size && candidate_max > boundary
        }
        MovedEdge::Left | MovedEdge::Bottom => {
            candidate_min < boundary && candidate_max > boundary - gap_size
        }
    }
}

fn overlaps_boundary_band(candidate: &Rect, boundary: f64, axis: Axis, tolerance: f64) -> bool {
    axis_min(candidate, axis) <= boundary + tolerance
        && axis_max(candidate, axis) >= boundary - tolerance
}

fn perpendicular_overlap(candidate: &Rect, focused: &Rect, moved_edge: MovedEdge) -> f64 {
    match moved_edge {
        MovedEdge::Left | MovedEdge::Right => interval_overlap(
            (min_y(candidate), max_y(candidate)),
            (min_y(focused), max_y(focused)),
        ),
        MovedEdge::Top | MovedEdge::Bottom => interval_overlap(
            (min_x(candidate), max_x(candidate)),
            (min_x(focused), max_x(focused)),
        ),
    }
}

fn interval_overlap(lhs: (f64, f64), rhs: (f64, f64)) -> f64 {
    swift_max(0.0, swift_min(lhs.1, rhs.1) - swift_max(lhs.0, rhs.0))
}

/// Рамка захваченного окна, от которой считается новая (`normalizedCapturedFrame`): у
/// соседа через край дальний край прижат к краю экрана (или к гэпу у него).
fn normalized_captured_frame(
    candidate: &Rect,
    focused_frame: &Rect,
    screen_frame: &Rect,
    moved_edge: MovedEdge,
    role: AffectedRole,
    tolerance: f64,
    gap_size: f64,
) -> Rect {
    let mut result = normalized_full_span_frame(candidate, focused_frame, moved_edge, tolerance);
    if role != AffectedRole::Adjacent {
        return result;
    }

    match moved_edge {
        MovedEdge::Right => {
            result.w = outer_max(
                max_x(candidate),
                min_x(screen_frame),
                max_x(screen_frame),
                gap_size,
            ) - min_x(&result);
        }
        MovedEdge::Left => {
            let old_max_x = max_x(&result);
            result.x = outer_min(
                min_x(candidate),
                min_x(screen_frame),
                max_x(screen_frame),
                gap_size,
            );
            result.w = old_max_x - min_x(&result);
        }
        MovedEdge::Top => {
            result.h = outer_max(
                max_y(candidate),
                min_y(screen_frame),
                max_y(screen_frame),
                gap_size,
            ) - min_y(&result);
        }
        MovedEdge::Bottom => {
            let old_max_y = max_y(&result);
            result.y = outer_min(
                min_y(candidate),
                min_y(screen_frame),
                max_y(screen_frame),
                gap_size,
            );
            result.h = old_max_y - min_y(&result);
        }
    }
    result
}

/// Край экрана, к которому ближе ближний край окна: сам край или край с гэпом.
fn outer_min(candidate_min: f64, screen_min: f64, screen_max: f64, gap_size: f64) -> f64 {
    let resolved_gap_size = swift_max(0.0, swift_min(gap_size, (screen_max - screen_min) / 2.0));
    if resolved_gap_size > 0.0 {
        let gap_min = screen_min + resolved_gap_size;
        if (candidate_min - screen_min).abs() < (candidate_min - gap_min).abs() {
            screen_min
        } else {
            gap_min
        }
    } else {
        screen_min
    }
}

/// Край экрана, к которому ближе дальний край окна: сам край или край с гэпом.
fn outer_max(candidate_max: f64, screen_min: f64, screen_max: f64, gap_size: f64) -> f64 {
    let resolved_gap_size = swift_max(0.0, swift_min(gap_size, (screen_max - screen_min) / 2.0));
    if resolved_gap_size > 0.0 {
        let gap_max = screen_max - resolved_gap_size;
        if (candidate_max - screen_max).abs() < (candidate_max - gap_max).abs() {
            screen_max
        } else {
            gap_max
        }
    } else {
        screen_max
    }
}

fn matches_outer_min(
    value: f64,
    screen_frame: &Rect,
    axis: Axis,
    tolerance: f64,
    gap_size: f64,
) -> bool {
    let screen_min = axis_min(screen_frame, axis);
    (value - screen_min).abs() <= tolerance || (value - (screen_min + gap_size)).abs() <= tolerance
}

fn matches_outer_max(
    value: f64,
    screen_frame: &Rect,
    axis: Axis,
    tolerance: f64,
    gap_size: f64,
) -> bool {
    let screen_max = axis_max(screen_frame, axis);
    (value - screen_max).abs() <= tolerance || (value - (screen_max - gap_size)).abs() <= tolerance
}

// ---------------------------------------------------------------- ограничения края

fn constrain_affected_window(
    edge_range: &mut EdgeRange,
    affected_window: &AffectedWindow,
    moved_edge: MovedEdge,
    axis: Axis,
    new_focused_frame: &Rect,
    screen_frame: &Rect,
    gap_size: f64,
) {
    let label = format!("окно {}", affected_window.candidate.id);
    match affected_window.role {
        AffectedRole::MatchingFocusedFrame | AffectedRole::MatchingMovingSpan => {
            constrain_same_side_window(
                edge_range,
                &label,
                moved_edge,
                axis,
                new_focused_frame,
                screen_frame,
                affected_window.minimum_size,
            )
        }
        AffectedRole::Adjacent => constrain_adjacent_window(
            edge_range,
            &label,
            moved_edge,
            axis,
            &affected_window.layout_frame,
            screen_frame,
            affected_window.minimum_size,
            gap_size,
        ),
    }
}

/// Окно по эту сторону края не меньше своего минимума (`constrainSameSideWindow`; у окна в
/// фокусе — `constrainFocusedWindow`).
fn constrain_same_side_window(
    edge_range: &mut EdgeRange,
    label: &str,
    moved_edge: MovedEdge,
    axis: Axis,
    new_focused_frame: &Rect,
    screen_frame: &Rect,
    minimum_size: Size,
) {
    let minimum_axis_size = axis_size(minimum_size, axis);
    match moved_edge {
        MovedEdge::Right | MovedEdge::Top => {
            let fixed_min = clamp(
                axis_min(new_focused_frame, axis),
                axis_min(screen_frame, axis),
                axis_max(screen_frame, axis),
            );
            edge_range.require_min(
                fixed_min + minimum_axis_size,
                format!("{label}: минимальная {}", axis_size_name(axis)),
            );
        }
        MovedEdge::Left | MovedEdge::Bottom => {
            let fixed_max = clamp(
                axis_max(new_focused_frame, axis),
                axis_min(screen_frame, axis),
                axis_max(screen_frame, axis),
            );
            edge_range.require_max(
                fixed_max - minimum_axis_size,
                format!("{label}: минимальная {}", axis_size_name(axis)),
            );
        }
    }
}

/// Сосед за краем не меньше своего минимума и не вылезает за рабочую область с гэпом
/// (`constrainAdjacentWindow`).
#[allow(clippy::too_many_arguments)]
fn constrain_adjacent_window(
    edge_range: &mut EdgeRange,
    label: &str,
    moved_edge: MovedEdge,
    axis: Axis,
    candidate_frame: &Rect,
    screen_frame: &Rect,
    minimum_size: Size,
    gap_size: f64,
) {
    let minimum_axis_size = axis_size(minimum_size, axis);
    match moved_edge {
        MovedEdge::Right | MovedEdge::Top => {
            let outer_max = swift_min(
                axis_max(candidate_frame, axis),
                axis_max(screen_frame, axis),
            );
            edge_range.require_max(
                outer_max - gap_size - minimum_axis_size,
                format!(
                    "{label}: минимальная {} в рабочей области с гэпом",
                    axis_size_name(axis)
                ),
            );
        }
        MovedEdge::Left | MovedEdge::Bottom => {
            let outer_min = swift_max(
                axis_min(candidate_frame, axis),
                axis_min(screen_frame, axis),
            );
            edge_range.require_min(
                outer_min + gap_size + minimum_axis_size,
                format!(
                    "{label}: минимальная {} в рабочей области с гэпом",
                    axis_size_name(axis)
                ),
            );
        }
    }
}

fn reduction_reasons(desired_edge: f64, edge_range: &EdgeRange) -> Vec<String> {
    if edge_range.min > edge_range.max {
        let reasons: Vec<String> = edge_range
            .lower_reasons
            .iter()
            .chain(&edge_range.upper_reasons)
            .cloned()
            .collect();
        return if reasons.is_empty() {
            vec!["ни одно положение края не удовлетворяет всем ограничениям".to_string()]
        } else {
            reasons
        };
    }
    if desired_edge < edge_range.min {
        return if edge_range.lower_reasons.is_empty() {
            vec!["нижняя граница рабочей области".to_string()]
        } else {
            edge_range.lower_reasons.clone()
        };
    }
    if desired_edge > edge_range.max {
        return if edge_range.upper_reasons.is_empty() {
            vec!["верхняя граница рабочей области".to_string()]
        } else {
            edge_range.upper_reasons.clone()
        };
    }
    vec!["округление до целого пикселя".to_string()]
}

// ---------------------------------------------------------------- рамки

fn frame_for_affected_window(
    affected_window: &AffectedWindow,
    edge: f64,
    moved_edge: MovedEdge,
    axis: Axis,
    new_focused_frame: &Rect,
    screen_frame: &Rect,
    gap_size: f64,
) -> Rect {
    let frame = match affected_window.role {
        AffectedRole::MatchingFocusedFrame => same_side_frame(
            new_focused_frame,
            moved_edge,
            axis,
            edge,
            new_focused_frame,
            screen_frame,
        ),
        AffectedRole::MatchingMovingSpan => same_side_frame(
            &affected_window.layout_frame,
            moved_edge,
            axis,
            edge,
            new_focused_frame,
            screen_frame,
        ),
        AffectedRole::Adjacent => adjacent_frame(
            &affected_window.layout_frame,
            moved_edge,
            axis,
            edge,
            screen_frame,
            gap_size,
        ),
    };
    frame_with_minimum_size(&frame, affected_window.minimum_size, axis, screen_frame)
}

/// Рамка по эту сторону края: от неподвижного края окна в фокусе до `edge`.
fn same_side_frame(
    base_frame: &Rect,
    moved_edge: MovedEdge,
    axis: Axis,
    edge: f64,
    new_focused_frame: &Rect,
    screen_frame: &Rect,
) -> Rect {
    let visible_min = axis_min(screen_frame, axis);
    let visible_max = axis_max(screen_frame, axis);
    match moved_edge {
        MovedEdge::Right | MovedEdge::Top => {
            let fixed_min = clamp(axis_min(new_focused_frame, axis), visible_min, visible_max);
            frame(base_frame, axis, fixed_min, edge, screen_frame)
        }
        MovedEdge::Left | MovedEdge::Bottom => {
            let fixed_max = clamp(axis_max(new_focused_frame, axis), visible_min, visible_max);
            frame(base_frame, axis, edge, fixed_max, screen_frame)
        }
    }
}

/// Рамка за краем: от `edge` через гэп до дальнего края соседа (не дальше экрана).
fn adjacent_frame(
    base_frame: &Rect,
    moved_edge: MovedEdge,
    axis: Axis,
    edge: f64,
    screen_frame: &Rect,
    gap_size: f64,
) -> Rect {
    match moved_edge {
        MovedEdge::Right | MovedEdge::Top => {
            let outer_max = swift_min(axis_max(base_frame, axis), axis_max(screen_frame, axis));
            frame(base_frame, axis, edge + gap_size, outer_max, screen_frame)
        }
        MovedEdge::Left | MovedEdge::Bottom => {
            let outer_min = swift_max(axis_min(base_frame, axis), axis_min(screen_frame, axis));
            frame(base_frame, axis, outer_min, edge - gap_size, screen_frame)
        }
    }
}

/// `base_frame` с отрезком `min…max` по оси; поперёк — в пределах рабочей области.
fn frame(base_frame: &Rect, axis: Axis, min: f64, max: f64, visible_frame: &Rect) -> Rect {
    let mut rect = *base_frame;
    let ordered_min = swift_min(min, max);
    let ordered_max = swift_max(min, max);
    match axis {
        Axis::Horizontal => {
            let perp_min = swift_max(min_y(base_frame), min_y(visible_frame));
            let perp_max = swift_min(max_y(base_frame), max_y(visible_frame));
            rect.x = ordered_min;
            rect.w = ordered_max - ordered_min;
            rect.y = perp_min;
            rect.h = swift_max(0.0, perp_max - perp_min);
        }
        Axis::Vertical => {
            let perp_min = swift_max(min_x(base_frame), min_x(visible_frame));
            let perp_max = swift_min(max_x(base_frame), max_x(visible_frame));
            rect.x = perp_min;
            rect.w = swift_max(0.0, perp_max - perp_min);
            rect.y = ordered_min;
            rect.h = ordered_max - ordered_min;
        }
    }
    rect
}

/// Округлить до целых пикселей и обрезать по рабочей области
/// (`roundedFrameInsideVisibleFrame`).
fn rounded_frame_inside_visible_frame(frame: &Rect, visible_frame: &Rect) -> Rect {
    let mut rect = Rect::new(
        frame.x.round(),
        frame.y.round(),
        width(frame).round(),
        height(frame).round(),
    );

    if min_x(&rect) < min_x(visible_frame) {
        let delta = min_x(visible_frame) - min_x(&rect);
        rect.x += delta;
        rect.w = swift_max(0.0, width(&rect) - delta);
    }
    if min_y(&rect) < min_y(visible_frame) {
        let delta = min_y(visible_frame) - min_y(&rect);
        rect.y += delta;
        rect.h = swift_max(0.0, height(&rect) - delta);
    }
    if max_x(&rect) > max_x(visible_frame) {
        rect.w = swift_max(0.0, max_x(visible_frame) - min_x(&rect));
    }
    if max_y(&rect) > max_y(visible_frame) {
        rect.h = swift_max(0.0, max_y(visible_frame) - min_y(&rect));
    }

    rect
}

/// Поперёк оси ресайза — не меньше минимального размера (`frameWithMinimumSize`).
fn frame_with_minimum_size(
    frame: &Rect,
    minimum_size: Size,
    resize_axis: Axis,
    visible_frame: &Rect,
) -> Rect {
    let mut rect = *frame;
    match resize_axis {
        Axis::Horizontal => {
            if height(&rect) < minimum_size.height {
                rect.h = swift_min(minimum_size.height, height(visible_frame));
                if max_y(&rect) > max_y(visible_frame) {
                    rect.y = max_y(visible_frame) - height(&rect);
                }
                if min_y(&rect) < min_y(visible_frame) {
                    rect.y = min_y(visible_frame);
                }
            }
        }
        Axis::Vertical => {
            if width(&rect) < minimum_size.width {
                rect.w = swift_min(minimum_size.width, width(visible_frame));
                if max_x(&rect) > max_x(visible_frame) {
                    rect.x = max_x(visible_frame) - width(&rect);
                }
                if min_x(&rect) < min_x(visible_frame) {
                    rect.x = min_x(visible_frame);
                }
            }
        }
    }
    rect
}

/// Край — целый пиксель внутри допустимого диапазона, если в нём есть целые (`roundedEdge`).
fn rounded_edge(edge: f64, legal_min: f64, legal_max: f64) -> f64 {
    let rounded = edge.round();
    let integer_min = legal_min.ceil();
    let integer_max = legal_max.floor();
    if integer_min <= integer_max {
        clamp(rounded, integer_min, integer_max)
    } else {
        rounded
    }
}

// ---------------------------------------------------------------- переделка плана

/// `frameNeedsCorrection`: окно вылезло за рабочую область (с допуском) или стоит не там.
fn frame_needs_correction(
    planned_frame: &Rect,
    actual_frame: Option<&Rect>,
    screen_frame: &Rect,
    tolerance: f64,
) -> bool {
    let Some(actual_frame) = actual_frame else {
        return false;
    };
    let inside = cg::inset_by(screen_frame, -tolerance, -tolerance)
        .is_some_and(|bounds| cg::contains(&bounds, actual_frame));
    if !inside {
        return true;
    }
    (min_x(planned_frame) - min_x(actual_frame)).abs() > tolerance
        || (min_y(planned_frame) - min_y(actual_frame)).abs() > tolerance
        || (width(planned_frame) - width(actual_frame)).abs() > tolerance
        || (height(planned_frame) - height(actual_frame)).abs() > tolerance
}

/// Минимальный размер по факту (`effectiveMinimumSize`): окно осталось больше плана —
/// значит, меньше оно не может.
fn effective_minimum_size(
    base: Size,
    planned_frame: &Rect,
    actual_frame: Option<&Rect>,
    layout_tolerance: f64,
) -> Size {
    let normalized_base = normalized_minimum_size(base);
    let Some(actual_frame) = actual_frame else {
        return normalized_base;
    };

    let effective_width = if width(actual_frame) > width(planned_frame) + layout_tolerance {
        swift_max(normalized_base.width, width(actual_frame))
    } else {
        normalized_base.width
    };
    let effective_height = if height(actual_frame) > height(planned_frame) + layout_tolerance {
        swift_max(normalized_base.height, height(actual_frame))
    } else {
        normalized_base.height
    };
    Size::new(effective_width, effective_height)
}

// ---------------------------------------------------------------- мелочи

/// Координата края (`edgeCoordinate`).
fn edge_coordinate(frame: &Rect, edge: MovedEdge) -> f64 {
    match edge {
        MovedEdge::Left => min_x(frame),
        MovedEdge::Right => max_x(frame),
        MovedEdge::Top => max_y(frame),
        MovedEdge::Bottom => min_y(frame),
    }
}

fn axis_min(frame: &Rect, axis: Axis) -> f64 {
    match axis {
        Axis::Horizontal => min_x(frame),
        Axis::Vertical => min_y(frame),
    }
}

fn axis_max(frame: &Rect, axis: Axis) -> f64 {
    match axis {
        Axis::Horizontal => max_x(frame),
        Axis::Vertical => max_y(frame),
    }
}

fn axis_size(size: Size, axis: Axis) -> f64 {
    match axis {
        Axis::Horizontal => size.width,
        Axis::Vertical => size.height,
    }
}

fn axis_size_name(axis: Axis) -> &'static str {
    match axis {
        Axis::Horizontal => "ширина",
        Axis::Vertical => "высота",
    }
}

/// Ось, по которой двигается край (`axis(for:)`).
pub fn axis_for(moved_edge: MovedEdge) -> Axis {
    match moved_edge {
        MovedEdge::Left | MovedEdge::Right => Axis::Horizontal,
        MovedEdge::Top | MovedEdge::Bottom => Axis::Vertical,
    }
}

fn perpendicular_axis(axis: Axis) -> Axis {
    match axis {
        Axis::Horizontal => Axis::Vertical,
        Axis::Vertical => Axis::Horizontal,
    }
}

/// `clamp(_:min:max:)` оригинала: при `min > max` — ближайшая из границ.
fn clamp(value: f64, min: f64, max: f64) -> f64 {
    if min > max {
        return if value < min { min } else { max };
    }
    swift_min(swift_max(value, min), max)
}

/// Не меньше пикселя по каждой стороне (`normalizedMinimumSize`).
fn normalized_minimum_size(size: Size) -> Size {
    Size::new(swift_max(1.0, size.width), swift_max(1.0, size.height))
}

/// Поперёк оси окно почти совпадает с окном в фокусе — выровнять точно
/// (`normalizedFullSpanFrame`).
fn normalized_full_span_frame(
    candidate: &Rect,
    focused_frame: &Rect,
    moved_edge: MovedEdge,
    tolerance: f64,
) -> Rect {
    let mut result = *candidate;
    match moved_edge {
        MovedEdge::Left | MovedEdge::Right => {
            if (min_y(candidate) - min_y(focused_frame)).abs() <= tolerance
                && (max_y(candidate) - max_y(focused_frame)).abs() <= tolerance
            {
                result.y = min_y(focused_frame);
                result.h = height(focused_frame);
            }
        }
        MovedEdge::Top | MovedEdge::Bottom => {
            if (min_x(candidate) - min_x(focused_frame)).abs() <= tolerance
                && (max_x(candidate) - max_x(focused_frame)).abs() <= tolerance
            {
                result.x = min_x(focused_frame);
                result.w = width(focused_frame);
            }
        }
    }
    result
}

/// Окно стоит вплотную (через гэп) к двигающемуся краю (`touchesOldMovingEdge`).
fn touches_old_moving_edge(
    candidate: &Rect,
    focused: &Rect,
    moved_edge: MovedEdge,
    tolerance: f64,
    gap_size: f64,
) -> bool {
    match moved_edge {
        MovedEdge::Left => ((min_x(focused) - max_x(candidate)) - gap_size).abs() <= tolerance,
        MovedEdge::Right => ((min_x(candidate) - max_x(focused)) - gap_size).abs() <= tolerance,
        MovedEdge::Bottom => ((min_y(focused) - max_y(candidate)) - gap_size).abs() <= tolerance,
        MovedEdge::Top => ((min_y(candidate) - max_y(focused)) - gap_size).abs() <= tolerance,
    }
}

/// Поперёк оси окно занимает полосу окна в фокусе или её часть от одного из её краёв
/// (`isSupportedPerpendicularSpan`).
fn is_supported_perpendicular_span(
    candidate: &Rect,
    focused: &Rect,
    moved_edge: MovedEdge,
    tolerance: f64,
) -> bool {
    match moved_edge {
        MovedEdge::Left | MovedEdge::Right => approximately_matches_span(
            min_y(candidate),
            max_y(candidate),
            min_y(focused),
            max_y(focused),
            tolerance,
        ),
        MovedEdge::Top | MovedEdge::Bottom => approximately_matches_span(
            min_x(candidate),
            max_x(candidate),
            min_x(focused),
            max_x(focused),
            tolerance,
        ),
    }
}

/// Поперёк оси та же полоса (`matchesPerpendicularSpan`).
fn matches_perpendicular_span(
    candidate: &Rect,
    focused: &Rect,
    moved_edge: MovedEdge,
    tolerance: f64,
) -> bool {
    match moved_edge {
        MovedEdge::Left | MovedEdge::Right => {
            (min_y(candidate) - min_y(focused)).abs() <= tolerance
                && (max_y(candidate) - max_y(focused)).abs() <= tolerance
        }
        MovedEdge::Top | MovedEdge::Bottom => {
            (min_x(candidate) - min_x(focused)).abs() <= tolerance
                && (max_x(candidate) - max_x(focused)).abs() <= tolerance
        }
    }
}

/// По оси та же полоса (`matchesMovingSpan`).
fn matches_moving_span(
    candidate: &Rect,
    focused: &Rect,
    moved_edge: MovedEdge,
    tolerance: f64,
) -> bool {
    match moved_edge {
        MovedEdge::Left | MovedEdge::Right => {
            (min_x(candidate) - min_x(focused)).abs() <= tolerance
                && (max_x(candidate) - max_x(focused)).abs() <= tolerance
        }
        MovedEdge::Top | MovedEdge::Bottom => {
            (min_y(candidate) - min_y(focused)).abs() <= tolerance
                && (max_y(candidate) - max_y(focused)).abs() <= tolerance
        }
    }
}

fn approximately_matches_span(
    candidate_min: f64,
    candidate_max: f64,
    focused_min: f64,
    focused_max: f64,
    tolerance: f64,
) -> bool {
    let full_match = (candidate_min - focused_min).abs() <= tolerance
        && (candidate_max - focused_max).abs() <= tolerance;
    if full_match {
        return true;
    }

    let candidate_is_within_focused =
        candidate_min >= focused_min - tolerance && candidate_max <= focused_max + tolerance;
    let shares_focused_boundary = (candidate_min - focused_min).abs() <= tolerance
        || (candidate_max - focused_max).abs() <= tolerance;
    let meaningful_span = candidate_max - candidate_min > tolerance;

    candidate_is_within_focused && shares_focused_boundary && meaningful_span
}

fn approximately_matches_frame(candidate: &Rect, focused: &Rect, tolerance: f64) -> bool {
    (min_x(candidate) - min_x(focused)).abs() <= tolerance
        && (max_x(candidate) - max_x(focused)).abs() <= tolerance
        && (min_y(candidate) - min_y(focused)).abs() <= tolerance
        && (max_y(candidate) - max_y(focused)).abs() <= tolerance
}

/// Рамка для журнала: `(x, y, ширина, высота)`.
pub(crate) fn describe(rect: &Rect) -> String {
    format!("({}, {}, {}, {})", rect.x, rect.y, rect.w, rect.h)
}

// ---------------------------------------------------------------- свойства действий

/// Сторона экрана, к которой прижато окно (`CooperativeResizeSide`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ResizeSide {
    Left,
    Right,
    Top,
    Bottom,
}

/// Ось согласованного ресайза действия (`cooperativeResizeAxis`): у углов — из настроек.
pub fn resize_axis(action: Action, config: &Config) -> Option<Axis> {
    match action {
        Action::TopLeft | Action::TopRight | Action::BottomLeft | Action::BottomRight => {
            Some(config.corner_cycle_expansion_axis)
        }
        Action::LeftHalf | Action::RightHalf => Some(Axis::Horizontal),
        Action::TopHalf | Action::BottomHalf => Some(Axis::Vertical),
        _ => None,
    }
}

/// `cooperativeResizeSide`.
fn resize_side(action: Action, config: &Config) -> Option<ResizeSide> {
    let horizontal = config.corner_cycle_expansion_axis == Axis::Horizontal;
    let side = match action {
        Action::LeftHalf => ResizeSide::Left,
        Action::RightHalf => ResizeSide::Right,
        Action::TopHalf => ResizeSide::Top,
        Action::BottomHalf => ResizeSide::Bottom,
        Action::TopLeft if horizontal => ResizeSide::Left,
        Action::TopLeft => ResizeSide::Top,
        Action::TopRight if horizontal => ResizeSide::Right,
        Action::TopRight => ResizeSide::Top,
        Action::BottomLeft if horizontal => ResizeSide::Left,
        Action::BottomLeft => ResizeSide::Bottom,
        Action::BottomRight if horizontal => ResizeSide::Right,
        Action::BottomRight => ResizeSide::Bottom,
        _ => return None,
    };
    Some(side)
}

/// Край, который двигает действие (`cooperativeResizeMovedEdge`): у окна, прижатого к
/// левому краю, двигается правый.
pub fn resize_moved_edge(action: Action, config: &Config) -> Option<MovedEdge> {
    resize_side(action, config).map(|side| match side {
        ResizeSide::Left => MovedEdge::Right,
        ResizeSide::Right => MovedEdge::Left,
        ResizeSide::Top => MovedEdge::Bottom,
        ResizeSide::Bottom => MovedEdge::Top,
    })
}

/// Действие-угол (`isCooperativeCornerAction`).
pub fn is_corner_action(action: Action) -> bool {
    matches!(
        action,
        Action::TopLeft | Action::TopRight | Action::BottomLeft | Action::BottomRight
    )
}

/// Повтор продолжает перебор прошлого действия (`isCompatibleRepeatedResizeAction`):
/// угол — только тот же угол, остальные — та же сторона и ось.
pub fn is_compatible_repeated_resize_action(
    action: Action,
    other: Option<Action>,
    config: &Config,
) -> bool {
    let Some(other) = other else {
        return false;
    };
    if is_corner_action(action) && is_corner_action(other) {
        return action == other;
    }
    resize_side(action, config) == resize_side(other, config)
        && resize_axis(action, config) == resize_axis(other, config)
}

impl ExecutionSource {
    /// Источник, из которого соседи подстраиваются (`allowsCooperativeResize`): в
    /// оригинале — горячие клавиши и drag-to-snap. Горячих клавиш в порте нет; меню, URL и
    /// заголовок окна соседей не трогают, как в оригинале.
    pub fn allows_cooperative_resize(self) -> bool {
        match self {
            ExecutionSource::DragToSnap => true,
            ExecutionSource::MenuItem | ExecutionSource::Url | ExecutionSource::TitleBar => false,
        }
    }
}
