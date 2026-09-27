//! Подсветка будущего места окна — порт `Snapping/FootprintWindow.swift`.
//!
//! Окно с заголовком, но без видимого заголовка и кнопок, уровня модальной
//! панели, без тени; содержимое — `NSBox` с заливкой `footprintColor` (по
//! умолчанию чёрной) и светло-серой рамкой `footprintBorderWidth`, прозрачность
//! окна — `footprintAlpha`. Если `footprintFade` не выключено, подсветка
//! проявляется и гаснет анимацией прозрачности; при
//! `footprintAnimationDurationMultiplier` > 0 она ещё и вырастает из угла или
//! края своей области (`show`).
//!
//! Вид подсветки читается при создании окна; менеджер пересоздаёт окно, когда
//! вид в настройках поменялся (`FootprintStyle`).

use std::cell::Cell;
use std::ptr::NonNull;
use std::rc::Rc;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::NSObjectProtocol;
use objc2::{define_class, msg_send, ClassType, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSAnimatablePropertyContainer, NSAnimationContext, NSApplication, NSBackingStoreType, NSBox,
    NSBoxType, NSColor, NSModalPanelWindowLevel, NSResponder, NSUserInterfaceItemIdentification,
    NSWindow, NSWindowButton, NSWindowCollectionBehavior, NSWindowStyleMask,
    NSWindowTitleVisibility,
};
use objc2_foundation::{NSObject, NSPoint, NSRect, NSSize, NSString};

use crate::config::{Config, Directional, FootprintColor};
use crate::geometry::Rect;
use crate::stage;

use super::zones;

/// Вид подсветки из настроек.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FootprintStyle {
    /// `footprintAlpha` — как `CGFloat` из `Float` в Swift (важно для сравнения
    /// прозрачности окна с ним).
    pub alpha: f64,
    /// `footprintBorderWidth`.
    pub border_width: f64,
    /// `footprintColor`; `None` — чёрный.
    pub color: Option<FootprintColor>,
    /// Плавное появление: `footprintFade` не выключено явно.
    pub fade: bool,
}

impl FootprintStyle {
    pub fn from_config(config: &Config) -> FootprintStyle {
        FootprintStyle {
            alpha: config.footprint_alpha as f64,
            border_width: config.footprint_border_width as f64,
            color: config.footprint_color,
            fade: config.footprint_fade != Some(false),
        }
    }
}

/// Скругление подсветки: как у окон своей версии macOS.
fn corner_radius() -> f64 {
    if crate::mac_tiling::macos_at_least(26, 0) {
        16.0
    } else if crate::mac_tiling::macos_at_least(11, 0) {
        10.0
    } else {
        5.0
    }
}

pub struct FootprintIvars {
    alpha: f64,
    fade: bool,
}

define_class!(
    /// `FootprintWindow`: `NSWindow` со своим `isVisible`.
    #[unsafe(super(NSWindow, NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "R2FootprintWindow"]
    #[ivars = FootprintIvars]
    pub struct FootprintWindow;

    impl FootprintWindow {
        /// Пока полоса Stage Manager на экране, подсветка всегда «видима» —
        /// иначе Stage Manager её выталкивает (обход из оригинала).
        #[unsafe(method(isVisible))]
        fn is_visible(&self) -> bool {
            // Без `return`: макрос оборачивает тело и переводит `bool` в `BOOL`.
            (super::stage_capable() && stage::stage_enabled() && stage::stage_strip_show())
                || self.real_is_visible()
        }
    }
);

impl FootprintWindow {
    fn new(mtm: MainThreadMarker, style: &FootprintStyle) -> Retained<FootprintWindow> {
        let this = FootprintWindow::alloc(mtm).set_ivars(FootprintIvars {
            alpha: style.alpha,
            fade: style.fade,
        });
        let initial = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(0.0, 0.0));
        let window: Retained<FootprintWindow> = unsafe {
            msg_send![
                super(this),
                initWithContentRect: initial,
                styleMask: NSWindowStyleMask::Titled,
                backing: NSBackingStoreType::Buffered,
                defer: false
            ]
        };

        window.setTitle(&NSString::from_str(&crate::app_delegate::app_name()));
        window.setIdentifier(Some(&NSString::from_str(FOOTPRINT_IDENTIFIER)));
        window.setOpaque(false);
        window.setLevel(NSModalPanelWindowLevel);
        window.setHasShadow(false);
        // Окно держит `Footprint`, а не AppKit.
        unsafe { window.setReleasedWhenClosed(false) };
        window.setAlphaValue(if style.fade { 0.0 } else { style.alpha });
        window.setStyleMask(window.styleMask() | NSWindowStyleMask::FullSizeContentView);
        window.setTitleVisibility(NSWindowTitleVisibility::Hidden);
        window.setTitlebarAppearsTransparent(true);
        window.setCollectionBehavior(
            window.collectionBehavior() | NSWindowCollectionBehavior::Transient,
        );
        for button in [
            NSWindowButton::CloseButton,
            NSWindowButton::MiniaturizeButton,
            NSWindowButton::ZoomButton,
            NSWindowButton::ToolbarButton,
        ] {
            if let Some(button) = window.standardWindowButton(button) {
                button.setHidden(true);
            }
        }

        let content = NSBox::new(mtm);
        content.setBoxType(NSBoxType::Custom);
        content.setBorderColor(&NSColor::lightGrayColor());
        content.setBorderWidth(style.border_width);
        content.setCornerRadius(corner_radius());
        content.setWantsLayer(true);
        let fill = match style.color {
            Some(color) => NSColor::colorWithRed_green_blue_alpha(
                color.red,
                color.green,
                color.blue,
                color.alpha.unwrap_or(1.0),
            ),
            None => NSColor::blackColor(),
        };
        content.setFillColor(&fill);
        window.setContentView(Some(&content));
        window
    }

    /// `realIsVisible`: без плавного появления — виден ли на самом деле;
    /// с ним — проявился ли полностью.
    fn real_is_visible(&self) -> bool {
        if self.ivars().fade {
            self.alphaValue() == self.ivars().alpha
        } else {
            unsafe { msg_send![super(self), isVisible] }
        }
    }
}

/// `NSWindow.identifier` окна подсветки: по нему её узнают среди окон
/// приложения другие подсистемы (сдвиг лесенкой не считает её занятым местом).
pub const FOOTPRINT_IDENTIFIER: &str = "FootprintWindow";

/// Номера окон подсветки среди окон приложения — как
/// `NSApp.windows.compactMap { $0 is FootprintWindow ? $0.windowNumber : nil }`
/// в `OverlapOffsetGeometry` оригинала. Окно живёт, пока прилипание включено
/// (спрятанное — тоже), и ещё гаснет на экране, когда окно уже ставят на место.
pub fn footprint_window_numbers(mtm: MainThreadMarker) -> Vec<isize> {
    NSApplication::sharedApplication(mtm)
        .windows()
        .iter()
        .filter(|window| window.isKindOfClass(FootprintWindow::class()))
        .map(|window| window.windowNumber())
        .collect()
}

/// Подсветка: окно и то, что нужно его анимациям.
pub struct Footprint {
    window: Retained<FootprintWindow>,
    style: FootprintStyle,
    /// `orderOutCanceled`: подсветку снова показали, пока она гасла.
    order_out_canceled: Rc<Cell<bool>>,
}

impl Footprint {
    pub fn new(mtm: MainThreadMarker, style: FootprintStyle) -> Footprint {
        Footprint {
            window: FootprintWindow::new(mtm, &style),
            style,
            order_out_canceled: Rc::new(Cell::new(false)),
        }
    }

    /// С какими настройками создана.
    pub fn style(&self) -> &FootprintStyle {
        &self.style
    }

    /// Номер окна в WindowServer (для снимков `screencapture -l`).
    pub fn window_number(&self) -> isize {
        self.window.windowNumber()
    }

    /// Рамка окна сейчас (Cocoa).
    pub fn frame(&self) -> Rect {
        let frame = self.window.frame();
        Rect::new(
            frame.origin.x,
            frame.origin.y,
            frame.size.width,
            frame.size.height,
        )
    }

    /// Прозрачность окна сейчас.
    pub fn alpha(&self) -> f64 {
        self.window.alphaValue()
    }

    /// Подсветка на экране (и, если она проявляется плавно, проявилась).
    pub fn real_is_visible(&self) -> bool {
        self.window.real_is_visible()
    }

    /// Показать подсветку области с направлением `directional` в рамке `rect`
    /// (Cocoa). `animation_multiplier` > 0 — рамка вырастает из края области
    /// за время, которое AppKit считает для такого изменения, умноженное на
    /// него (`footprintAnimationDurationMultiplier`).
    pub fn show(&self, rect: Rect, directional: Directional, animation_multiplier: f64) {
        let frame = ns_rect(&rect);
        if animation_multiplier > 0.0 {
            if !self.real_is_visible() {
                if let Some((x, y)) = zones::footprint_animation_origin(directional, &rect) {
                    let origin = NSRect::new(NSPoint::new(x, y), NSSize::new(0.0, 0.0));
                    self.window.setFrame_display(origin, false);
                }
            }
        } else {
            self.window.setFrame_display(frame, true);
        }
        self.order_front();
        if animation_multiplier > 0.0 {
            let window = self.window.clone();
            let duration = window.animationResizeTime(frame) * animation_multiplier;
            let changes = RcBlock::new(move |context: NonNull<NSAnimationContext>| {
                unsafe { context.as_ref() }.setDuration(duration);
                let window: &NSWindow = &window;
                window.animator().setFrame_display(frame, true);
            });
            NSAnimationContext::runAnimationGroup(&changes);
        }
    }

    /// `orderFront`: вывести, с плавным появлением — проявить до `footprintAlpha`.
    pub fn order_front(&self) {
        if !self.style.fade {
            self.window.orderFront(None);
            return;
        }
        self.order_out_canceled.set(true);
        self.window.orderFront(None);
        let window: &NSWindow = &self.window;
        window.animator().setAlphaValue(self.style.alpha);
    }

    /// `orderOut`: убрать, с плавным появлением — сначала погасить.
    pub fn order_out(&self) {
        if !self.style.fade {
            self.window.orderOut(None);
            return;
        }
        self.order_out_canceled.set(false);
        let fading = self.window.clone();
        let changes = RcBlock::new(move |_context: NonNull<NSAnimationContext>| {
            let window: &NSWindow = &fading;
            window.animator().setAlphaValue(0.0);
        });
        let window = self.window.clone();
        let canceled = self.order_out_canceled.clone();
        let completion = RcBlock::new(move || {
            if !canceled.get() {
                window.orderOut(None);
            }
        });
        NSAnimationContext::runAnimationGroup_completionHandler(&changes, Some(&completion));
    }
}

impl Drop for Footprint {
    /// `box = nil`: окно уходит с экрана сразу.
    fn drop(&mut self) {
        self.order_out_canceled.set(true);
        self.window.orderOut(None);
    }
}

fn ns_rect(rect: &Rect) -> NSRect {
    NSRect::new(NSPoint::new(rect.x, rect.y), NSSize::new(rect.w, rect.h))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn style_follows_settings() {
        let style = FootprintStyle::from_config(&Config::default());
        assert_eq!(style.alpha, 0.3f32 as f64);
        assert_eq!(style.border_width, 2.0);
        assert_eq!(style.color, None);
        assert!(style.fade);

        let custom = FootprintStyle::from_config(&Config {
            footprint_alpha: 0.6,
            footprint_border_width: 5.0,
            footprint_fade: Some(false),
            footprint_color: Some(FootprintColor {
                red: 1.0,
                green: 0.0,
                blue: 0.0,
                alpha: None,
            }),
            ..Config::default()
        });
        assert_eq!(custom.alpha, 0.6f32 as f64);
        assert_eq!(custom.border_width, 5.0);
        assert!(!custom.fade);
        assert_eq!(custom.color.map(|color| color.red), Some(1.0));
        // Явное «да» — тоже плавно.
        assert!(
            FootprintStyle::from_config(&Config {
                footprint_fade: Some(true),
                ..Config::default()
            })
            .fade
        );
    }

    #[test]
    fn corner_radius_matches_this_macos() {
        let radius = corner_radius();
        assert!(radius == 16.0 || radius == 10.0);
    }
}
