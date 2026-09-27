//! Расчёты раскладок: куда и какого размера поставить окно — порт
//! `WindowCalculation/*.swift` из Rectangle 2.
//!
//! Чистая математика без обращения к системе: её проверяют тесты и Swift-оракул
//! (`tools/swift-oracle`), а применяет к окну конвейер действия через AX. Устройство —
//! как в оригинале: у каждого действия свой расчёт `calculate_rect` (метод `calculateRect`
//! классов из `WindowCalculationFactory`), а `calculate` добавляет к нему действие-итог
//! (`resultingAction`); у нескольких действий `calculate` свой, как и в Swift.
//!
//! Координаты — Cocoa (начало снизу слева), числа — `CGFloat` (`f64`), а доли и проценты
//! из настроек — `Float` (`f32`) там же, где у оригинала. Повторные нажатия перебирают
//! размеры и ячейки по истории окна (`LastAction`): действию, под-действию и счётчику.
//!
//! Расчёты для нескольких экранов (соседний экран, дисплей по номеру, переезд половинами
//! и «к краю» между экранами) — дело слоя экранов: в параметрах нет соседних экранов,
//! и здесь эти ветки ведут себя так, как оригинал без соседей.

mod columns;
mod cooperative;
mod gaps;
mod grid;
mod repeated;
mod simple;

#[cfg(test)]
mod tests;

pub use gaps::{apply_gaps, apply_gaps_raw};

use crate::actions::{Action, SubAction};
use crate::config::Config;
use crate::geometry::Rect;

/// Что было с окном в прошлый раз (`RectangleAction`): по этому считаются повторные
/// нажатия.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LastAction {
    pub action: Action,
    pub sub_action: Option<SubAction>,
    /// Рамка окна после действия, координаты AX (начало сверху слева), как записывает
    /// конвейер.
    pub rect: Rect,
    /// Сколько раз подряд выполнялось это действие.
    pub count: u32,
}

/// Параметры расчёта (`WindowCalculationParameters`).
#[derive(Clone, Copy, Debug)]
pub struct CalcParams<'a> {
    /// Рамка окна, Cocoa.
    pub window: Rect,
    /// Рабочая область текущего экрана с отступами от краёв (`adjustedVisibleFrame`).
    pub visible: Rect,
    /// Та же рабочая область без полосы Stage Manager (`adjustedVisibleFrame(_, true)`):
    /// по ней центрируют «центр» и «центр крупно». `None` — как `visible`.
    pub visible_ignoring_stage: Option<Rect>,
    pub action: Action,
    pub last: Option<&'a LastAction>,
    pub config: &'a Config,
    /// Рабочая область экрана, на котором окно сейчас (для переездов между экранами).
    pub source_visible: Option<Rect>,
    /// Сколько всего экранов (`usableScreens.numScreens`).
    pub num_screens: usize,
    /// `NSScreen.screens[0].frame.maxY` — высота для перевода рамки из истории (AX)
    /// в Cocoa (`screenFlipped`).
    pub primary_max_y: f64,
}

impl CalcParams<'_> {
    /// Параметры расчёта прямоугольника (`asRectParams()`).
    pub fn rect_params(&self) -> RectParams<'_> {
        RectParams {
            window: self.window,
            visible: self.visible,
            action: self.action,
            last: self.last,
            config: self.config,
            primary_max_y: self.primary_max_y,
        }
    }
}

/// Итог расчёта (`WindowCalculationResult`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CalcResult {
    /// Рамка окна до гэпов, Cocoa. Она же `initialRect` — по ней конвейер запоминает
    /// доли сторон (`side_split_ratios::record_side_action`).
    pub rect: Rect,
    /// Действие, которое записывается в историю (`resultingAction`): обычно нажатое,
    /// но, например, «центр» у окна больше экрана даёт «развернуть».
    pub action: Action,
    pub sub_action: Option<SubAction>,
    /// Рабочая область, по которой считали, если она не обычная (`resultingScreenFrame`).
    pub screen_frame: Option<Rect>,
}

/// Параметры расчёта прямоугольника (`RectCalculationParameters`).
#[derive(Clone, Copy, Debug)]
pub struct RectParams<'a> {
    /// Рамка окна, Cocoa.
    pub window: Rect,
    /// Рабочая область, в которой считается раскладка.
    pub visible: Rect,
    pub action: Action,
    pub last: Option<&'a LastAction>,
    pub config: &'a Config,
    /// `NSScreen.screens[0].frame.maxY` (см. `CalcParams::primary_max_y`).
    pub primary_max_y: f64,
}

/// Прямоугольник расчёта (`RectResult`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RectResult {
    pub rect: Rect,
    /// Действие-итог, если расчёт его меняет («центр» → «развернуть»).
    pub resulting_action: Option<Action>,
    pub sub_action: Option<SubAction>,
}

impl RectResult {
    fn new(rect: Rect) -> Self {
        RectResult {
            rect,
            resulting_action: None,
            sub_action: None,
        }
    }

    fn with_sub(rect: Rect, sub_action: SubAction) -> Self {
        RectResult {
            rect,
            resulting_action: None,
            sub_action: Some(sub_action),
        }
    }
}

/// Точка входа: итог расчёта для действия (`calculate`). `None` — действие окно не
/// двигает (у него нет расчёта или расчёт невозможен, например «следующий дисплей» при
/// одном экране) — конвейер пищит.
///
/// «К краю» с переездом на соседний экран при повторе (`canTraverseDisplays`) — дело
/// слоя экранов; здесь «к краю» считается как в оригинале без соседних экранов.
pub fn calculate(p: &CalcParams) -> Option<CalcResult> {
    match p.action {
        Action::LeftHalf | Action::RightHalf => Some(repeated::left_right_half(p)),
        Action::Center | Action::CenterProminently => Some(simple::center(p)),
        Action::NextDisplay | Action::PreviousDisplay | Action::Display(_) => {
            // `guard usableScreens.numScreens > 1 else { return nil }`.
            if p.num_screens <= 1 {
                return None;
            }
            Some(display_move(p))
        }
        _ => {
            let result = calculate_rect(&p.rect_params())?;
            Some(CalcResult {
                rect: result.rect,
                action: p.action,
                sub_action: result.sub_action,
                screen_frame: None,
            })
        }
    }
}

/// Прямоугольник для действия (`calculateRect` расчёта из `WindowCalculationFactory`):
/// им же рисуют подсветку при drag-to-snap и повторяют действие на другом экране.
/// `None` — у действия нет расчёта.
pub fn calculate_rect(p: &RectParams) -> Option<RectResult> {
    use Action::*;
    let result = match p.action {
        LeftHalf | RightHalf => repeated::left_right_half_rect(p),
        TopHalf | BottomHalf => repeated::vertical_half_rect(p),
        CenterHalf => repeated::center_half_rect(p),
        TopLeft | TopRight | BottomLeft | BottomRight => repeated::corner_rect(p),
        MoveLeft | MoveRight => repeated::move_left_right_rect(p),
        MoveUp | MoveDown => repeated::move_up_down_rect(p),
        Maximize => simple::maximize_rect(p),
        MaximizeHeight => simple::maximize_height_rect(p),
        AlmostMaximize => simple::almost_maximize_rect(p),
        Specified => simple::specified_rect(p),
        Center => simple::center_rect(p),
        CenterProminently => simple::center_prominently_rect(p),
        Larger | Smaller | LargerWidth | SmallerWidth | LargerHeight | SmallerHeight => {
            simple::change_size_rect(p)
        }
        HalveHeightUp | HalveHeightDown | HalveWidthLeft | HalveWidthRight | DoubleHeightUp
        | DoubleHeightDown | DoubleWidthLeft | DoubleWidthRight => {
            simple::half_or_double_dimension_rect(p)
        }
        LeftTodo | RightTodo => simple::todo_rect(p),
        NextDisplay | PreviousDisplay => simple::next_prev_display_rect(p),
        Display(_) => simple::center_rect(p),
        Column { count, index } => columns::column_rect(p, count, index),
        Restore | ReverseAll | TileAll | CascadeAll | CascadeActiveApp | TileActiveApp => {
            return None
        }
        grid => return grid::calculate_rect(p, grid),
    };
    Some(result)
}

/// Переход на другой экран: окно центрируется на нём (как в Rectangle),
/// а если предыдущее действие было «развернуть» — разворачивается.
fn display_move(p: &CalcParams) -> CalcResult {
    let maximize = p
        .last
        .map(|last| last.action == Action::Maximize)
        .unwrap_or(false)
        && p.config.auto_maximize != Some(false);

    if maximize {
        return CalcResult {
            rect: p.visible,
            action: Action::Maximize,
            sub_action: None,
            screen_frame: None,
        };
    }

    let centered = simple::center_rect(&p.rect_params());
    CalcResult {
        rect: centered.rect,
        action: centered.resulting_action.unwrap_or(p.action),
        sub_action: None,
        screen_frame: None,
    }
}

/// `Swift.min(x, y)`: `y < x ? y : x` — так же ведёт себя с NaN и нулями разного знака.
fn swift_min(x: f64, y: f64) -> f64 {
    if y < x {
        y
    } else {
        x
    }
}

/// `Swift.max(x, y)`: `y >= x ? y : x`.
fn swift_max(x: f64, y: f64) -> f64 {
    if y >= x {
        y
    } else {
        x
    }
}
