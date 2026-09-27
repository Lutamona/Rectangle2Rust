//! Запуск при входе в систему — `LaunchOnLogin.swift` оригинала:
//! `SMAppService.mainApp` (macOS 13+).
//!
//! Включён — служба в состоянии `.enabled`. Включение — `register()` (если
//! служба уже включена, сначала `unregister()`, как в оригинале), выключение —
//! `unregister()`; ошибки уходят в журнал. Класс `SMAppService` берётся из
//! рантайма по имени, поэтому бинарь без него запускается и на macOS 11–12:
//! там запуска при входе нет (помощника `RectangleLauncher`, которым оригинал
//! обходился до macOS 13, в порте нет).

use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject};
use objc2::{available, msg_send};
use objc2_foundation::NSError;

use crate::log;

#[link(name = "ServiceManagement", kind = "framework")]
extern "C" {}

/// `SMAppServiceStatus`: `.notRegistered` 0, `.enabled` 1, `.requiresApproval` 2,
/// `.notFound` 3.
const STATUS_ENABLED: isize = 1;

/// `SMAppService.mainApp` — служба «само приложение»; до macOS 13 — `None`.
fn main_app_service() -> Option<Retained<AnyObject>> {
    if !available!(macos = 13.0) {
        return None;
    }
    let class = AnyClass::get(c"SMAppService")?;
    // SAFETY: `+[SMAppService mainAppService]` — свойство класса без аргументов,
    // возвращает объект службы.
    unsafe { msg_send![class, mainAppService] }
}

/// `SMAppService.status`.
fn status(service: &AnyObject) -> isize {
    // SAFETY: `status` — свойство службы типа `SMAppServiceStatus` (NSInteger).
    unsafe { msg_send![service, status] }
}

/// Включён ли запуск при входе (`SMAppService.mainApp.status == .enabled`).
pub fn is_enabled() -> bool {
    main_app_service().is_some_and(|service| status(&service) == STATUS_ENABLED)
}

/// Включить или выключить запуск при входе. Не вышло — строка в журнале.
pub fn set_enabled(enabled: bool) {
    let Some(service) = main_app_service() else {
        log!("Запуск при входе недоступен: SMAppService есть только в macOS 13 и новее");
        return;
    };
    // SAFETY: `registerAndReturnError:` и `unregisterAndReturnError:`
    // возвращают BOOL и пишут ошибку в последний аргумент — это `_`.
    let result: Result<(), Retained<NSError>> = unsafe {
        if enabled {
            if status(&service) == STATUS_ENABLED {
                // Как в оригинале (`try?`): ошибка снятия не важна.
                let _: Result<(), Retained<NSError>> =
                    msg_send![&service, unregisterAndReturnError: _];
            }
            msg_send![&service, registerAndReturnError: _]
        } else {
            msg_send![&service, unregisterAndReturnError: _]
        }
    };
    if let Err(error) = result {
        log!(
            "Не удалось {} запуск при входе: {}",
            if enabled {
                "включить"
            } else {
                "выключить"
            },
            error.localizedDescription()
        );
    }
}

/// `checkLaunchOnLogin` оригинала (macOS 13+), при запуске приложения:
/// запуск при входе включён в настройках (`launch_on_login`), а служба не
/// зарегистрирована (её сняли в «Объектах входа», приложение перенесли) —
/// зарегистрировать заново.
pub fn check_at_launch(launch_on_login: bool) {
    if launch_on_login && !is_enabled() {
        set_enabled(true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_binary_is_not_a_login_item() {
        // Только чтение: тестовый бинарь — не бандл приложения, службы у него
        // нет, но класс и вызовы рантайма должны работать.
        if available!(macos = 13.0) {
            assert!(main_app_service().is_some());
        }
        assert!(!is_enabled());
    }
}
