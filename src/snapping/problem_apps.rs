//! Приложения, которые плохо уживаются с прилипанием при перетаскивании, —
//! `AppDelegate.checkForProblematicApps` оригинала.
//!
//! Слежение за кликами мешает некоторым программам (MATLAB, Illustrator, …,
//! Java-программы на install4j). Если такие установлены и не в «Игнорировать»,
//! при запуске один раз показывается предупреждение (`notifiedOfProblemApps`).

use objc2::MainThreadMarker;
use objc2_app_kit::NSWorkspace;
use objc2_foundation::{NSBundle, NSString};

use crate::app_delegate::app_name;
use crate::config::{self, Config};
use crate::mac_tiling::run_alert;

/// Проблемные приложения по bundle id.
pub(crate) const PROBLEM_BUNDLE_IDS: [&str; 5] = [
    "com.mathworks.matlab",
    "com.live2d.cubism.CECubismEditorApp",
    "com.aquafold.datastudio.DataStudio",
    "com.adobe.illustrator",
    "com.adobe.AfterEffects",
];

/// Java-программы с меняющимся bundle id — ищутся по имени.
pub(crate) const PROBLEM_JAVA_APP_NAMES: [&str; 2] = ["thinkorswim", "Trader Workstation"];

/// Начало bundle id у Java-программ на install4j.
const INSTALL4J_PREFIX: &str = "com.install4j";

/// Установленное приложение: bundle id и `CFBundleName`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct InstalledApp {
    pub bundle_id: Option<String>,
    pub name: Option<String>,
}

/// Какие проблемные приложения назвать. `None` — не проверять: прилипание
/// выключено или предупреждение уже показывали. `by_bundle_id` / `by_name` —
/// найти установленное приложение.
pub(crate) fn problem_apps(
    config: &Config,
    by_bundle_id: impl Fn(&str) -> Option<InstalledApp>,
    by_name: impl Fn(&str) -> Option<InstalledApp>,
) -> Option<Vec<InstalledApp>> {
    if config.window_snapping == Some(false) || config.notified_of_problem_apps {
        return None;
    }
    let mut apps: Vec<InstalledApp> = PROBLEM_BUNDLE_IDS
        .iter()
        .filter(|bundle_id| !config.is_app_disabled(bundle_id))
        .filter_map(|bundle_id| by_bundle_id(bundle_id))
        .collect();
    for name in PROBLEM_JAVA_APP_NAMES {
        let Some(app) = by_name(name) else {
            continue;
        };
        let Some(bundle_id) = app.bundle_id.as_deref() else {
            continue;
        };
        if !config.is_app_disabled(bundle_id) && bundle_id.starts_with(INSTALL4J_PREFIX) {
            apps.push(app);
        }
    }
    Some(apps)
}

/// Текст предупреждения: имена приложений по строке.
pub(crate) fn warning_text(apps: &[InstalledApp], app: &str) -> String {
    let names: Vec<&str> = apps.iter().filter_map(|app| app.name.as_deref()).collect();
    format!(
        "{}\n\nУ этих приложений бывают сбои, когда в {app} включено прилипание окон при \
         перетаскивании к краю экрана.\n\nИх можно игнорировать через меню {app} или выключить \
         прилипание при перетаскивании в настройках {app}.",
        names.join("\n")
    )
}

fn installed(bundle: &NSBundle) -> InstalledApp {
    let name = bundle
        .objectForInfoDictionaryKey(&NSString::from_str("CFBundleName"))
        .and_then(|value| value.downcast::<NSString>().ok())
        .map(|name| name.to_string());
    InstalledApp {
        bundle_id: bundle.bundleIdentifier().map(|id| id.to_string()),
        name,
    }
}

fn installed_by_bundle_id(bundle_id: &str) -> Option<InstalledApp> {
    let url = NSWorkspace::sharedWorkspace()
        .URLForApplicationWithBundleIdentifier(&NSString::from_str(bundle_id))?;
    NSBundle::bundleWithURL(&url).map(|bundle| installed(&bundle))
}

fn installed_by_name(name: &str) -> Option<InstalledApp> {
    #[allow(deprecated)]
    let path = NSWorkspace::sharedWorkspace().fullPathForApplication(&NSString::from_str(name))?;
    NSBundle::bundleWithPath(&path).map(|bundle| installed(&bundle))
}

/// Проверить при запуске и, если есть что сказать, предупредить один раз.
pub(crate) fn check_for_problematic_apps() {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let config = config::current();
    let Some(apps) = problem_apps(&config, installed_by_bundle_id, installed_by_name) else {
        return;
    };
    if apps.is_empty() {
        return;
    }
    run_alert(
        mtm,
        "Известные проблемы с установленными приложениями",
        &warning_text(&apps, &app_name()),
        &["OK"],
    );
    config::update(|config| config.notified_of_problem_apps = true);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn app(bundle_id: &str, name: &str) -> InstalledApp {
        InstalledApp {
            bundle_id: Some(bundle_id.to_string()),
            name: Some(name.to_string()),
        }
    }

    fn lookup(installed: &[InstalledApp]) -> impl Fn(&str) -> Option<InstalledApp> + '_ {
        move |bundle_id| {
            installed
                .iter()
                .find(|app| app.bundle_id.as_deref() == Some(bundle_id))
                .cloned()
        }
    }

    #[test]
    fn installed_problem_apps_are_found_unless_ignored() {
        let installed = [
            app("com.adobe.illustrator", "Adobe Illustrator"),
            app("com.mathworks.matlab", "MATLAB"),
            app("com.apple.Safari", "Safari"),
        ];
        let java = |name: &str| match name {
            "thinkorswim" => Some(app("com.install4j.1234-5678", "thinkorswim")),
            "Trader Workstation" => Some(app("com.interactivebrokers.tws", "Trader Workstation")),
            _ => None,
        };
        let config = Config::default();
        let found = problem_apps(&config, lookup(&installed), java).unwrap();
        let names: Vec<_> = found.iter().filter_map(|app| app.name.clone()).collect();
        // Порядок — как в списке оригинала; Java-программа не на install4j не считается.
        assert_eq!(names, vec!["MATLAB", "Adobe Illustrator", "thinkorswim"]);

        let ignored = Config {
            disabled_apps: Some(BTreeSet::from([
                "com.mathworks.matlab".to_string(),
                "com.install4j.1234-5678".to_string(),
            ])),
            ..Config::default()
        };
        let found = problem_apps(&ignored, lookup(&installed), java).unwrap();
        assert_eq!(
            found,
            vec![app("com.adobe.illustrator", "Adobe Illustrator")]
        );
    }

    #[test]
    fn warning_is_shown_once_and_not_with_snapping_off() {
        let none = |_: &str| None;
        let notified = Config {
            notified_of_problem_apps: true,
            ..Config::default()
        };
        assert_eq!(problem_apps(&notified, none, none), None);
        let off = Config {
            window_snapping: Some(false),
            ..Config::default()
        };
        assert_eq!(problem_apps(&off, none, none), None);
        assert_eq!(
            problem_apps(&Config::default(), none, none),
            Some(Vec::new())
        );
    }

    #[test]
    fn warning_lists_names_line_by_line() {
        let text = warning_text(
            &[
                app("com.mathworks.matlab", "MATLAB"),
                InstalledApp {
                    bundle_id: Some("com.adobe.illustrator".to_string()),
                    name: None,
                },
                app("com.adobe.AfterEffects", "After Effects"),
            ],
            "Rectangle 2 (Rust)",
        );
        assert!(text.starts_with("MATLAB\nAfter Effects\n\n"));
        assert!(text.contains("в Rectangle 2 (Rust) включено прилипание"));
    }
}
