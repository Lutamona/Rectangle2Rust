//! Stage Manager — порт `Utilities/StageUtil.swift`.
//!
//! Включён ли Stage Manager и видна ли его полоса с миниатюрами: настройки
//! `com.apple.WindowManager` и `com.apple.dock`, окна процесса WindowManager в
//! списке окон, группы окон полосы через AX. Рабочая область экрана без полосы
//! (`stageSize`) считается в `screen_detection::adjusted_visible_frame`.

use std::ffi::{c_char, c_int, c_void, CString};
use std::sync::OnceLock;

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::AnyThread;
use objc2_foundation::{NSLocale, NSLocaleLanguageDirection, NSNumber, NSString, NSUserDefaults};

use crate::ax::{self, AxElement};
use crate::geometry::Rect;
use crate::screens::{self, Screen};

/// С какой стороны экрана полоса Stage Manager.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StageStripPosition {
    Left,
    Right,
}

extern "C" {
    fn sysctlbyname(
        name: *const c_char,
        oldp: *mut c_void,
        oldlenp: *mut usize,
        newp: *mut c_void,
        newlen: usize,
    ) -> c_int;
}

/// Старший номер версии macOS (`kern.osproductversion`); 0 — не удалось узнать.
fn macos_major_version() -> u32 {
    let name = CString::new("kern.osproductversion").expect("имя без нулей");
    let mut buffer = [0u8; 32];
    let mut length = buffer.len();
    let status = unsafe {
        sysctlbyname(
            name.as_ptr(),
            buffer.as_mut_ptr() as *mut c_void,
            &mut length,
            std::ptr::null_mut(),
            0,
        )
    };
    if status != 0 {
        return 0;
    }
    let text = String::from_utf8_lossy(&buffer[..length.min(buffer.len())]);
    parse_major_version(text.trim_end_matches('\0'))
}

fn parse_major_version(text: &str) -> u32 {
    text.split('.')
        .next()
        .and_then(|major| major.trim().parse().ok())
        .unwrap_or(0)
}

/// Stage Manager бывает только в macOS 13 и новее (`stageCapable`).
pub fn stage_capable() -> bool {
    macos_major_version() >= 13
}

fn suite(name: &str) -> Option<Retained<NSUserDefaults>> {
    NSUserDefaults::initWithSuiteName(NSUserDefaults::alloc(), Some(&NSString::from_str(name)))
}

/// Настройки `com.apple.WindowManager` — один экземпляр на всё время работы,
/// как `StageUtil.windowManagerDefaults` (прилипание читает их на каждом
/// событии перетаскивания). NSUserDefaults потокобезопасен.
fn window_manager_defaults() -> Option<&'static NSUserDefaults> {
    static DEFAULTS: OnceLock<Option<Retained<NSUserDefaults>>> = OnceLock::new();
    DEFAULTS
        .get_or_init(|| suite("com.apple.WindowManager"))
        .as_deref()
}

/// Настройки `com.apple.dock` — как `StageUtil.dockDefaults`.
fn dock_defaults() -> Option<&'static NSUserDefaults> {
    static DEFAULTS: OnceLock<Option<Retained<NSUserDefaults>>> = OnceLock::new();
    DEFAULTS.get_or_init(|| suite("com.apple.dock")).as_deref()
}

/// `object(forKey:) as? Bool`: число 0/1 (в том числе `CFBoolean`), иначе `None`.
fn object_as_bool(object: &AnyObject) -> Option<bool> {
    let number = object.downcast_ref::<NSNumber>()?;
    let value = number.doubleValue();
    if value == 0.0 {
        Some(false)
    } else if value == 1.0 {
        Some(true)
    } else {
        None
    }
}

fn window_manager_bool(key: &str) -> Option<bool> {
    let defaults = window_manager_defaults()?;
    let object = defaults.objectForKey(&NSString::from_str(key))?;
    object_as_bool(&object)
}

/// Stage Manager включён (`com.apple.WindowManager GloballyEnabled`).
pub fn stage_enabled() -> bool {
    window_manager_bool("GloballyEnabled").unwrap_or(false)
}

/// Полоса не прячется (`AutoHide` не задан как «да»).
pub fn stage_strip_show() -> bool {
    match window_manager_bool("AutoHide") {
        Some(auto_hide) => !auto_hide,
        None => false,
    }
}

/// Сторона полосы: напротив Дока, а при Доке снизу — слева (справа при языке
/// с письмом справа налево).
pub fn stage_strip_position() -> StageStripPosition {
    let orientation = dock_defaults()
        .and_then(|defaults| defaults.stringForKey(&NSString::from_str("orientation")))
        .map(|value| value.to_string());
    strip_position_for(orientation.as_deref(), locale_is_right_to_left())
}

fn strip_position_for(dock_orientation: Option<&str>, right_to_left: bool) -> StageStripPosition {
    match dock_orientation {
        Some("left") => StageStripPosition::Right,
        Some("right") => StageStripPosition::Left,
        _ if right_to_left => StageStripPosition::Right,
        _ => StageStripPosition::Left,
    }
}

/// `Locale.current.language.characterDirection == .rightToLeft`.
fn locale_is_right_to_left() -> bool {
    let locale = NSLocale::currentLocale();
    let language = locale.languageCode();
    NSLocale::characterDirectionForLanguage(&language) == NSLocaleLanguageDirection::RightToLeft
}

/// На каких экранах видна полоса: у каждого окна процесса WindowManager ищем
/// экран, в который оно входит по вертикали и к краю которого (со стороны
/// полосы) оно ближе всего. Полоса видна там, где таких окон хотя бы два —
/// одно окно может оказаться просто перетаскиваемым (`isStageStripVisible`).
///
/// `window_frames` — рамки окон WindowManager в координатах Cocoa.
pub fn strip_visible_on(
    screen: &Screen,
    screens: &[Screen],
    window_frames: &[Rect],
    position: StageStripPosition,
) -> bool {
    let count = window_frames
        .iter()
        .filter(|frame| {
            let candidates = screens
                .iter()
                .filter(|s| s.frame.min_y() <= frame.min_y() && frame.max_y() <= s.frame.max_y());
            let distance = |s: &&Screen| match position {
                StageStripPosition::Left => (frame.min_x() - s.frame.min_x()).abs(),
                StageStripPosition::Right => (s.frame.max_x() - frame.max_x()).abs(),
            };
            // `min(by:)` Swift: при равенстве остаётся первый.
            let mut best: Option<&Screen> = None;
            for candidate in candidates {
                if best.is_none_or(|current| distance(&candidate) < distance(&current)) {
                    best = Some(candidate);
                }
            }
            best.is_some_and(|best| best.same_display(screen))
        })
        .count();
    count >= 2
}

/// Рамки окон процесса WindowManager (Cocoa) — для `strip_visible_on`.
pub fn stage_window_frames(primary_height: f64) -> Vec<Rect> {
    ax::window_list()
        .into_iter()
        .filter(|info| info.process_name.as_deref() == Some("WindowManager"))
        .map(|info| info.frame.screen_flipped(primary_height))
        .collect()
}

/// Что известно о Stage Manager на момент действия: где полоса отнимает место.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StageState {
    /// Полоса включена и не прячется (`stageCapable && stageEnabled && stageStripShow`).
    pub active: bool,
    pub position: Option<StageStripPosition>,
    /// Номера дисплеев, на которых полоса сейчас видна.
    pub visible_on: Vec<u32>,
}

impl StageState {
    /// Снять состояние (одним запросом списка окон на все экраны). Если полоса
    /// выключена, окна не запрашиваются вовсе.
    pub fn current(screens: &[Screen], primary_height: f64) -> StageState {
        if !(stage_capable() && stage_enabled() && stage_strip_show()) {
            return StageState::default();
        }
        let position = stage_strip_position();
        let frames = stage_window_frames(primary_height);
        let visible_on = screens
            .iter()
            .filter(|screen| strip_visible_on(screen, screens, &frames, position))
            .map(|screen| screen.id)
            .collect();
        StageState {
            active: true,
            position: Some(position),
            visible_on,
        }
    }

    /// Сколько полоса отнимает у рабочей области экрана и с какой стороны.
    pub fn strip_on(&self, screen: &Screen) -> Option<StageStripPosition> {
        if self.active && self.visible_on.contains(&screen.id) {
            self.position
        } else {
            None
        }
    }
}

/// Группы окон в полосе на экране `screen` (`getStageStripWindowGroups`):
/// AX-приложение `com.apple.WindowManager` → группа полосы на этом экране →
/// список → кнопки, у каждой `AXWindowsIDs`.
pub fn stage_strip_window_groups(screen: &Screen) -> Vec<Vec<u32>> {
    let primary_height = screens::primary_screen_height();
    let Some(app_element) = AxElement::for_bundle_id("com.apple.WindowManager") else {
        return Vec::new();
    };
    let Some(strips) = app_element.children_with_role("AXGroup") else {
        return Vec::new();
    };
    let strip = strips.into_iter().find(|strip| {
        strip
            .frame()
            .map(|frame| screen.frame.contains(&frame.screen_flipped(primary_height)))
            .unwrap_or(false)
    });
    let Some(buttons) = strip
        .and_then(|strip| strip.child_with_role("AXList"))
        .and_then(|list| list.children_with_role("AXButton"))
    else {
        return Vec::new();
    };
    buttons
        .iter()
        .filter_map(|button| button.window_ids())
        .collect()
}

/// Группа полосы, в которой есть окно `window_id` (`getStageStripWindowGroup`).
pub fn stage_strip_window_group(window_id: u32, screen: &Screen) -> Option<Vec<u32>> {
    stage_strip_window_groups(screen)
        .into_iter()
        .find(|group| group.contains(&window_id))
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
    fn major_version_is_parsed_from_product_version() {
        assert_eq!(parse_major_version("27.0"), 27);
        assert_eq!(parse_major_version("13.6.1"), 13);
        assert_eq!(parse_major_version(""), 0);
        assert!(macos_major_version() >= 11);
    }

    #[test]
    fn stage_defaults_are_created_once() {
        let first = window_manager_defaults().map(|defaults| defaults as *const NSUserDefaults);
        let second = window_manager_defaults().map(|defaults| defaults as *const NSUserDefaults);
        assert!(first.is_some());
        assert_eq!(first, second);
        let dock = dock_defaults().map(|defaults| defaults as *const NSUserDefaults);
        assert!(dock.is_some());
        assert_eq!(dock, dock_defaults().map(|defaults| defaults as *const _));
        assert_ne!(first, dock);
        // Чтение через общий экземпляр работает (ключа может и не быть).
        let _ = (stage_enabled(), stage_strip_show(), stage_strip_position());
    }

    #[test]
    fn strip_is_opposite_the_dock() {
        assert_eq!(
            strip_position_for(Some("left"), false),
            StageStripPosition::Right
        );
        assert_eq!(
            strip_position_for(Some("right"), false),
            StageStripPosition::Left
        );
        assert_eq!(
            strip_position_for(Some("bottom"), false),
            StageStripPosition::Left
        );
        assert_eq!(strip_position_for(None, false), StageStripPosition::Left);
        assert_eq!(strip_position_for(None, true), StageStripPosition::Right);
        // Док сбоку важнее направления письма.
        assert_eq!(
            strip_position_for(Some("right"), true),
            StageStripPosition::Left
        );
    }

    #[test]
    fn strip_needs_two_windows_near_the_screen_edge() {
        let left = screen(1, Rect::new(0.0, 0.0, 1728.0, 1117.0));
        let right = screen(2, Rect::new(1728.0, 0.0, 2560.0, 1440.0));
        let screens = vec![left.clone(), right.clone()];
        // Миниатюры полосы у левого края левого экрана.
        let thumbnails = vec![
            Rect::new(10.0, 700.0, 150.0, 120.0),
            Rect::new(10.0, 500.0, 150.0, 120.0),
        ];
        assert!(strip_visible_on(
            &left,
            &screens,
            &thumbnails,
            StageStripPosition::Left
        ));
        assert!(!strip_visible_on(
            &right,
            &screens,
            &thumbnails,
            StageStripPosition::Left
        ));

        // Одно окно — это может быть перетаскиваемое окно, полосы нет.
        assert!(!strip_visible_on(
            &left,
            &screens,
            &thumbnails[..1],
            StageStripPosition::Left
        ));

        // Полоса справа: ближе к правому краю правого экрана.
        let right_side = vec![
            Rect::new(4100.0, 700.0, 150.0, 120.0),
            Rect::new(4100.0, 500.0, 150.0, 120.0),
        ];
        assert!(strip_visible_on(
            &right,
            &screens,
            &right_side,
            StageStripPosition::Right
        ));
        assert!(!strip_visible_on(
            &left,
            &screens,
            &right_side,
            StageStripPosition::Right
        ));
    }

    #[test]
    fn inactive_stage_takes_no_space() {
        let display = screen(7, Rect::new(0.0, 0.0, 1728.0, 1117.0));
        assert_eq!(StageState::default().strip_on(&display), None);
        let state = StageState {
            active: true,
            position: Some(StageStripPosition::Left),
            visible_on: vec![7],
        };
        assert_eq!(state.strip_on(&display), Some(StageStripPosition::Left));
        let other = screen(8, Rect::new(1728.0, 0.0, 1728.0, 1117.0));
        assert_eq!(state.strip_on(&other), None);
    }
}
