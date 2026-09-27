//! Окно «О Todo режиме» (`AboutTodoWindowController`, docs/ui-spec.md §11) —
//! открывается кнопкой «ⓘ» рядом с «Показывать Todo режим в меню».
//!
//! Тексты — русский перевод оригинала, «Rectangle» заменено именем приложения.
//! Из последнего абзаца убрано «или при помощи соответствующей горячей
//! клавиши»: горячих клавиш в порте нет.

use std::cell::RefCell;

use objc2::rc::Retained;
use objc2::MainThreadMarker;
use objc2_app_kit::{
    NSApplication, NSColor, NSFont, NSImage, NSImageNameApplicationIcon, NSImageScaling,
    NSImageView, NSLayoutAttribute, NSTextAlignment, NSTextField, NSUserInterfaceLayoutOrientation,
    NSView, NSWindow, NSWindowStyleMask, NSWindowTitleVisibility,
};
use objc2_foundation::{NSPoint, NSRect, NSSize};

use crate::app_delegate;
use crate::ui::controls::{self, ns, DEFAULT_HIGH};

/// Ширина текста (`width = 300 @750`).
const TEXT_WIDTH: f64 = 300.0;

thread_local! {
    static WINDOW: RefCell<Option<Retained<NSWindow>>> = const { RefCell::new(None) };
}

/// Показать окно (создать при первом вызове).
pub(crate) fn show(mtm: MainThreadMarker) {
    let window = WINDOW.with(|slot| slot.borrow_mut().get_or_insert_with(|| build(mtm)).clone());
    #[allow(deprecated)]
    NSApplication::sharedApplication(mtm).activateIgnoringOtherApps(true);
    window.makeKeyAndOrderFront(None);
}

fn secondary(
    mtm: MainThreadMarker,
    text: &str,
    alignment: NSTextAlignment,
) -> Retained<NSTextField> {
    let field = NSTextField::wrappingLabelWithString(&ns(text), mtm);
    field.setTextColor(Some(&NSColor::secondaryLabelColor()));
    field.setAlignment(alignment);
    field.setPreferredMaxLayoutWidth(TEXT_WIDTH);
    field
}

fn build(mtm: MainThreadMarker) -> Retained<NSWindow> {
    let app_name = app_delegate::app_name();

    let title = controls::label(mtm, "О Todo режиме");
    title.setFont(Some(&NSFont::systemFontOfSize(22.0)));

    let icon = NSImage::imageNamed(unsafe { NSImageNameApplicationIcon })
        .map(|image| NSImageView::imageViewWithImage(&image, mtm))
        .unwrap_or_else(|| NSImageView::new(mtm));
    icon.setImageScaling(NSImageScaling::ScaleProportionallyDown);
    icon.setTranslatesAutoresizingMaskIntoConstraints(false);
    controls::fix_width(&icon, 60.0);
    icon.heightAnchor()
        .constraintEqualToConstant(60.0)
        .setActive(true);

    let summary = secondary(
        mtm,
        "Держать выбранное приложение постоянно видимым в правой части основного экрана",
        NSTextAlignment::Center,
    );
    let steps = [
        "1. Разместите выбранное в качестве Todo приложение на переднем плане".to_string(),
        format!(
            "2. В меню {app_name} выберите \"Использовать [Приложение] в качестве приложения Todo\""
        ),
        format!("3.  В меню {app_name} включите Todo режим."),
    ]
    .map(|text| secondary(mtm, &text, NSTextAlignment::Left));
    let steps_stack = controls::column(mtm, 22.0, &[&steps[0], &steps[1], &steps[2]]);
    let reflow = secondary(
        mtm,
        &format!(
            "При включенном Todo режиме вы можете обновить положение окна Todo приложения, выбрав \"Обновить положение Todo окна\" в меню {app_name}."
        ),
        NSTextAlignment::Left,
    );

    let content = controls::stack(
        mtm,
        NSUserInterfaceLayoutOrientation::Vertical,
        NSLayoutAttribute::CenterX,
        22.0,
        &[&title, &icon, &summary, &steps_stack, &reflow],
    );
    let full_width: [&NSView; 3] = [&summary, &steps_stack, &reflow];
    for view in full_width {
        view.widthAnchor()
            .constraintEqualToAnchor(&content.widthAnchor())
            .setActive(true);
    }
    for step in &steps {
        step.widthAnchor()
            .constraintEqualToAnchor(&steps_stack.widthAnchor())
            .setActive(true);
    }
    controls::activate(
        content.widthAnchor().constraintEqualToConstant(TEXT_WIDTH),
        DEFAULT_HIGH,
    );

    let root = NSView::new(mtm);
    root.addSubview(&content);
    for constraint in [
        content
            .topAnchor()
            .constraintEqualToAnchor_constant(&root.topAnchor(), 30.0),
        root.bottomAnchor()
            .constraintEqualToAnchor_constant(&content.bottomAnchor(), 20.0),
        content
            .leadingAnchor()
            .constraintEqualToAnchor_constant(&root.leadingAnchor(), 20.0),
        root.trailingAnchor()
            .constraintEqualToAnchor_constant(&content.trailingAnchor(), 20.0),
    ] {
        constraint.setActive(true);
    }

    let window = super::new_window(
        mtm,
        NSRect::new(NSPoint::new(425.0, 462.0), NSSize::new(340.0, 424.0)),
        NSWindowStyleMask::Titled
            | NSWindowStyleMask::Closable
            | NSWindowStyleMask::Miniaturizable
            | NSWindowStyleMask::FullSizeContentView,
    );
    // Окно живёт до выхода, его держит `WINDOW`.
    unsafe { window.setReleasedWhenClosed(false) };
    window.setTitle(&ns("О Todo режиме"));
    window.setTitleVisibility(NSWindowTitleVisibility::Hidden);
    window.setTitlebarAppearsTransparent(true);
    window.setContentView(Some(&root));
    let size = root.fittingSize();
    window.setContentSize(size);
    super::clamp_to_screen(mtm, &window);
    window
}
