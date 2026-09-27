//! Гэпы между окнами — порт `GapCalculation.swift`.

use crate::actions::{Action, Dimension, SubAction};
use crate::config::Config;
use crate::geometry::{Edge, Rect};

/// Гэпы для итога расчёта: общие с соседями края берутся у под-действия, если оно
/// есть, иначе у самого действия. Одна формула и для конвейера (перед тем как ставить
/// окно), и для определения текущего столбика.
pub fn apply_gaps(
    rect: Rect,
    action: Action,
    sub_action: Option<SubAction>,
    config: &Config,
) -> Rect {
    let dimension = action.gaps_applicable(
        config.resize_on_directional_move,
        config.apply_gaps_to_maximize != Some(false),
        config.apply_gaps_to_maximize_height != Some(false),
    );
    if !gaps_enabled(config) || dimension == Dimension::NONE {
        return rect;
    }

    let shared_edges = match sub_action {
        Some(sub) => sub.gap_shared_edge(),
        None => action.gap_shared_edge(config.resize_on_directional_move),
    };

    apply_gaps_raw(
        rect,
        dimension,
        shared_edges,
        config.gap_size,
        config.skip_gap_top_edge,
    )
}

/// Гэпы включены: `gapSize > 0` (NaN — тоже без гэпов, как у оригинала).
pub(super) fn gaps_enabled(config: &Config) -> bool {
    config.gap_size > 0.0
}

/// Гэпы по осям `dimension`: по гэпу со всех сторон, а у общих с соседями краёв —
/// половина (`applyGaps(_:dimension:sharedEdges:gapSize:skipTopGap:)`). Гэп — `Float`
/// из настроек.
pub fn apply_gaps_raw(
    rect: Rect,
    dimension: Dimension,
    shared_edges: Edge,
    gap_size: f32,
    skip_top_gap: bool,
) -> Rect {
    let gap = gap_size as f64;
    let half_gap = gap / 2.0;
    let mut with_gaps = rect.inset_by(
        if dimension.contains(Dimension::HORIZONTAL) {
            gap
        } else {
            0.0
        },
        if dimension.contains(Dimension::VERTICAL) {
            gap
        } else {
            0.0
        },
    );

    if dimension.contains(Dimension::HORIZONTAL) {
        if shared_edges.contains(Edge::LEFT) {
            with_gaps.x -= half_gap;
            with_gaps.w += half_gap;
        }
        if shared_edges.contains(Edge::RIGHT) {
            with_gaps.w += half_gap;
        }
    }

    if dimension.contains(Dimension::VERTICAL) {
        if shared_edges.contains(Edge::BOTTOM) {
            with_gaps.y -= half_gap;
            with_gaps.h += half_gap;
        }
        if shared_edges.contains(Edge::TOP) {
            with_gaps.h += half_gap;
        }
        if skip_top_gap && !shared_edges.contains(Edge::TOP) {
            with_gaps.h += gap;
        }
    }

    with_gaps
}
