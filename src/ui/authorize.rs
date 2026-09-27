//! Окно «Разрешить Rectangle» — `AccessibilityWindowController` и
//! `AccessibilityViewController` оригинала (docs/ui-spec.md §10): как выдать
//! доступ к управлению компьютером.
//!
//! Окно показывает `accessibility::check` при запуске без доступа и пункт
//! «Авторизовать…» меню, закрывает — опрос доступа, когда доступ выдали.
//! Вёрстка — сцена `5D9-0a-Mbi` из `Main.storyboard`: вертикальный стек по
//! центру с шагом 22, отступы 36 сверху и 20 по краям, ширина стека 250 с
//! приоритетом 750 (заголовок длиннее раздвигает окно). Размер окна задаёт
//! содержимое, как у окна с `contentViewController`. Красная кнопка завершает
//! приложение (`exit(1)`), как в оригинале: без доступа ему делать нечего.
//!
//! Подписи — русский перевод оригинала, где «Rectangle 2» заменено именем
//! приложения. На macOS 13+ путь и кнопка — про «Системные настройки», а
//! подсказки про замок нет, как в `viewDidLoad` оригинала.

use std::cell::{OnceCell, RefCell};

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject};
use objc2::{available, define_class, msg_send, sel, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSBackingStoreType, NSBezelStyle, NSButton, NSColor, NSFont, NSImage,
    NSImageNameApplicationIcon, NSImageScaling, NSImageView, NSLayoutAttribute, NSLayoutConstraint,
    NSLayoutConstraintOrientation, NSLayoutPriority, NSStackView, NSTextAlignment, NSTextField,
    NSUserInterfaceLayoutOrientation, NSView, NSWindow, NSWindowButton, NSWindowStyleMask,
    NSWindowTitleVisibility,
};
use objc2_foundation::{NSArray, NSPoint, NSRect, NSSize, NSString};

use crate::app_delegate::app_name;

/// Отступ стека сверху — место под прозрачным заголовком окна.
const TOP_INSET: f64 = 36.0;
/// Отступ стека слева, справа и снизу.
const SIDE_INSET: f64 = 20.0;
/// Ширина стека: `= 250` и `≥ 250`, обе с приоритетом 750.
const STACK_WIDTH: f64 = 250.0;
const STACK_WIDTH_PRIORITY: NSLayoutPriority = 750.0;
/// Шаг между строками стека.
const SPACING: f64 = 22.0;
/// Сторона иконки приложения.
const ICON_SIZE: f64 = 60.0;

/// Подписи окна.
#[derive(Debug, PartialEq, Eq)]
struct Texts {
    /// Заголовок окна (скрыт, виден в Mission Control и списке окон).
    window_title: String,
    title: String,
    needs_permission: String,
    settings_path: String,
    open_settings: String,
    enable_app: String,
    /// Подсказка про замок — только до macOS 13.
    padlock: Option<String>,
}

/// Подписи для приложения `name`; `system_settings` — macOS 13+, где
/// «Системные настройки» устроены иначе.
fn texts(name: &str, system_settings: bool) -> Texts {
    let (settings_path, open_settings, padlock) = if system_settings {
        (
            // macOS 13+: вкладки «Конфиденциальность» больше нет — в переводе оригинала
            // она осталась от старой схемы, здесь путь как на самом деле.
            "Перейдите в Системные настройки → Конфиденциальность и безопасность → \
             Универсальный доступ",
            "Открыть Системные настройки",
            None,
        )
    } else {
        (
            "Перейдите в Системные настройки → Защита и безопасность → \
             Конфиденциальность → Универсальный доступ",
            "Открыть Системные настройки",
            Some("Если флажок отключен, щелкните на замок и введите свой пароль".to_string()),
        )
    };
    Texts {
        window_title: format!("Разрешить {name}"),
        title: format!("Дать {name} доступ"),
        needs_permission: format!(
            "{name} необходимо ваше разрешение для управления положением окон"
        ),
        settings_path: settings_path.to_string(),
        open_settings: open_settings.to_string(),
        // В оригинале «Check Rectangle.app» (поставить галочку) переведено как
        // «Проверьте файл …» — здесь по смыслу.
        enable_app: format!("Включите переключатель «{name}»."),
        padlock,
    }
}

define_class!(
    /// Цель кнопок окна: «Открыть Системные настройки» и красной кнопки.
    #[unsafe(super(NSObject))]
    #[name = "R2AuthorizeTarget"]
    #[thread_kind = MainThreadOnly]
    #[ivars = ()]
    struct Target;

    impl Target {
        /// `openSystemPrefs:` — «Универсальный доступ» в Системных настройках.
        #[unsafe(method(openSystemPrefs:))]
        fn open_system_prefs(&self, _sender: Option<&AnyObject>) {
            crate::accessibility::open_accessibility_settings();
        }

        /// Красная кнопка (`AccessibilityWindowController.quit`).
        #[unsafe(method(quit:))]
        fn quit(&self, _sender: Option<&AnyObject>) {
            std::process::exit(1);
        }
    }
);

impl Target {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm);
        unsafe { msg_send![this, init] }
    }
}

thread_local! {
    static WINDOW: RefCell<Option<Retained<NSWindow>>> = const { RefCell::new(None) };
    /// Цель кнопок окна. NSControl свою цель не удерживает, а закрытое окно
    /// AppKit может держать ещё какое-то время, поэтому цель (объект без
    /// состояния) живёт всё время работы.
    static TARGET: OnceCell<Retained<Target>> = const { OnceCell::new() };
}

/// Показать окно (создать при первом вызове): развернуть, если свёрнуто, и
/// вывести приложение вперёд — `checkAccessibility` и `showAuthorizationWindow`
/// оригинала.
pub fn show() {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let existing = WINDOW.with(|slot| slot.borrow().clone());
    let window = existing.unwrap_or_else(|| {
        let window = build(mtm);
        WINDOW.with(|slot| *slot.borrow_mut() = Some(window.clone()));
        window
    });
    if window.isMiniaturized() {
        window.deminiaturize(None);
    }
    #[allow(deprecated)]
    NSApplication::sharedApplication(mtm).activateIgnoringOtherApps(true);
    window.makeKeyAndOrderFront(None);
}

/// Закрыть окно, если оно открыто (доступ выдан).
pub fn close() {
    if let Some(window) = WINDOW.with(|slot| slot.borrow_mut().take()) {
        window.close();
    }
}

fn build(mtm: MainThreadMarker) -> Retained<NSWindow> {
    let texts = texts(&app_name(), available!(macos = 13.0));
    let target = TARGET.with(|target| target.get_or_init(|| Target::new(mtm)).clone());

    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(mtm),
            NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(480.0, 270.0)),
            NSWindowStyleMask::Titled
                | NSWindowStyleMask::Closable
                | NSWindowStyleMask::Miniaturizable
                | NSWindowStyleMask::FullSizeContentView,
            NSBackingStoreType::Buffered,
            false,
        )
    };
    // Окно держит `WINDOW`, а не AppKit: иначе закрытие освободило бы его дважды.
    unsafe { window.setReleasedWhenClosed(false) };
    window.setTitle(&NSString::from_str(&texts.window_title));
    window.setTitleVisibility(NSWindowTitleVisibility::Hidden);
    window.setTitlebarAppearsTransparent(true);

    let stack = NSStackView::new(mtm);
    stack.setOrientation(NSUserInterfaceLayoutOrientation::Vertical);
    stack.setAlignment(NSLayoutAttribute::CenterX);
    stack.setSpacing(SPACING);
    stack.setDetachesHiddenViews(true);
    for orientation in [
        NSLayoutConstraintOrientation::Horizontal,
        NSLayoutConstraintOrientation::Vertical,
    ] {
        stack.setHuggingPriority_forOrientation(249.999_98, orientation);
    }
    stack.setTranslatesAutoresizingMaskIntoConstraints(false);

    let title = NSTextField::labelWithString(&NSString::from_str(&texts.title), mtm);
    title.setFont(Some(&NSFont::systemFontOfSize(22.0)));
    set_priorities(&title, 251.0, 750.0, 751.0);
    stack.addArrangedSubview(&title);

    let icon = NSImageView::new(mtm);
    icon.setImage(NSImage::imageNamed(unsafe { NSImageNameApplicationIcon }).as_deref());
    icon.setImageScaling(NSImageScaling::ScaleProportionallyDown);
    set_hugging(&icon, 251.0, 251.0);
    stack.addArrangedSubview(&icon);

    let needs_permission = wrapping_label(mtm, &texts.needs_permission, 13.0);
    stack.addArrangedSubview(&needs_permission);

    let settings_path = wrapping_label(mtm, &texts.settings_path, 11.0);
    stack.addArrangedSubview(&settings_path);

    let open_settings = unsafe {
        NSButton::buttonWithTitle_target_action(
            &NSString::from_str(&texts.open_settings),
            Some(&target),
            Some(sel!(openSystemPrefs:)),
            mtm,
        )
    };
    open_settings.setBezelStyle(NSBezelStyle::FlexiblePush);
    open_settings.setKeyEquivalent(&NSString::from_str("\r"));
    open_settings
        .setContentHuggingPriority_forOrientation(750.0, NSLayoutConstraintOrientation::Vertical);
    stack.addArrangedSubview(&open_settings);

    let enable_app = NSTextField::labelWithString(&NSString::from_str(&texts.enable_app), mtm);
    set_hugging(&enable_app, 251.0, 750.0);
    stack.addArrangedSubview(&enable_app);

    let padlock = wrapping_label(mtm, texts.padlock.as_deref().unwrap_or_default(), 13.0);
    padlock.setHidden(texts.padlock.is_none());
    stack.addArrangedSubview(&padlock);

    let content = NSView::new(mtm);
    content.addSubview(&stack);
    let width_at_least = stack
        .widthAnchor()
        .constraintGreaterThanOrEqualToConstant(STACK_WIDTH);
    width_at_least.setPriority(STACK_WIDTH_PRIORITY);
    let width = stack.widthAnchor().constraintEqualToConstant(STACK_WIDTH);
    width.setPriority(STACK_WIDTH_PRIORITY);
    NSLayoutConstraint::activateConstraints(&NSArray::from_retained_slice(&[
        stack
            .topAnchor()
            .constraintEqualToAnchor_constant(&content.topAnchor(), TOP_INSET),
        stack
            .leadingAnchor()
            .constraintEqualToAnchor_constant(&content.leadingAnchor(), SIDE_INSET),
        content
            .trailingAnchor()
            .constraintEqualToAnchor_constant(&stack.trailingAnchor(), SIDE_INSET),
        content
            .bottomAnchor()
            .constraintEqualToAnchor_constant(&stack.bottomAnchor(), SIDE_INSET),
        width_at_least,
        width,
        icon.widthAnchor().constraintEqualToConstant(ICON_SIZE),
        icon.heightAnchor().constraintEqualToConstant(ICON_SIZE),
    ]));
    window.setContentView(Some(&content));
    // Два прохода: ширину задают стек и заголовок, а высоту многострочных
    // подписей раскладка узнаёт, только когда знает их ширину.
    for _ in 0..2 {
        window.setContentSize(content.fittingSize());
        content.layoutSubtreeIfNeeded();
    }
    window.center();

    if let Some(close_button) = window.standardWindowButton(NSWindowButton::CloseButton) {
        unsafe {
            close_button.setTarget(Some(&target));
            close_button.setAction(Some(sel!(quit:)));
        }
    }
    window
}

/// Многострочная подпись по центру вторичным цветом (как в storyboard).
fn wrapping_label(mtm: MainThreadMarker, text: &str, font_size: f64) -> Retained<NSTextField> {
    let label = NSTextField::wrappingLabelWithString(&NSString::from_str(text), mtm);
    label.setSelectable(false);
    label.setAlignment(NSTextAlignment::Center);
    label.setFont(Some(&NSFont::systemFontOfSize(font_size)));
    label.setTextColor(Some(&NSColor::secondaryLabelColor()));
    set_priorities(&label, 251.0, 750.0, 749.0);
    label
}

fn set_hugging(view: &NSView, horizontal: NSLayoutPriority, vertical: NSLayoutPriority) {
    view.setContentHuggingPriority_forOrientation(
        horizontal,
        NSLayoutConstraintOrientation::Horizontal,
    );
    view.setContentHuggingPriority_forOrientation(
        vertical,
        NSLayoutConstraintOrientation::Vertical,
    );
}

/// Приоритеты подписи: прижатие по горизонтали и вертикали, сопротивление
/// сжатию по горизонтали.
fn set_priorities(
    view: &NSView,
    horizontal_hugging: NSLayoutPriority,
    vertical_hugging: NSLayoutPriority,
    horizontal_compression: NSLayoutPriority,
) {
    set_hugging(view, horizontal_hugging, vertical_hugging);
    view.setContentCompressionResistancePriority_forOrientation(
        horizontal_compression,
        NSLayoutConstraintOrientation::Horizontal,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texts_are_russian_with_app_name() {
        let modern = texts("Rectangle 2 (Rust)", true);
        assert_eq!(modern.window_title, "Разрешить Rectangle 2 (Rust)");
        assert_eq!(modern.title, "Дать Rectangle 2 (Rust) доступ");
        assert_eq!(
            modern.needs_permission,
            "Rectangle 2 (Rust) необходимо ваше разрешение для управления положением окон"
        );
        assert_eq!(
            modern.settings_path,
            "Перейдите в Системные настройки → Конфиденциальность и безопасность → \
             Универсальный доступ"
        );
        assert_eq!(modern.open_settings, "Открыть Системные настройки");
        assert_eq!(
            modern.enable_app,
            "Включите переключатель «Rectangle 2 (Rust)»."
        );
        assert_eq!(modern.padlock, None);
    }

    #[test]
    fn old_macos_texts_mention_padlock() {
        let old = texts("Rectangle 2 (Rust)", false);
        assert_eq!(
            old.settings_path,
            "Перейдите в Системные настройки → Защита и безопасность → \
             Конфиденциальность → Универсальный доступ"
        );
        assert_eq!(
            old.padlock.as_deref(),
            Some("Если флажок отключен, щелкните на замок и введите свой пароль")
        );
    }
}
