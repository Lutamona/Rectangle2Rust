//! Rectangle 2 (Rust): библиотека приложения.
//!
//! Модули разделены по слоям: чистая геометрия и расчёты (`geometry`, `calc`,
//! `screen_detection`, `screen_calculation`, `cooperative_resize`), настройки
//! (`config`, `defaults_store`, `json`), система (`ax`, `ax_observer`, `screens`, `stage`),
//! исполнение действий (`window_manager`, `window_history`, `movers`, `multi_window`,
//! `cooperative_resize_manager`, `overlap_offset`), прилипание при перетаскивании
//! (`snapping`, `event_monitor`, `mac_tiling`), двойной клик по заголовку и зелёная
//! кнопка окна (`title_bar`, `green_button`), интерфейс (`menu`, `stack_badge`) и связка
//! (`app`, `app_columns`). Горячих клавиш нет — только мышь.

pub mod accessibility;
pub mod actions;
pub mod app;
pub mod app_columns;
pub mod app_delegate;
pub mod ax;
pub mod ax_observer;
pub mod calc;
pub mod config;
pub mod cooperative_resize;
pub mod cooperative_resize_manager;
pub mod defaults_store;
pub mod event_monitor;
pub mod events;
pub mod geometry;
pub mod green_button;
pub mod json;
pub mod launch_on_login;
pub mod localization;
pub mod logging;
pub mod mac_tiling;
pub mod menu;
pub mod movers;
pub mod multi_window;
pub mod overlap_offset;
pub mod screen_calculation;
pub mod screen_detection;
pub mod screens;
pub mod side_split_ratios;
pub mod snapping;
pub mod stack_badge;
pub mod stage;
pub mod subsystems;
pub mod support_config;
pub mod title_bar;
pub mod todo;
pub mod ui;
pub mod url_scheme;
pub mod window_history;
pub mod window_manager;
