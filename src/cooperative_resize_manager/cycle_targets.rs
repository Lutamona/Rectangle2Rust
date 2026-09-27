//! Размеры перебора в согласованном ресайзе — чистая часть
//! `WindowManager+CooperativeCornerResize.swift`: где стоит окно-источник прошлого
//! действия, до какого размера вернуть соседей, когда окно в фокусе ушло
//! (`cleanup*`), и какой размер перебора пропустить, если сосед уже упёрся в минимум
//! (`cycleLookAheadTargetForMinimumRestrictedAdjacent`).
//!
//! Настройки и доли сторон (`ActiveSideSplitRatios`) берутся из `CycleContext::config`
//! там, где Swift читает `Defaults`.

use std::cmp::Ordering;

use crate::actions::{Action, Dimension};
use crate::calc::apply_gaps_raw;
use crate::config::{Config, CornerCycleExpansionAxis as Axis, CycleSize};
use crate::cooperative_resize::cg::{
    height, intersects, max_x, max_y, min_x, min_y, swift_max, swift_max_by, swift_min,
    swift_min_by, width,
};
use crate::cooperative_resize::{
    focused_window_is_expanding, frames_differ, is_corner_action, AdjustmentKind, Candidate,
    MovedEdge,
};
use crate::geometry::{corner_rect, horizontal_rect, vertical_rect, Rect, Side};
use crate::side_split_ratios;

/// Общие параметры проверок: действие, рабочая область, ось и край перебора, допуск
/// общих краёв, гэп и настройки.
#[derive(Clone, Copy, Debug)]
pub struct CycleContext<'a> {
    pub action: Action,
    pub screen_frame: Rect,
    pub axis: Axis,
    pub moved_edge: MovedEdge,
    pub tolerance: f64,
    pub gap_size: f64,
    pub config: &'a Config,
}

impl CycleContext<'_> {
    fn with_action(&self, action: Action) -> Self {
        CycleContext { action, ..*self }
    }
}

/// Размер перебора и его рамка до и после гэпов (`CooperativeCycleTargetFrame`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CycleTargetFrame {
    pub cycle_size: CycleSize,
    pub raw_frame: Rect,
    pub gapped_frame: Rect,
}

/// Размер перебора, до которого перескакивает повтор (`CooperativeCycleLookAheadTarget`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CycleLookAheadTarget {
    /// Рамка до гэпов — по ней запоминается доля стороны.
    pub raw_frame: Rect,
    pub gapped_frame: Rect,
    pub skipped_cycle_size: CycleSize,
    pub target_cycle_size: CycleSize,
    /// Сосед, который уже в полосе минимального размера.
    pub restricted_adjacent_id: u32,
}

/// Окно, которое занимает место прошлого действия (`observedCooperativeSourceFrame`):
/// тот же размер по оси, прижато к тем же краям экрана, а у половин — ещё и та же полоса.
/// Из нескольких — самое крупное, при равных — с меньшим номером.
pub fn observed_cooperative_source_frame(
    ctx: &CycleContext,
    old_focused_frame: &Rect,
    candidates: &[Candidate],
    capture_tolerance: f64,
) -> Option<Rect> {
    let axis = ctx.axis;
    let size_tolerance = swift_max(ctx.tolerance, capture_tolerance);
    let source_candidates = candidates.iter().filter(|candidate| {
        let frame = &candidate.frame;
        if !((axis_size(frame, axis) - axis_size(old_focused_frame, axis)).abs() <= size_tolerance
            && frame_occupies_source(
                frame,
                ctx.action,
                &ctx.screen_frame,
                ctx.tolerance,
                ctx.gap_size,
            ))
        {
            return false;
        }
        is_corner_action(ctx.action)
            || matches_perpendicular_span(frame, old_focused_frame, ctx.moved_edge, size_tolerance)
    });

    swift_max_by(source_candidates, |lhs, rhs| {
        let lhs_size = axis_size(&lhs.frame, axis);
        let rhs_size = axis_size(&rhs.frame, axis);
        if (lhs_size - rhs_size).abs() > ctx.tolerance {
            return lhs_size < rhs_size;
        }
        lhs.id > rhs.id
    })
    .map(|candidate| candidate.frame)
}

/// Откуда возвращать соседей (`cleanupSourceFrame`): окно на месте прошлого действия,
/// а у угла, который сам упирался в минимум соседа, — его прежняя рамка.
pub fn cleanup_source_frame(
    ctx: &CycleContext,
    old_focused_frame: &Rect,
    candidates: &[Candidate],
    capture_tolerance: f64,
) -> Option<Rect> {
    if let Some(observed_source_frame) =
        observed_cooperative_source_frame(ctx, old_focused_frame, candidates, capture_tolerance)
    {
        return Some(observed_source_frame);
    }
    departed_minimum_restricted_cleanup_source_frame(ctx, old_focused_frame)
}

fn departed_minimum_restricted_cleanup_source_frame(
    ctx: &CycleContext,
    old_focused_frame: &Rect,
) -> Option<Rect> {
    if !(is_corner_action(ctx.action)
        && minimum_restricted_cleanup_target_frame(ctx, old_focused_frame).is_some())
    {
        return None;
    }
    Some(*old_focused_frame)
}

/// До какой рамки вернуть окно-источник (`cleanupTargetFrame`): больше размера из
/// настроек — к ближайшему из намеченных размеров, застряло между двумя меньшими
/// размерами перебора — к меньшему. `None` — возвращать не нужно.
pub fn cleanup_target_frame(
    ctx: &CycleContext,
    observed_frame: &Rect,
    include_cycle_targets: bool,
    candidates: &[Candidate],
) -> Option<Rect> {
    let axis = ctx.axis;
    let observed_size = axis_size(observed_frame, axis);
    let target_frames = intended_cleanup_target_frames(ctx, include_cycle_targets);
    if let Some(configured_frame) =
        configured_action_frame(ctx.action, &ctx.screen_frame, ctx.config)
    {
        let configured_size = axis_size(
            &gapped_frame(&configured_frame, ctx.action, ctx.gap_size, ctx.config),
            axis,
        );
        if observed_size > configured_size + ctx.tolerance {
            if observed_frame_matches_non_configured_cycle_target(
                ctx,
                observed_frame,
                configured_size,
            ) {
                return None;
            }
            if adjacent_smaller_cycle_target_contains_window(
                ctx,
                candidates,
                observed_frame,
                configured_size,
            ) {
                return None;
            }

            if let Some(target_size) =
                nearest_target_size(observed_size, &target_frames, axis, ctx.tolerance)
            {
                return Some(frame_with_axis_size(
                    observed_frame,
                    axis,
                    ctx.moved_edge,
                    target_size,
                    &ctx.screen_frame,
                ));
            }
        }
    }

    if include_cycle_targets && observed_frame_matches_cycle_target(ctx, observed_frame) {
        return None;
    }

    minimum_restricted_cleanup_target_frame(ctx, observed_frame)
}

/// Возвращать источник можно (`cleanupDestinationAllowsSourceResize`): окно в фокусе
/// ушло к нему вплотную или источник упирался в минимум.
pub fn cleanup_destination_allows_source_resize(
    ctx: &CycleContext,
    observed_frame: &Rect,
    target_frame: &Rect,
    focused_destination_frame: &Rect,
) -> bool {
    if frame_is_adjacent_to_cleanup_source(
        focused_destination_frame,
        observed_frame,
        ctx.moved_edge,
        ctx.axis,
        ctx.tolerance,
        ctx.gap_size,
    ) {
        return true;
    }

    let Some(minimum_restricted_target) =
        minimum_restricted_cleanup_target_frame(ctx, observed_frame)
    else {
        return false;
    };

    frames_match(
        &minimum_restricted_target,
        target_frame,
        swift_max(4.0, ctx.tolerance),
    )
}

fn observed_frame_matches_non_configured_cycle_target(
    ctx: &CycleContext,
    observed_frame: &Rect,
    configured_size: f64,
) -> bool {
    let observed_size = axis_size(observed_frame, ctx.axis);
    cycle_target_frames(ctx).iter().any(|target_frame| {
        let target_size = axis_size(&target_frame.gapped_frame, ctx.axis);
        (target_size - observed_size).abs() <= ctx.tolerance
            && (target_size - configured_size).abs() > ctx.tolerance
    })
}

fn observed_frame_matches_cycle_target(ctx: &CycleContext, observed_frame: &Rect) -> bool {
    let observed_size = axis_size(observed_frame, ctx.axis);
    cycle_target_frames(ctx).iter().any(|target_frame| {
        (axis_size(&target_frame.gapped_frame, ctx.axis) - observed_size).abs() <= ctx.tolerance
    })
}

fn adjacent_smaller_cycle_target_contains_window(
    ctx: &CycleContext,
    candidates: &[Candidate],
    observed_frame: &Rect,
    configured_size: f64,
) -> bool {
    let Some(adjacent_action) = adjacent_cycle_action(ctx.action, ctx.moved_edge) else {
        return false;
    };

    let adjacent_cycle_sizes: Vec<f64> = cycle_target_frames(&ctx.with_action(adjacent_action))
        .iter()
        .map(|target_frame| axis_size(&target_frame.gapped_frame, ctx.axis))
        .collect();
    let size_tolerance = swift_max(4.0, ctx.tolerance);

    candidates.iter().any(|candidate| {
        let frame = &candidate.frame;
        if !frame_is_adjacent_to_cleanup_source(
            frame,
            observed_frame,
            ctx.moved_edge,
            ctx.axis,
            ctx.tolerance,
            ctx.gap_size,
        ) {
            return false;
        }

        let current_size = axis_size(frame, ctx.axis);
        current_size < configured_size - size_tolerance
            && adjacent_cycle_sizes
                .iter()
                .any(|size| (size - current_size).abs() <= size_tolerance)
    })
}

/// Окно стоит вплотную (через гэп) к краю источника, в той же полосе
/// (`frameIsAdjacentToCleanupSource`).
pub fn frame_is_adjacent_to_cleanup_source(
    frame: &Rect,
    source_frame: &Rect,
    moved_edge: MovedEdge,
    axis: Axis,
    tolerance: f64,
    gap_size: f64,
) -> bool {
    if !(moved_edge_matches(axis, moved_edge)
        && matches_perpendicular_span(frame, source_frame, moved_edge, tolerance))
    {
        return false;
    }

    match moved_edge {
        MovedEdge::Right => ((min_x(frame) - max_x(source_frame)) - gap_size).abs() <= tolerance,
        MovedEdge::Left => ((min_x(source_frame) - max_x(frame)) - gap_size).abs() <= tolerance,
        MovedEdge::Top => ((min_y(frame) - max_y(source_frame)) - gap_size).abs() <= tolerance,
        MovedEdge::Bottom => ((min_y(source_frame) - max_y(frame)) - gap_size).abs() <= tolerance,
    }
}

/// Повтор растит окно, а сосед уже зажат между двумя меньшими размерами перебора —
/// такое нажатие ничего бы не сделало, поэтому берётся следующий размер
/// (`cycleLookAheadTargetForMinimumRestrictedAdjacent`).
pub fn cycle_look_ahead_target_for_minimum_restricted_adjacent(
    ctx: &CycleContext,
    old_focused_frame: &Rect,
    requested_focused_frame: &Rect,
    candidates: &[Candidate],
) -> Option<CycleLookAheadTarget> {
    let axis = ctx.axis;
    if !focused_window_is_expanding(old_focused_frame, requested_focused_frame, axis) {
        return None;
    }
    let adjacent_action = adjacent_cycle_action(ctx.action, ctx.moved_edge)?;

    let target_frames = cycle_target_frames(ctx);
    if target_frames.len() <= 1 {
        return None;
    }
    let requested_index = target_frames.iter().position(|target_frame| {
        frames_match(&target_frame.gapped_frame, requested_focused_frame, 1.0)
    })?;

    let adjacent_cycle_sizes = unique_sorted_sizes(
        cycle_target_frames(&ctx.with_action(adjacent_action))
            .iter()
            .map(|target_frame| axis_size(&target_frame.gapped_frame, axis))
            .collect(),
    );
    if adjacent_cycle_sizes.len() < 2 {
        return None;
    }

    let minimum_cycle_size = adjacent_cycle_sizes[0];
    let second_minimum_cycle_size = adjacent_cycle_sizes[1];
    let size_tolerance = swift_max(4.0, ctx.tolerance);
    // Соседей ищет допуск общих краёв; на границе размера перебора — только допуск раскладки.
    let cycle_boundary_tolerance = 4.0;

    let restricted_adjacent = candidates.iter().find(|candidate| {
        let frame = &candidate.frame;
        if !frame_is_adjacent_to_cleanup_source(
            frame,
            old_focused_frame,
            ctx.moved_edge,
            axis,
            ctx.tolerance,
            ctx.gap_size,
        ) {
            return false;
        }

        let current_size = axis_size(frame, axis);
        let proposed_size =
            adjacent_axis_size(frame, requested_focused_frame, ctx.moved_edge, ctx.gap_size);
        current_size > minimum_cycle_size + cycle_boundary_tolerance
            && current_size < second_minimum_cycle_size - cycle_boundary_tolerance
            && proposed_size < current_size - size_tolerance
    })?;

    let mut next_index = (requested_index + 1) % target_frames.len();
    for _ in 0..target_frames.len() {
        let target_frame = &target_frames[next_index];
        if !frames_match(
            &target_frame.gapped_frame,
            old_focused_frame,
            size_tolerance,
        ) {
            return Some(CycleLookAheadTarget {
                raw_frame: target_frame.raw_frame,
                gapped_frame: target_frame.gapped_frame,
                skipped_cycle_size: target_frames[requested_index].cycle_size,
                target_cycle_size: target_frame.cycle_size,
                restricted_adjacent_id: restricted_adjacent.id,
            });
        }
        next_index = (next_index + 1) % target_frames.len();
    }

    None
}

/// Источник застрял между двумя меньшими размерами перебора — вернуть его к меньшему
/// (`minimumRestrictedCleanupTargetFrame`).
fn minimum_restricted_cleanup_target_frame(
    ctx: &CycleContext,
    observed_frame: &Rect,
) -> Option<Rect> {
    let cycle_sizes = unique_sorted_sizes(
        cycle_target_frames(ctx)
            .iter()
            .map(|target_frame| axis_size(&target_frame.gapped_frame, ctx.axis))
            .collect(),
    );
    if cycle_sizes.len() < 2 {
        return None;
    }

    let minimum_cycle_size = cycle_sizes[0];
    let second_minimum_cycle_size = cycle_sizes[1];
    let observed_size = axis_size(observed_frame, ctx.axis);
    // Размер, упёршийся в минимум у самой границы, — не тот же, что размер перебора.
    let cycle_boundary_tolerance = 4.0;

    if !(observed_size > minimum_cycle_size + cycle_boundary_tolerance
        && observed_size < second_minimum_cycle_size - cycle_boundary_tolerance)
    {
        return None;
    }

    Some(frame_with_axis_size(
        observed_frame,
        ctx.axis,
        ctx.moved_edge,
        minimum_cycle_size,
        &ctx.screen_frame,
    ))
}

fn moved_edge_matches(axis: Axis, moved_edge: MovedEdge) -> bool {
    matches!(
        (axis, moved_edge),
        (Axis::Horizontal, MovedEdge::Left)
            | (Axis::Horizontal, MovedEdge::Right)
            | (Axis::Vertical, MovedEdge::Top)
            | (Axis::Vertical, MovedEdge::Bottom)
    )
}

fn matches_perpendicular_span(
    frame: &Rect,
    source_frame: &Rect,
    moved_edge: MovedEdge,
    tolerance: f64,
) -> bool {
    match moved_edge {
        MovedEdge::Left | MovedEdge::Right => {
            (min_y(frame) - min_y(source_frame)).abs() <= tolerance
                && (max_y(frame) - max_y(source_frame)).abs() <= tolerance
        }
        MovedEdge::Top | MovedEdge::Bottom => {
            (min_x(frame) - min_x(source_frame)).abs() <= tolerance
                && (max_x(frame) - max_x(source_frame)).abs() <= tolerance
        }
    }
}

/// Рамки, к которым возвращаются соседи (`intendedCleanupTargetFrames`): размер из
/// настроек, а после повторов — и размеры перебора.
fn intended_cleanup_target_frames(ctx: &CycleContext, include_cycle_targets: bool) -> Vec<Rect> {
    let mut frames: Vec<Rect> = Vec::new();
    let append_unique = |frames: &mut Vec<Rect>, frame: Rect| {
        if !frames
            .iter()
            .any(|existing| !frames_differ(existing, &frame, 0.001))
        {
            frames.push(frame);
        }
    };

    if let Some(configured_frame) =
        configured_action_frame(ctx.action, &ctx.screen_frame, ctx.config)
    {
        append_unique(
            &mut frames,
            gapped_frame(&configured_frame, ctx.action, ctx.gap_size, ctx.config),
        );
    }

    if !(include_cycle_targets && ctx.config.subsequent_execution_mode.resizes()) {
        return frames;
    }

    for target_frame in cycle_target_frames(ctx) {
        append_unique(&mut frames, target_frame.gapped_frame);
    }
    frames
}

fn nearest_target_size(
    observed_size: f64,
    frames: &[Rect],
    axis: Axis,
    tolerance: f64,
) -> Option<f64> {
    swift_min_by(
        frames.iter().map(|frame| axis_size(frame, axis)),
        |lhs, rhs| (lhs - observed_size).abs() < (rhs - observed_size).abs(),
    )
    .filter(|target_size| (target_size - observed_size).abs() > tolerance)
}

/// Размеры перебора по порядку (`sortedCooperativeCycleSizes`).
fn sorted_cooperative_cycle_sizes(config: &Config) -> Vec<CycleSize> {
    config.effective_cycle_sizes().sorted_sizes()
}

/// Рамки действия во всех размерах перебора (`cycleTargetFrames`).
pub fn cycle_target_frames(ctx: &CycleContext) -> Vec<CycleTargetFrame> {
    sorted_cooperative_cycle_sizes(ctx.config)
        .into_iter()
        .filter_map(|cycle_size| {
            let raw_frame = raw_cycle_frame(
                ctx.action,
                cycle_size,
                &ctx.screen_frame,
                ctx.axis,
                ctx.config,
            )?;
            Some(CycleTargetFrame {
                cycle_size,
                raw_frame,
                gapped_frame: gapped_frame(&raw_frame, ctx.action, ctx.gap_size, ctx.config),
            })
        })
        .collect()
}

fn raw_cycle_frame(
    action: Action,
    cycle_size: CycleSize,
    screen_frame: &Rect,
    axis: Axis,
    config: &Config,
) -> Option<Rect> {
    let fraction = cycle_size.fraction();
    match action {
        Action::LeftHalf => Some(horizontal_rect(screen_frame, Side::Leading, fraction)),
        Action::RightHalf => Some(horizontal_rect(screen_frame, Side::Trailing, fraction)),
        Action::TopHalf => Some(vertical_rect(screen_frame, Side::Leading, fraction)),
        Action::BottomHalf => Some(vertical_rect(screen_frame, Side::Trailing, fraction)),
        Action::TopLeft | Action::TopRight | Action::BottomLeft | Action::BottomRight => {
            raw_corner_cycle_frame(action, fraction, screen_frame, axis, config)
        }
        _ => None,
    }
}

/// Угол в размере перебора: по оси перебора — доля размера, по другой — доля стороны.
fn raw_corner_cycle_frame(
    action: Action,
    cycle_fraction: f32,
    screen_frame: &Rect,
    axis: Axis,
    config: &Config,
) -> Option<Rect> {
    let horizontal_ratio = side_split_ratios::horizontal_ratio(screen_frame, config);
    let vertical_ratio = side_split_ratios::vertical_ratio(screen_frame, config);
    let (horizontal_side, vertical_side) = corner_sides(action)?;

    let horizontal_fraction = if axis == Axis::Horizontal {
        cycle_fraction
    } else if horizontal_side == Side::Trailing {
        1.0 - horizontal_ratio
    } else {
        horizontal_ratio
    };
    let vertical_fraction = if axis == Axis::Vertical {
        cycle_fraction
    } else if vertical_side == Side::Trailing {
        1.0 - vertical_ratio
    } else {
        vertical_ratio
    };

    Some(corner_rect(
        screen_frame,
        horizontal_side,
        vertical_side,
        horizontal_fraction,
        vertical_fraction,
    ))
}

fn corner_sides(action: Action) -> Option<(Side, Side)> {
    match action {
        Action::TopLeft => Some((Side::Leading, Side::Leading)),
        Action::TopRight => Some((Side::Trailing, Side::Leading)),
        Action::BottomLeft => Some((Side::Leading, Side::Trailing)),
        Action::BottomRight => Some((Side::Trailing, Side::Trailing)),
        _ => None,
    }
}

/// Рамка с гэпами действия (`gappedFrame`).
fn gapped_frame(frame: &Rect, action: Action, gap_size: f64, config: &Config) -> Rect {
    let gaps_applicable = action.gaps_applicable(
        config.resize_on_directional_move,
        config.apply_gaps_to_maximize != Some(false),
        config.apply_gaps_to_maximize_height != Some(false),
    );
    if gap_size > 0.0 && gaps_applicable != Dimension::NONE {
        apply_gaps_raw(
            *frame,
            gaps_applicable,
            action.gap_shared_edge(config.resize_on_directional_move),
            gap_size as f32,
            config.skip_gap_top_edge,
        )
    } else {
        *frame
    }
}

/// Действие соседа за двигающимся краем (`adjacentCycleAction`): у левой половины,
/// которая двигает правый край, — правая половина.
pub fn adjacent_cycle_action(action: Action, moved_edge: MovedEdge) -> Option<Action> {
    use Action::*;
    let adjacent = match (action, moved_edge) {
        (LeftHalf, MovedEdge::Right) => RightHalf,
        (RightHalf, MovedEdge::Left) => LeftHalf,
        (TopHalf, MovedEdge::Bottom) => BottomHalf,
        (BottomHalf, MovedEdge::Top) => TopHalf,
        (TopLeft, MovedEdge::Right) => TopRight,
        (TopLeft, MovedEdge::Bottom) => BottomLeft,
        (TopRight, MovedEdge::Left) => TopLeft,
        (TopRight, MovedEdge::Bottom) => BottomRight,
        (BottomLeft, MovedEdge::Right) => BottomRight,
        (BottomLeft, MovedEdge::Top) => TopLeft,
        (BottomRight, MovedEdge::Left) => BottomLeft,
        (BottomRight, MovedEdge::Top) => TopRight,
        _ => return None,
    };
    Some(adjacent)
}

/// Какое действие записать соседу в историю (`cooperativeHistoryAction`): растущему
/// вместе с окном — то же, соседу за краем — действие его стороны.
pub fn cooperative_history_action(
    kind: AdjustmentKind,
    source_action: Action,
    moved_edge: MovedEdge,
) -> Option<Action> {
    match kind {
        AdjustmentKind::MatchingFocusedFrame => Some(source_action),
        AdjustmentKind::Adjacent => adjacent_cycle_action(source_action, moved_edge),
    }
}

fn unique_sorted_sizes(mut sizes: Vec<f64>) -> Vec<f64> {
    sizes.sort_by(|lhs, rhs| lhs.partial_cmp(rhs).unwrap_or(Ordering::Equal));
    let mut unique_sizes: Vec<f64> = Vec::new();
    for size in sizes {
        if !unique_sizes
            .iter()
            .any(|existing| (existing - size).abs() <= 0.001)
        {
            unique_sizes.push(size);
        }
    }
    unique_sizes
}

/// Размер соседа по оси после того, как окно в фокусе встанет в `focused_frame`.
fn adjacent_axis_size(
    frame: &Rect,
    focused_frame: &Rect,
    moved_edge: MovedEdge,
    gap_size: f64,
) -> f64 {
    match moved_edge {
        MovedEdge::Right => swift_max(0.0, max_x(frame) - (max_x(focused_frame) + gap_size)),
        MovedEdge::Left => swift_max(0.0, (min_x(focused_frame) - gap_size) - min_x(frame)),
        MovedEdge::Top => swift_max(0.0, max_y(frame) - (max_y(focused_frame) + gap_size)),
        MovedEdge::Bottom => swift_max(0.0, (min_y(focused_frame) - gap_size) - min_y(frame)),
    }
}

fn frames_match(lhs: &Rect, rhs: &Rect, tolerance: f64) -> bool {
    !frames_differ(lhs, rhs, tolerance)
}

/// Рамка действия по долям сторон, без перебора (`configuredActionFrame`).
fn configured_action_frame(action: Action, screen_frame: &Rect, config: &Config) -> Option<Rect> {
    let horizontal_ratio = side_split_ratios::horizontal_ratio(screen_frame, config);
    let vertical_ratio = side_split_ratios::vertical_ratio(screen_frame, config);

    match action {
        Action::LeftHalf => {
            return Some(horizontal_rect(
                screen_frame,
                Side::Leading,
                horizontal_ratio,
            ))
        }
        Action::RightHalf => {
            return Some(horizontal_rect(
                screen_frame,
                Side::Trailing,
                1.0 - horizontal_ratio,
            ))
        }
        Action::TopHalf => return Some(vertical_rect(screen_frame, Side::Leading, vertical_ratio)),
        Action::BottomHalf => {
            return Some(vertical_rect(
                screen_frame,
                Side::Trailing,
                1.0 - vertical_ratio,
            ))
        }
        _ => {}
    }

    let (horizontal_side, vertical_side) = corner_sides(action)?;
    let horizontal_fraction = if horizontal_side == Side::Trailing {
        1.0 - horizontal_ratio
    } else {
        horizontal_ratio
    };
    let vertical_fraction = if vertical_side == Side::Trailing {
        1.0 - vertical_ratio
    } else {
        vertical_ratio
    };
    Some(corner_rect(
        screen_frame,
        horizontal_side,
        vertical_side,
        horizontal_fraction,
        vertical_fraction,
    ))
}

/// Окно прижато к краям экрана, как рамка действия (`frame(_:occupiesSourceFor:...)`).
fn frame_occupies_source(
    frame: &Rect,
    action: Action,
    screen_frame: &Rect,
    tolerance: f64,
    gap_size: f64,
) -> bool {
    if !intersects(screen_frame, frame) {
        return false;
    }

    let left = || {
        matches_outer_min(
            min_x(frame),
            screen_frame,
            Axis::Horizontal,
            tolerance,
            gap_size,
        )
    };
    let right = || {
        matches_outer_max(
            max_x(frame),
            screen_frame,
            Axis::Horizontal,
            tolerance,
            gap_size,
        )
    };
    let top = || {
        matches_outer_max(
            max_y(frame),
            screen_frame,
            Axis::Vertical,
            tolerance,
            gap_size,
        )
    };
    let bottom = || {
        matches_outer_min(
            min_y(frame),
            screen_frame,
            Axis::Vertical,
            tolerance,
            gap_size,
        )
    };
    match action {
        Action::LeftHalf => left(),
        Action::RightHalf => right(),
        Action::TopHalf => top(),
        Action::BottomHalf => bottom(),
        Action::TopLeft => left() && top(),
        Action::TopRight => right() && top(),
        Action::BottomLeft => left() && bottom(),
        Action::BottomRight => right() && bottom(),
        _ => false,
    }
}

/// Та же рамка с размером `size` по оси; двигается край `moved_edge`
/// (`frame(_:axis:movedEdge:size:screenFrame:)`).
fn frame_with_axis_size(
    observed_frame: &Rect,
    axis: Axis,
    moved_edge: MovedEdge,
    size: f64,
    screen_frame: &Rect,
) -> Rect {
    let mut target = *observed_frame;
    let screen_size = match axis {
        Axis::Horizontal => width(screen_frame),
        Axis::Vertical => height(screen_frame),
    };
    let bounded_size = swift_max(0.0, swift_min(size, screen_size));
    match moved_edge {
        MovedEdge::Right => target.w = bounded_size,
        MovedEdge::Left => {
            target.x = max_x(observed_frame) - bounded_size;
            target.w = bounded_size;
        }
        MovedEdge::Top => target.h = bounded_size,
        MovedEdge::Bottom => {
            target.y = max_y(observed_frame) - bounded_size;
            target.h = bounded_size;
        }
    }
    target
}

fn axis_size(frame: &Rect, axis: Axis) -> f64 {
    match axis {
        Axis::Horizontal => width(frame),
        Axis::Vertical => height(frame),
    }
}

fn matches_outer_min(
    value: f64,
    screen_frame: &Rect,
    axis: Axis,
    tolerance: f64,
    gap_size: f64,
) -> bool {
    let screen_min = match axis {
        Axis::Horizontal => min_x(screen_frame),
        Axis::Vertical => min_y(screen_frame),
    };
    (value - screen_min).abs() <= tolerance || (value - (screen_min + gap_size)).abs() <= tolerance
}

fn matches_outer_max(
    value: f64,
    screen_frame: &Rect,
    axis: Axis,
    tolerance: f64,
    gap_size: f64,
) -> bool {
    let screen_max = match axis {
        Axis::Horizontal => max_x(screen_frame),
        Axis::Vertical => max_y(screen_frame),
    };
    (value - screen_max).abs() <= tolerance || (value - (screen_max - gap_size)).abs() <= tolerance
}
