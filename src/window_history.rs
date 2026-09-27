//! История действий над окнами — порт `WindowHistory.swift` и
//! `WindowManager.recordAction`.
//!
//! - `restore_rects` — рамка, которую окну последним дал пользователь
//!   (для «Восстановить» и возврата размера при отрыве в drag-to-snap);
//! - `last_actions` — что с окном последним сделал Rectangle и в какую рамку его
//!   поставил (для повторных нажатий, «Восстановить» по двойному клику и т.п.).
//!
//! Рамки — в координатах AX, как их отдаёт окно. Ключ — номер окна
//! (`AxElement::get_window_id`). Живёт на главном потоке, как `AppDelegate.windowHistory`.
//!
//! Номера окон в WindowServer только растут, и записи закрытых окон в Swift
//! копятся всё время работы. Здесь, когда записей становится больше
//! `PRUNE_THRESHOLD`, история сверяется со списком окон сеанса и забывает окна,
//! которых уже нет.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use crate::actions::{Action, SubAction};
use crate::ax;
use crate::geometry::Rect;

/// Последнее действие над окном (`RectangleAction`).
pub use crate::calc::LastAction;

/// Сколько записей держать без сверки со списком окон.
pub const PRUNE_THRESHOLD: usize = 500;

#[derive(Debug, Default)]
pub struct WindowHistory {
    /// Рамка, которую окну последним дал пользователь.
    pub restore_rects: HashMap<u32, Rect>,
    /// Последнее действие Rectangle над окном и рамка после него.
    pub last_actions: HashMap<u32, LastAction>,
    /// При скольких записях сверяться со списком окон в следующий раз; меньше
    /// `PRUNE_THRESHOLD` — значит, `PRUNE_THRESHOLD`.
    prune_at: usize,
}

impl WindowHistory {
    /// Сколько всего записей (рамки для «Восстановить» и последние действия).
    pub fn len(&self) -> usize {
        self.restore_rects.len() + self.last_actions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Записей больше порога — забыть окна, которых уже нет. `existing` отдаёт
    /// номера всех окон сеанса (`None` — список не получить: тогда ничего не
    /// трогаем) и зовётся, только если порог превышен. Производные номера (#640)
    /// в списке окон не бывают — их записи остаются. Следующая сверка — когда
    /// записей станет вдвое больше, чем осталось (но не меньше порога): если все
    /// окна живы, список не запрашивается на каждой новой записи. Возвращает,
    /// была ли сверка.
    pub fn prune_if_needed(&mut self, existing: impl FnOnce() -> Option<HashSet<u32>>) -> bool {
        if self.len() <= self.prune_at.max(PRUNE_THRESHOLD) {
            return false;
        }
        if let Some(existing) = existing() {
            let keep = |id: &u32| existing.contains(id) || ax::is_derived_window_id(*id);
            self.restore_rects.retain(|id, _| keep(id));
            self.last_actions.retain(|id, _| keep(id));
        }
        self.prune_at = self.len() * 2;
        true
    }

    /// `recordAction`: то же действие, что в прошлый раз, — счётчик +1 (или
    /// прежний, если `increment_count == false`), другое — 1.
    pub fn record_action(
        &mut self,
        window_id: u32,
        rect: Rect,
        action: Action,
        sub_action: Option<SubAction>,
        increment_count: bool,
    ) {
        let count = match self.last_actions.get(&window_id) {
            Some(last) if last.action == action => {
                if increment_count {
                    last.count + 1
                } else {
                    last.count
                }
            }
            _ => 1,
        };
        self.last_actions.insert(
            window_id,
            LastAction {
                action,
                sub_action,
                rect,
                count,
            },
        );
    }
}

thread_local! {
    static HISTORY: RefCell<WindowHistory> = RefCell::new(WindowHistory::default());
}

/// Доступ к истории приложения (главный поток). После каждого доступа —
/// сверка со списком окон, если записей стало слишком много.
pub fn with<R>(f: impl FnOnce(&mut WindowHistory) -> R) -> R {
    HISTORY.with(|history| {
        let mut history = history.borrow_mut();
        let result = f(&mut history);
        history.prune_if_needed(ax::existing_window_ids);
        result
    })
}

/// Последнее действие над окном.
pub fn last_action(window_id: u32) -> Option<LastAction> {
    with(|history| history.last_actions.get(&window_id).copied())
}

/// Забыть последнее действие над окном: следующее нажатие начнёт цикл заново.
pub fn remove_last_action(window_id: u32) {
    with(|history| {
        history.last_actions.remove(&window_id);
    });
}

/// Рамка для «Восстановить».
pub fn restore_rect(window_id: u32) -> Option<Rect> {
    with(|history| history.restore_rects.get(&window_id).copied())
}

/// Запомнить рамку для «Восстановить».
pub fn set_restore_rect(window_id: u32, rect: Rect) {
    with(|history| {
        history.restore_rects.insert(window_id, rect);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_action_increments_count() {
        let mut history = WindowHistory::default();
        let rect = Rect::new(0.0, 25.0, 864.0, 1001.0);
        history.record_action(7, rect, Action::LeftHalf, None, true);
        assert_eq!(history.last_actions[&7].count, 1);
        history.record_action(7, rect, Action::LeftHalf, None, true);
        assert_eq!(history.last_actions[&7].count, 2);
        // Без увеличения — счётчик прежний (согласованный ресайз соседей).
        history.record_action(7, rect, Action::LeftHalf, None, false);
        assert_eq!(history.last_actions[&7].count, 2);
        // Другое действие — снова 1.
        history.record_action(7, rect, Action::RightHalf, None, true);
        assert_eq!(history.last_actions[&7].count, 1);
        assert_eq!(history.last_actions[&7].action, Action::RightHalf);
    }

    #[test]
    fn windows_are_tracked_separately() {
        let mut history = WindowHistory::default();
        history.record_action(
            1,
            Rect::new(0.0, 0.0, 10.0, 10.0),
            Action::Maximize,
            None,
            true,
        );
        history.record_action(
            2,
            Rect::new(0.0, 0.0, 10.0, 10.0),
            Action::Maximize,
            None,
            true,
        );
        history.record_action(
            2,
            Rect::new(0.0, 0.0, 10.0, 10.0),
            Action::Maximize,
            None,
            true,
        );
        assert_eq!(history.last_actions[&1].count, 1);
        assert_eq!(history.last_actions[&2].count, 2);
    }

    #[test]
    fn global_history_round_trip() {
        let rect = Rect::new(10.0, 20.0, 300.0, 200.0);
        set_restore_rect(42, rect);
        assert_eq!(restore_rect(42), Some(rect));
        with(|history| history.record_action(42, rect, Action::Center, None, true));
        assert_eq!(
            last_action(42).map(|last| last.action),
            Some(Action::Center)
        );
        remove_last_action(42);
        assert!(last_action(42).is_none());
        // «Восстановить» остаётся: удаляется только последнее действие.
        assert_eq!(restore_rect(42), Some(rect));
    }

    /// История с рамками «Восстановить» у окон `ids` и последним действием у
    /// каждого второго.
    fn history_of(ids: impl IntoIterator<Item = u32>) -> WindowHistory {
        let mut history = WindowHistory::default();
        let rect = Rect::new(0.0, 25.0, 864.0, 1001.0);
        for id in ids {
            history.restore_rects.insert(id, rect);
            if id % 2 == 0 {
                history.record_action(id, rect, Action::LeftHalf, None, true);
            }
        }
        history
    }

    #[test]
    fn small_history_is_not_checked_against_window_list() {
        let mut history = history_of(1..=300);
        assert_eq!(history.len(), 450);
        let checked = history.prune_if_needed(|| panic!("список окон не нужен"));
        assert!(!checked);
        assert_eq!(history.len(), 450);
    }

    #[test]
    fn closed_windows_are_forgotten_past_the_threshold() {
        let derived = ax::derive_window_id(0x1234);
        let mut history = history_of((1..=400).chain([derived]));
        assert!(history.len() > PRUNE_THRESHOLD);

        // Живы окна 1…50: остальные настоящие номера забываются, производный — нет.
        let checked = history.prune_if_needed(|| Some((1..=50).collect()));
        assert!(checked);
        let mut kept: Vec<u32> = history.restore_rects.keys().copied().collect();
        kept.sort_unstable();
        assert_eq!(kept, (1..=50).chain([derived]).collect::<Vec<_>>());
        // Последние действия — у чётных: 25 живых и производный.
        assert_eq!(history.last_actions.len(), 26);
        assert!(history
            .last_actions
            .keys()
            .all(|&id| id <= 50 || id == derived));
    }

    #[test]
    fn live_windows_postpone_the_next_check() {
        let mut history = history_of(1..=400);
        let alive: HashSet<u32> = (1..=2000).collect();
        assert!(history.prune_if_needed(|| Some(alive.clone())));
        assert_eq!(history.len(), 600);

        // Все живы: следующая сверка — только при 1200 записях.
        let rect = Rect::new(0.0, 0.0, 10.0, 10.0);
        for id in 401..=1000 {
            history.restore_rects.insert(id, rect);
        }
        assert_eq!(history.len(), 1200);
        assert!(!history.prune_if_needed(|| panic!("рано")));
        history.restore_rects.insert(1001, rect);
        assert!(history.prune_if_needed(|| Some((1..=10).collect())));
        assert_eq!(history.restore_rects.len(), 10);
        assert_eq!(history.last_actions.len(), 5);

        // Осталось мало — порог снова обычный.
        for id in 2001..=2500 {
            history.restore_rects.insert(id, rect);
        }
        assert!(history.prune_if_needed(|| Some(HashSet::new())));
    }

    #[test]
    fn unknown_window_list_keeps_everything() {
        let mut history = history_of(1..=400);
        assert!(history.prune_if_needed(|| None));
        assert_eq!(history.len(), 600);
        // И не спрашивает список снова на каждой записи.
        history
            .restore_rects
            .insert(401, Rect::new(0.0, 0.0, 1.0, 1.0));
        assert!(!history.prune_if_needed(|| panic!("рано")));
    }

    #[test]
    fn global_history_forgets_windows_that_no_longer_exist() {
        let Some(existing) = ax::existing_window_ids() else {
            return;
        };
        // Номера, которых у WindowServer нет: настоящие номера малы.
        let gone: Vec<u32> = (0x7F10_0000..)
            .filter(|id| !existing.contains(id))
            .take(PRUNE_THRESHOLD + 1)
            .collect();
        let rect = Rect::new(0.0, 0.0, 10.0, 10.0);
        for &id in &gone {
            set_restore_rect(id, rect);
        }
        assert!(with(|history| gone
            .iter()
            .all(|id| !history.restore_rects.contains_key(id))));
    }
}
