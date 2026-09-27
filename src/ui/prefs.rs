//! Окно настроек (`PrefsWindowController` оригинала, docs/ui-spec.md §3):
//! вкладки «Области прилипания» и «Основные» на панели инструментов. Вкладки
//! «Горячие клавиши» нет — горячих клавиш в порте нет (решения D1, D5).
//!
//! Окно создаётся при первом `show()` и живёт до выхода, как контроллер
//! оригинала: повторный `show()` выводит его вперёд, а выбранная вкладка и
//! состояние контролов сохраняются. Позиция между запусками не запоминается.
//!
//! Высоту окна задаёт выбранная вкладка; если окно не помещается на экран, оно
//! становится ниже, а вкладка прокручивается (`fit_window`).

pub(crate) mod about_todo;
mod extras;
mod general;
mod logic;
mod snap_areas;

use std::cell::RefCell;
use std::path::Path;

use objc2::rc::Retained;
use objc2::runtime::NSObject;
use objc2::{define_class, msg_send, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSBackingStoreType, NSResponder, NSScreen, NSTabView, NSTabViewController,
    NSTabViewControllerTabStyle, NSTabViewItem, NSViewController,
    NSViewControllerTransitionOptions, NSWindow, NSWindowStyleMask,
};
use objc2_foundation::{NSPoint, NSRect, NSSize};

use crate::geometry::Rect;
use crate::ui::controls::{self, ns};
use crate::{app_delegate, config, defaults_store, events, log};

/// Вкладка окна настроек.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    /// «Области прилипания».
    SnapAreas,
    /// «Основные».
    General,
}

impl Tab {
    /// Порядок на панели: как у оригинала без «Горячих клавиш».
    const ALL: [Tab; 2] = [Tab::SnapAreas, Tab::General];

    fn index(self) -> isize {
        match self {
            Tab::SnapAreas => 0,
            Tab::General => 1,
        }
    }

    fn from_index(index: isize) -> Option<Tab> {
        Tab::ALL.into_iter().find(|tab| tab.index() == index)
    }

    fn label(self) -> &'static str {
        match self {
            Tab::SnapAreas => "Области прилипания",
            Tab::General => "Основные",
        }
    }

    /// Картинка вкладки из Assets оригинала (template).
    fn image(self) -> &'static str {
        match self {
            Tab::SnapAreas => "snapAreaTemplate",
            Tab::General => "toolbarSettingsTemplate",
        }
    }
}

/// Окно настроек или «О Todo режиме». Сочетания ⌘W, ⌘M и правку в полях
/// ввода (⌘X, ⌘C, ⌘V, ⌘A, ⌘Z, ⇧⌘Z) обрабатывает невидимое главное меню
/// (`menu::main_menu`), как в оригинале, — в том числе в русской раскладке.
fn new_window(
    mtm: MainThreadMarker,
    content_rect: NSRect,
    style: NSWindowStyleMask,
) -> Retained<NSWindow> {
    // SAFETY: обычная инициализация окна; буфер — как у окон storyboard.
    unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(mtm),
            content_rect,
            style,
            NSBackingStoreType::Buffered,
            false,
        )
    }
}

define_class!(
    /// Вкладки на панели инструментов (`NSTabViewController`, стиль toolbar).
    /// Размер окна под вкладку подгоняет `fit_window`: сам контроллер окно
    /// при смене вкладки не меняет.
    #[unsafe(super(NSTabViewController, NSViewController, NSResponder, NSObject))]
    #[name = "R2PrefsTabViewController"]
    #[thread_kind = MainThreadOnly]
    struct PrefsTabs;

    impl PrefsTabs {
        #[unsafe(method(tabView:didSelectTabViewItem:))]
        fn tab_view_did_select_tab_view_item(
            &self,
            tab_view: &NSTabView,
            item: Option<&NSTabViewItem>,
        ) {
            unsafe {
                let _: () = msg_send![super(self), tabView: tab_view, didSelectTabViewItem: item];
            }
            fit_window(self.mtm(), true);
        }
    }
);

impl PrefsTabs {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm);
        unsafe { msg_send![this, init] }
    }
}

struct Prefs {
    window: Retained<NSWindow>,
    tabs: Retained<PrefsTabs>,
}

thread_local! {
    static PREFS: RefCell<Option<Prefs>> = const { RefCell::new(None) };
}

/// Показать окно настроек (создать при первом вызове) и вывести его вперёд.
pub fn show() {
    show_tab(None);
}

/// Показать окно на вкладке `tab` (`None` — на той, что была выбрана).
pub fn show_tab(tab: Option<Tab>) {
    let Some(mtm) = MainThreadMarker::new() else {
        log!("Окно настроек открывают только с главного потока");
        return;
    };
    let existing = PREFS.with(|slot| {
        slot.borrow()
            .as_ref()
            .map(|prefs| (prefs.window.clone(), prefs.tabs.clone()))
    });
    let created = existing.is_none();
    // Создаётся вне заимствования: выбор вкладки при создании зовёт `fit_window`.
    let (window, tabs) = existing.unwrap_or_else(|| {
        let prefs = create(mtm);
        let pair = (prefs.window.clone(), prefs.tabs.clone());
        PREFS.with(|slot| *slot.borrow_mut() = Some(prefs));
        pair
    });
    if let Some(tab) = tab {
        tabs.setSelectedTabViewItemIndex(tab.index());
    }
    // Настройки могли поменяться, пока окно было закрыто.
    general::refresh();
    #[allow(deprecated)]
    NSApplication::sharedApplication(mtm).activateIgnoringOtherApps(true);
    window.makeKeyAndOrderFront(None);
    if created {
        // AppKit ставит фокус в первое поле ввода — при открытии окна никакое
        // поле не выбрано.
        window.makeFirstResponder(None);
    }
    fit_window(mtm, false);
}

fn create(mtm: MainThreadMarker) -> Prefs {
    // Место окна — как contentRect в storyboard (кодом окно не центрируется);
    // `fit_window` затем держит верхний край и не даёт вылезти за экран.
    let window = new_window(
        mtm,
        NSRect::new(NSPoint::new(245.0, 301.0), NSSize::new(433.0, 270.0)),
        NSWindowStyleMask::Titled | NSWindowStyleMask::Closable | NSWindowStyleMask::Miniaturizable,
    );
    // Окно живёт до выхода, его держит `PREFS`.
    unsafe { window.setReleasedWhenClosed(false) };
    window.setTitle(&ns(&format!("Настройки {}", app_delegate::app_name())));
    window.setTitlebarAppearsTransparent(true);

    let tabs = PrefsTabs::new(mtm);
    tabs.setTabStyle(NSTabViewControllerTabStyle::Toolbar);
    tabs.setTransitionOptions(NSViewControllerTransitionOptions::AllowUserInteraction);
    for tab in Tab::ALL {
        let controller: Retained<NSViewController> = match tab {
            Tab::SnapAreas => snap_areas::build(mtm),
            Tab::General => general::build(mtm),
        };
        let item = NSTabViewItem::tabViewItemWithViewController(&controller);
        item.setLabel(&ns(tab.label()));
        item.setImage(controls::template_image(tab.image()).as_deref());
        tabs.addTabViewItem(&item);
    }
    tabs.setSelectedTabViewItemIndex(0);
    window.setContentViewController(Some(&tabs));

    config::subscribe(Box::new(general::config_changed));
    events::on_screens_changed(move || fit_window(mtm, false));
    Prefs { window, tabs }
}

fn to_rect(rect: NSRect) -> Rect {
    Rect::new(
        rect.origin.x,
        rect.origin.y,
        rect.size.width,
        rect.size.height,
    )
}

fn to_nsrect(rect: Rect) -> NSRect {
    NSRect::new(NSPoint::new(rect.x, rect.y), NSSize::new(rect.w, rect.h))
}

/// Видимая область экрана окна (или главного экрана, пока окно не показано).
fn visible_frame(mtm: MainThreadMarker, window: &NSWindow) -> Option<Rect> {
    window
        .screen()
        .or_else(|| NSScreen::mainScreen(mtm))
        .map(|screen| to_rect(screen.visibleFrame()))
}

/// Подогнать окно под выбранную вкладку: высота — по содержимому, но не выше
/// видимой области экрана; верхний край на месте.
fn fit_window(mtm: MainThreadMarker, animated: bool) {
    let Some((window, tabs)) = PREFS.with(|slot| {
        slot.borrow()
            .as_ref()
            .map(|prefs| (prefs.window.clone(), prefs.tabs.clone()))
    }) else {
        return;
    };
    let Some(content_view) = window.contentView() else {
        return;
    };
    let content_height = match Tab::from_index(tabs.selectedTabViewItemIndex()) {
        Some(Tab::SnapAreas) => snap_areas::content_height(),
        _ => general::content_height(),
    };
    let frame = window.frame();
    let content = content_view.frame();
    let chrome = frame.size.height - content.size.height;
    let width = frame.size.width - content.size.width + general::VIEW_WIDTH;
    let fitted = logic::fit_window_frame(
        to_rect(frame),
        chrome,
        width,
        content_height,
        visible_frame(mtm, &window),
    );
    if fitted != to_rect(frame) {
        window.setFrame_display_animate(to_nsrect(fitted), true, animated && window.isVisible());
    }
}

/// Сдвинуть окно в видимую область экрана, не меняя размера.
fn clamp_to_screen(mtm: MainThreadMarker, window: &NSWindow) {
    let frame = to_rect(window.frame());
    let clamped = logic::fit_window_frame(frame, 0.0, frame.w, frame.h, visible_frame(mtm, window));
    if clamped != frame {
        window.setFrame_display(to_nsrect(clamped), true);
    }
}

/// Экспорт настроек в JSON в формате Rectangle (кнопка «Экспорт»).
pub fn export_config_file(path: &Path) -> Result<(), String> {
    let version = app_delegate::bundle_version().unwrap_or_default();
    let text = defaults_store::export_json(&config::current(), &version);
    std::fs::write(path, text).map_err(|error| format!("{}: {error}", path.display()))
}

/// Импорт настроек из JSON (кнопка «Импорт»): настройки из файла применяются
/// сразу (подписчики настроек перезагружают меню и подсистемы), контролы окна
/// перечитываются, как после `configImported` у оригинала.
pub fn import_config_file(path: &Path) -> Result<(), String> {
    let size = std::fs::metadata(path)
        .map_err(|error| format!("{}: {error}", path.display()))?
        .len();
    if size > defaults_store::MAX_IMPORT_SIZE as u64 {
        return Err(format!(
            "{}: файл больше {} байт — такие не импортируются",
            path.display(),
            defaults_store::MAX_IMPORT_SIZE
        ));
    }
    let text =
        std::fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let imported = defaults_store::import_json(&text, &config::current())?;
    general::write(|config| *config = imported);
    general::refresh_after_import();
    Ok(())
}
