//! «О программе» — стандартная панель macOS, как у оригинала
//! (`NSApp.orderFrontStandardAboutPanel`): имя, версия, иконка и копирайт из Info.plist.

use objc2::MainThreadMarker;
use objc2_app_kit::NSApplication;

/// Показать стандартную панель «О программе» поверх других приложений.
pub fn show() {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let application = NSApplication::sharedApplication(mtm);
    #[allow(deprecated)]
    application.activateIgnoringOtherApps(true);
    application.orderFrontStandardAboutPanel(None);
}
