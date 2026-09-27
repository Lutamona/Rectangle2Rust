//! Главное меню приложения (`Main Menu` из storyboard оригинала,
//! `docs/ui-spec.md` §2.6).
//!
//! Приложение — агент (`LSUIElement`): строки меню у него нет, и это меню
//! никто не видит. Оно нужно ради сочетаний клавиш, пока активно окно
//! приложения (настройки, журнал): ⌘W закрывает окно, ⌘C/⌘V/⌘A работают в
//! полях ввода, ⌘Q завершает приложение. Состав и сочетания — как в оригинале.

use objc2::rc::Retained;
use objc2::runtime::Sel;
use objc2::{sel, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSApplication, NSEventModifierFlags, NSMenu, NSMenuItem};
use objc2_foundation::NSString;

use crate::localization::{self, main_menu as text};

/// Поставить главное меню приложения.
pub fn install(mtm: MainThreadMarker) {
    let application = NSApplication::sharedApplication(mtm);
    let app_name = crate::app_delegate::app_name();
    let main = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str("Main Menu"));

    let (app_menu, services) = app_menu(mtm, &app_name);
    add_submenu(mtm, &main, &app_name, &app_menu);
    add_submenu(mtm, &main, text::FILE, &file_menu(mtm));
    add_submenu(mtm, &main, text::EDIT, &edit_menu(mtm));
    add_submenu(mtm, &main, text::VIEW, &view_menu(mtm));
    let window_menu = window_menu(mtm);
    add_submenu(mtm, &main, text::WINDOW, &window_menu);
    let help_menu = help_menu(mtm, &app_name);
    add_submenu(mtm, &main, text::HELP, &help_menu);

    application.setMainMenu(Some(&main));
    application.setServicesMenu(Some(&services));
    application.setWindowsMenu(Some(&window_menu));
    application.setHelpMenu(Some(&help_menu));
}

/// Пункт; `action` без цели идёт по цепочке ответчиков, как в storyboard.
fn item(
    mtm: MainThreadMarker,
    title: &str,
    action: Option<Sel>,
    key: &str,
    modifiers: NSEventModifierFlags,
) -> Retained<NSMenuItem> {
    let item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str(title),
            action,
            &NSString::from_str(key),
        )
    };
    item.setKeyEquivalentModifierMask(modifiers);
    item
}

fn menu(mtm: MainThreadMarker, title: &str) -> Retained<NSMenu> {
    NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str(title))
}

fn add_submenu(mtm: MainThreadMarker, parent: &NSMenu, title: &str, submenu: &NSMenu) {
    let parent_item = item(mtm, title, None, "", NSEventModifierFlags::empty());
    parent_item.setSubmenu(Some(submenu));
    parent.addItem(&parent_item);
}

const COMMAND: NSEventModifierFlags = NSEventModifierFlags::Command;

fn command_with(flags: NSEventModifierFlags) -> NSEventModifierFlags {
    COMMAND.union(flags)
}

/// Меню приложения и его подменю «Службы».
fn app_menu(mtm: MainThreadMarker, app_name: &str) -> (Retained<NSMenu>, Retained<NSMenu>) {
    let none = NSEventModifierFlags::empty();
    let app_menu = menu(mtm, app_name);
    app_menu.addItem(&item(
        mtm,
        &localization::about_app(app_name),
        Some(sel!(orderFrontStandardAboutPanel:)),
        "",
        none,
    ));
    app_menu.addItem(&NSMenuItem::separatorItem(mtm));
    // В оригинале у пункта нет действия — он всегда серый.
    app_menu.addItem(&item(mtm, text::PREFERENCES, None, ",", COMMAND));
    app_menu.addItem(&NSMenuItem::separatorItem(mtm));
    let services = menu(mtm, text::SERVICES);
    add_submenu(mtm, &app_menu, text::SERVICES, &services);
    app_menu.addItem(&NSMenuItem::separatorItem(mtm));
    app_menu.addItem(&item(
        mtm,
        &text::hide(app_name),
        Some(sel!(hide:)),
        "h",
        COMMAND,
    ));
    app_menu.addItem(&item(
        mtm,
        text::HIDE_OTHERS,
        Some(sel!(hideOtherApplications:)),
        "h",
        command_with(NSEventModifierFlags::Option),
    ));
    app_menu.addItem(&item(
        mtm,
        text::SHOW_ALL,
        Some(sel!(unhideAllApplications:)),
        "",
        none,
    ));
    app_menu.addItem(&NSMenuItem::separatorItem(mtm));
    app_menu.addItem(&item(
        mtm,
        &localization::quit(app_name),
        Some(sel!(terminate:)),
        "q",
        COMMAND,
    ));
    (app_menu, services)
}

fn file_menu(mtm: MainThreadMarker) -> Retained<NSMenu> {
    let file = menu(mtm, text::FILE);
    for (title, action, key) in [
        (text::NEW, sel!(newDocument:), "n"),
        (text::OPEN, sel!(openDocument:), "o"),
        (text::CLOSE, sel!(performClose:), "w"),
        (text::SAVE, sel!(saveDocument:), "s"),
    ] {
        file.addItem(&item(mtm, title, Some(action), key, COMMAND));
    }
    file
}

fn edit_menu(mtm: MainThreadMarker) -> Retained<NSMenu> {
    let edit = menu(mtm, text::EDIT);
    // Заглавная буква в сочетании — это ⇧: «Z» = ⇧⌘Z.
    edit.addItem(&item(mtm, text::UNDO, Some(sel!(undo:)), "z", COMMAND));
    edit.addItem(&item(mtm, text::REDO, Some(sel!(redo:)), "Z", COMMAND));
    edit.addItem(&NSMenuItem::separatorItem(mtm));
    edit.addItem(&item(mtm, text::CUT, Some(sel!(cut:)), "x", COMMAND));
    edit.addItem(&item(mtm, text::COPY, Some(sel!(copy:)), "c", COMMAND));
    edit.addItem(&item(mtm, text::PASTE, Some(sel!(paste:)), "v", COMMAND));
    edit.addItem(&item(
        mtm,
        text::PASTE_AND_MATCH_STYLE,
        Some(sel!(pasteAsPlainText:)),
        "V",
        command_with(NSEventModifierFlags::Option),
    ));
    edit.addItem(&item(
        mtm,
        text::DELETE,
        Some(sel!(delete:)),
        "",
        NSEventModifierFlags::empty(),
    ));
    edit.addItem(&item(
        mtm,
        text::SELECT_ALL,
        Some(sel!(selectAll:)),
        "a",
        COMMAND,
    ));
    edit.addItem(&NSMenuItem::separatorItem(mtm));

    let find = menu(mtm, text::FIND);
    // Теги — `NSFindPanelAction`: 1 показать, 12 заменить, 2 дальше, 3 назад, 7 из выделения.
    for (title, tag, key, modifiers) in [
        (text::FIND_ELLIPSIS, 1, "f", COMMAND),
        (
            text::FIND_AND_REPLACE,
            12,
            "f",
            command_with(NSEventModifierFlags::Option),
        ),
        (text::FIND_NEXT, 2, "g", COMMAND),
        (text::FIND_PREVIOUS, 3, "G", COMMAND),
        (text::USE_SELECTION_FOR_FIND, 7, "e", COMMAND),
    ] {
        let find_item = item(
            mtm,
            title,
            Some(sel!(performFindPanelAction:)),
            key,
            modifiers,
        );
        find_item.setTag(tag);
        find.addItem(&find_item);
    }
    find.addItem(&item(
        mtm,
        text::JUMP_TO_SELECTION,
        Some(sel!(centerSelectionInVisibleArea:)),
        "j",
        COMMAND,
    ));
    add_submenu(mtm, &edit, text::FIND, &find);
    edit
}

fn view_menu(mtm: MainThreadMarker) -> Retained<NSMenu> {
    let view = menu(mtm, text::VIEW);
    view.addItem(&item(
        mtm,
        text::ENTER_FULL_SCREEN,
        Some(sel!(toggleFullScreen:)),
        "f",
        command_with(NSEventModifierFlags::Control),
    ));
    view
}

fn window_menu(mtm: MainThreadMarker) -> Retained<NSMenu> {
    let window = menu(mtm, text::WINDOW);
    window.addItem(&item(
        mtm,
        text::MINIMIZE,
        Some(sel!(performMiniaturize:)),
        "m",
        COMMAND,
    ));
    window
}

fn help_menu(mtm: MainThreadMarker, app_name: &str) -> Retained<NSMenu> {
    let help = menu(mtm, text::HELP);
    help.addItem(&item(
        mtm,
        &text::help(app_name),
        Some(sel!(showHelp:)),
        "?",
        COMMAND,
    ));
    help
}
