//! Делегат приложения — жизненный цикл как у `AppDelegate.swift` оригинала,
//! без шорткатов, окна приветствия и Sparkle.
//!
//! Запуск (`applicationDidFinishLaunching`): настройки → `RectangleConfig.json`
//! из Application Support (`Defaults.loadFromSupportDir`, `support_config`) →
//! миграция `showEighthsInMenu` → `checkVersion` → шина событий → меню и иконка
//! (`hideMenubarIcon`) → запуск при входе (`checkLaunchOnLogin`) → проверка
//! доступа. С доступом запускаются подсистемы и показывается меню с
//! действиями; без него — меню «нет доступа», а когда доступ выдадут, то же
//! самое делает колбэк проверки (и открывает окно настроек, как оригинал).
//! Повторный запуск открывает настройки или меню (`relaunchOpensMenu`).

use objc2::rc::Retained;
use objc2::runtime::{NSObject, NSObjectProtocol};
use objc2::{define_class, msg_send, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSAlert, NSAlertStyle, NSApplication, NSApplicationDelegate, NSWorkspace};
use objc2_foundation::{NSArray, NSBundle, NSNotification, NSString, NSURL};

use crate::config::{self, Config};
use crate::defaults_store::{Store, UserDefaultsStore};
use crate::events::{self, Event};
use crate::{
    accessibility, launch_on_login, log, logging, menu, subsystems, support_config, ui, url_scheme,
};

/// Имя приложения, если в бандле его нет (запуск без бандла).
const DEFAULT_APP_NAME: &str = "Rectangle 2 (Rust)";

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "R2AppDelegate"]
    #[thread_kind = MainThreadOnly]
    #[ivars = ()]
    pub struct AppDelegate;

    unsafe impl NSObjectProtocol for AppDelegate {}

    unsafe impl NSApplicationDelegate for AppDelegate {
        #[unsafe(method(applicationDidFinishLaunching:))]
        fn application_did_finish_launching(&self, _notification: &NSNotification) {
            did_finish_launching(self.mtm());
        }

        #[unsafe(method(applicationWillBecomeActive:))]
        fn application_will_become_active(&self, _notification: &NSNotification) {
            events::emit(&Event::AppWillBecomeActive);
        }

        /// Повторный запуск уже запущенного приложения (двойной клик в Finder).
        #[unsafe(method(applicationShouldHandleReopen:hasVisibleWindows:))]
        fn application_should_handle_reopen(
            &self,
            _sender: &NSApplication,
            _has_visible_windows: bool,
        ) -> bool {
            // `relaunchOpensMenu` — меню, иначе настройки (как в оригинале).
            if config::with(|config| config.relaunch_opens_menu) {
                menu::open_status_menu();
            } else {
                ui::prefs::show();
            }
            true
        }

        #[unsafe(method(application:openURLs:))]
        fn application_open_urls(&self, _application: &NSApplication, urls: &NSArray<NSURL>) {
            let urls = urls
                .iter()
                .filter_map(|url| url.absoluteString())
                .map(|url| url.to_string())
                .collect();
            url_scheme::open(urls);
        }
    }
);

impl AppDelegate {
    pub fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm);
        unsafe { msg_send![this, init] }
    }
}

fn did_finish_launching(mtm: MainThreadMarker) {
    logging::init_from_env();
    load_config();
    support_config::load_at_launch(mtm);
    migrate_show_eighths_in_menu();
    let current_version = bundle_version();
    config::update(|config| check_version(config, current_version));

    events::install(mtm);
    menu::install(mtm);
    launch_on_login::check_at_launch(config::current().launch_on_login);

    let already_trusted = accessibility::check(move || access_granted(mtm));
    if already_trusted {
        accessibility_trusted(mtm);
    }
    menu::set_authorized(already_trusted);
    events::on_config_changed(config_changed);

    log!(
        "Запуск {} {} (сборка {}), доступ к управлению компьютером: {}",
        app_name(),
        info_string("CFBundleShortVersionString").unwrap_or_default(),
        bundle_version().unwrap_or_default(),
        if already_trusted {
            "есть"
        } else {
            "нет"
        }
    );
}

/// Настройки из UserDefaults (`Defaults`). Старый config.conf переносит
/// только настоящее приложение: файл принадлежит ему, а перенос его
/// переименовывает, — dev-вариант и запуск без бандла читают свои
/// UserDefaults и файл не трогают.
fn load_config() {
    if bundle_identifier().as_deref() == Some(config::APP_BUNDLE_ID) {
        config::init_from_user_defaults();
    } else {
        config::init_with_store(Box::new(UserDefaultsStore::standard()));
    }
}

/// `migrateShowEighthsInMenu`: старый ключ `showEighthsInMenu` (1 — да, 2 — нет)
/// переходит в `showAdditionalSizesInMenu`, если тот ещё не задан.
fn migrate_show_eighths_in_menu() {
    let old_value = UserDefaultsStore::standard().integer("showEighthsInMenu");
    config::update(|config| migrate_show_eighths(config, old_value));
}

fn migrate_show_eighths(config: &mut Config, old_value: i64) {
    if old_value != 0 && config.show_additional_sizes_in_menu.is_none() {
        config.show_additional_sizes_in_menu = Some(old_value == 1);
    }
}

/// `checkVersion`: номер сборки (`CFBundleVersion`) запоминается в
/// `lastVersion`, а при первом запуске (прошлого номера нет или он не число) —
/// ещё и в `installVersion`. Миграции оригинала по номеру прошлой сборки
/// (шорткаты до 46, зоны прилипания до 64, помощник автозапуска до 72)
/// привязаны к нумерации сборок Rectangle; у порта нумерация своя, а шорткатов
/// и помощника автозапуска нет.
fn check_version(config: &mut Config, current: Option<String>) {
    let first_launch = config
        .last_version
        .as_deref()
        .and_then(|version| version.parse::<i64>().ok())
        .is_none();
    if first_launch {
        config.install_version = current.clone();
    }
    config.last_version = current;
}

/// Доступ выдали, пока приложение работало (колбэк `checkAccessibility`).
fn access_granted(mtm: MainThreadMarker) {
    check_for_conflicting_apps(mtm);
    ui::prefs::show();
    menu::set_authorized(true);
    accessibility_trusted(mtm);
}

/// `accessibilityTrusted()`: запустить всё, чему нужен доступ.
fn accessibility_trusted(mtm: MainThreadMarker) {
    subsystems::start_all(mtm);
}

fn config_changed(old: &Config, new: &Config) {
    if old.hide_menu_bar_icon != new.hide_menu_bar_icon {
        menu::refresh_visibility();
    }
    subsystems::reload_all();
}

/// Менеджеры окон, которые конфликтуют с Rectangle (`checkForConflictingApps`).
const CONFLICTING_APPS: [(&str, &str); 4] = [
    ("com.divisiblebyzero.Spectacle", "Spectacle"),
    ("com.crowdcafe.windowmagnet", "Magnet"),
    ("com.hegenberg.BetterSnapTool", "BetterSnapTool"),
    ("com.manytricks.Moom", "Moom"),
];

/// Первое запущенное приложение из списка конфликтующих.
fn conflicting_app<I: IntoIterator<Item = String>>(running_bundle_ids: I) -> Option<&'static str> {
    running_bundle_ids.into_iter().find_map(|bundle_id| {
        CONFLICTING_APPS
            .iter()
            .find(|(conflicting, _)| *conflicting == bundle_id)
            .map(|(_, name)| *name)
    })
}

/// Предупредить, если запущен другой менеджер окон (только первый найденный).
fn check_for_conflicting_apps(mtm: MainThreadMarker) {
    let running = NSWorkspace::sharedWorkspace()
        .runningApplications()
        .iter()
        .filter_map(|application| application.bundleIdentifier())
        .map(|bundle_id| bundle_id.to_string())
        .collect::<Vec<_>>();
    let Some(conflicting) = conflicting_app(running) else {
        return;
    };
    let alert = NSAlert::new(mtm);
    alert.setAlertStyle(NSAlertStyle::Warning);
    alert.setMessageText(&NSString::from_str(&format!(
        "Возможен конфликт менеджеров окон: {conflicting}"
    )));
    alert.setInformativeText(&NSString::from_str(&format!(
        "{conflicting} может делать то же, что и {name}, — лучше отключить {conflicting} или выйти из него.",
        name = app_name()
    )));
    alert.addButtonWithTitle(&NSString::from_str("OK"));
    #[allow(deprecated)]
    NSApplication::sharedApplication(mtm).activateIgnoringOtherApps(true);
    alert.runModal();
}

// ---------------------------------------------------------------- бандл

/// Строковое значение из Info.plist бандла; без бандла — `None`.
pub fn info_string(key: &str) -> Option<String> {
    let value = NSBundle::mainBundle().objectForInfoDictionaryKey(&NSString::from_str(key))?;
    let text = value.downcast::<NSString>().ok()?;
    Some(text.to_string())
}

/// Bundle id (`local.rectangle2rust`, у dev-варианта `local.rectangle2rust.dev`).
pub fn bundle_identifier() -> Option<String> {
    NSBundle::mainBundle()
        .bundleIdentifier()
        .map(|bundle_id| bundle_id.to_string())
}

/// Номер сборки (`CFBundleVersion`).
pub fn bundle_version() -> Option<String> {
    info_string("CFBundleVersion")
}

/// Имя приложения для интерфейса: «Rectangle 2 (Rust)», у dev-варианта
/// «Rectangle 2 (Rust, тест)».
pub fn app_name() -> String {
    info_string("CFBundleDisplayName")
        .or_else(|| info_string("CFBundleName"))
        .unwrap_or_else(|| DEFAULT_APP_NAME.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_launch_records_install_version() {
        let mut config = Config::default();
        check_version(&mut config, Some("12".to_string()));
        assert_eq!(config.install_version.as_deref(), Some("12"));
        assert_eq!(config.last_version.as_deref(), Some("12"));

        // Следующий запуск новой сборки: installVersion остаётся прежним.
        check_version(&mut config, Some("15".to_string()));
        assert_eq!(config.install_version.as_deref(), Some("12"));
        assert_eq!(config.last_version.as_deref(), Some("15"));
    }

    #[test]
    fn unparsable_last_version_counts_as_first_launch() {
        let mut config = Config {
            last_version: Some("1.0".to_string()),
            install_version: Some("3".to_string()),
            ..Config::default()
        };
        check_version(&mut config, Some("20".to_string()));
        assert_eq!(config.install_version.as_deref(), Some("20"));
        assert_eq!(config.last_version.as_deref(), Some("20"));

        // Без бандла номера сборки нет — как `Defaults.lastVersion.value = nil`.
        check_version(&mut config, None);
        assert_eq!(config.install_version.as_deref(), Some("20"));
        assert_eq!(config.last_version, None);
    }

    #[test]
    fn show_eighths_migrates_only_when_not_set() {
        let mut config = Config::default();
        migrate_show_eighths(&mut config, 0);
        assert_eq!(config.show_additional_sizes_in_menu, None);
        migrate_show_eighths(&mut config, 1);
        assert_eq!(config.show_additional_sizes_in_menu, Some(true));
        migrate_show_eighths(&mut config, 2);
        assert_eq!(config.show_additional_sizes_in_menu, Some(true));

        let mut config = Config::default();
        migrate_show_eighths(&mut config, 2);
        assert_eq!(config.show_additional_sizes_in_menu, Some(false));
    }

    #[test]
    fn first_running_conflicting_app_is_reported() {
        let running = |ids: &[&str]| ids.iter().map(|id| id.to_string()).collect::<Vec<_>>();
        assert_eq!(
            conflicting_app(running(&["com.apple.Terminal", "com.apple.Safari"])),
            None
        );
        assert_eq!(
            conflicting_app(running(&[
                "com.apple.Terminal",
                "com.manytricks.Moom",
                "com.crowdcafe.windowmagnet"
            ])),
            Some("Moom")
        );
    }
}
