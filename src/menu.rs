//! Иконка в статус-баре и её меню — `RectangleStatusItem` и меню
//! `AppDelegate` оригинала (`docs/ui-spec.md` §2).
//!
//! Состав меню — данные из `layout` (там же правила видимости и тесты); здесь
//! из них собирается `NSMenu`. При каждом открытии меню и подменю
//! (`menuWillOpen`, как `updateWindowActionMenuItems` оригинала) пункты
//! обновляются: трети на портретном экране получают повёрнутые иконки, пункты
//! действий гаснут, если нет активного окна, «Следующий/предыдущий экран» и
//! «Экран N» прячутся при одном экране, «Игнорировать …» и пункты Todo берут
//! имя активного приложения. При закрытии (`menuDidClose`) пункты снова
//! включаются. Горячих клавиш нет (решение D1): у пунктов действий подсказок
//! нет, есть только ⌘Q у «Выход» и ⌥ у «Просмотр журнала…».

mod icons;
pub mod layout;
mod main_menu;

use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::ptr;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol, ProtocolObject, Sel};
use objc2::{define_class, msg_send, sel, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSAlert, NSAlertFirstButtonReturn, NSApplication, NSControlStateValueOff,
    NSControlStateValueOn, NSEventModifierFlags, NSMenu, NSMenuDelegate, NSMenuItem, NSScreen,
    NSStatusBar, NSStatusItem, NSStatusItemBehavior, NSVariableStatusItemLength, NSWorkspace,
};
use objc2_foundation::{
    ns_string, NSDictionary, NSKeyValueChangeKey, NSKeyValueChangeNewKey, NSKeyValueChangeOldKey,
    NSKeyValueObservingOptions, NSNumber, NSObjectNSKeyValueObserverRegistration, NSString, NSURL,
};

use crate::actions::{Action, WindowActionCategory};
use crate::{app, app_delegate, ax, config, events, localization, screens, todo, ui};
use layout::{Displays, FrontApp, Item, Service, Settings};

thread_local! {
    /// Цель пунктов и делегат меню держим живыми: AppKit их не удерживает.
    static TARGET: RefCell<Option<Retained<MenuTarget>>> = const { RefCell::new(None) };
    static DELEGATE: RefCell<Option<Retained<MenuDelegate>>> = const { RefCell::new(None) };
    static STATUS_ITEM: RefCell<Option<Retained<NSStatusItem>>> = const { RefCell::new(None) };
    /// Следит, не убрал ли пользователь иконку из строки меню (⌘-перетаскиванием).
    static VISIBILITY_OBSERVER: RefCell<Option<Retained<VisibilityObserver>>> =
        const { RefCell::new(None) };
    /// Меню с действиями (`mainStatusMenu`) и меню «нет доступа» (`unauthorizedMenu`).
    static MAIN_MENU: RefCell<Option<Retained<NSMenu>>> = const { RefCell::new(None) };
    static UNAUTHORIZED_MENU: RefCell<Option<Retained<NSMenu>>> = const { RefCell::new(None) };
    /// Доступ к управлению компьютером есть — у иконки меню с действиями.
    static AUTHORIZED: Cell<bool> = const { Cell::new(false) };
    /// Настройки, по которым собрано меню.
    static SETTINGS: Cell<Settings> = Cell::new(Settings::default());
}

define_class!(
    /// Цель пунктов меню: действия над окнами и служебные пункты. Селекторы —
    /// как у действий `AppDelegate` оригинала.
    #[unsafe(super(NSObject))]
    #[name = "R2MenuTarget"]
    #[thread_kind = MainThreadOnly]
    #[ivars = ()]
    struct MenuTarget;

    impl MenuTarget {
        /// Пункт действия: tag — `Action::raw`. Меню работает и для
        /// игнорируемых приложений (решение D4).
        #[unsafe(method(executeMenuWindowAction:))]
        fn execute_menu_window_action(&self, sender: &NSMenuItem) {
            if let Some(action) = window_action(sender) {
                app::execute(action);
            }
        }

        /// «Окна приложения столбиками»: сигнал при неудаче подаёт сама раскладка.
        #[unsafe(method(tileAppColumns:))]
        fn tile_app_columns(&self, _sender: &NSMenuItem) {
            let _ = crate::app_columns::tile_frontmost_app();
        }

        /// «Держать окна «…» столбиками»: включить/выключить режим для приложения.
        #[unsafe(method(toggleKeepAppColumns:))]
        fn toggle_keep_app_columns(&self, _sender: &NSMenuItem) {
            crate::app_columns::toggle_keeping_frontmost();
        }

        #[unsafe(method(ignoreFrontMostApp:))]
        fn ignore_front_most_app(&self, _sender: &NSMenuItem) {
            toggle_front_app_ignored();
        }

        #[unsafe(method(openPreferences:))]
        fn open_preferences(&self, _sender: &NSMenuItem) {
            ui::prefs::show();
        }

        #[unsafe(method(showAbout:))]
        fn show_about(&self, _sender: &NSMenuItem) {
            ui::about::show();
        }

        #[unsafe(method(viewLogging:))]
        fn view_logging(&self, _sender: &NSMenuItem) {
            ui::log_viewer::show();
        }

        #[unsafe(method(checkForUpdates:))]
        fn check_for_updates(&self, _sender: &NSMenuItem) {
            show_updates_disabled_alert();
        }

        #[unsafe(method(authorizeAccessibility:))]
        fn authorize_accessibility(&self, _sender: &NSMenuItem) {
            ui::authorize::show();
        }

        #[unsafe(method(toggleTodoMode:))]
        fn toggle_todo_mode(&self, _sender: &NSMenuItem) {
            todo::toggle_mode();
        }

        #[unsafe(method(setTodoApp:))]
        fn set_todo_app(&self, _sender: &NSMenuItem) {
            todo::set_todo_app_to_frontmost();
        }

        #[unsafe(method(setTodoWindow:))]
        fn set_todo_window(&self, _sender: &NSMenuItem) {
            todo::set_todo_window_to_front();
        }

        #[unsafe(method(todoReflow:))]
        fn todo_reflow(&self, _sender: &NSMenuItem) {
            todo::reflow();
        }
    }
);

define_class!(
    /// Делегат меню и подменю: обновляет пункты при открытии и включает при закрытии.
    #[unsafe(super(NSObject))]
    #[name = "R2MenuDelegate"]
    #[thread_kind = MainThreadOnly]
    #[ivars = ()]
    struct MenuDelegate;

    unsafe impl NSObjectProtocol for MenuDelegate {}

    unsafe impl NSMenuDelegate for MenuDelegate {
        #[unsafe(method(menuWillOpen:))]
        fn menu_will_open(&self, menu: &NSMenu) {
            update_items(menu, self.mtm());
        }

        /// Пункты, погашенные без активного окна, снова включаются.
        #[unsafe(method(menuDidClose:))]
        fn menu_did_close(&self, menu: &NSMenu) {
            for item in menu.itemArray().iter() {
                item.setEnabled(true);
            }
        }
    }
);

define_class!(
    /// KVO на `isVisible` иконки: пользователь убрал её из строки меню
    /// (`behavior = .removalAllowed`) — значит, «Скрыть значок».
    #[unsafe(super(NSObject))]
    #[name = "R2StatusItemVisibilityObserver"]
    #[thread_kind = MainThreadOnly]
    #[ivars = ()]
    struct VisibilityObserver;

    impl VisibilityObserver {
        #[unsafe(method(observeValueForKeyPath:ofObject:change:context:))]
        fn observe_value(
            &self,
            _key_path: Option<&NSString>,
            _object: Option<&AnyObject>,
            change: Option<&NSDictionary<NSKeyValueChangeKey, AnyObject>>,
            _context: *mut c_void,
        ) {
            let flag = |key: &NSKeyValueChangeKey| {
                change
                    .and_then(|change| change.objectForKey(key))
                    .and_then(|value| value.downcast::<NSNumber>().ok())
                    .map(|value| value.boolValue())
            };
            // SAFETY: ключи словаря изменений — константы Foundation.
            let (old, new) = unsafe { (flag(NSKeyValueChangeOldKey), flag(NSKeyValueChangeNewKey)) };
            on_visibility_change(old, new, events::run_later);
        }
    }
);

/// Иконку убрали из строки меню (`isVisible`: было «да», стало «нет») — это
/// «Скрыть значок». Настройка пишется не сразу, а на следующем обороте цикла
/// (`later` — `events::run_later`): она тут же убирает иконку (`config_changed`
/// → `refresh_visibility`) и отпускает наблюдателя KVO, а изменение приходит
/// изнутри `-[NSStatusItem setVisible:]`, пока метод наблюдателя ещё на стеке.
fn on_visibility_change(
    old: Option<bool>,
    new: Option<bool>,
    later: impl FnOnce(Box<dyn FnOnce()>),
) {
    if old == Some(true) && new == Some(false) {
        later(Box::new(|| {
            config::update(|config| config.hide_menu_bar_icon = true)
        }));
    }
}

impl MenuTarget {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm);
        unsafe { msg_send![this, init] }
    }
}

impl MenuDelegate {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm);
        unsafe { msg_send![this, init] }
    }
}

impl VisibilityObserver {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm);
        unsafe { msg_send![this, init] }
    }
}

/// Собрать меню и показать иконку в статус-баре, если она не спрятана
/// (`hideMenubarIcon`). До `set_authorized(true)` у иконки меню «нет доступа».
pub fn install(mtm: MainThreadMarker) {
    let target = MenuTarget::new(mtm);
    let delegate = MenuDelegate::new(mtm);

    let settings = config::with(Settings::from_config);
    let menu = NSMenu::new(mtm);
    // Пункты гасит и включает делегат, а не AppKit (`autoenablesItems = false`).
    menu.setAutoenablesItems(false);
    menu.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    fill(mtm, &menu, &layout::layout(&settings), &target, &delegate);
    SETTINGS.with(|slot| slot.set(settings));

    UNAUTHORIZED_MENU.with(|slot| *slot.borrow_mut() = Some(unauthorized_menu(mtm, &target)));
    MAIN_MENU.with(|slot| *slot.borrow_mut() = Some(menu));
    TARGET.with(|slot| *slot.borrow_mut() = Some(target));
    DELEGATE.with(|slot| *slot.borrow_mut() = Some(delegate));

    main_menu::install(mtm);
    // `showAdditionalSizesInMenuChanged` → `rebuildMenu` оригинала.
    events::on_config_changed(|old, new| {
        if Settings::from_config(old) != Settings::from_config(new) {
            rebuild();
        }
    });
    refresh_visibility();
}

/// Собрать пункты меню заново — поменялись настройки его состава.
fn rebuild() {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let (Some(menu), Some(target), Some(delegate)) = (
        MAIN_MENU.with(|slot| slot.borrow().clone()),
        TARGET.with(|slot| slot.borrow().clone()),
        DELEGATE.with(|slot| slot.borrow().clone()),
    ) else {
        return;
    };
    let settings = config::with(Settings::from_config);
    menu.removeAllItems();
    fill(mtm, &menu, &layout::layout(&settings), &target, &delegate);
    SETTINGS.with(|slot| slot.set(settings));
}

/// Меню иконки: с действиями, если доступ есть, иначе «нет доступа».
fn current_menu() -> Option<Retained<NSMenu>> {
    let menu = if AUTHORIZED.with(Cell::get) {
        &MAIN_MENU
    } else {
        &UNAUTHORIZED_MENU
    };
    menu.with(|slot| slot.borrow().clone())
}

/// `RectangleStatusItem.add()`: иконка в статус-баре с текущим меню. Её
/// можно убрать ⌘-перетаскиванием — это то же, что «Скрыть значок».
fn add_status_item(mtm: MainThreadMarker) {
    let status_item =
        NSStatusBar::systemStatusBar().statusItemWithLength(NSVariableStatusItemLength);
    status_item.setMenu(current_menu().as_deref());
    if let Some(button) = status_item.button(mtm) {
        match icons::status_icon() {
            Some(icon) => button.setImage(Some(&icon)),
            None => button.setTitle(ns_string!("◫")),
        }
        button.setToolTip(Some(&NSString::from_str(&app_delegate::app_name())));
    }
    status_item.setBehavior(NSStatusItemBehavior::RemovalAllowed);

    let observer = VisibilityObserver::new(mtm);
    // SAFETY: наблюдатель отписывается в `remove_status_item`, до того как
    // иконку отпустят, и живёт, пока подписан (VISIBILITY_OBSERVER).
    unsafe {
        status_item.addObserver_forKeyPath_options_context(
            &observer,
            ns_string!("isVisible"),
            NSKeyValueObservingOptions::Old | NSKeyValueObservingOptions::New,
            ptr::null_mut(),
        );
    }
    // Иконку, которую пользователь уже убирал, система помнит скрытой.
    status_item.setVisible(true);

    VISIBILITY_OBSERVER.with(|slot| *slot.borrow_mut() = Some(observer));
    STATUS_ITEM.with(|slot| *slot.borrow_mut() = Some(status_item));
}

/// `RectangleStatusItem.remove()`.
fn remove_status_item() {
    let Some(item) = STATUS_ITEM.with(|slot| slot.borrow_mut().take()) else {
        return;
    };
    if let Some(observer) = VISIBILITY_OBSERVER.with(|slot| slot.borrow_mut().take()) {
        // SAFETY: наблюдатель подписан на эту иконку в `add_status_item`.
        unsafe { item.removeObserver_forKeyPath(&observer, ns_string!("isVisible")) };
    }
    NSStatusBar::systemStatusBar().removeStatusItem(&item);
}

/// `RectangleStatusItem.refreshVisibility()`: спрятать или показать иконку по
/// настройке `hideMenubarIcon`.
pub fn refresh_visibility() {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    if config::with(|config| config.hide_menu_bar_icon) {
        remove_status_item();
    } else if STATUS_ITEM.with(|slot| slot.borrow().is_none()) {
        add_status_item(mtm);
    }
}

/// `RectangleStatusItem.openMenu()`: открыть меню иконки (повторный запуск
/// приложения). Спрятанная иконка показывается на время меню.
pub fn open_status_menu() {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    if STATUS_ITEM.with(|slot| slot.borrow().is_none()) {
        add_status_item(mtm);
    }
    let button = STATUS_ITEM.with(|slot| slot.borrow().as_ref().and_then(|item| item.button(mtm)));
    if let Some(button) = button {
        // Возвращается, когда меню закрыто.
        unsafe { button.performClick(None) };
    }
    refresh_visibility();
}

/// Доступ к управлению компьютером есть или нет: у иконки меню с действиями
/// или меню «нет доступа».
pub fn set_authorized(authorized: bool) {
    AUTHORIZED.with(|slot| slot.set(authorized));
    let menu = current_menu();
    STATUS_ITEM.with(|slot| {
        if let Some(item) = slot.borrow().as_ref() {
            item.setMenu(menu.as_deref());
        }
    });
}

/// Страница последнего релиза: оттуда скачивают новую версию.
const RELEASES_URL: &str = "https://github.com/Lutamona/Rectangle2Rust/releases/latest";

/// «Проверить обновления…» (`checkForUpdates` оригинала): автообновления нет,
/// новая версия — со страницы релизов.
pub fn show_updates_disabled_alert() {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let alert = NSAlert::new(mtm);
    alert.setMessageText(&NSString::from_str(localization::UPDATES_DISABLED_TITLE));
    alert.setInformativeText(&NSString::from_str(localization::UPDATES_DISABLED_TEXT));
    alert.addButtonWithTitle(&NSString::from_str(localization::OPEN_RELEASES));
    alert.addButtonWithTitle(&NSString::from_str(localization::OK));
    #[allow(deprecated)]
    NSApplication::sharedApplication(mtm).activateIgnoringOtherApps(true);
    if alert.runModal() == NSAlertFirstButtonReturn {
        if let Some(url) = NSURL::URLWithString(&NSString::from_str(RELEASES_URL)) {
            NSWorkspace::sharedWorkspace().openURL(&url);
        }
    }
}

// ---------------------------------------------------------------- сборка

/// `unauthorizedMenu` (§2.5): без делегата, пункт без действия — серый.
fn unauthorized_menu(mtm: MainThreadMarker, target: &MenuTarget) -> Retained<NSMenu> {
    let app_name = app_delegate::app_name();
    let menu = NSMenu::new(mtm);
    menu.addItem(&menu_item(mtm, localization::NOT_AUTHORIZED, None, ""));
    let authorize = menu_item(
        mtm,
        localization::AUTHORIZE,
        Some(sel!(authorizeAccessibility:)),
        "",
    );
    unsafe { authorize.setTarget(Some(target)) };
    menu.addItem(&authorize);
    menu.addItem(&NSMenuItem::separatorItem(mtm));
    let about = menu_item(
        mtm,
        &localization::about_app(&app_name),
        Some(sel!(showAbout:)),
        "",
    );
    unsafe { about.setTarget(Some(target)) };
    menu.addItem(&about);
    menu.addItem(&menu_item(
        mtm,
        &localization::quit(&app_name),
        Some(sel!(terminate:)),
        "q",
    ));
    menu
}

/// Добавить в `menu` пункты по данным `layout`.
fn fill(
    mtm: MainThreadMarker,
    menu: &NSMenu,
    items: &[Item],
    target: &MenuTarget,
    delegate: &MenuDelegate,
) {
    let app_name = app_delegate::app_name();
    for item in items {
        match item {
            Item::Separator => menu.addItem(&NSMenuItem::separatorItem(mtm)),
            Item::Action(action) => {
                if let Some(item) = action_item(mtm, *action, target) {
                    menu.addItem(&item);
                }
            }
            Item::Submenu(category, items) => {
                let title = NSString::from_str(category.display_name());
                let submenu = NSMenu::initWithTitle(NSMenu::alloc(mtm), &title);
                submenu.setAutoenablesItems(false);
                submenu.setDelegate(Some(ProtocolObject::from_ref(delegate)));
                fill(mtm, &submenu, items, target, delegate);
                let parent = menu_item(mtm, category.display_name(), None, "");
                parent.setSubmenu(Some(&submenu));
                menu.addItem(&parent);
            }
            Item::Service(service) => menu.addItem(&service_item(mtm, *service, target, &app_name)),
        }
    }
}

fn menu_item(
    mtm: MainThreadMarker,
    title: &str,
    action: Option<Sel>,
    key: &str,
) -> Retained<NSMenuItem> {
    unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str(title),
            action,
            &NSString::from_str(key),
        )
    }
}

/// Пункт действия (`executeMenuWindowAction`), без подсказки клавиш.
fn action_item(
    mtm: MainThreadMarker,
    action: Action,
    target: &MenuTarget,
) -> Option<Retained<NSMenuItem>> {
    let title = localization::action_title(action)?;
    let item = menu_item(mtm, title, Some(sel!(executeMenuWindowAction:)), "");
    unsafe { item.setTarget(Some(target)) };
    item.setTag(action.raw() as isize);
    item.setImage(Some(&icons::action_icon(action, false)));
    Some(item)
}

/// Действие пункта меню; `None` — пункт не действие.
fn window_action(item: &NSMenuItem) -> Option<Action> {
    if item.action() != Some(sel!(executeMenuWindowAction:)) {
        return None;
    }
    i32::try_from(item.tag()).ok().and_then(Action::from_raw)
}

/// Селектор служебного пункта; `terminate:` уходит в NSApplication.
fn service_selector(service: Service) -> Sel {
    match service {
        Service::AppColumnsTile => sel!(tileAppColumns:),
        Service::AppColumnsKeep => sel!(toggleKeepAppColumns:),
        Service::TodoMode => sel!(toggleTodoMode:),
        Service::TodoApp => sel!(setTodoApp:),
        Service::TodoWindow => sel!(setTodoWindow:),
        Service::TodoReflow => sel!(todoReflow:),
        Service::IgnoreApp => sel!(ignoreFrontMostApp:),
        Service::Settings => sel!(openPreferences:),
        Service::About => sel!(showAbout:),
        Service::ViewLogging => sel!(viewLogging:),
        Service::CheckForUpdates => sel!(checkForUpdates:),
        Service::Quit => sel!(terminate:),
    }
}

/// Служебный пункт меню; `None` — пункт не служебный.
fn service(item: &NSMenuItem) -> Option<Service> {
    let action = item.action()?;
    Service::ALL
        .into_iter()
        .find(|service| service_selector(*service) == action)
}

fn service_item(
    mtm: MainThreadMarker,
    service: Service,
    target: &MenuTarget,
    app_name: &str,
) -> Retained<NSMenuItem> {
    let key = if service == Service::Quit { "q" } else { "" };
    let item = menu_item(
        mtm,
        &service.title(None, app_name),
        Some(service_selector(service)),
        key,
    );
    if service != Service::Quit {
        unsafe { item.setTarget(Some(target)) };
    }
    match service {
        // Пара «О программе» / «Просмотр журнала…» различается только ⌥.
        Service::About => item.setKeyEquivalentModifierMask(NSEventModifierFlags::empty()),
        Service::ViewLogging => {
            item.setAlternate(true);
            item.setKeyEquivalentModifierMask(NSEventModifierFlags::Option);
        }
        _ => {}
    }
    // Системные иконки служебных пунктов (`addMenuIcons`).
    let symbol = match service {
        Service::Settings => Some("gear"),
        Service::ViewLogging => Some("doc.text"),
        Service::CheckForUpdates => Some("arrow.down.circle"),
        _ => None,
    };
    if let Some(image) = symbol.and_then(icons::symbol) {
        item.setImage(Some(&image));
    }
    item
}

// ---------------------------------------------------------------- открытие

/// `menuWillOpen`: обновить пункты этого меню (подменю обновляются сами,
/// когда открываются).
fn update_items(menu: &NSMenu, mtm: MainThreadMarker) {
    let displays = Displays {
        count: screens::screens().len(),
        combined: config::with(|config| config.combined_display_mode == Some(true)),
    };
    let front = events::front_app();
    let settings = SETTINGS.with(Cell::get);
    let front_app = FrontApp {
        name: front.as_ref().and_then(|app| app.name.as_deref()),
        is_todo_app: settings.todo && todo::is_todo_app_active(),
        todo_window_front: settings.todo && todo::is_todo_window_front(),
    };
    let portrait = NSScreen::mainScreen(mtm).is_some_and(|screen| {
        let size = screen.frame().size;
        size.width <= size.height
    });
    // Активное окно спрашиваем только у меню, где есть действия.
    let mut has_front_window = None;

    let items = menu.itemArray();
    let mut entries = Vec::with_capacity(items.count());
    for item in items.iter() {
        let shown = if let Some(action) = window_action(&item) {
            if action.classification() == Some(WindowActionCategory::Thirds) {
                item.setImage(Some(&icons::action_icon(action, portrait)));
            }
            if !*has_front_window.get_or_insert_with(|| ax::front_window().is_some()) {
                item.setEnabled(false);
            }
            layout::action_shown(action, displays)
        } else if let Some(service) = service(&item) {
            // «Окна приложения столбиками», как и действия, гаснут без активного окна.
            if service.needs_front_window()
                && !*has_front_window.get_or_insert_with(|| ax::front_window().is_some())
            {
                item.setEnabled(false);
            }
            update_service_item(&item, service, &front_app, front.as_ref());
            match service {
                // Без bundle id режим «держать» не запомнить — пункт прячется.
                Service::AppColumnsKeep => {
                    layout::service_shown(service, &front_app)
                        && crate::app_columns::keep_menu_state().is_some()
                }
                _ => layout::service_shown(service, &front_app),
            }
        } else {
            true
        };
        entries.push((item.isSeparatorItem(), shown));
    }
    for (item, visible) in items.iter().zip(layout::collapse_separators(entries)) {
        item.setHidden(!visible);
    }
}

/// Подписи, галочки и доступность служебных пунктов (`menuWillOpen`,
/// `updateTodoModeMenuItems`).
fn update_service_item(
    item: &NSMenuItem,
    service: Service,
    front_app: &FrontApp,
    front: Option<&events::AppInfo>,
) {
    let checked = |on: bool| {
        if on {
            NSControlStateValueOn
        } else {
            NSControlStateValueOff
        }
    };
    let app_name = app_delegate::app_name();
    match service {
        Service::AppColumnsKeep => {
            if let Some((title, keeping)) = crate::app_columns::keep_menu_state() {
                item.setTitle(&NSString::from_str(&title));
                item.setState(checked(keeping));
            }
        }
        Service::IgnoreApp => {
            if front_app.name.is_some() {
                let ignored =
                    front
                        .and_then(|app| app.bundle_id.as_deref())
                        .is_some_and(|bundle_id| {
                            config::with(|config| config.is_app_disabled(bundle_id))
                        });
                item.setTitle(&NSString::from_str(
                    &service.title(front_app.name, &app_name),
                ));
                item.setState(checked(ignored));
            }
        }
        Service::TodoMode => item.setState(checked(todo::is_mode_on())),
        Service::TodoApp => {
            if front_app.name.is_some() {
                item.setTitle(&NSString::from_str(
                    &service.title(front_app.name, &app_name),
                ));
                item.setEnabled(!front_app.is_todo_app);
                item.setState(checked(front_app.is_todo_app));
            }
        }
        Service::TodoReflow => item.setEnabled(todo::is_mode_on()),
        _ => {}
    }
}

/// «Игнорировать <приложение>» (`ignoreFrontMostApp`): приложение попадает в
/// `disabledApps` или уходит оттуда. Решение D4: игнор выключает для него
/// drag-to-snap и двойной клик по заголовку, пункты меню работают.
fn toggle_front_app_ignored() {
    let Some(bundle_id) = events::front_app().and_then(|app| app.bundle_id) else {
        return;
    };
    config::update(|config| {
        let apps = config.disabled_apps.get_or_insert_with(Default::default);
        if !apps.remove(&bundle_id) {
            apps.insert(bundle_id);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removed_icon_is_hidden_on_the_next_run_loop_turn() {
        let mut scheduled = Vec::new();
        on_visibility_change(Some(true), Some(false), |task| scheduled.push(task));
        // Внутри колбэка KVO — ни записи настройки, ни снятия иконки.
        assert_eq!(scheduled.len(), 1);
        assert!(!config::with(|config| config.hide_menu_bar_icon));

        // Показали, не поменялось или нечего сравнить — ничего не делаем.
        for (old, new) in [
            (Some(false), Some(true)),
            (Some(true), Some(true)),
            (Some(false), Some(false)),
            (None, Some(false)),
            (Some(true), None),
        ] {
            on_visibility_change(old, new, |task| scheduled.push(task));
        }
        assert_eq!(scheduled.len(), 1);
    }
}
