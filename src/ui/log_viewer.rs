//! Окно журнала — `LogViewer.storyboard` и `LogViewer.swift` оригинала
//! (docs/ui-spec.md §12).
//!
//! Открытие окна включает журнал (`Logger.logging = true`) и показывает то,
//! что в нём уже накоплено (`logging::lines()`), а новые строки дописывает по
//! мере записи (`logging::set_observer`). Закрытие выключает журнал и стирает
//! его, как `windowWillClose` оригинала; журнал, включённый переменной
//! `R2_LOG`, закрытие окна не выключает. В тулбаре одна кнопка «Очистить».
//! Строки — Monaco 10; если перед новой строкой текст был прокручен до конца,
//! он прокручивается дальше. ⌘W закрывает окно, ⌘H прячет его
//! (`KeyDownTextView`).
//!
//! В оригинале окно не переведено («Rectangle Logging», «Clear»); здесь —
//! «Журнал <имя приложения>» и «Очистить».

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{define_class, msg_send, sel, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSAutoresizingMaskOptions, NSBackingStoreType, NSBorderType, NSColor, NSEvent,
    NSEventModifierFlags, NSFont, NSFontAttributeName, NSForegroundColorAttributeName, NSImage,
    NSImageNameTrashEmpty, NSResponder, NSScrollView, NSStandardKeyBindingResponding, NSText,
    NSTextView, NSToolbar, NSToolbarDelegate, NSToolbarDisplayMode, NSToolbarItem, NSView,
    NSWindow, NSWindowDelegate, NSWindowStyleMask,
};
use objc2_foundation::{
    NSArray, NSAttributedString, NSDictionary, NSNotification, NSPoint, NSRect, NSSize, NSString,
};

use crate::app_delegate::app_name;
use crate::logging;

/// Место и размер содержимого окна из storyboard.
const CONTENT_RECT: NSRect = NSRect::new(NSPoint::new(245.0, 301.0), NSSize::new(480.0, 270.0));
/// Наименьший размер содержимого.
const MIN_CONTENT_SIZE: NSSize = NSSize::new(100.0, 50.0);
/// Шрифт строк; нет Monaco — системный того же размера.
const FONT_NAME: &str = "Monaco";
const FONT_SIZE: f64 = 10.0;
/// «Бесконечный» размер текста по вертикали (`FLT_MAX`).
const UNLIMITED: f64 = f32::MAX as f64;
const TOOLBAR_ID: &str = "R2LogToolbar";
const CLEAR_ITEM_ID: &str = "R2LogClear";
const CLEAR_TITLE: &str = "Очистить";
const CLEAR_ITEM_SIZE: NSSize = NSSize::new(72.0, 72.0);

define_class!(
    /// `KeyDownTextView`: ⌘W закрывает окно, ⌘H прячет его.
    #[unsafe(super(NSTextView, NSText, NSView, NSResponder, NSObject))]
    #[name = "R2LogTextView"]
    #[thread_kind = MainThreadOnly]
    #[ivars = ()]
    struct LogTextView;

    impl LogTextView {
        #[unsafe(method(keyDown:))]
        fn key_down(&self, event: &NSEvent) {
            let command = event.modifierFlags().contains(NSEventModifierFlags::Command);
            let key = event.charactersIgnoringModifiers().map(|key| key.to_string());
            match (command, key.as_deref()) {
                (true, Some("w")) => {
                    if let Some(window) = self.window() {
                        window.close();
                    }
                }
                (true, Some("h")) => {
                    if let Some(window) = self.window() {
                        window.orderOut(None);
                    }
                }
                _ => unsafe {
                    let _: () = msg_send![super(self), keyDown: event];
                },
            }
        }
    }
);

impl LogTextView {
    fn new(mtm: MainThreadMarker, frame: NSRect) -> Retained<Self> {
        let this = Self::alloc(mtm);
        unsafe { msg_send![this, initWithFrame: frame] }
    }
}

define_class!(
    /// `LogWindowController`: делегат окна и тулбара, цель кнопки «Очистить».
    #[unsafe(super(NSObject))]
    #[name = "R2LogWindowController"]
    #[thread_kind = MainThreadOnly]
    #[ivars = ()]
    struct Controller;

    unsafe impl NSObjectProtocol for Controller {}

    unsafe impl NSWindowDelegate for Controller {
        #[unsafe(method(windowWillClose:))]
        fn window_will_close(&self, _notification: &NSNotification) {
            window_will_close();
        }
    }

    unsafe impl NSToolbarDelegate for Controller {
        #[unsafe(method_id(toolbar:itemForItemIdentifier:willBeInsertedIntoToolbar:))]
        fn toolbar_item(
            &self,
            _toolbar: &NSToolbar,
            identifier: &NSString,
            _will_be_inserted: bool,
        ) -> Option<Retained<NSToolbarItem>> {
            (identifier.to_string() == CLEAR_ITEM_ID).then(|| clear_item(self))
        }

        #[unsafe(method_id(toolbarDefaultItemIdentifiers:))]
        fn default_items(&self, _toolbar: &NSToolbar) -> Retained<NSArray<NSString>> {
            item_identifiers()
        }

        /// Настраивать тулбар нельзя, так что допустимы те же элементы.
        #[unsafe(method_id(toolbarAllowedItemIdentifiers:))]
        fn allowed_items(&self, _toolbar: &NSToolbar) -> Retained<NSArray<NSString>> {
            item_identifiers()
        }
    }

    impl Controller {
        /// `clearClicked:` — кнопка «Очистить».
        #[unsafe(method(clearClicked:))]
        fn clear_clicked(&self, _sender: Option<&AnyObject>) {
            clear();
        }
    }
);

impl Controller {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm);
        unsafe { msg_send![this, init] }
    }
}

fn item_identifiers() -> Retained<NSArray<NSString>> {
    NSArray::from_retained_slice(&[NSString::from_str(CLEAR_ITEM_ID)])
}

/// Кнопка «Очистить»: картинка `NSTrashEmpty` без рамки, 72×72, tag −1, как
/// в storyboard. Рамку снимаем явно: элемент с действием, созданный кодом,
/// новая macOS рисует круглой кнопкой, а элемент из storyboard — нет; размер
/// 72×72 делает картинку такой же крупной, как в оригинале.
fn clear_item(controller: &Controller) -> Retained<NSToolbarItem> {
    let item = NSToolbarItem::initWithItemIdentifier(
        NSToolbarItem::alloc(controller.mtm()),
        &NSString::from_str(CLEAR_ITEM_ID),
    );
    let title = NSString::from_str(CLEAR_TITLE);
    item.setLabel(&title);
    item.setPaletteLabel(&title);
    item.setTag(-1);
    item.setBordered(false);
    #[allow(deprecated)]
    {
        item.setMinSize(CLEAR_ITEM_SIZE);
        item.setMaxSize(CLEAR_ITEM_SIZE);
    }
    item.setImage(NSImage::imageNamed(unsafe { NSImageNameTrashEmpty }).as_deref());
    unsafe {
        item.setTarget(Some(controller));
        item.setAction(Some(sel!(clearClicked:)));
    }
    item
}

struct LogWindow {
    window: Retained<NSWindow>,
    text_view: Retained<LogTextView>,
    /// Шрифт и цвет строк.
    attributes: Retained<NSDictionary<NSString, AnyObject>>,
    /// Делегат окна и тулбара, цель кнопки: AppKit их не удерживает.
    _controller: Retained<Controller>,
}

thread_local! {
    /// Окно создаётся при первом показе и живёт до конца работы, как
    /// `Logger.logWindowController` оригинала.
    static LOG_WINDOW: RefCell<Option<Rc<LogWindow>>> = const { RefCell::new(None) };
    /// Окно открыто: от `show` до `windowWillClose`.
    static OPEN: Cell<bool> = const { Cell::new(false) };
    /// Журнал был включён до открытия окна (`R2_LOG`).
    static ENABLED_BEFORE: Cell<bool> = const { Cell::new(false) };
}

/// Показать окно журнала (создать при первом вызове) и включить журнал —
/// `Logger.showLogging` оригинала.
pub fn show() {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let log_window = current().unwrap_or_else(|| {
        let created = Rc::new(build(mtm));
        LOG_WINDOW.with(|slot| *slot.borrow_mut() = Some(created.clone()));
        created
    });
    if !OPEN.with(|open| open.replace(true)) {
        ENABLED_BEFORE.with(|before| before.set(logging::is_enabled()));
        let text: String = logging::lines()
            .iter()
            .map(|line| format!("{line}\n"))
            .collect();
        append(&log_window, &text);
        logging::set_observer(Some(Box::new(append_line)));
    }
    #[allow(deprecated)]
    NSApplication::sharedApplication(mtm).activateIgnoringOtherApps(true);
    log_window.window.makeKeyAndOrderFront(None);
    logging::set_enabled(true);
}

fn current() -> Option<Rc<LogWindow>> {
    LOG_WINDOW.with(|slot| slot.borrow().clone())
}

/// Новая строка журнала (наблюдатель `logging`).
fn append_line(line: &str) {
    if let Some(log_window) = current() {
        append(&log_window, &format!("{line}\n"));
    }
}

/// Дописать текст в конец; был прокручен до конца — прокрутить дальше.
fn append(log_window: &LogWindow, text: &str) {
    if text.is_empty() {
        return;
    }
    let text_view = &log_window.text_view;
    let Some(storage) = (unsafe { text_view.textStorage() }) else {
        return;
    };
    let visible = text_view.visibleRect();
    let bounds = text_view.bounds();
    let scrolled_to_end =
        visible.origin.y + visible.size.height >= bounds.origin.y + bounds.size.height;
    let line = unsafe {
        NSAttributedString::new_with_attributes(&NSString::from_str(text), &log_window.attributes)
    };
    storage.appendAttributedString(&line);
    if scrolled_to_end {
        unsafe { text_view.scrollToEndOfDocument(None) };
    }
}

/// «Очистить»: стереть текст окна и сам журнал.
fn clear() {
    logging::clear();
    if let Some(log_window) = current() {
        log_window.text_view.setString(&NSString::new());
    }
}

/// `windowWillClose`: журнал выключается (если его не включили до окна) и стирается.
fn window_will_close() {
    if !OPEN.with(|open| open.replace(false)) {
        return;
    }
    logging::set_observer(None);
    logging::set_enabled(ENABLED_BEFORE.with(Cell::get));
    clear();
}

fn build(mtm: MainThreadMarker) -> LogWindow {
    let controller = Controller::new(mtm);
    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(mtm),
            CONTENT_RECT,
            NSWindowStyleMask::Titled
                | NSWindowStyleMask::Closable
                | NSWindowStyleMask::Miniaturizable
                | NSWindowStyleMask::Resizable,
            NSBackingStoreType::Buffered,
            false,
        )
    };
    // Окно держит `LOG_WINDOW`, а не AppKit: иначе закрытие освободило бы его.
    unsafe { window.setReleasedWhenClosed(false) };
    window.setTitle(&NSString::from_str(&format!("Журнал {}", app_name())));
    window.setContentMinSize(MIN_CONTENT_SIZE);

    let sizable =
        NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable;
    let scroll_view = NSScrollView::initWithFrame(
        NSScrollView::alloc(mtm),
        NSRect::new(NSPoint::new(0.0, 0.0), CONTENT_RECT.size),
    );
    scroll_view.setBorderType(NSBorderType::NoBorder);
    scroll_view.setHasVerticalScroller(true);
    scroll_view.setHasHorizontalScroller(false);
    scroll_view.setDrawsBackground(false);
    scroll_view.contentView().setDrawsBackground(false);
    scroll_view.setAutoresizingMask(sizable);

    let font = NSFont::fontWithName_size(&NSString::from_str(FONT_NAME), FONT_SIZE)
        .unwrap_or_else(|| NSFont::systemFontOfSize(FONT_SIZE));
    let content_size = scroll_view.contentSize();
    let text_view = LogTextView::new(mtm, NSRect::new(NSPoint::new(0.0, 0.0), content_size));
    text_view.setMinSize(NSSize::new(0.0, content_size.height));
    text_view.setMaxSize(NSSize::new(UNLIMITED, UNLIMITED));
    text_view.setVerticallyResizable(true);
    text_view.setHorizontallyResizable(false);
    text_view.setAutoresizingMask(sizable);
    if let Some(container) = unsafe { text_view.textContainer() } {
        container.setContainerSize(NSSize::new(content_size.width, UNLIMITED));
        container.setWidthTracksTextView(true);
    }
    text_view.setRichText(false);
    text_view.setImportsGraphics(false);
    text_view.setEditable(false);
    text_view.setFont(Some(&font));
    text_view.setTextColor(Some(&NSColor::textColor()));
    text_view.setBackgroundColor(&NSColor::textBackgroundColor());
    scroll_view.setDocumentView(Some(&text_view));
    window.setContentView(Some(&scroll_view));
    window.setInitialFirstResponder(Some(&text_view));
    window.makeFirstResponder(Some(&text_view));

    let toolbar =
        NSToolbar::initWithIdentifier(NSToolbar::alloc(mtm), &NSString::from_str(TOOLBAR_ID));
    toolbar.setDelegate(Some(ProtocolObject::from_ref(&*controller)));
    toolbar.setDisplayMode(NSToolbarDisplayMode::IconOnly);
    toolbar.setAllowsUserCustomization(false);
    toolbar.setAutosavesConfiguration(false);
    window.setToolbar(Some(&toolbar));
    window.setDelegate(Some(ProtocolObject::from_ref(&*controller)));

    let color = NSColor::textColor();
    let keys: [&NSString; 2] = unsafe { [NSFontAttributeName, NSForegroundColorAttributeName] };
    let values: [&AnyObject; 2] = [font.as_ref(), color.as_ref()];
    LogWindow {
        window,
        text_view,
        attributes: NSDictionary::from_slices(&keys, &values),
        _controller: controller,
    }
}
