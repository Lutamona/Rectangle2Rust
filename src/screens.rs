//! Экраны (NSScreen) и положение курсора (NSEvent).
//!
//! Все координаты — как у Cocoa (origin снизу слева). NSScreen — API главного
//! потока: вне его `screens()` возвращает пустой список, а не лезет в AppKit
//! (проверки с настоящими экранами — `examples/screens_check.rs`, он работает
//! на главном потоке).

use objc2::rc::Retained;
use objc2::runtime::NSObjectProtocol;
use objc2::{sel, MainThreadMarker};
use objc2_app_kit::{NSEvent, NSScreen};
use objc2_foundation::{NSNumber, NSRect, NSString};

use crate::geometry::Rect;

/// Один дисплей — снимок `NSScreen` на момент вызова.
#[derive(Clone, Debug, PartialEq)]
pub struct Screen {
    /// `deviceDescription["NSScreenNumber"]` (CGDirectDisplayID); 0 — не удалось узнать.
    /// По нему экраны сравниваются между собой, как объекты `NSScreen` в Swift.
    pub id: u32,
    /// `NSScreen.frame` — весь экран, координаты Cocoa.
    pub frame: Rect,
    /// `NSScreen.visibleFrame` — рабочая область (без меню-бара и дока).
    pub visible_frame: Rect,
    /// `localizedName`, может быть пустым.
    pub name: String,
    /// `NSScreen.main` — экран, на котором ключевое окно. Это не основной экран:
    /// основной — `screens()[0]` (меню-бар, начало координат).
    pub is_main: bool,
    /// `backingScaleFactor` (1.0 / 2.0 …).
    pub scale: f64,
    /// `safeAreaInsets.top` — высота выреза камеры; 0 — выреза нет (или macOS < 12).
    pub safe_area_top: f64,
}

impl Screen {
    /// Тот же дисплей. Номера дисплеев надёжнее рамок; рамки сравниваются,
    /// только если номер неизвестен.
    pub fn same_display(&self, other: &Screen) -> bool {
        if self.id != 0 && other.id != 0 {
            self.id == other.id
        } else {
            self.frame == other.frame
        }
    }
}

fn nsrect_to_rect(r: NSRect) -> Rect {
    Rect::new(r.origin.x, r.origin.y, r.size.width, r.size.height)
}

fn display_id(screen: &NSScreen) -> u32 {
    let description = screen.deviceDescription();
    let key = NSString::from_str("NSScreenNumber");
    description
        .objectForKey(&key)
        .and_then(|value| value.downcast::<NSNumber>().ok())
        .map(|number: Retained<NSNumber>| number.unsignedIntValue())
        .unwrap_or(0)
}

fn safe_area_top(screen: &NSScreen) -> f64 {
    // `safeAreaInsets` появился в macOS 12 (`#available(macOS 12.0, *)` в Swift).
    if screen.respondsToSelector(sel!(safeAreaInsets)) {
        screen.safeAreaInsets().top
    } else {
        0.0
    }
}

/// `NSScreen.screens` — порядок как у системы, `[0]` — основной экран.
/// Вне главного потока — пустой список.
pub fn screens() -> Vec<Screen> {
    let Some(mtm) = MainThreadMarker::new() else {
        return Vec::new();
    };
    let list = NSScreen::screens(mtm);
    let main_id = NSScreen::mainScreen(mtm).map(|main| display_id(&main));
    let mut out = Vec::with_capacity(list.len());
    for screen in list.iter() {
        let id = display_id(&screen);
        out.push(Screen {
            id,
            frame: nsrect_to_rect(screen.frame()),
            visible_frame: nsrect_to_rect(screen.visibleFrame()),
            name: screen.localizedName().to_string(),
            is_main: main_id.is_some_and(|main| main != 0 && main == id),
            scale: screen.backingScaleFactor(),
            safe_area_top: safe_area_top(&screen),
        });
    }
    out
}

/// `NSScreen.screens[0].frame.maxY` — база для переворота координат Cocoa ↔ AX.
/// 0.0, если экранов нет (или вызов не с главного потока).
pub fn primary_screen_height() -> f64 {
    screens().first().map(|s| s.frame.max_y()).unwrap_or(0.0)
}

/// `NSScreen.screensHaveSeparateSpaces` — «Мониторы с отдельными пространствами»
/// (в macOS включено по умолчанию). Вне главного потока — `true`.
pub fn screens_have_separate_spaces() -> bool {
    match MainThreadMarker::new() {
        Some(mtm) => NSScreen::screensHaveSeparateSpaces(mtm),
        None => true,
    }
}

/// `NSEvent.mouseLocation` — координаты Cocoa (y снизу).
/// На практике всегда `Some`; `None` не предусмотрено API, оставлено
/// `Option` по контракту вызывающего кода.
pub fn cursor_position() -> Option<(f64, f64)> {
    let p = NSEvent::mouseLocation();
    Some((p.x, p.y))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen(id: u32, frame: Rect) -> Screen {
        Screen {
            id,
            frame,
            visible_frame: frame,
            name: String::new(),
            is_main: false,
            scale: 2.0,
            safe_area_top: 0.0,
        }
    }

    #[test]
    fn screens_are_not_read_off_the_main_thread() {
        // libtest запускает тесты в своих потоках: AppKit не трогаем, списка нет.
        if MainThreadMarker::new().is_none() {
            assert!(screens().is_empty());
            assert_eq!(primary_screen_height(), 0.0);
            assert!(screens_have_separate_spaces());
        }
    }

    #[test]
    fn same_display_prefers_display_numbers() {
        let a = screen(1, Rect::new(0.0, 0.0, 1728.0, 1117.0));
        let moved = screen(1, Rect::new(0.0, 0.0, 1512.0, 982.0));
        let other = screen(2, Rect::new(0.0, 0.0, 1728.0, 1117.0));
        assert!(a.same_display(&moved));
        assert!(!a.same_display(&other));

        // Номер неизвестен — сравниваем рамки.
        let unknown = screen(0, Rect::new(0.0, 0.0, 1728.0, 1117.0));
        assert!(unknown.same_display(&other));
        assert!(!unknown.same_display(&moved));
    }

    #[test]
    fn cursor_position_does_not_crash() {
        // Просто не должно падать; позиция зависит от мыши.
        let _ = cursor_position();
    }
}
