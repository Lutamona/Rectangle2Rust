//! Доступ к управлению компьютером (Accessibility) — порт
//! `AccessibilityAuthorization/AccessibilityAuthorization.swift`.
//!
//! Без доступа при запуске: системный запрос (он же добавляет приложение в
//! список «Универсальный доступ» — иначе его пришлось бы добавлять кнопкой «+»),
//! окно-инструкция «Разрешить Rectangle» и опрос `AXIsProcessTrusted`: первые
//! ~10 секунд часто (доступ обычно выдают сразу), дальше реже. Как только доступ
//! появился — окно закрывается и вызывается колбэк. Окно-инструкция —
//! `ui::authorize`.

use std::time::Duration;

use objc2_app_kit::NSWorkspace;
use objc2_foundation::{NSString, NSURL};

use crate::{ax, events, log, ui};

/// Страница «Конфиденциальность и безопасность → Универсальный доступ».
const ACCESSIBILITY_SETTINGS_URL: &str =
    "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility";

/// Сколько первых проверок идут часто.
const FAST_POLL_ATTEMPTS: u32 = 32;

/// Доступ к управлению компьютером есть (`AXIsProcessTrusted`).
pub fn is_trusted() -> bool {
    ax::is_process_trusted(false)
}

/// `checkAccessibility(completion:)`. Доступ есть — `true`, колбэк не
/// вызывается. Нет — системный запрос, окно-инструкция и опрос; `on_granted`
/// вызывается на главном потоке, когда доступ появится, — из таймера цикла
/// событий (`events::run_in_run_loop`): он показывает модальные алерты.
pub fn check(on_granted: impl FnOnce() + 'static) -> bool {
    if is_trusted() {
        return true;
    }
    // Системный запрос — при каждом запуске без доступа, как в оригинале.
    ax::is_process_trusted(true);
    // Окно «Разрешить Rectangle»: приложение выходит вперёд, окно показывается.
    // Красная кнопка окна завершает приложение (`exit(1)`).
    ui::authorize::show();
    log!("Нет доступа к управлению компьютером: ждём разрешения");
    poll(Box::new(on_granted), 0);
    false
}

/// Пауза перед проверкой номер `attempt` (с нуля): 0,3 с первые 32 раза, потом 2 с.
pub fn poll_delay(attempt: u32) -> Duration {
    if attempt < FAST_POLL_ATTEMPTS {
        Duration::from_millis(300)
    } else {
        Duration::from_secs(2)
    }
}

fn poll(on_granted: Box<dyn FnOnce()>, attempt: u32) {
    events::run_after(poll_delay(attempt), move || {
        if is_trusted() {
            ui::authorize::close();
            log!("Доступ к управлению компьютером получен");
            // Опрос — блок главной очереди, а колбэк показывает алерты: из
            // такого блока вложенный цикл модального окна очередь не обслуживает.
            events::run_in_run_loop(on_granted);
        } else {
            poll(on_granted, attempt + 1);
        }
    });
}

/// Пункт «Авторизовать…» меню без доступа (`showAuthorizationWindow`):
/// развернуть окно «Разрешить Rectangle», если оно свёрнуто, и вывести
/// приложение вперёд.
pub fn show_authorization_window() {
    ui::authorize::show();
}

/// Открыть «Системные настройки → Конфиденциальность и безопасность →
/// Универсальный доступ» (кнопка окна «Разрешить Rectangle»).
pub fn open_accessibility_settings() {
    if let Some(url) = NSURL::URLWithString(&NSString::from_str(ACCESSIBILITY_SETTINGS_URL)) {
        NSWorkspace::sharedWorkspace().openURL(&url);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn polls_often_first_then_rarely() {
        assert_eq!(poll_delay(0), Duration::from_millis(300));
        assert_eq!(poll_delay(31), Duration::from_millis(300));
        assert_eq!(poll_delay(32), Duration::from_secs(2));
        assert_eq!(poll_delay(1000), Duration::from_secs(2));
        // Частая фаза — около 10 секунд, как в оригинале.
        let fast: Duration = (0..FAST_POLL_ATTEMPTS).map(poll_delay).sum();
        assert!(fast >= Duration::from_secs(9) && fast <= Duration::from_secs(10));
    }
}
