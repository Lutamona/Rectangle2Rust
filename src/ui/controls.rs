//! Помощники для контролов AppKit, из которых кодом собираются окна: подписи,
//! флажки, попапы, ползунки, разделители, стеки, картинки, — с параметрами по
//! умолчанию, как их ставит Interface Builder в `Main.storyboard` оригинала.

use std::cell::RefCell;
use std::collections::HashMap;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2::{define_class, msg_send, AllocAnyThread, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSBox, NSBoxType, NSButton, NSColor, NSControlStateValueOff, NSControlStateValueOn, NSFont,
    NSImage, NSImageRep, NSLayoutAttribute, NSLayoutConstraint, NSLayoutConstraintOrientation,
    NSLayoutPriority, NSPopUpButton, NSSlider, NSStackView, NSStackViewDistribution, NSTextField,
    NSUserInterfaceLayoutOrientation, NSView,
};
use objc2_foundation::{NSBundle, NSPoint, NSRect, NSSize, NSString};

/// Приоритеты Auto Layout (`NSLayoutPriority…`).
pub const REQUIRED: NSLayoutPriority = 1000.0;
pub const DEFAULT_HIGH: NSLayoutPriority = 750.0;
pub const DEFAULT_LOW: NSLayoutPriority = 250.0;

pub fn ns(text: &str) -> Retained<NSString> {
    NSString::from_str(text)
}

/// Подпись (`NSTextField(labelWithString:)`): системный шрифт 13, `labelColor`.
pub fn label(mtm: MainThreadMarker, text: &str) -> Retained<NSTextField> {
    NSTextField::labelWithString(&ns(text), mtm)
}

/// Поясняющая подпись под контролом: шрифт 11, `secondaryLabelColor`,
/// переносится по словам в пределах `max_width`.
pub fn note(mtm: MainThreadMarker, text: &str, max_width: f64) -> Retained<NSTextField> {
    let field = NSTextField::wrappingLabelWithString(&ns(text), mtm);
    field.setFont(Some(&NSFont::systemFontOfSize(
        NSFont::smallSystemFontSize(),
    )));
    field.setTextColor(Some(&NSColor::secondaryLabelColor()));
    // До вставки в стек: стек спрашивает размер один раз, и с нулевой
    // шириной подпись осталась бы без высоты (как в оригинале).
    field.setPreferredMaxLayoutWidth(max_width);
    field.setContentCompressionResistancePriority_forOrientation(
        REQUIRED,
        NSLayoutConstraintOrientation::Vertical,
    );
    field.setContentHuggingPriority_forOrientation(
        DEFAULT_HIGH,
        NSLayoutConstraintOrientation::Vertical,
    );
    field
}

/// Флажок (`NSButton(checkboxWithTitle:target:action:)`).
pub fn checkbox(
    mtm: MainThreadMarker,
    title: &str,
    target: &AnyObject,
    action: Sel,
) -> Retained<NSButton> {
    // SAFETY: у цели есть метод `action` с аргументом-отправителем.
    let button = unsafe {
        NSButton::checkboxWithTitle_target_action(&ns(title), Some(target), Some(action), mtm)
    };
    button.setContentCompressionResistancePriority_forOrientation(
        REQUIRED,
        NSLayoutConstraintOrientation::Vertical,
    );
    // Подпись не обрезается: колонка расширится (русские подписи длиннее).
    button.setContentCompressionResistancePriority_forOrientation(
        REQUIRED,
        NSLayoutConstraintOrientation::Horizontal,
    );
    button
}

/// Радиокнопка; кнопки с одним действием в одном стеке AppKit сам связывает в группу.
pub fn radio(
    mtm: MainThreadMarker,
    title: &str,
    target: &AnyObject,
    action: Sel,
) -> Retained<NSButton> {
    // SAFETY: у цели есть метод `action` с аргументом-отправителем.
    let button = unsafe {
        NSButton::radioButtonWithTitle_target_action(&ns(title), Some(target), Some(action), mtm)
    };
    button.setContentCompressionResistancePriority_forOrientation(
        REQUIRED,
        NSLayoutConstraintOrientation::Vertical,
    );
    button
}

/// Обычная кнопка (push, rounded).
pub fn push_button(
    mtm: MainThreadMarker,
    title: &str,
    target: &AnyObject,
    action: Sel,
) -> Retained<NSButton> {
    // SAFETY: у цели есть метод `action` с аргументом-отправителем.
    unsafe { NSButton::buttonWithTitle_target_action(&ns(title), Some(target), Some(action), mtm) }
}

/// Флажок/радиокнопка включены.
pub fn is_on(button: &NSButton) -> bool {
    button.state() == NSControlStateValueOn
}

pub fn set_on(button: &NSButton, on: bool) {
    button.setState(if on {
        NSControlStateValueOn
    } else {
        NSControlStateValueOff
    });
}

/// Попап (`NSPopUpButton`, push, pullsDown = NO) с пунктами `(подпись, tag)`.
pub fn popup(
    mtm: MainThreadMarker,
    items: &[(&str, isize)],
    target: &AnyObject,
    action: Sel,
) -> Retained<NSPopUpButton> {
    let button = NSPopUpButton::initWithFrame_pullsDown(
        NSPopUpButton::alloc(mtm),
        NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(100.0, 24.0)),
        false,
    );
    for (title, tag) in items {
        button.addItemWithTitle(&ns(title));
        if let Some(item) = button.lastItem() {
            item.setTag(*tag);
        }
    }
    // SAFETY: у цели есть метод `action` с аргументом-отправителем.
    unsafe {
        button.setTarget(Some(target));
        button.setAction(Some(action));
    }
    button
}

/// Ползунок без засечек; значение шлётся непрерывно, пока его тянут.
pub fn slider(
    mtm: MainThreadMarker,
    min: f64,
    max: f64,
    target: &AnyObject,
    action: Sel,
) -> Retained<NSSlider> {
    // SAFETY: у цели есть метод `action` с аргументом-отправителем.
    let slider = unsafe {
        NSSlider::sliderWithValue_minValue_maxValue_target_action(
            min,
            min,
            max,
            Some(target),
            Some(action),
            mtm,
        )
    };
    slider.setContinuous(true);
    slider
}

/// Горизонтальный разделитель высотой 20 (`NSBox`, separator).
pub fn separator(mtm: MainThreadMarker) -> Retained<NSBox> {
    let separator = NSBox::new(mtm);
    separator.setBoxType(NSBoxType::Separator);
    separator.setTranslatesAutoresizingMaskIntoConstraints(false);
    separator.setContentHuggingPriority_forOrientation(
        DEFAULT_HIGH,
        NSLayoutConstraintOrientation::Vertical,
    );
    separator
        .heightAnchor()
        .constraintEqualToConstant(20.0)
        .setActive(true);
    separator
}

/// Стек с ориентацией, выравниванием и промежутком.
pub fn stack(
    mtm: MainThreadMarker,
    orientation: NSUserInterfaceLayoutOrientation,
    alignment: NSLayoutAttribute,
    spacing: f64,
    views: &[&NSView],
) -> Retained<NSStackView> {
    let stack = NSStackView::new(mtm);
    // В коде по умолчанию gravity areas; в storyboard у всех стеков fill.
    stack.setDistribution(NSStackViewDistribution::Fill);
    stack.setOrientation(orientation);
    stack.setAlignment(alignment);
    stack.setSpacing(spacing);
    stack.setTranslatesAutoresizingMaskIntoConstraints(false);
    for view in views {
        stack.addArrangedSubview(view);
    }
    stack
}

/// Горизонтальный стек, выровненный по центру по вертикали.
pub fn row(mtm: MainThreadMarker, spacing: f64, views: &[&NSView]) -> Retained<NSStackView> {
    stack(
        mtm,
        NSUserInterfaceLayoutOrientation::Horizontal,
        NSLayoutAttribute::CenterY,
        spacing,
        views,
    )
}

/// Вертикальный стек, выровненный по левому краю.
pub fn column(mtm: MainThreadMarker, spacing: f64, views: &[&NSView]) -> Retained<NSStackView> {
    stack(
        mtm,
        NSUserInterfaceLayoutOrientation::Vertical,
        NSLayoutAttribute::Leading,
        spacing,
        views,
    )
}

/// Включить ограничение с приоритетом.
pub fn activate(
    constraint: Retained<NSLayoutConstraint>,
    priority: NSLayoutPriority,
) -> Retained<NSLayoutConstraint> {
    constraint.setPriority(priority);
    constraint.setActive(true);
    constraint
}

/// Ширина вида равна `width`.
pub fn fix_width(view: &NSView, width: f64) {
    view.widthAnchor()
        .constraintEqualToConstant(width)
        .setActive(true);
}

define_class!(
    /// Вид с началом координат сверху: содержимое прокрутки прижато к верху.
    #[unsafe(super(NSView))]
    #[name = "R2FlippedView"]
    #[thread_kind = MainThreadOnly]
    pub struct FlippedView;

    impl FlippedView {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }
    }
);

impl FlippedView {
    pub fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm);
        unsafe { msg_send![this, init] }
    }
}

// ---------------------------------------------------------------- картинки

thread_local! {
    /// Загруженные картинки: NSImage создаётся один раз на имя.
    static IMAGES: RefCell<HashMap<String, Retained<NSImage>>> = RefCell::new(HashMap::new());
}

/// PNG из `Resources/icons` бандла, а без бандла (`cargo run`) — из
/// `packaging/icons` дерева исходников.
fn icon_path(file: &str) -> Option<String> {
    if let Some(resources) = NSBundle::mainBundle().resourcePath() {
        let path = format!("{resources}/icons/{file}");
        if std::path::Path::new(&path).exists() {
            return Some(path);
        }
    }
    [
        format!("packaging/icons/{file}"),
        format!("{}/packaging/icons/{file}", env!("CARGO_MANIFEST_DIR")),
    ]
    .into_iter()
    .find(|path| std::path::Path::new(path).exists())
}

/// Template-картинка из `packaging/icons`: `имя.png` (1x) и, если есть,
/// `имя@2x.png` — как набор 1x/2x в Assets оригинала. Размер в точках — по 1x.
pub fn template_image(name: &str) -> Option<Retained<NSImage>> {
    if let Some(image) = IMAGES.with(|cache| cache.borrow().get(name).cloned()) {
        return Some(image);
    }
    let base = NSImageRep::imageRepWithContentsOfFile(&ns(&icon_path(&format!("{name}.png"))?))?;
    let size = NSSize::new(base.pixelsWide() as f64, base.pixelsHigh() as f64);
    base.setSize(size);
    let image = NSImage::initWithSize(NSImage::alloc(), size);
    image.addRepresentation(&base);
    if let Some(retina) = icon_path(&format!("{name}@2x.png"))
        .and_then(|path| NSImageRep::imageRepWithContentsOfFile(&ns(&path)))
    {
        retina.setSize(size);
        image.addRepresentation(&retina);
    }
    image.setTemplate(true);
    IMAGES.with(|cache| cache.borrow_mut().insert(name.to_string(), image.clone()));
    Some(image)
}

/// Системный символ SF Symbols (как `catalog="system"` в storyboard).
pub fn symbol(name: &str) -> Option<Retained<NSImage>> {
    NSImage::imageWithSystemSymbolName_accessibilityDescription(&ns(name), None)
}
