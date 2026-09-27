//! Подсистемы, которым нужен доступ к управлению компьютером, —
//! `AppDelegate.accessibilityTrusted()` оригинала.
//!
//! `start_all` вызывается один раз, когда доступ есть (сразу при запуске или
//! когда его выдали), `reload_all` — при каждой смене настроек после запуска.
//! Каждый модуль (игнор приложений, drag-to-snap, todo, stack badge,
//! заголовок окна, зелёная кнопка, …) добавляет по строке в `STARTS` и, если
//! ему нужно, в `RELOADS`. `reload_all` зовётся на любую смену настроек:
//! перезагрузка модуля должна быть дешёвой, а что именно изменилось, модуль
//! решает сам (или подписывается на `events::on_config_changed`).

use std::cell::Cell;

use objc2::MainThreadMarker;

/// Запуск модулей — в порядке `accessibilityTrusted()`: по строке на модуль,
/// например `crate::snapping::install,`.
const STARTS: &[fn(MainThreadMarker)] = &[
    crate::window_manager::install,
    start_app_columns,
    crate::snapping::install,
    crate::stack_badge::install,
    crate::title_bar::install,
    crate::green_button::install,
    // Последним, как `initializeTodo()` в `accessibilityTrusted()`.
    crate::todo::install,
];

/// «Окна приложения столбиками»: наблюдение за приложениями из списка
/// «держать столбиками» (нужен доступ к управлению компьютером — AXObserver).
fn start_app_columns(_mtm: MainThreadMarker) {
    crate::app_columns::install();
}

/// Перезагрузка модулей при смене настроек, например `crate::snapping::reload,`.
const RELOADS: &[fn()] = &[
    crate::snapping::reload,
    crate::stack_badge::reload,
    crate::title_bar::reload,
    crate::green_button::reload,
    crate::todo::reload,
];

thread_local! {
    static STARTED: Cell<bool> = const { Cell::new(false) };
}

/// Подсистемы запущены.
pub fn is_started() -> bool {
    STARTED.with(Cell::get)
}

/// Запустить подсистемы. Повторный вызов ничего не делает.
pub fn start_all(mtm: MainThreadMarker) {
    if STARTED.with(|started| started.replace(true)) {
        return;
    }
    for start in STARTS {
        start(mtm);
    }
}

/// Настройки изменились — перечитать их в запущенных подсистемах.
pub fn reload_all() {
    if !is_started() {
        return;
    }
    for reload in RELOADS {
        reload();
    }
}
