//! Окна значка стопки — пилюля с числом окон и список их названий
//! (`makeBadgeWindow`, `makeListWindow`, `appIcon` и `StackBadgeRowView`
//! оригинала; вид — docs/ui-spec.md §14). Рамки считает `geometry`, здесь —
//! только AppKit.

use objc2::rc::Retained;
use objc2::{
    define_class, msg_send, AllocAnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, Message,
};
use objc2_app_kit::{
    NSAutoresizingMaskOptions, NSBackingStoreType, NSColor, NSEvent, NSFloatingWindowLevel, NSFont,
    NSFontWeightSemibold, NSImage, NSImageScaling, NSImageSymbolConfiguration, NSImageView,
    NSLineBreakMode, NSPanel, NSResponder, NSRunningApplication, NSTextField, NSTrackingArea,
    NSTrackingAreaOptions, NSView, NSVisualEffectBlendingMode, NSVisualEffectMaterial,
    NSVisualEffectState, NSVisualEffectView, NSWindow, NSWindowCollectionBehavior,
    NSWindowStyleMask,
};
use objc2_foundation::{NSObject, NSPoint, NSRect, NSSize, NSString};
use objc2_quartz_core::kCACornerCurveContinuous;

use super::geometry::{self, Point, BADGE_HEIGHT};
use super::StackedWindow;
use crate::geometry::Rect;

/// Значок пилюли (SF Symbols) и его описание для VoiceOver.
const STACK_SYMBOL: &str = "rectangle.stack.fill";
const STACK_SYMBOL_DESCRIPTION: &str = "стопка окон";
/// Кегль значка и числа пилюли (полужирные).
const SYMBOL_POINT_SIZE: f64 = 14.0;
const COUNT_FONT_SIZE: f64 = 15.0;
/// Скругление списка.
const LIST_CORNER_RADIUS: f64 = 8.0;
/// Иконка приложения в строке: сторона и место.
const ICON_SIZE: f64 = 16.0;
const ICON_ORIGIN: (f64, f64) = (6.0, 3.0);
/// Название в строке: отступ слева, поле справа, высота, кегль.
const TITLE_X: f64 = 28.0;
const TITLE_RIGHT_INSET: f64 = 6.0;
const TITLE_HEIGHT: f64 = 16.0;
const TITLE_FONT_SIZE: f64 = 13.0;
/// Скругление подсветки строки.
const ROW_CORNER_RADIUS: f64 = 5.0;

fn ns_rect(rect: Rect) -> NSRect {
    NSRect::new(NSPoint::new(rect.x, rect.y), NSSize::new(rect.w, rect.h))
}

pub(super) fn rect(frame: NSRect) -> Rect {
    Rect::new(
        frame.origin.x,
        frame.origin.y,
        frame.size.width,
        frame.size.height,
    )
}

/// Скруглить вид слоем с «непрерывной» кривой углов, как у системных
/// элементов; `clip` — ещё и обрезать содержимое по скруглению.
fn round_corners(view: &NSView, radius: f64, clip: bool) {
    view.setWantsLayer(true);
    if let Some(layer) = view.layer() {
        layer.setCornerRadius(radius);
        // SAFETY: константа QuartzCore, живёт всё время работы процесса.
        layer.setCornerCurve(unsafe { kCACornerCurveContinuous });
        if clip {
            layer.setMasksToBounds(true);
        }
    }
}

/// Общие свойства пилюли и списка: прозрачный фон, над обычными окнами, с
/// тенью, вне Mission Control и перебора окон (⌘`).
fn set_overlay_style(window: &NSWindow) {
    window.setOpaque(false);
    window.setBackgroundColor(Some(&NSColor::clearColor()));
    window.setLevel(NSFloatingWindowLevel);
    window.setHasShadow(true);
    // Окно держит менеджер, а не AppKit.
    unsafe { window.setReleasedWhenClosed(false) };
    window.setCollectionBehavior(
        NSWindowCollectionBehavior::Transient | NSWindowCollectionBehavior::IgnoresCycle,
    );
}

// ---------------------------------------------------------------- пилюля

/// Значок `rectangle.stack.fill` 14 pt полужирный; нет такого значка — `None`,
/// и в пилюле остаётся только число.
fn stack_symbol() -> Option<Retained<NSImage>> {
    let symbol = NSImage::imageWithSystemSymbolName_accessibilityDescription(
        &NSString::from_str(STACK_SYMBOL),
        Some(&NSString::from_str(STACK_SYMBOL_DESCRIPTION)),
    )?;
    // SAFETY: константа AppKit, живёт всё время работы процесса.
    let weight = unsafe { NSFontWeightSemibold };
    let configuration =
        NSImageSymbolConfiguration::configurationWithPointSize_weight(SYMBOL_POINT_SIZE, weight);
    symbol.imageWithSymbolConfiguration(&configuration)
}

/// Пилюля с числом окон у верхнего левого края стопки (`makeBadgeWindow`):
/// сплошная капсула цвета акцента системы со значком стопки и числом белым,
/// по размеру содержимого; верхний левый угол — `anchor` (AppKit). Щелчки
/// проходят насквозь: их принимает список, а не пилюля.
pub(super) fn make_badge_window(
    mtm: MainThreadMarker,
    count: usize,
    anchor: Point,
) -> Retained<NSWindow> {
    let label = NSTextField::labelWithString(&NSString::from_str(&count.to_string()), mtm);
    // SAFETY: константа AppKit, живёт всё время работы процесса.
    let weight = unsafe { NSFontWeightSemibold };
    label.setFont(Some(&NSFont::systemFontOfSize_weight(
        COUNT_FONT_SIZE,
        weight,
    )));
    // Белый читается на насыщенном цвете акцента в любом оформлении.
    label.setTextColor(Some(&NSColor::whiteColor()));
    label.sizeToFit();
    let label_size = label.frame().size;

    let symbol_view = stack_symbol().map(|symbol| {
        let view = NSImageView::imageViewWithImage(&symbol, mtm);
        view.setContentTintColor(Some(&NSColor::whiteColor()));
        view
    });
    let layout = geometry::badge_layout(
        anchor,
        (label_size.width.ceil(), label_size.height.ceil()),
        symbol_view.is_some(),
    );

    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(mtm),
            ns_rect(layout.frame),
            NSWindowStyleMask::Borderless,
            NSBackingStoreType::Buffered,
            false,
        )
    };
    set_overlay_style(&window);
    window.setIgnoresMouseEvents(true);

    let bounds = Rect::new(0.0, 0.0, layout.frame.w, layout.frame.h);
    let container = NSView::initWithFrame(NSView::alloc(mtm), ns_rect(bounds));
    round_corners(&container, BADGE_HEIGHT / 2.0, true);
    if let Some(layer) = container.layer() {
        // Выбранный пользователем цвет акцента: пилюля заметна на любом окне.
        layer.setBackgroundColor(Some(&NSColor::controlAccentColor().CGColor()));
    }
    if let (Some(view), Some(frame)) = (&symbol_view, layout.symbol) {
        view.setFrame(ns_rect(frame));
        container.addSubview(view);
    }
    label.setFrame(ns_rect(layout.label));
    container.addSubview(&label);

    window.setContentView(Some(&container));
    window
}

// ---------------------------------------------------------------- список

/// Показанный список: панель и её строки (строки нужны проверкам).
pub(super) struct ListWindow {
    pub panel: Retained<NSPanel>,
    pub rows: Vec<Retained<RowView>>,
}

/// Список названий окон (`makeListWindow`): открывается вниз от `list_top`
/// (AppKit) и не выходит за рабочую область `screen_frame`, чтобы до каждой
/// строки можно было дотянуться. Неактивирующая панель — щелчок по названию не
/// делает активным само приложение. Щелчок по строке вызывает `on_select` с её
/// окном.
pub(super) fn make_list_window(
    mtm: MainThreadMarker,
    windows: &[StackedWindow],
    list_top: Point,
    screen_frame: &Rect,
    on_select: fn(&StackedWindow),
) -> ListWindow {
    let frame = geometry::list_frame(windows.len(), list_top, screen_frame);
    let panel = NSPanel::initWithContentRect_styleMask_backing_defer(
        NSPanel::alloc(mtm),
        ns_rect(frame),
        NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel,
        NSBackingStoreType::Buffered,
        false,
    );
    set_overlay_style(&panel);

    let bounds = Rect::new(0.0, 0.0, frame.w, frame.h);
    let container =
        NSVisualEffectView::initWithFrame(NSVisualEffectView::alloc(mtm), ns_rect(bounds));
    container.setMaterial(NSVisualEffectMaterial::HUDWindow);
    container.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
    container.setState(NSVisualEffectState::Active);
    // Маска заодно прячет строки, не влезшие в обрезанный у низа экрана список.
    round_corners(&container, LIST_CORNER_RADIUS, true);

    let mut rows = Vec::with_capacity(windows.len());
    for (index, window) in windows.iter().enumerate() {
        let selected = window.clone();
        let row = RowView::new(
            mtm,
            &window.title,
            app_icon(window.pid).as_deref(),
            Box::new(move || on_select(&selected)),
        );
        row.setFrame(ns_rect(geometry::row_frame(index, frame.h)));
        container.addSubview(&row);
        rows.push(row);
    }

    panel.setContentView(Some(&container));
    ListWindow { panel, rows }
}

/// Иконка запущенного приложения, перерисованная в чёткую копию размером со
/// строку, — общую иконку в полном разрешении не трогаем (`appIcon`).
fn app_icon(pid: i32) -> Option<Retained<NSImage>> {
    let icon = NSRunningApplication::runningApplicationWithProcessIdentifier(pid)?.icon()?;
    let size = NSSize::new(ICON_SIZE, ICON_SIZE);
    let resized = NSImage::initWithSize(NSImage::alloc(), size);
    // Как в оригинале: `lockFocus` рисует в растр под масштаб экрана.
    #[allow(deprecated)]
    {
        resized.lockFocus();
        icon.drawInRect(NSRect::new(NSPoint::new(0.0, 0.0), size));
        resized.unlockFocus();
    }
    Some(resized)
}

// ---------------------------------------------------------------- строка

/// Что делает щелчок по строке.
type OnClick = Box<dyn Fn()>;

pub(super) struct RowIvars {
    title: Retained<NSTextField>,
    on_click: OnClick,
}

define_class!(
    /// Строка списка (`StackBadgeRowView`): иконка приложения и название окна.
    /// Под курсором подсвечивается, как пункт меню, — системным цветом
    /// выделения с белым текстом; щелчок выводит окно вперёд.
    #[unsafe(super(NSView, NSResponder, NSObject))]
    #[name = "R2StackBadgeRowView"]
    #[thread_kind = MainThreadOnly]
    #[ivars = RowIvars]
    pub(super) struct RowView;

    impl RowView {
        #[unsafe(method(layout))]
        fn layout(&self) {
            unsafe {
                let _: () = msg_send![super(self), layout];
            }
            let bounds = self.bounds();
            self.ivars().title.setFrame(NSRect::new(
                NSPoint::new(TITLE_X, (bounds.size.height - TITLE_HEIGHT) / 2.0),
                NSSize::new(bounds.size.width - TITLE_X - TITLE_RIGHT_INSET, TITLE_HEIGHT),
            ));
        }

        #[unsafe(method(updateTrackingAreas))]
        fn update_tracking_areas(&self) {
            unsafe {
                let _: () = msg_send![super(self), updateTrackingAreas];
            }
            for area in self.trackingAreas().iter() {
                self.removeTrackingArea(&area);
            }
            // SAFETY: владелец — сама строка, она же получает события наведения;
            // область живёт не дольше её.
            let area = unsafe {
                NSTrackingArea::initWithRect_options_owner_userInfo(
                    NSTrackingArea::alloc(),
                    self.bounds(),
                    NSTrackingAreaOptions::MouseEnteredAndExited
                        | NSTrackingAreaOptions::ActiveAlways,
                    Some(self),
                    None,
                )
            };
            self.addTrackingArea(&area);
        }

        #[unsafe(method(mouseEntered:))]
        fn mouse_entered(&self, _event: &NSEvent) {
            self.set_selected(true);
        }

        #[unsafe(method(mouseExited:))]
        fn mouse_exited(&self, _event: &NSEvent) {
            self.set_selected(false);
        }

        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, event: &NSEvent) {
            let point = self.convertPoint_fromView(event.locationInWindow(), None);
            if geometry::contains_point(&rect(self.bounds()), (point.x, point.y)) {
                self.click();
            }
        }

        /// Щелчок в любом месте строки — и по иконке, и по названию —
        /// достаётся самой строке: вся строка — одна цель.
        #[unsafe(method_id(hitTest:))]
        fn hit_test(&self, point: NSPoint) -> Option<Retained<NSView>> {
            // SAFETY: AppKit спрашивает строку, пока она в иерархии видов, —
            // родитель жив.
            let superview = unsafe { self.superview() };
            let local = self.convertPoint_fromView(point, superview.as_deref());
            geometry::contains_point(&rect(self.bounds()), (local.x, local.y))
                .then(|| Retained::into_super(self.retain()))
        }

        /// Список — неактивирующая панель в фоне: без этого первый щелчок по
        /// строке, пока активно другое приложение, ушёл бы на активацию, а не
        /// на вывод окна вперёд.
        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _event: Option<&NSEvent>) -> bool {
            true
        }
    }
);

impl RowView {
    fn new(
        mtm: MainThreadMarker,
        title: &str,
        icon: Option<&NSImage>,
        on_click: OnClick,
    ) -> Retained<Self> {
        let title = NSTextField::labelWithString(&NSString::from_str(title), mtm);
        let this = Self::alloc(mtm).set_ivars(RowIvars {
            title: title.clone(),
            on_click,
        });
        let this: Retained<Self> = unsafe {
            msg_send![super(this), initWithFrame: NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(0.0, 0.0))]
        };
        round_corners(&this, ROW_CORNER_RADIUS, false);

        let icon_view = NSImageView::initWithFrame(
            NSImageView::alloc(mtm),
            NSRect::new(
                NSPoint::new(ICON_ORIGIN.0, ICON_ORIGIN.1),
                NSSize::new(ICON_SIZE, ICON_SIZE),
            ),
        );
        icon_view.setImage(icon);
        icon_view.setImageScaling(NSImageScaling::ScaleProportionallyUpOrDown);
        this.addSubview(&icon_view);

        title.setFont(Some(&NSFont::systemFontOfSize(TITLE_FONT_SIZE)));
        title.setTextColor(Some(&NSColor::labelColor()));
        title.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
        title.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable);
        this.addSubview(&title);
        this
    }

    /// Подсветка строки под курсором.
    pub(super) fn set_selected(&self, selected: bool) {
        if let Some(layer) = self.layer() {
            let color = selected.then(|| NSColor::selectedContentBackgroundColor().CGColor());
            layer.setBackgroundColor(color.as_deref());
        }
        let text_color = if selected {
            NSColor::selectedMenuItemTextColor()
        } else {
            NSColor::labelColor()
        };
        self.ivars().title.setTextColor(Some(&text_color));
    }

    /// Действие строки — то же, что щелчок по ней.
    pub(super) fn click(&self) {
        (self.ivars().on_click)();
    }
}
