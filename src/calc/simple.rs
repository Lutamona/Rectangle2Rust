//! Расчёты без перебора: развернуть, центр, «почти развернуть», заданный размер,
//! больше/меньше, половина/двойной размер, боковая панель Todo.

use super::{swift_max, swift_min, CalcParams, CalcResult, RectParams, RectResult};
use crate::actions::Action;
use crate::config::{Config, TodoSidebarWidthUnit};
use crate::geometry::Rect;

/// `MaximizeCalculation`.
pub(super) fn maximize_rect(p: &RectParams) -> RectResult {
    RectResult::new(p.visible)
}

/// `MaximizeHeightCalculation`: высота во всю область, ширина и x окна.
pub(super) fn maximize_height_rect(p: &RectParams) -> RectResult {
    let mut rect = p.window;
    rect.y = p.visible.min_y();
    rect.h = p.visible.h;
    RectResult::new(rect)
}

/// Доля «почти развернуть»: 0 и меньше или больше 1 — 0,9 (`AlmostMaximizeCalculation`).
fn almost_maximize_fraction(value: f32) -> f64 {
    if value <= 0.0 || value > 1.0 {
        0.9
    } else {
        value as f64
    }
}

/// `AlmostMaximizeCalculation`.
pub(super) fn almost_maximize_rect(p: &RectParams) -> RectResult {
    let visible = p.visible;
    let mut rect = visible;
    rect.h = (visible.h * almost_maximize_fraction(p.config.almost_maximize_height)).round();
    rect.w = (visible.w * almost_maximize_fraction(p.config.almost_maximize_width)).round();
    rect.x = ((visible.w - rect.w) / 2.0).round() + visible.min_x();
    rect.y = ((visible.h - rect.h) / 2.0).round() + visible.min_y();
    RectResult::new(rect)
}

/// `SpecifiedCalculation`: до 1 — доля области, больше — пиксели (ширина не больше области).
pub(super) fn specified_rect(p: &RectParams) -> RectResult {
    let visible = p.visible;
    let specified_height = p.config.specified_height as f64;
    let specified_width = p.config.specified_width as f64;
    let mut rect = visible;
    rect.h = if specified_height <= 1.0 {
        visible.h * specified_height
    } else {
        specified_height.round()
    };
    rect.w = if specified_width <= 1.0 {
        visible.w * specified_width
    } else {
        swift_min(visible.w, specified_width.round())
    };
    rect.x = ((visible.w - rect.w) / 2.0).round() + visible.min_x();
    rect.y = ((visible.h - rect.h) / 2.0).round() + visible.min_y();
    RectResult::new(rect)
}

/// «Центр» и «центр крупно» (`CenterCalculation.calculate`): центрируют по рабочей
/// области без полосы Stage Manager, если не включено `alwaysAccountForStage`.
pub(super) fn center(p: &CalcParams) -> CalcResult {
    let screen_frame = if p.config.always_account_for_stage != Some(true) {
        Some(p.visible_ignoring_stage.unwrap_or(p.visible))
    } else {
        None
    };
    let mut params = p.rect_params();
    if let Some(frame) = screen_frame {
        params.visible = frame;
    }
    let result = if p.action == Action::CenterProminently {
        center_prominently_rect(&params)
    } else {
        center_rect(&params)
    };
    CalcResult {
        rect: result.rect,
        action: result.resulting_action.unwrap_or(p.action),
        sub_action: None,
        screen_frame,
    }
}

/// `CenterCalculation.calculateRect`: размер окна сохраняется; окно больше области по
/// обеим сторонам — «развернуть».
pub(super) fn center_rect(p: &RectParams) -> RectResult {
    let visible = p.visible;
    let window = p.window;
    let mut rect = window;
    let height_exceeded = window.h > visible.h;
    let width_exceeded = window.w > visible.w;

    if height_exceeded && width_exceeded {
        return RectResult {
            rect: visible,
            resulting_action: Some(Action::Maximize),
            sub_action: None,
        };
    }

    if height_exceeded {
        rect.h = visible.h;
        rect.y = visible.min_y();
    } else {
        rect.y = ((visible.h - window.h) / 2.0).round() + visible.min_y();
    }

    if width_exceeded {
        rect.w = visible.w;
        rect.x = visible.min_x();
    } else {
        rect.x = ((visible.w - window.w) / 2.0).round() + visible.min_x();
    }

    RectResult::new(rect)
}

/// `CenterProminentlyCalculation.calculateRect`: центр, сдвинутый вверх.
pub(super) fn center_prominently_rect(p: &RectParams) -> RectResult {
    let centered = center_rect(p);
    let mut rect = centered.rect;
    rect.y += -0.25 * rect.h + 0.25 * p.visible.h;
    RectResult {
        rect,
        resulting_action: centered.resulting_action,
        sub_action: None,
    }
}

/// `NextPrevDisplayCalculation.calculateRect`: развёрнутое окно разворачивается и на
/// новом экране, если не выключено `autoMaximize`; остальные — по центру.
pub(super) fn next_prev_display_rect(p: &RectParams) -> RectResult {
    if p.last.map(|last| last.action) == Some(Action::Maximize)
        && p.config.auto_maximize != Some(false)
    {
        return RectResult {
            rect: maximize_rect(p).rect,
            resulting_action: Some(Action::Maximize),
            sub_action: None,
        };
    }
    center_rect(p)
}

/// Боковая панель Todo (`LeftTodoCalculation`/`RightTodoCalculation`).
pub(super) fn todo_rect(p: &RectParams) -> RectResult {
    let visible = p.visible;
    let sidebar_width = todo_sidebar_width(visible.w, p.config);
    let mut rect = visible;
    if p.action == Action::RightTodo {
        rect.x = visible.max_x() - sidebar_width;
        rect.w = sidebar_width;
        RectResult::with_sub(rect, crate::actions::SubAction::RightTodo)
    } else {
        rect.w = sidebar_width;
        RectResult::with_sub(rect, crate::actions::SubAction::LeftTodo)
    }
}

/// Ширина панели Todo (`TodoManager.getSidebarWidth`): до 1 — доля области, иначе
/// пиксели или проценты по единице из настроек.
fn todo_sidebar_width(visible_width: f64, config: &Config) -> f64 {
    let sidebar_width = config.todo_sidebar_width as f64;
    if sidebar_width > 0.0 && sidebar_width <= 1.0 {
        sidebar_width * visible_width
    } else if config.todo_sidebar_width_unit == TodoSidebarWidthUnit::Pct {
        // TodoManager.convert(width:toUnit: .pixels, visibleFrameWidth:)
        ((sidebar_width * 0.01) * visible_width).round()
    } else {
        sidebar_width
    }
}

// ---------------------------------------------------------------- больше / меньше

/// `ChangeSizeCalculation`: «больше/меньше» и то же по ширине или высоте. Окно,
/// прижатое к краю области, растёт и сжимается от этого края («шторка»).
pub(super) fn change_size_rect(p: &RectParams) -> RectResult {
    let config = p.config;
    // Настройки, которые оригинал читает в init расчёта.
    let screen_edge_gap_size = if config.gap_size <= 0.0 {
        5.0
    } else {
        config.gap_size as f64
    };
    let size_offset_abs = if config.size_offset <= 0.0 {
        30.0
    } else {
        config.size_offset as f64
    };
    let curtain_change_size = config.curtain_change_size != Some(false);
    let smaller_shrinks_maximized_height = config.smaller_shrinks_maximized_height;
    let width_offset_abs = config.width_step_size as f64;

    let size_offset = match p.action {
        Action::Larger | Action::LargerHeight => size_offset_abs,
        Action::Smaller | Action::SmallerHeight => -size_offset_abs,
        Action::LargerWidth => width_offset_abs,
        Action::SmallerWidth => -width_offset_abs,
        _ => 0.0,
    };

    let visible = p.visible;
    let window = p.window;
    let edges = ScreenEdges {
        visible,
        tolerance: screen_edge_gap_size,
        gap_size: config.gap_size as f64,
    };

    let mut rect = window;
    if matches!(
        p.action,
        Action::Larger | Action::Smaller | Action::LargerWidth | Action::SmallerWidth
    ) {
        rect.w += size_offset;
        rect.x = rect.min_x() - (size_offset / 2.0).floor();
        if curtain_change_size {
            rect = edges.against_left_and_right(&window, rect);
        }
        if rect.w >= visible.w {
            rect.w = visible.w;
        }
    }

    if matches!(
        p.action,
        Action::Larger | Action::Smaller | Action::LargerHeight | Action::SmallerHeight
    ) {
        rect.h += size_offset;
        rect.y = rect.min_y() - (size_offset / 2.0).floor();
        // «Меньше по высоте» не держится за верх и низ, чтобы сжать и окно во всю высоту;
        // smallerShrinksMaximizedHeight распространяет это и на «меньше».
        let height_curtain_exempt = if smaller_shrinks_maximized_height {
            matches!(p.action, Action::Smaller | Action::SmallerHeight)
        } else {
            p.action == Action::SmallerHeight
        };
        if curtain_change_size && !height_curtain_exempt {
            rect = edges.against_top_and_bottom(&window, rect);
        }
        if rect.h >= visible.h {
            rect.h = visible.h;
            rect.y = window.min_y();
        }
    }

    if edges.against_all(&window) && size_offset < 0.0 {
        rect.w = window.w + size_offset;
        rect.x = window.x - (size_offset / 2.0).floor();
        rect.h = window.h + size_offset;
        rect.y = window.y - (size_offset / 2.0).floor();
    }

    if matches!(
        p.action,
        Action::Smaller | Action::SmallerWidth | Action::SmallerHeight
    ) && resized_window_rect_is_too_small(&rect, &visible, config)
    {
        rect = window;
    }

    RectResult::new(rect)
}

/// Проверки «окно прижато к краю области» из `ChangeSizeCalculation`.
struct ScreenEdges {
    visible: Rect,
    /// Допуск прижатия: гэп, а без гэпов — 5 px (`screenEdgeGapSize`).
    tolerance: f64,
    /// Гэп из настроек как есть — отступ, на который окно встаёт от края.
    gap_size: f64,
}

impl ScreenEdges {
    fn against(&self, gap: f64) -> bool {
        gap.abs() <= self.tolerance
    }

    fn left(&self, rect: &Rect) -> bool {
        self.against(rect.min_x() - self.visible.min_x())
    }

    fn right(&self, rect: &Rect) -> bool {
        self.against(rect.max_x() - self.visible.max_x())
    }

    fn top(&self, rect: &Rect) -> bool {
        self.against(rect.max_y() - self.visible.max_y())
    }

    fn bottom(&self, rect: &Rect) -> bool {
        self.against(rect.min_y() - self.visible.min_y())
    }

    fn against_all(&self, rect: &Rect) -> bool {
        self.left(rect) && self.right(rect) && self.top(rect) && self.bottom(rect)
    }

    fn against_left_and_right(&self, original: &Rect, resized: Rect) -> Rect {
        let mut adjusted = resized;
        if self.right(original) {
            adjusted.x = self.visible.max_x() - adjusted.w - self.gap_size;
            if self.left(original) {
                adjusted.w = self.visible.w - self.gap_size * 2.0;
            }
        }
        if self.left(original) {
            adjusted.x = self.visible.min_x() + self.gap_size;
        }
        adjusted
    }

    fn against_top_and_bottom(&self, original: &Rect, resized: Rect) -> Rect {
        let mut adjusted = resized;
        if self.top(original) {
            adjusted.y = self.visible.max_y() - adjusted.h - self.gap_size;
            if self.bottom(original) {
                adjusted.h = self.visible.h - self.gap_size * 2.0;
            }
        }
        if self.bottom(original) {
            adjusted.y = self.visible.min_y() + self.gap_size;
        }
        adjusted
    }
}

/// Окно после уменьшения стало меньше минимума (`resizedWindowRectIsTooSmall`):
/// минимум — доля области из настроек (вне 0…1 — ¼), но не меньше 1 px.
fn resized_window_rect_is_too_small(rect: &Rect, visible: &Rect, config: &Config) -> bool {
    let minimum_width = swift_max(
        1.0,
        (visible.w * minimum_window_fraction(config.minimum_window_width)).floor(),
    );
    let minimum_height = swift_max(
        1.0,
        (visible.h * minimum_window_fraction(config.minimum_window_height)).floor(),
    );
    rect.w < minimum_width || rect.h < minimum_height
}

fn minimum_window_fraction(value: f64) -> f64 {
    if !value.is_finite() || value < 0.0 || value > 1.0 {
        0.25
    } else {
        value
    }
}

/// `HalfOrDoubleDimensionCalculation`: половина или двойной размер по одной стороне.
pub(super) fn half_or_double_dimension_rect(p: &RectParams) -> RectResult {
    let window = p.window;
    let mut resized = window;
    match p.action {
        Action::HalveHeightUp | Action::HalveHeightDown => resized.h *= 0.5,
        Action::HalveWidthLeft | Action::HalveWidthRight => resized.w *= 0.5,
        Action::DoubleHeightUp | Action::DoubleHeightDown => resized.h *= 2.0,
        Action::DoubleWidthLeft | Action::DoubleWidthRight => resized.w *= 2.0,
        _ => {}
    }

    let mut rect = match p.action {
        Action::HalveHeightUp => resized.offset_by(0.0, resized.h),
        Action::HalveWidthRight => resized.offset_by(resized.w, 0.0),
        Action::DoubleHeightDown => resized.offset_by(0.0, -window.h),
        Action::DoubleWidthLeft => resized.offset_by(-window.w, 0.0),
        _ => resized,
    };

    let size_reducing = matches!(
        p.action,
        Action::HalveHeightUp
            | Action::HalveHeightDown
            | Action::HalveWidthLeft
            | Action::HalveWidthRight
    );
    if size_reducing && resized_window_rect_is_too_small(&rect, &p.visible, p.config) {
        rect = window;
    }
    RectResult::new(rect)
}
