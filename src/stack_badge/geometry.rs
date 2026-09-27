//! Чистая геометрия значка стопки — `StackBadgeGeometry.swift` и расчёты из
//! `StackBadgeManager.swift`: где на экране стоят верхние левые углы ячеек
//! сетки, у какого угла замер курсор, какие окна образуют стопку и где встают
//! пилюля, список и «коридор» между ними. Без AX и AppKit — только числа.
//!
//! Система координат подписана у каждой функции: AppKit (y вверх, как у
//! `NSScreen` и `NSEvent.mouseLocation`) или AX (y вниз от верха основного
//! экрана, как у списка окон).

use crate::ax::WindowInfo;
use crate::geometry::Rect;

/// Точка `(x, y)`.
pub type Point = (f64, f64);

/// Все сетки, по которым Rectangle раскладывает окна, — (столбцы, строки).
/// Углы их ячеек — места, где окна могут встать стопкой.
pub const GRID_DIMENSIONS: [(u32, u32); 14] = [
    // половины
    (2, 1),
    (1, 2),
    // трети
    (3, 1),
    (1, 3),
    // четверти
    (2, 2),
    // четвертинки в ряд
    (4, 1),
    (1, 4),
    // шестые
    (3, 2),
    (2, 3),
    // восьмые
    (4, 2),
    // девятые
    (3, 3),
    // двенадцатые
    (4, 3),
    (3, 4),
    // шестнадцатые
    (4, 4),
];

/// Сторона зоны наведения у угла, pt (`hoverZone`); к ней прибавляется зазор
/// между окнами.
pub const HOVER_ZONE: f64 = 48.0;
/// Насколько курсор может зайти выше и левее угла, pt.
const HOVER_SLACK: f64 = 4.0;
/// Допуск совпадения положений окон стопки, pt (`tolerance` в `query`).
pub const TOLERANCE: f64 = 4.0;
/// `kCGNormalWindowLevel`: стопку образуют только обычные окна.
pub const NORMAL_WINDOW_LEVEL: i32 = 0;
/// Окно больше этой доли рабочей области по обеим сторонам — «во весь экран»
/// и в стопку не входит.
const COVERS_SCREEN: f64 = 0.9;

/// Пилюля и список опускаются на столько ниже верхнего края стопки, чтобы
/// «светофор» переднего окна оставался доступен, а список не закрывал
/// типичную панель инструментов (`titleBarClearance`).
pub const TITLE_BAR_CLEARANCE: f64 = 50.0;
/// Высота пилюли; радиус скругления — половина высоты.
pub const BADGE_HEIGHT: f64 = 28.0;
/// Поля пилюли слева и справа.
pub const BADGE_PADDING: f64 = 12.0;
/// Значок `rectangle.stack.fill` в пилюле: ширина и высота.
pub const SYMBOL_SIZE: (f64, f64) = (19.0, 16.0);
/// Зазор между значком и числом.
pub const SYMBOL_GAP: f64 = 7.0;
/// Список сдвинут вправо от пилюли (под «выглядывающие» окна) …
const LIST_INDENT: f64 = 12.0;
/// … и отступает от её низа.
const LIST_GAP: f64 = 6.0;
/// Ширина списка.
pub const LIST_WIDTH: f64 = 260.0;
/// Высота строки списка.
pub const ROW_HEIGHT: f64 = 22.0;
/// Поля списка вокруг строк.
pub const LIST_PADDING: f64 = 4.0;
/// Курсор в пределах стольких pt от пилюли, списка или коридора — всё ещё «на них».
const UI_MARGIN: f64 = 8.0;

/// Точка в прямоугольнике: левый и нижний края внутри, правый и верхний —
/// снаружи (`NSPointInRect`, `CGRect.contains`).
pub fn contains_point(rect: &Rect, point: Point) -> bool {
    point.0 >= rect.min_x()
        && point.0 < rect.max_x()
        && point.1 >= rect.min_y()
        && point.1 < rect.max_y()
}

/// Прямоугольник с неотрицательными сторонами, как `CGRect.standardized`:
/// Swift-версия берёт у рабочей области `width`, `minX`, `maxY`, а они у
/// `CGRect` всегда нормализованы (отступы от краёв больше экрана дают
/// отрицательную сторону).
pub fn standardized(rect: &Rect) -> Rect {
    let (x, w) = if rect.w < 0.0 {
        (rect.x + rect.w, -rect.w)
    } else {
        (rect.x, rect.w)
    };
    let (y, h) = if rect.h < 0.0 {
        (rect.y + rect.h, -rect.h)
    } else {
        (rect.y, rect.h)
    };
    Rect::new(x, y, w, h)
}

// ---------------------------------------------------------------- углы

/// Верхние левые углы (AppKit) всех ячеек всех сеток в рабочей области
/// `screen_frame`, без повторов: точки ближе 1 pt друг к другу — одна точка.
/// На обычном экране их 36.
pub fn corner_points(screen_frame: &Rect) -> Vec<Point> {
    let screen_frame = standardized(screen_frame);
    if !(screen_frame.w > 0.0 && screen_frame.h > 0.0) {
        return Vec::new();
    }
    let mut points: Vec<Point> = Vec::new();
    for (cols, rows) in GRID_DIMENSIONS {
        for col in 0..cols {
            for row in 0..rows {
                let point = (
                    screen_frame.min_x() + f64::from(col) * screen_frame.w / f64::from(cols),
                    screen_frame.max_y() - f64::from(row) * screen_frame.h / f64::from(rows),
                );
                let seen = points
                    .iter()
                    .any(|seen| (seen.0 - point.0).abs() < 1.0 && (seen.1 - point.1).abs() < 1.0);
                if !seen {
                    points.push(point);
                }
            }
        }
    }
    points
}

/// Угол, в зоне наведения которого стоит точка (AppKit), или `None`. Зона —
/// квадрат со стороной `zone` вправо и вниз от угла (плюс 4 pt выше и левее):
/// там, где из-за зазоров оказываются окна и их заголовки. Из нескольких
/// подходящих — ближайший.
pub fn corner_near(point: Point, corners: &[Point], zone: f64) -> Option<Point> {
    let mut best: Option<(Point, f64)> = None;
    for &corner in corners {
        let dx = point.0 - corner.0;
        let dy = corner.1 - point.1;
        let inside = dx >= -HOVER_SLACK && dx <= zone && dy >= -HOVER_SLACK && dy <= zone;
        if !inside {
            continue;
        }
        let distance = dx * dx + dy * dy;
        if best.is_none_or(|(_, best_distance)| distance < best_distance) {
            best = Some((corner, distance));
        }
    }
    best.map(|(corner, _)| corner)
}

// ---------------------------------------------------------------- стопка

/// Номера положений окон (AX), которые образуют стопку-лесенку. Сдвиг при
/// наложении идёт по диагонали: по x всегда вправо, а по y в любую сторону
/// (он задаётся в координатах AppKit), поэтому окно по y симметрично. Каждое
/// положение пробуется левым краем стопки и побеждает самое плотное скопление —
/// так ни соседи из расширенной зазором коробки, ни чужое окно левее стопки не
/// искажают счёт. Номера — по возрастанию.
pub fn stack_indices(origins: &[Point], cascade_range: f64, tolerance: f64) -> Vec<usize> {
    let mut best: Vec<usize> = Vec::new();
    for anchor in origins {
        let cluster: Vec<usize> = (0..origins.len())
            .filter(|&index| {
                let dx = origins[index].0 - anchor.0;
                let dy = origins[index].1 - anchor.1;
                dx >= -tolerance
                    && dx <= cascade_range
                    && dy >= -cascade_range
                    && dy <= cascade_range
            })
            .collect();
        if cluster.len() > best.len() {
            best = cluster;
        }
    }
    best
}

/// Размах поиска стопки по настройкам.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StackRanges {
    /// Насколько разъезжаются окна одной стопки-лесенки (`cascadeRange`):
    /// сдвиг при наложении, умноженный на число шагов, плюс допуск.
    pub cascade: f64,
    /// Коробка сбора кандидатов у угла (`candidateRange`): зазор ставит окно
    /// ячейки на целый зазор от геометрического угла.
    pub candidate: f64,
}

impl StackRanges {
    /// Из настроек `gapSize`, `cyclingOverlapOffsetSize` и
    /// `cyclingOverlapMaxCascade` (шагов лесенки — от 1 до 5).
    pub fn new(gap_size: f32, overlap_offset_size: f32, overlap_max_cascade: i64) -> StackRanges {
        let max_cascade = overlap_max_cascade.clamp(1, 5) as f64;
        let cascade = f64::from(overlap_offset_size).max(1.0) * max_cascade + TOLERANCE;
        StackRanges {
            cascade,
            candidate: f64::from(gap_size) + cascade,
        }
    }
}

/// Окно — кандидат в стопку у угла `corner` (AX): обычного уровня, не во весь
/// экран `screen_frame` (AX) и стоит в коробке у угла — не дальше допуска левее,
/// не дальше `candidate_range` правее, выше и ниже.
pub fn is_candidate(
    info: &WindowInfo,
    corner: Point,
    screen_frame: &Rect,
    candidate_range: f64,
) -> bool {
    if info.level != NORMAL_WINDOW_LEVEL {
        return false;
    }
    let covers_screen = info.frame.w > screen_frame.w * COVERS_SCREEN
        && info.frame.h > screen_frame.h * COVERS_SCREEN;
    if covers_screen {
        return false;
    }
    let dx = info.frame.x - corner.0;
    let dy = info.frame.y - corner.1;
    dx >= -TOLERANCE && dx <= candidate_range && dy >= -candidate_range && dy <= candidate_range
}

/// Стопка у угла.
#[derive(Clone, Debug, PartialEq)]
pub struct Stack {
    /// Окна стопки в порядке списка окон — спереди назад.
    pub windows: Vec<WindowInfo>,
    /// Видимый верхний левый край стопки (AX): x самого левого окна и y самого
    /// верхнего — там начинаются заголовки, а не геометрический угол, от
    /// которого окна отодвинул зазор.
    pub top_left: Point,
}

/// Стопка у угла `corner` среди окон `windows` (список окон, спереди назад):
/// кандидаты из расширенной коробки, из них — самое плотное скопление-лесенка;
/// меньше двух окон — не стопка. Все координаты — AX.
pub fn find_stack(
    windows: &[WindowInfo],
    corner: Point,
    screen_frame: &Rect,
    ranges: StackRanges,
) -> Option<Stack> {
    let candidates: Vec<&WindowInfo> = windows
        .iter()
        .filter(|info| is_candidate(info, corner, screen_frame, ranges.candidate))
        .collect();
    let origins: Vec<Point> = candidates
        .iter()
        .map(|info| (info.frame.x, info.frame.y))
        .collect();
    let stacked: Vec<WindowInfo> = stack_indices(&origins, ranges.cascade, TOLERANCE)
        .into_iter()
        .map(|index| candidates[index].clone())
        .collect();
    if stacked.len() < 2 {
        return None;
    }
    let left = stacked
        .iter()
        .map(|info| info.frame.x)
        .fold(f64::INFINITY, f64::min);
    let top = stacked
        .iter()
        .map(|info| info.frame.y)
        .fold(f64::INFINITY, f64::min);
    Some(Stack {
        windows: stacked,
        top_left: (left, top),
    })
}

// ---------------------------------------------------------------- пилюля, список, коридор

/// Верхний левый угол пилюли (AppKit) для стопки с верхним левым краем
/// `top_left` (AppKit): ниже полосы заголовка, чтобы «светофор» переднего окна
/// оставался доступен. Кнопки окон под ним достаются щелчком по названию.
pub fn badge_anchor(top_left: Point) -> Point {
    (top_left.0, top_left.1 - TITLE_BAR_CLEARANCE)
}

/// Раскладка пилюли: рамка окна (AppKit) и, внутри неё, рамки значка и числа.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BadgeLayout {
    pub frame: Rect,
    /// Значок `rectangle.stack.fill`; `None` — значка нет, в пилюле только число.
    pub symbol: Option<Rect>,
    pub label: Rect,
}

/// Пилюля по размеру содержимого: верхний левый угол в `anchor` (AppKit),
/// поля 12, значок 19×16, зазор 7, число `label_size` (уже округлено вверх),
/// всё по центру высоты 28.
pub fn badge_layout(anchor: Point, label_size: (f64, f64), with_symbol: bool) -> BadgeLayout {
    let (symbol_width, gap) = if with_symbol {
        (SYMBOL_SIZE.0, SYMBOL_GAP)
    } else {
        (0.0, 0.0)
    };
    let (label_width, label_height) = label_size;
    let width = symbol_width + gap + label_width + BADGE_PADDING * 2.0;
    let frame = Rect::new(anchor.0, anchor.1 - BADGE_HEIGHT, width, BADGE_HEIGHT);

    let mut x = BADGE_PADDING;
    let symbol = with_symbol.then(|| {
        let rect = Rect::new(
            x,
            (BADGE_HEIGHT - SYMBOL_SIZE.1) / 2.0,
            SYMBOL_SIZE.0,
            SYMBOL_SIZE.1,
        );
        x += symbol_width + gap;
        rect
    });
    let label = Rect::new(
        x,
        (BADGE_HEIGHT - label_height) / 2.0,
        label_width,
        label_height,
    );
    BadgeLayout {
        frame,
        symbol,
        label,
    }
}

/// Верх списка (AppKit): под пилюлей с настоящим зазором, чтобы она не
/// наезжала на список, и со сдвигом вправо — под «выглядывающие» окна.
pub fn list_top(anchor: Point, badge_frame: &Rect) -> Point {
    (anchor.0 + LIST_INDENT, badge_frame.min_y() - LIST_GAP)
}

/// Рамка списка из `count` строк (AppKit): верх прибит к `list_top`, список
/// растёт вниз. Не влезает до низа рабочей области — высота обрезается (а не
/// сдвигается вверх, на пилюлю); лишние строки скрывает маска. За правый край
/// рабочей области не выходит.
pub fn list_frame(count: usize, list_top: Point, screen_frame: &Rect) -> Rect {
    let full_height = count as f64 * ROW_HEIGHT + LIST_PADDING * 2.0;
    let height = full_height.min((list_top.1 - screen_frame.min_y()).max(0.0));
    let mut frame = Rect::new(list_top.0, list_top.1 - height, LIST_WIDTH, height);
    if frame.max_x() > screen_frame.max_x() {
        frame.x = screen_frame.max_x() - frame.w;
    }
    frame
}

/// Рамка строки `index` внутри списка высотой `list_height`: строки идут
/// сверху вниз.
pub fn row_frame(index: usize, list_height: f64) -> Rect {
    Rect::new(
        LIST_PADDING,
        list_height - LIST_PADDING - (index + 1) as f64 * ROW_HEIGHT,
        LIST_WIDTH - LIST_PADDING * 2.0,
        ROW_HEIGHT,
    )
}

/// «Коридор» (AppKit): пилюля и список стоят ниже места, где курсор их вызвал,
/// и без мостика курсор по дороге вниз проходил бы пустоту и всё закрывалось.
/// Мостик — только от верха списка до `peek_y` (верха стопки), во всю ширину
/// пилюли и списка: его высота не растёт с числом окон.
pub fn corridor(badge: &Rect, list: &Rect, peek_y: f64) -> Rect {
    let min_x = badge.min_x().min(list.min_x());
    let max_x = badge.max_x().max(list.max_x());
    let bottom = list.max_y();
    Rect::new(min_x, bottom, max_x - min_x, (peek_y - bottom).max(0.0))
}

/// Курсор (AppKit) на показанных пилюле, списке или коридоре — с запасом 8 pt.
pub fn inside_visible_ui(frames: &[Rect], location: Point) -> bool {
    frames
        .iter()
        .any(|frame| contains_point(&frame.inset_by(-UI_MARGIN, -UI_MARGIN), location))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Рабочая область ноутбука 1728×1117: меню-бар 32, док 83 (AppKit).
    fn laptop() -> Rect {
        Rect::new(0.0, 83.0, 1728.0, 1001.0)
    }

    fn window(id: u32, x: f64, y: f64) -> WindowInfo {
        WindowInfo {
            id,
            pid: 100 + id as i32,
            level: NORMAL_WINDOW_LEVEL,
            frame: Rect::new(x, y, 800.0, 600.0),
            process_name: Some(format!("app{id}")),
        }
    }

    fn has(points: &[Point], point: Point) -> bool {
        points
            .iter()
            .any(|p| (p.0 - point.0).abs() < 1e-9 && (p.1 - point.1).abs() < 1e-9)
    }

    #[test]
    fn corners_cover_every_grid_cell_once() {
        let frame = laptop();
        let corners = corner_points(&frame);
        // x: 0, ¼, ⅓, ½, ⅔, ¾ ширины; y: столько же долей высоты сверху,
        // но не во всех сочетаниях: 16 (4×4) + 8 (3×3) + 6 (4×3) + 6 (3×4).
        assert_eq!(corners.len(), 36);
        let top = frame.max_y();
        assert_eq!(corners[0], (0.0, top), "первый — верхний левый угол экрана");
        assert!(has(&corners, (864.0, top)), "правая половина");
        assert!(has(&corners, (0.0, top - 500.5)), "нижняя половина");
        assert!(has(&corners, (576.0, top)), "средняя треть");
        assert!(has(&corners, (1152.0, top - 1001.0 / 3.0 * 2.0)), "девятые");
        assert!(has(&corners, (1296.0, top - 750.75)), "шестнадцатые");
        // Без повторов.
        for (index, a) in corners.iter().enumerate() {
            for b in &corners[index + 1..] {
                assert!((a.0 - b.0).abs() >= 1.0 || (a.1 - b.1).abs() >= 1.0);
            }
        }
    }

    #[test]
    fn corners_closer_than_a_point_merge() {
        // Ширина 3 pt: доли ¼, ⅓, ½ … ближе 1 pt друг к другу и сливаются.
        let corners = corner_points(&Rect::new(0.0, 0.0, 3.0, 3.0));
        assert!(corners.len() < 36);
        assert_eq!(corners[0], (0.0, 3.0));
    }

    #[test]
    fn empty_screen_has_no_corners() {
        assert!(corner_points(&Rect::new(0.0, 0.0, 0.0, 900.0)).is_empty());
        assert!(corner_points(&Rect::new(0.0, 0.0, f64::NAN, 900.0)).is_empty());
    }

    #[test]
    fn negative_sides_are_standardized_like_cgrect() {
        assert_eq!(
            standardized(&Rect::new(10.0, 20.0, -4.0, -6.0)),
            Rect::new(6.0, 14.0, 4.0, 6.0)
        );
        assert_eq!(
            corner_points(&Rect::new(0.0, 0.0, 900.0, -2.0)),
            corner_points(&Rect::new(0.0, -2.0, 900.0, 2.0))
        );
        assert_eq!(corner_points(&Rect::new(0.0, 0.0, 900.0, -2.0)).len(), 12);
    }

    #[test]
    fn hover_zone_reaches_right_and_down_from_the_corner() {
        let corners = [(100.0, 500.0)];
        let zone = 48.0;
        // Справа и ниже угла (в AppKit «ниже» — меньше y).
        assert_eq!(
            corner_near((120.0, 480.0), &corners, zone),
            Some((100.0, 500.0))
        );
        assert_eq!(
            corner_near((148.0, 452.0), &corners, zone),
            Some((100.0, 500.0))
        );
        // До 4 pt левее и выше — ещё зона.
        assert_eq!(
            corner_near((96.0, 504.0), &corners, zone),
            Some((100.0, 500.0))
        );
        // Дальше — нет.
        assert_eq!(corner_near((95.9, 480.0), &corners, zone), None);
        assert_eq!(corner_near((120.0, 504.1), &corners, zone), None);
        assert_eq!(corner_near((148.1, 480.0), &corners, zone), None);
        assert_eq!(corner_near((120.0, 451.9), &corners, zone), None);
        // Зазор расширяет зону.
        assert_eq!(
            corner_near((160.0, 440.0), &corners, zone + 12.0),
            Some((100.0, 500.0))
        );
    }

    #[test]
    fn nearest_corner_wins() {
        let corners = [(0.0, 1000.0), (40.0, 1000.0), (40.0, 960.0)];
        assert_eq!(
            corner_near((45.0, 995.0), &corners, 48.0),
            Some((40.0, 1000.0))
        );
        assert_eq!(
            corner_near((45.0, 955.0), &corners, 48.0),
            Some((40.0, 960.0))
        );
        assert_eq!(
            corner_near((10.0, 990.0), &corners, 48.0),
            Some((0.0, 1000.0))
        );
        // Равные расстояния — первый в списке.
        let tie = [(10.0, 100.0), (12.0, 100.0)];
        assert_eq!(corner_near((11.0, 97.0), &tie, 48.0), Some((10.0, 100.0)));
        assert_eq!(corner_near((11.0, 97.0), &[], 48.0), None);
    }

    #[test]
    fn stack_is_the_densest_cascade() {
        // Три окна лесенкой по 11 вправо и вниз, диапазон 11 × 2 + 4.
        let origins = [(100.0, 50.0), (111.0, 61.0), (122.0, 72.0)];
        assert_eq!(stack_indices(&origins, 26.0, 4.0), vec![0, 1, 2]);
        // По y лесенка может идти и вверх.
        let up = [(100.0, 50.0), (111.0, 39.0), (122.0, 28.0)];
        assert_eq!(stack_indices(&up, 26.0, 4.0), vec![0, 1, 2]);
    }

    #[test]
    fn unrelated_leftmost_window_does_not_mask_the_stack() {
        // Чужое окно левее стопки на 20: от него до стопки не достать, но и
        // стопку оно не прячет — выигрывает скопление плотнее.
        let origins = [(80.0, 50.0), (100.0, 50.0), (100.0, 50.0), (111.0, 50.0)];
        assert_eq!(stack_indices(&origins, 15.0, 4.0), vec![1, 2, 3]);
    }

    #[test]
    fn neighbours_in_the_widened_box_are_not_counted() {
        // Два окна в углу и одно в 30 pt правее (попало в коробку из-за зазора).
        let origins = [(100.0, 50.0), (130.0, 50.0), (101.0, 51.0)];
        assert_eq!(stack_indices(&origins, 15.0, 4.0), vec![0, 2]);
    }

    #[test]
    fn stack_tolerates_small_offsets_to_the_left() {
        let origins = [(100.0, 50.0), (96.0, 50.0)];
        assert_eq!(stack_indices(&origins, 15.0, 4.0), vec![0, 1]);
        // На 5 pt левее — уже не допуск, но левое окно само становится краем
        // стопки и забирает оба.
        let origins = [(100.0, 50.0), (95.0, 50.0)];
        assert_eq!(stack_indices(&origins, 15.0, 4.0), vec![0, 1]);
        // Лесенка короче сдвига — каждое окно «стопка» из одного; побеждает первое.
        assert_eq!(stack_indices(&origins, 4.0, 4.0), vec![0]);
        assert!(stack_indices(&[], 15.0, 4.0).is_empty());
    }

    #[test]
    fn ranges_follow_overlap_settings() {
        // По умолчанию: сдвиг 11, один шаг, без зазоров.
        let ranges = StackRanges::new(0.0, 11.0, 1);
        assert_eq!(ranges.cascade, 15.0);
        assert_eq!(ranges.candidate, 15.0);
        // Шагов 1…5, сдвиг не меньше 1, зазор расширяет коробку.
        assert_eq!(StackRanges::new(0.0, 11.0, 9).cascade, 59.0);
        assert_eq!(StackRanges::new(0.0, 11.0, 0).cascade, 15.0);
        assert_eq!(StackRanges::new(0.0, 0.0, 3).cascade, 7.0);
        assert_eq!(StackRanges::new(10.0, 11.0, 2).candidate, 36.0);
    }

    #[test]
    fn candidates_are_normal_windows_near_the_corner() {
        let screen = Rect::new(0.0, 33.0, 1728.0, 1001.0);
        let corner = (864.0, 33.0);
        let near = window(1, 870.0, 40.0);
        assert!(is_candidate(&near, corner, &screen, 15.0));
        // Уровень не обычный (плавающая панель, меню-бар).
        let floating = WindowInfo {
            level: 3,
            ..near.clone()
        };
        assert!(!is_candidate(&floating, corner, &screen, 15.0));
        // Развёрнутое окно в стопку не входит.
        let maximized = WindowInfo {
            frame: Rect::new(864.0, 33.0, 1600.0, 950.0),
            ..near.clone()
        };
        assert!(!is_candidate(&maximized, corner, &screen, 15.0));
        // Широкое, но невысокое — входит.
        let wide = WindowInfo {
            frame: Rect::new(864.0, 33.0, 1600.0, 400.0),
            ..near.clone()
        };
        assert!(is_candidate(&wide, corner, &screen, 15.0));
        // Коробка: левее — не больше 4, правее, выше и ниже — не больше диапазона.
        assert!(is_candidate(&window(2, 860.0, 33.0), corner, &screen, 15.0));
        assert!(!is_candidate(
            &window(3, 859.0, 33.0),
            corner,
            &screen,
            15.0
        ));
        assert!(is_candidate(&window(4, 879.0, 18.0), corner, &screen, 15.0));
        assert!(!is_candidate(
            &window(5, 879.5, 33.0),
            corner,
            &screen,
            15.0
        ));
        assert!(!is_candidate(
            &window(6, 864.0, 48.5),
            corner,
            &screen,
            15.0
        ));
        assert!(!is_candidate(
            &window(7, 864.0, 17.5),
            corner,
            &screen,
            15.0
        ));
    }

    #[test]
    fn stack_keeps_window_list_order_and_visual_top_left() {
        let screen = Rect::new(0.0, 33.0, 1728.0, 1001.0);
        let corner = (864.0, 33.0);
        let ranges = StackRanges::new(10.0, 11.0, 1);
        let list = vec![
            window(1, 885.0, 43.0),  // спереди: сдвинуто лесенкой
            window(2, 300.0, 300.0), // далеко
            window(3, 874.0, 43.0),  // зазор 10 от угла
            window(4, 1300.0, 43.0), // в другой ячейке
        ];
        let stack = find_stack(&list, corner, &screen, ranges).unwrap();
        let ids: Vec<u32> = stack.windows.iter().map(|info| info.id).collect();
        assert_eq!(ids, vec![1, 3]);
        assert_eq!(stack.top_left, (874.0, 43.0));
    }

    #[test]
    fn single_window_is_not_a_stack() {
        let screen = Rect::new(0.0, 33.0, 1728.0, 1001.0);
        let ranges = StackRanges::new(0.0, 11.0, 1);
        let list = vec![window(1, 864.0, 33.0), window(2, 0.0, 33.0)];
        assert_eq!(find_stack(&list, (864.0, 33.0), &screen, ranges), None);
        assert_eq!(find_stack(&[], (864.0, 33.0), &screen, ranges), None);
    }

    #[test]
    fn top_left_takes_leftmost_x_and_topmost_y_separately() {
        let screen = Rect::new(0.0, 33.0, 1728.0, 1001.0);
        let ranges = StackRanges::new(0.0, 11.0, 2);
        // Лесенка вверх: левое окно ниже, правое выше.
        let list = vec![window(1, 875.0, 30.0), window(2, 864.0, 41.0)];
        let stack = find_stack(&list, (864.0, 33.0), &screen, ranges).unwrap();
        assert_eq!(stack.top_left, (864.0, 30.0));
    }

    #[test]
    fn badge_sits_below_the_title_bar_and_fits_its_content() {
        let anchor = badge_anchor((864.0, 1084.0));
        assert_eq!(anchor, (864.0, 1034.0));
        let layout = badge_layout(anchor, (11.0, 18.0), true);
        assert_eq!(layout.frame, Rect::new(864.0, 1006.0, 61.0, 28.0));
        assert_eq!(layout.symbol, Some(Rect::new(12.0, 6.0, 19.0, 16.0)));
        assert_eq!(layout.label, Rect::new(38.0, 5.0, 11.0, 18.0));
        // Без значка — только число.
        let plain = badge_layout(anchor, (11.0, 18.0), false);
        assert_eq!(plain.frame.w, 35.0);
        assert_eq!(plain.symbol, None);
        assert_eq!(plain.label.x, 12.0);
    }

    #[test]
    fn list_opens_below_the_badge_and_grows_down() {
        let screen = laptop();
        let badge = Rect::new(864.0, 1006.0, 61.0, 28.0);
        let top = list_top((864.0, 1034.0), &badge);
        assert_eq!(top, (876.0, 1000.0));
        let frame = list_frame(3, top, &screen);
        assert_eq!(frame, Rect::new(876.0, 926.0, 260.0, 74.0));
        assert_eq!(row_frame(0, frame.h), Rect::new(4.0, 48.0, 252.0, 22.0));
        assert_eq!(row_frame(2, frame.h), Rect::new(4.0, 4.0, 252.0, 22.0));
    }

    #[test]
    fn list_is_cut_at_the_screen_bottom_and_kept_on_screen() {
        let screen = laptop();
        // До низа рабочей области 60 pt, а строк на 8 × 22 + 8: высоту режем,
        // верх остаётся на месте.
        let frame = list_frame(8, (100.0, 143.0), &screen);
        assert_eq!(frame, Rect::new(100.0, 83.0, 260.0, 60.0));
        assert!(
            row_frame(7, frame.h).y < 0.0,
            "нижние строки уходят под маску"
        );
        // Ниже рабочей области — высота 0.
        assert_eq!(list_frame(2, (100.0, 50.0), &screen).h, 0.0);
        // У правого края — сдвиг влево.
        let right = list_frame(2, (1600.0, 700.0), &screen);
        assert_eq!(right.x, 1728.0 - 260.0);
    }

    #[test]
    fn corridor_bridges_from_the_list_top_to_the_peek() {
        let badge = Rect::new(864.0, 1006.0, 61.0, 28.0);
        let list = Rect::new(876.0, 926.0, 260.0, 74.0);
        let bridge = corridor(&badge, &list, 1084.0);
        assert_eq!(bridge, Rect::new(864.0, 1000.0, 272.0, 84.0));
        // Список, сдвинутый влево у края экрана, расширяет коридор влево.
        let shifted = Rect::new(800.0, 926.0, 260.0, 74.0);
        assert_eq!(corridor(&badge, &shifted, 1084.0).x, 800.0);
        // Пик ниже верха списка — коридор нулевой высоты.
        assert_eq!(corridor(&badge, &list, 990.0).h, 0.0);
    }

    #[test]
    fn cursor_near_the_ui_keeps_it_open() {
        let frames = [
            Rect::new(864.0, 1006.0, 61.0, 28.0),
            Rect::new(876.0, 926.0, 260.0, 74.0),
            Rect::new(864.0, 1000.0, 272.0, 84.0),
        ];
        assert!(inside_visible_ui(&frames, (900.0, 950.0)), "на списке");
        assert!(inside_visible_ui(&frames, (1000.0, 1080.0)), "в коридоре");
        assert!(inside_visible_ui(&frames, (1143.9, 950.0)), "в 8 pt справа");
        assert!(!inside_visible_ui(&frames, (1144.0, 950.0)));
        assert!(inside_visible_ui(&frames, (900.0, 918.0)), "в 8 pt снизу");
        assert!(!inside_visible_ui(&frames, (900.0, 917.9)));
        assert!(
            !inside_visible_ui(&frames, (900.0, 1092.0)),
            "выше коридора"
        );
        assert!(!inside_visible_ui(&[], (900.0, 950.0)));
    }

    #[test]
    fn point_containment_is_half_open() {
        let rect = Rect::new(0.0, 0.0, 100.0, 50.0);
        assert!(contains_point(&rect, (0.0, 0.0)));
        assert!(contains_point(&rect, (99.9, 49.9)));
        assert!(!contains_point(&rect, (100.0, 10.0)));
        assert!(!contains_point(&rect, (10.0, 50.0)));
        assert!(!contains_point(&Rect::new(0.0, 0.0, 0.0, 0.0), (0.0, 0.0)));
    }
}
