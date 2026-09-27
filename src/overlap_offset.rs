//! Сдвиг лесенкой (`cyclingOverlapOffset`) — порт `Utilities/OverlapOffsetGeometry.swift`:
//! окно, которое встаёт ровно туда, где уже стоит другое окно (тот же левый верхний
//! угол), сдвигается на шаг вправо и вверх — до `cyclingOverlapMaxCascade` раз, пока
//! есть место до края рабочей области.
//!
//! Координаты — Cocoa (начало снизу слева), поэтому «вверх» — это `+y`: окно сдвигается
//! к правому верхнему углу экрана, как в оригинале. Занятые позиции — левые верхние углы
//! всех окон на экране, кроме самого окна, свёрнутых, скрытых, листов и подсветки
//! drag-to-snap (`FootprintWindow` оригинала — `snapping::footprint::footprint_window_numbers`);
//! развёрнутые на весь экран окна не считаются, если само окно не такое же.

use std::ffi::c_void;

use crate::ax::AxElement;
use crate::config::Config;
use crate::cooperative_resize::cg::{height, intersects, max_x, max_y, min_x, width};
use crate::cooperative_resize_manager::scoped_window_elements;
use crate::geometry::Rect;
use crate::log;

/// Сколько ждать ответа приложения при обходе окон через AX, секунд (`axScanTimeout`):
/// зависшее приложение не должно подвешивать действие.
pub const AX_SCAN_TIMEOUT: f32 = 0.25;

/// Допуск совпадения углов, пикселей.
const TOLERANCE: f64 = 4.0;

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXUIElementCreateSystemWide() -> *const c_void;
    fn AXUIElementSetMessagingTimeout(element: *const c_void, timeout: f32) -> i32;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFRelease(value: *const c_void);
}

/// Общий для всех AX-вызовов процесса таймаут (он ставится на системном элементе) на
/// время обхода окон; при выходе — снова по умолчанию (0).
struct ScanTimeout;

impl ScanTimeout {
    fn set(seconds: f32) {
        unsafe {
            let system_wide = AXUIElementCreateSystemWide();
            if !system_wide.is_null() {
                AXUIElementSetMessagingTimeout(system_wide, seconds);
                CFRelease(system_wide);
            }
        }
    }

    fn start() -> ScanTimeout {
        Self::set(AX_SCAN_TIMEOUT);
        ScanTimeout
    }
}

impl Drop for ScanTimeout {
    fn drop(&mut self) {
        Self::set(0.0);
    }
}

/// Сдвинуть рамку лесенкой, если её позиция уже занята (`applyOverlapOffsetIfNeeded`).
/// `rect` — рамка с гэпами (Cocoa), `window_id` — окно, которое ставим (без номера —
/// не сдвигаем), `screen_frame` — рабочая область экрана назначения
/// (`adjustedVisibleFrame()`), `primary_height` — база переворота Cocoa ↔ AX.
pub fn apply_overlap_offset_if_needed(
    rect: Rect,
    window_id: Option<u32>,
    screen_frame: &Rect,
    primary_height: f64,
    config: &Config,
) -> Rect {
    let overlap_offset = config.cycling_overlap_offset_size as f64;
    let Some(window_id) = window_id.filter(|_| overlap_offset > 0.0) else {
        return rect;
    };
    if !can_offset(&rect, screen_frame, overlap_offset) {
        return rect;
    }

    let _timeout = ScanTimeout::start();

    let screen_frame_ax = screen_frame.screen_flipped(primary_height);
    let max_cascade = config.cycling_overlap_max_cascade.clamp(1, 5);
    let placed_covers_screen = covers_screen(&rect, screen_frame);
    // Как `OverlapOffsetGeometry`: не считать занятым место под подсветкой drag-to-snap.
    let footprint_window_ids: Vec<u32> = objc2::MainThreadMarker::new()
        .map(|mtm| {
            crate::snapping::footprint::footprint_window_numbers(mtm)
                .into_iter()
                .filter_map(|number| u32::try_from(number).ok())
                .collect()
        })
        .unwrap_or_default();

    let occupied_top_lefts: Vec<(f64, f64)> = scoped_window_elements()
        .iter()
        .filter_map(|element| {
            occupied_top_left(
                element,
                window_id,
                &footprint_window_ids,
                &screen_frame_ax,
                screen_frame,
                placed_covers_screen,
                primary_height,
            )
        })
        .collect();

    let offset_rect = cascaded_rect(
        rect,
        &occupied_top_lefts,
        screen_frame,
        overlap_offset,
        max_cascade,
        TOLERANCE,
    );
    if offset_rect != rect {
        log!(
            "Позиция занята другим окном — сдвиг лесенкой в ({}, {})",
            offset_rect.x,
            offset_rect.y
        );
    }
    offset_rect
}

/// Левый верхний угол окна, если оно занимает позицию (Cocoa).
fn occupied_top_left(
    element: &AxElement,
    window_id: u32,
    footprint_window_ids: &[u32],
    screen_frame_ax: &Rect,
    screen_frame: &Rect,
    placed_covers_screen: bool,
    primary_height: f64,
) -> Option<(f64, f64)> {
    let element_window_id = element.get_window_id()?;
    if element_window_id == window_id
        || !element.is_window()
        || element.is_minimized()
        || element.is_hidden() == Some(true)
        || element.is_sheet()
        || footprint_window_ids.contains(&element_window_id)
    {
        return None;
    }

    let frame_ax = element.frame()?;
    if !intersects(screen_frame_ax, &frame_ax) {
        return None;
    }

    // Развёрнутое окно не должно сдвигать все неразвёрнутые.
    let frame = frame_ax.screen_flipped(primary_height);
    if !(placed_covers_screen || !covers_screen(&frame, screen_frame)) {
        return None;
    }
    Some(top_left(&frame))
}

/// Левый верхний угол (`topLeft(of:)`): по нему сравниваются окна, чтобы маленькое окно в
/// том же углу, что и большое, тоже считалось наложением.
pub fn top_left(rect: &Rect) -> (f64, f64) {
    (min_x(rect), max_y(rect))
}

/// Окно почти во весь экран (`coversScreen`): больше 90% по обеим сторонам.
pub fn covers_screen(rect: &Rect, screen_frame: &Rect) -> bool {
    width(rect) > width(screen_frame) * 0.9 && height(rect) > height(screen_frame) * 0.9
}

/// Шаг по каждой оси (`offsetStep`): ноль там, где сдвиг вывел бы окно за край рабочей
/// области, — иначе окно легло бы вплотную к краю, съев гэп и снова накрыв нижнее окно.
pub fn offset_step(rect: &Rect, screen_frame: &Rect, offset: f64) -> (f64, f64) {
    let dx = if max_x(rect) + offset <= max_x(screen_frame) {
        offset
    } else {
        0.0
    };
    let dy = if max_y(rect) + offset <= max_y(screen_frame) {
        offset
    } else {
        0.0
    };
    (dx, dy)
}

/// Сдвинуть есть куда (`canOffset`): развёрнутому без гэпов окну — некуда, и окна можно
/// не перебирать.
pub fn can_offset(rect: &Rect, screen_frame: &Rect, offset: f64) -> bool {
    let (dx, dy) = offset_step(rect, screen_frame, offset);
    dx != 0.0 || dy != 0.0
}

/// Рамка, сдвинутая с чужих углов до `max_cascade` раз (`cascadedRect`); ничего не
/// накрывает или сдвигать некуда — как есть.
pub fn cascaded_rect(
    rect: Rect,
    occupied_top_lefts: &[(f64, f64)],
    screen_frame: &Rect,
    offset: f64,
    max_cascade: i64,
    tolerance: f64,
) -> Rect {
    let mut candidate = rect;
    let steps = if offset > 0.0 { max_cascade.max(0) } else { 0 };
    for _ in 0..steps {
        let corner = top_left(&candidate);
        let overlaps = occupied_top_lefts.iter().any(|occupied| {
            (occupied.0 - corner.0).abs() < tolerance && (occupied.1 - corner.1).abs() < tolerance
        });
        if !overlaps {
            break;
        }

        let (dx, dy) = offset_step(&candidate, screen_frame, offset);
        if dx == 0.0 && dy == 0.0 {
            break;
        }
        candidate.x += dx;
        candidate.y += dy;
    }
    candidate
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: Rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 1728.0,
        h: 1001.0,
    };

    #[test]
    fn step_stops_at_screen_edges() {
        // Левая половина во всю высоту: вверх некуда, только вправо.
        let left_half = Rect::new(0.0, 0.0, 864.0, 1001.0);
        assert_eq!(offset_step(&left_half, &SCREEN, 11.0), (11.0, 0.0));
        // Развёрнутое без гэпов: некуда совсем.
        assert!(!can_offset(&SCREEN, &SCREEN, 11.0));
        // С гэпами по 10: места больше шага нет, но на шаг меньше гэпа — есть.
        let gapped = Rect::new(10.0, 10.0, 1708.0, 981.0);
        assert!(!can_offset(&gapped, &SCREEN, 11.0));
        assert!(can_offset(&gapped, &SCREEN, 10.0));
    }

    #[test]
    fn cascade_moves_right_and_up_until_the_corner_is_free() {
        let quarter = Rect::new(0.0, 0.0, 864.0, 500.0);
        // Угол занят, и следующий тоже: два шага при лесенке до трёх.
        let occupied = [(0.0, 500.0), (11.0, 511.0)];
        let moved = cascaded_rect(quarter, &occupied, &SCREEN, 11.0, 3, 4.0);
        assert_eq!(moved, Rect::new(22.0, 22.0, 864.0, 500.0));
        // Лесенка в одну ступень — один шаг.
        let one = cascaded_rect(quarter, &occupied, &SCREEN, 11.0, 1, 4.0);
        assert_eq!(one, Rect::new(11.0, 11.0, 864.0, 500.0));
        // Совпадение с допуском строго меньше 4 пикселей.
        let near = cascaded_rect(quarter, &[(3.9, 496.1)], &SCREEN, 11.0, 1, 4.0);
        assert_eq!(near, Rect::new(11.0, 11.0, 864.0, 500.0));
        let far = cascaded_rect(quarter, &[(4.0, 500.0)], &SCREEN, 11.0, 1, 4.0);
        assert_eq!(far, quarter);
        // Нулевой шаг и нулевая лесенка ничего не двигают.
        assert_eq!(
            cascaded_rect(quarter, &occupied, &SCREEN, 0.0, 3, 4.0),
            quarter
        );
        assert_eq!(
            cascaded_rect(quarter, &occupied, &SCREEN, 11.0, 0, 4.0),
            quarter
        );
    }

    #[test]
    fn maximized_windows_are_recognised() {
        assert!(covers_screen(&Rect::new(5.0, 5.0, 1600.0, 950.0), &SCREEN));
        assert!(!covers_screen(
            &Rect::new(0.0, 0.0, 1555.0, 1001.0),
            &SCREEN
        ));
        assert_eq!(
            top_left(&Rect::new(10.0, 20.0, 300.0, 400.0)),
            (10.0, 420.0)
        );
    }
}
