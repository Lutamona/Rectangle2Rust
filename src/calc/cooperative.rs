//! Свойства действий для кооперативного ресайза, которые нужны расчётам перебора —
//! порт `WindowActionCooperativeResize.swift`.

use crate::actions::Action;
use crate::config::{Config, CornerCycleExpansionAxis};

/// Сторона экрана, к которой прижато окно (`CooperativeResizeSide`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ResizeSide {
    Left,
    Right,
    Top,
    Bottom,
}

/// Край окна, который двигается при переборе размеров (`CooperativeCornerResize.MovedEdge`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum MovedEdge {
    Left,
    Right,
    Top,
    Bottom,
}

/// Ось, по которой меняется размер (`cooperativeResizeAxis`).
fn resize_axis(action: Action, config: &Config) -> Option<CornerCycleExpansionAxis> {
    match action {
        Action::TopLeft | Action::TopRight | Action::BottomLeft | Action::BottomRight => {
            Some(config.corner_cycle_expansion_axis)
        }
        Action::LeftHalf | Action::RightHalf => Some(CornerCycleExpansionAxis::Horizontal),
        Action::TopHalf | Action::BottomHalf => Some(CornerCycleExpansionAxis::Vertical),
        _ => None,
    }
}

/// `cooperativeResizeSide`.
fn resize_side(action: Action, config: &Config) -> Option<ResizeSide> {
    let horizontal = config.corner_cycle_expansion_axis == CornerCycleExpansionAxis::Horizontal;
    match action {
        Action::LeftHalf => Some(ResizeSide::Left),
        Action::RightHalf => Some(ResizeSide::Right),
        Action::TopHalf => Some(ResizeSide::Top),
        Action::BottomHalf => Some(ResizeSide::Bottom),
        Action::TopLeft => Some(if horizontal {
            ResizeSide::Left
        } else {
            ResizeSide::Top
        }),
        Action::TopRight => Some(if horizontal {
            ResizeSide::Right
        } else {
            ResizeSide::Top
        }),
        Action::BottomLeft => Some(if horizontal {
            ResizeSide::Left
        } else {
            ResizeSide::Bottom
        }),
        Action::BottomRight => Some(if horizontal {
            ResizeSide::Right
        } else {
            ResizeSide::Bottom
        }),
        _ => None,
    }
}

/// `cooperativeResizeMovedEdge`: у окна, прижатого к левому краю, двигается правый.
pub(super) fn moved_edge(action: Action, config: &Config) -> Option<MovedEdge> {
    resize_side(action, config).map(|side| match side {
        ResizeSide::Left => MovedEdge::Right,
        ResizeSide::Right => MovedEdge::Left,
        ResizeSide::Top => MovedEdge::Bottom,
        ResizeSide::Bottom => MovedEdge::Top,
    })
}

/// `isCooperativeCornerAction`.
pub(super) fn is_corner_action(action: Action) -> bool {
    matches!(
        action,
        Action::TopLeft | Action::TopRight | Action::BottomLeft | Action::BottomRight
    )
}

/// Повтор продолжает перебор прошлого действия (`isCompatibleRepeatedResizeAction`):
/// угол — только тот же угол, остальные — та же сторона и ось (у действий без стороны
/// и оси — любое такое же).
pub(super) fn is_compatible_repeated_resize_action(
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
