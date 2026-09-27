//! Окна интерфейса — кодом на AppKit по разметке `Main.storyboard` оригинала
//! (спецификация — `docs/ui-spec.md`).
//!
//! Сигнатуры `show()`/`close()` договорные: их зовут меню и делегат приложения,
//! а наполняют модули этапов 4C (настройки), 4D (области прилипания), 4E
//! (доступ, журнал) и 5B (окно «О Todo режиме»).

pub mod about;
pub mod about_todo;
pub mod authorize;
pub mod controls;
pub mod log_viewer;
pub mod prefs;
