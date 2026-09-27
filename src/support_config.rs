//! Настройки из `RectangleConfig.json` при запуске — `Defaults.loadFromSupportDir`
//! оригинала (`PrefsWindow/Config.swift`).
//!
//! Файл в формате экспорта кладут в папку приложения в Application Support, и
//! при следующем запуске он применяется — только после подтверждения: любой
//! процесс пользователя может подложить туда файл. Как в оригинале:
//! - файл-ссылку или файл, открытый на запись всем, не загружаем: алерт, файл
//!   удаляется;
//! - «Применить» — настройки из файла загружаются (как кнопка «Импорт»), а файл
//!   переименовывается в `RectangleConfig<время>.json`; не вышло — удаляется,
//!   не вышло и это — алерт;
//! - «Отказаться» — файл удаляется.
//!
//! Отличие от оригинала — папка: своя, `~/Library/Application Support/Rectangle2Rust`
//! (у dev-варианта — `Rectangle2Rust-dev`), а не `Rectangle`. Файл в `Rectangle`
//! ждёт Swift-версия, и порт не должен его забирать. Без бандла автоимпорта нет.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use objc2::MainThreadMarker;
use objc2_app_kit::{NSAlert, NSAlertFirstButtonReturn, NSAlertStyle, NSApplication};
use objc2_foundation::NSString;

use crate::logging::{self, LocalTime};
use crate::{app_delegate, config, log, ui};

/// Имя файла, который применяется при запуске.
pub const FILE_NAME: &str = "RectangleConfig.json";

/// Bundle id dev-варианта (`./build.sh --dev`).
const DEV_BUNDLE_ID: &str = "local.rectangle2rust.dev";

/// Папка приложения в Application Support по bundle id; `None` — автоимпорта нет.
pub fn support_folder(bundle_id: Option<&str>) -> Option<&'static str> {
    match bundle_id {
        Some(config::APP_BUNDLE_ID) => Some("Rectangle2Rust"),
        Some(DEV_BUNDLE_ID) => Some("Rectangle2Rust-dev"),
        _ => None,
    }
}

/// Что с файлом настроек.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileCheck {
    /// Файла нет (как `checkResourceIsReachable`: висячая ссылка — тоже нет).
    Missing,
    /// Ссылка или файл, открытый на запись всем, — не загружаем.
    Unsafe,
    /// Можно предлагать применить.
    Safe,
}

/// Проверка файла перед загрузкой. Атрибуты не прочитались — как в оригинале,
/// файл считается безопасным.
pub fn check_file(path: &Path) -> FileCheck {
    use std::os::unix::fs::PermissionsExt;

    if !path.exists() {
        return FileCheck::Missing;
    }
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => FileCheck::Unsafe,
        Ok(metadata) if metadata.permissions().mode() & 0o002 != 0 => FileCheck::Unsafe,
        _ => FileCheck::Safe,
    }
}

/// Имя, под которым применённый файл остаётся в папке: `RectangleConfig` + время
/// в формате `y-MM-dd_H-mm-ss-SSSS` (`DateFormatter` оригинала) + `.json`.
pub fn archived_name(time: &LocalTime) -> String {
    format!(
        "RectangleConfig{}-{:02}-{:02}_{}-{:02}-{:02}-{:04}.json",
        time.year,
        time.month,
        time.day,
        time.hour,
        time.minute,
        time.second,
        time.nanos / 100_000
    )
}

/// `loadFromSupportDir`: вызывается в начале запуска, когда настройки уже
/// прочитаны, а подсистемы ещё не запущены.
pub fn load_at_launch(mtm: MainThreadMarker) {
    let Some(folder) = support_folder(app_delegate::bundle_identifier().as_deref()) else {
        return;
    };
    let Some(home) = std::env::var_os("HOME") else {
        return;
    };
    let dir = PathBuf::from(home)
        .join("Library/Application Support")
        .join(folder);
    let path = dir.join(FILE_NAME);

    match check_file(&path) {
        FileCheck::Missing => return,
        FileCheck::Unsafe => {
            show_alert(
                mtm,
                "Файл настроек не загружен",
                &format!(
                    "Файл настроек {} — символическая ссылка или открыт на запись всем. \
                     {} отказался его загружать и удалил. Положите файл заново с обычными \
                     правами и перезапустите приложение.",
                    path.display(),
                    app_delegate::app_name()
                ),
                &["OK"],
            );
            remove(&path);
            return;
        }
        FileCheck::Safe => {}
    }

    let apply = show_alert(
        mtm,
        "Применить настройки Rectangle?",
        &format!(
            "Найден файл настроек {}. Если применить его, текущие настройки будут заменены. \
             Если отказаться, файл будет удалён.",
            path.display()
        ),
        &["Применить", "Отказаться"],
    );
    if !apply {
        remove(&path);
        return;
    }

    match ui::prefs::import_config_file(&path) {
        Ok(()) => log!("Настройки загружены из {}", path.display()),
        Err(error) => log!("Настройки из {FILE_NAME} не загружены: {error}"),
    }
    let archived =
        logging::local_time(SystemTime::now()).map(|time| dir.join(archived_name(&time)));
    let kept = archived.is_some_and(|archived| std::fs::rename(&path, archived).is_ok());
    if !kept && std::fs::remove_file(&path).is_err() {
        show_alert(
            mtm,
            "Ошибка после загрузки настроек",
            &format!(
                "Не удалось переименовать или удалить {FILE_NAME} в {} после загрузки.",
                dir.display()
            ),
            &["OK"],
        );
    }
}

fn remove(path: &Path) {
    if let Err(error) = std::fs::remove_file(path) {
        log!("Не удалось удалить {}: {error}", path.display());
    }
}

/// Алерт с кнопками `buttons`; `true` — нажата первая.
fn show_alert(mtm: MainThreadMarker, title: &str, text: &str, buttons: &[&str]) -> bool {
    let alert = NSAlert::new(mtm);
    alert.setAlertStyle(NSAlertStyle::Warning);
    alert.setMessageText(&NSString::from_str(title));
    alert.setInformativeText(&NSString::from_str(text));
    for button in buttons {
        alert.addButtonWithTitle(&NSString::from_str(button));
    }
    #[allow(deprecated)]
    NSApplication::sharedApplication(mtm).activateIgnoringOtherApps(true);
    alert.runModal() == NSAlertFirstButtonReturn
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    /// Свой временный каталог на тест (удаляется в конце).
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> TempDir {
            let dir = std::env::temp_dir()
                .join(format!("r2-support-config-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            TempDir(dir)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn folder_is_own_for_app_and_dev_and_none_without_bundle() {
        assert_eq!(
            support_folder(Some("local.rectangle2rust")),
            Some("Rectangle2Rust")
        );
        assert_eq!(
            support_folder(Some("local.rectangle2rust.dev")),
            Some("Rectangle2Rust-dev")
        );
        // Папку Swift-версии («Rectangle») порт не читает.
        assert_eq!(support_folder(Some("com.knollsoft.Rectangle")), None);
        assert_eq!(support_folder(None), None);
    }

    #[test]
    fn file_check_missing_safe_and_unsafe() {
        let dir = TempDir::new("check");
        let path = dir.0.join(FILE_NAME);
        assert_eq!(check_file(&path), FileCheck::Missing);

        std::fs::write(&path, "{}").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(check_file(&path), FileCheck::Safe);

        // Открыт на запись всем.
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o666)).unwrap();
        assert_eq!(check_file(&path), FileCheck::Unsafe);
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();

        // Ссылка на существующий файл — не загружаем; висячая ссылка — файла нет.
        let target = dir.0.join("target.json");
        std::fs::write(&target, "{}").unwrap();
        let link = dir.0.join("link.json");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert_eq!(check_file(&link), FileCheck::Unsafe);
        std::fs::remove_file(&target).unwrap();
        assert_eq!(check_file(&link), FileCheck::Missing);
    }

    #[test]
    fn archived_name_matches_swift_date_format() {
        let time = LocalTime {
            year: 2026,
            month: 9,
            day: 7,
            hour: 3,
            minute: 5,
            second: 9,
            nanos: 123_456_789,
            offset_seconds: 3 * 3600,
        };
        // y-MM-dd_H-mm-ss-SSSS: час без ведущего нуля, доля секунды — 4 знака.
        assert_eq!(
            archived_name(&time),
            "RectangleConfig2026-09-07_3-05-09-1234.json"
        );
        let midnight = LocalTime {
            hour: 0,
            nanos: 5_000_000,
            ..time
        };
        assert_eq!(
            archived_name(&midnight),
            "RectangleConfig2026-09-07_0-05-09-0050.json"
        );
    }
}
