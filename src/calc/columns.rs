//! Столбики (Rectangle 2): окно занимает один из N столбиков во всю высоту
//! рабочей области — порт `ColumnCalculation.swift`.
//!
//! Повторное нажатие того же действия переставляет окно в следующий столбик по кругу,
//! если окно после прошлого нажатия не трогали. Где окно стоит сейчас, решает его
//! геометрия, а не история: приложение могло подогнать размер (терминал — под сетку
//! символов), и тогда записанная рамка разойдётся с настоящей.

use super::{apply_gaps, LastAction, RectParams, RectResult};
use crate::actions::Action;
use crate::config::SubsequentExecutionMode;
use crate::geometry::{columns, Rect};

/// `ColumnCalculation.calculateRect`.
pub(super) fn column_rect(p: &RectParams, count: u8, index: u8) -> RectResult {
    if p.config.subsequent_execution_mode != SubsequentExecutionMode::None {
        if let Some(last) = p.last {
            if last.action == p.action && window_is_still_where_we_left_it(last, p) {
                if let Some(current) = current_column_index(p, count) {
                    let next = current % count + 1;
                    return RectResult::new(columns::rect(&p.visible, count, next));
                }
            }
        }
    }
    RectResult::new(columns::rect(&p.visible, count, index))
}

/// Окно не двигали после нашего последнего действия: допуск 2 px, как в конвейере.
/// Рамка из истории — в координатах AX, её переворачиваем в Cocoa.
fn window_is_still_where_we_left_it(last: &LastAction, p: &RectParams) -> bool {
    last.rect
        .screen_flipped(p.primary_max_y)
        .is_close(&p.window, 2.0)
}

/// В каком столбике раскладки стоит окно (`None` — ни в одном).
fn current_column_index(p: &RectParams, count: u8) -> Option<u8> {
    let candidates: Vec<Rect> = (1..=count)
        .map(|candidate| on_screen_rect(p, count, candidate))
        .collect();
    columns::best_match_index(&p.window, &candidates, 6.0, 12.0)
}

/// Столбик ровно таким, каким его увидит пользователь, — с гэпами, как их накладывает
/// конвейер.
fn on_screen_rect(p: &RectParams, count: u8, index: u8) -> Rect {
    let rect = columns::rect(&p.visible, count, index);
    match Action::column_action(count, index) {
        Some(action) => apply_gaps(rect, action, None, p.config),
        None => rect,
    }
}
