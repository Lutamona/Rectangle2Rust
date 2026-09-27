//! Rectangle 2 (Rust) — менеджер окон для macOS.
//!
//! Порт Rectangle 2: меню в статус-баре, окно двигается через Accessibility API.
//! Горячих клавиш нет — только мышь. Всё, что происходит при запуске и
//! дальше, — в `app_delegate` (как `AppDelegate.swift` оригинала).

use objc2::runtime::ProtocolObject;
use objc2::MainThreadMarker;
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};

use rectangle2rust::app_delegate::AppDelegate;

fn main() {
    let mtm = MainThreadMarker::new().expect("приложение должно стартовать в главном потоке");
    let application = NSApplication::sharedApplication(mtm);
    // Без Dock-иконки и строки меню; в бандле то же задаёт LSUIElement, а это —
    // для запуска без бандла (cargo run).
    application.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    // AppKit держит делегата слабой ссылкой: `delegate` живёт до конца `run`.
    let delegate = AppDelegate::new(mtm);
    application.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    application.run();
}
