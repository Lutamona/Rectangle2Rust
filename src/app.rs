//! Связка интерфейса с менеджером окон: пункт меню выполняет действие над
//! активным окном, меню узнаёт, какое приложение впереди. Само исполнение —
//! `window_manager`.

use crate::actions::Action;
use crate::ax;
use crate::window_manager::{self, ExecutionParameters};

pub use crate::window_manager::on_frontmost_app_changed;

/// Выполнить действие над активным окном (пункт меню, `postMenu`). Как в
/// оригинале, меню работает и для приложений из списка «Игнорировать».
pub fn execute(action: Action) {
    window_manager::execute(ExecutionParameters::menu(action));
}

/// pid приложения по bundle id (для диагностики и скриншотов).
pub fn pid_for_bundle(bundle_id: &str) -> Option<i32> {
    ax::pid_for_bundle(bundle_id)
}

/// Имя приложения, которое сейчас в фокусе (для пункта меню «Игнорировать …»).
pub fn frontmost_app_name() -> String {
    ax::frontmost_app_name()
}

/// Bundle id приложения в фокусе.
pub fn frontmost_bundle_id() -> Option<String> {
    ax::frontmost_bundle_id()
}
