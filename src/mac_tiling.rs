//! Встроенное прилипание окон macOS 15+ — порт `Utilities/MacTilingDefaults.swift`.
//!
//! В macOS Sequoia появилось своё прилипание окон к краям экрана (Системные
//! настройки → Рабочий стол и Dock → Окна). Если оно включено вместе с
//! прилипанием порта, окна дёргают обе программы — об этом предупреждает алерт.
//!
//! Системную настройку порт меняет только по кнопке в алерте. Оригинал при
//! конфликте у верхнего края выключал её в macOS сам, без вопроса; здесь
//! вместо этого спрашивается (и, как у оригинала, после первого показа —
//! только при включении прилипания в настройках).

use objc2::rc::Retained;
use objc2::{AnyThread, MainThreadMarker};
use objc2_app_kit::{NSAlert, NSAlertStyle, NSApplication, NSWorkspace};
use objc2_foundation::{NSProcessInfo, NSString, NSUserDefaults, NSURL};

use crate::app_delegate::app_name;
use crate::config;
use crate::log;
use crate::screens;
use crate::snapping::area_model;

// ---------------------------------------------------------------- версия macOS

/// Версия macOS `(старшая, младшая)` (`ProcessInfo.operatingSystemVersion`).
pub fn macos_version() -> (isize, isize) {
    let version = NSProcessInfo::processInfo().operatingSystemVersion();
    (version.majorVersion, version.minorVersion)
}

/// `#available(macOS major.minor, *)`.
pub fn macos_at_least(major: isize, minor: isize) -> bool {
    macos_version() >= (major, minor)
}

// ---------------------------------------------------------------- настройки macOS

/// Ключи `com.apple.WindowManager` (`MacTilingDefaults`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MacTilingDefault {
    /// Перетаскивание окна к краю экрана.
    TilingByEdgeDrag,
    /// Перетаскивание с ⌥.
    TilingOptionAccelerator,
    /// Поля между окнами.
    TiledWindowMargins,
    /// Перетаскивание к верхнему краю разворачивает окно (macOS 15.1+).
    TopTilingByEdgeDrag,
}

impl MacTilingDefault {
    pub fn key(self) -> &'static str {
        match self {
            MacTilingDefault::TilingByEdgeDrag => "EnableTilingByEdgeDrag",
            MacTilingDefault::TilingOptionAccelerator => "EnableTilingOptionAccelerator",
            MacTilingDefault::TiledWindowMargins => "EnableTiledWindowMargins",
            MacTilingDefault::TopTilingByEdgeDrag => "EnableTopTilingByEdgeDrag",
        }
    }

    /// Включено ли в macOS: ключа нет — включено (так по умолчанию); до
    /// macOS 15 — нет.
    pub fn enabled(self) -> bool {
        if !macos_at_least(15, 0) {
            return false;
        }
        let Some(defaults) = window_manager_defaults() else {
            return false;
        };
        let key = NSString::from_str(self.key());
        if defaults.objectForKey(&key).is_none() {
            return true;
        }
        defaults.boolForKey(&key)
    }

    /// Выключить в macOS. Только по кнопке пользователя в алерте.
    fn disable(self) {
        let Some(defaults) = window_manager_defaults() else {
            return;
        };
        defaults.setBool_forKey(false, &NSString::from_str(self.key()));
        defaults.synchronize();
    }
}

fn window_manager_defaults() -> Option<Retained<NSUserDefaults>> {
    NSUserDefaults::initWithSuiteName(
        NSUserDefaults::alloc(),
        Some(&NSString::from_str("com.apple.WindowManager")),
    )
}

/// Открыть Системные настройки → Рабочий стол и Dock.
pub fn open_system_settings() {
    if let Some(url) = NSURL::URLWithString(&NSString::from_str(
        "x-apple.systempreferences:com.apple.preference.Desktop-Settings.extension",
    )) {
        NSWorkspace::sharedWorkspace().openURL(&url);
    }
}

// ---------------------------------------------------------------- алерты

/// Алерт-предупреждение с кнопками по порядку (`AlertUtil`); возвращает номер
/// нажатой кнопки с нуля. Приложение перед показом выводится вперёд, иначе
/// алерт может оказаться под чужими окнами. Звать не из блока главной очереди
/// GCD, а, например, из `events::run_in_run_loop`: иначе, пока алерт на экране,
/// очередь стоит.
pub(crate) fn run_alert(
    mtm: MainThreadMarker,
    message: &str,
    text: &str,
    buttons: &[&str],
) -> usize {
    let alert = NSAlert::new(mtm);
    alert.setAlertStyle(NSAlertStyle::Warning);
    alert.setMessageText(&NSString::from_str(message));
    alert.setInformativeText(&NSString::from_str(text));
    for title in buttons {
        alert.addButtonWithTitle(&NSString::from_str(title));
    }
    #[allow(deprecated)]
    NSApplication::sharedApplication(mtm).activateIgnoringOtherApps(true);
    // NSAlertFirstButtonReturn = 1000, дальше по порядку.
    usize::try_from(alert.runModal() - 1000).unwrap_or(usize::MAX)
}

/// Алерт «… выключено» с кнопкой «Открыть Системные настройки».
fn tell_and_offer_settings(mtm: MainThreadMarker, message: &str, text: &str) {
    if run_alert(mtm, message, text, &["OK", "Открыть Системные настройки"]) == 1
    {
        open_system_settings();
    }
}

const ADJUST_IN_SETTINGS: &str =
    "Настроить прилипание окон в macOS можно в Системных настройках → Рабочий стол и Dock → Окна.";

// ---------------------------------------------------------------- проверка

/// Что известно о системе и настройках для проверки конфликта.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct TilingState {
    pub macos_15: bool,
    pub macos_15_1: bool,
    pub edge_drag: bool,
    pub option_accelerator: bool,
    pub top_edge_drag: bool,
    /// В порте у верхнего края настроена область (`isTopConfigured`).
    pub top_configured: bool,
}

/// Что спросить у пользователя.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TilingConflict {
    /// Конфликта нет (или о нём уже спрашивали).
    Nothing,
    /// Прилипание к краям включено и в macOS (`isStandardTilingConflicting`).
    Standard,
    /// Только у верхнего края (`isTopTilingConflicting`).
    Top,
}

/// Решение `checkForBuiltInTiling`: `None` — проверять нечего (macOS до 15 или
/// прилипание в порте выключено), отметку «уже сообщали» тогда не ставим.
pub(crate) fn tiling_conflict(
    state: &TilingState,
    window_snapping: Option<bool>,
    skip_if_already_notified: bool,
    already_notified: bool,
) -> Option<TilingConflict> {
    if !state.macos_15 || window_snapping == Some(false) {
        return None;
    }
    let skip = skip_if_already_notified && already_notified;
    if (state.edge_drag || state.option_accelerator) && !skip {
        return Some(TilingConflict::Standard);
    }
    if state.macos_15_1 && state.top_edge_drag && state.top_configured && !skip {
        return Some(TilingConflict::Top);
    }
    Some(TilingConflict::Nothing)
}

fn current_state() -> TilingState {
    let config = config::current();
    let portrait = area_model::portrait_display_connected(&screens::screens());
    TilingState {
        macos_15: macos_at_least(15, 0),
        macos_15_1: macos_at_least(15, 1),
        edge_drag: MacTilingDefault::TilingByEdgeDrag.enabled(),
        option_accelerator: MacTilingDefault::TilingOptionAccelerator.enabled(),
        top_edge_drag: MacTilingDefault::TopTilingByEdgeDrag.enabled(),
        top_configured: area_model::is_top_configured(&config, portrait),
    }
}

/// Проверить конфликт с прилипанием macOS (`checkForBuiltInTiling`): при
/// запуске — `true` (только если ещё не сообщали), при включении прилипания в
/// настройках — `false` (всегда).
pub fn check_for_built_in_tiling(skip_if_already_notified: bool) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let (window_snapping, already_notified) =
        config::with(|config| (config.window_snapping, config.internal_tiling_notified));
    let Some(conflict) = tiling_conflict(
        &current_state(),
        window_snapping,
        skip_if_already_notified,
        already_notified,
    ) else {
        return;
    };
    match conflict {
        TilingConflict::Standard => resolve_standard_conflict(mtm),
        TilingConflict::Top => resolve_top_conflict(mtm),
        TilingConflict::Nothing => {}
    }
    config::update(|config| config.internal_tiling_notified = true);
}

/// `resolveStandardTilingConflict`: выключить в macOS, выключить у себя или закрыть.
fn resolve_standard_conflict(mtm: MainThreadMarker) {
    let app = app_name();
    let answer = run_alert(
        mtm,
        "Конфликт с прилипанием окон в macOS",
        &format!("Прилипание окон при перетаскивании к краю экрана включено и в {app}, и в macOS."),
        &[
            "Выключить в macOS",
            &format!("Выключить в {app}"),
            "Закрыть",
        ],
    );
    match answer {
        0 => {
            for setting in [
                MacTilingDefault::TilingByEdgeDrag,
                MacTilingDefault::TilingOptionAccelerator,
                MacTilingDefault::TopTilingByEdgeDrag,
            ] {
                setting.disable();
            }
            log!("Прилипание окон в macOS выключено по кнопке в алерте");
            tell_and_offer_settings(
                mtm,
                "Прилипание окон в macOS выключено",
                "Чтобы включить его снова, откройте Системные настройки → Рабочий стол и Dock → Окна.",
            );
        }
        1 => {
            config::update(|config| config.window_snapping = Some(false));
            tell_and_offer_settings(
                mtm,
                &format!("Прилипание окон в {app} выключено"),
                ADJUST_IN_SETTINGS,
            );
        }
        _ => {}
    }
}

/// Конфликт у верхнего края (`resolveTopTilingConflict`): оригинал выключал
/// эту настройку macOS сам; здесь — только если пользователь согласился.
fn resolve_top_conflict(mtm: MainThreadMarker) {
    let app = app_name();
    let answer = run_alert(
        mtm,
        "Конфликт с прилипанием к верхнему краю в macOS",
        &format!(
            "В macOS окно разворачивается, если перетащить его к верхнему краю экрана, \
             а в {app} у верхнего края своя область прилипания. Выключить это в macOS?"
        ),
        &["Выключить в macOS", "Оставить как есть"],
    );
    if answer == 0 {
        MacTilingDefault::TopTilingByEdgeDrag.disable();
        log!("Прилипание к верхнему краю в macOS выключено по кнопке в алерте");
        tell_and_offer_settings(
            mtm,
            "Прилипание к верхнему краю экрана в macOS выключено",
            ADJUST_IN_SETTINGS,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sequoia() -> TilingState {
        TilingState {
            macos_15: true,
            macos_15_1: true,
            edge_drag: false,
            option_accelerator: false,
            top_edge_drag: false,
            top_configured: true,
        }
    }

    #[test]
    fn nothing_to_check_before_sequoia_or_with_snapping_off() {
        let old = TilingState {
            edge_drag: true,
            ..TilingState::default()
        };
        assert_eq!(tiling_conflict(&old, None, false, false), None);
        let state = TilingState {
            edge_drag: true,
            ..sequoia()
        };
        assert_eq!(tiling_conflict(&state, Some(false), false, false), None);
    }

    #[test]
    fn standard_conflict_is_asked_once_at_launch_and_always_from_settings() {
        for state in [
            TilingState {
                edge_drag: true,
                ..sequoia()
            },
            TilingState {
                option_accelerator: true,
                ..sequoia()
            },
        ] {
            assert_eq!(
                tiling_conflict(&state, None, true, false),
                Some(TilingConflict::Standard)
            );
            // При запуске после первого раза — молчим.
            assert_eq!(
                tiling_conflict(&state, None, true, true),
                Some(TilingConflict::Nothing)
            );
            // Включили прилипание в настройках — спрашиваем снова.
            assert_eq!(
                tiling_conflict(&state, Some(true), false, true),
                Some(TilingConflict::Standard)
            );
        }
    }

    #[test]
    fn top_conflict_needs_15_1_and_a_top_area() {
        let top = TilingState {
            top_edge_drag: true,
            ..sequoia()
        };
        assert_eq!(
            tiling_conflict(&top, None, true, false),
            Some(TilingConflict::Top)
        );
        assert_eq!(
            tiling_conflict(&top, None, true, true),
            Some(TilingConflict::Nothing)
        );
        assert_eq!(
            tiling_conflict(&top, None, false, true),
            Some(TilingConflict::Top)
        );
        let no_area = TilingState {
            top_configured: false,
            ..top
        };
        assert_eq!(
            tiling_conflict(&no_area, None, true, false),
            Some(TilingConflict::Nothing)
        );
        let fifteen_zero = TilingState {
            macos_15_1: false,
            ..top
        };
        assert_eq!(
            tiling_conflict(&fifteen_zero, None, true, false),
            Some(TilingConflict::Nothing)
        );
        // Общий конфликт важнее верхнего.
        let both = TilingState {
            edge_drag: true,
            ..top
        };
        assert_eq!(
            tiling_conflict(&both, None, true, false),
            Some(TilingConflict::Standard)
        );
    }

    #[test]
    fn keys_match_the_system() {
        assert_eq!(
            MacTilingDefault::TilingByEdgeDrag.key(),
            "EnableTilingByEdgeDrag"
        );
        assert_eq!(
            MacTilingDefault::TilingOptionAccelerator.key(),
            "EnableTilingOptionAccelerator"
        );
        assert_eq!(
            MacTilingDefault::TiledWindowMargins.key(),
            "EnableTiledWindowMargins"
        );
        assert_eq!(
            MacTilingDefault::TopTilingByEdgeDrag.key(),
            "EnableTopTilingByEdgeDrag"
        );
    }

    #[test]
    fn version_is_known() {
        let (major, _) = macos_version();
        assert!(major >= 11);
        assert!(macos_at_least(11, 0));
        assert!(!macos_at_least(major + 1, 0));
    }
}
