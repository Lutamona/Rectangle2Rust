//! Прямоугольники и чистая геометрия раскладки.
//!
//! Система координат внутри приложения — как у Cocoa (origin снизу слева).
//! AX API работает в координатах с origin сверху слева, поэтому рамки окон
//! конвертируются через `Rect::screen_flipped`.

/// Прямоугольник. Ширина и высота могут быть отрицательными только у «пустого» —
/// такие не создаём, вместо них используем Option.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    /// `CGRect.null` — «рамки нет»: начало в плюс бесконечности, размер нулевой.
    /// С любой настоящей рамкой не совпадает и не бывает «близка» (`is_close`), а
    /// сама с собой равна — как в Swift.
    pub const NULL: Rect = Rect {
        x: f64::INFINITY,
        y: f64::INFINITY,
        w: 0.0,
        h: 0.0,
    };

    pub fn new(x: f64, y: f64, w: f64, h: f64) -> Self {
        Rect { x, y, w, h }
    }

    /// `CGRect.isNull`.
    pub fn is_null(&self) -> bool {
        self.x == f64::INFINITY || self.y == f64::INFINITY
    }

    pub fn min_x(&self) -> f64 {
        self.x
    }
    pub fn max_x(&self) -> f64 {
        self.x + self.w
    }
    pub fn min_y(&self) -> f64 {
        self.y
    }
    pub fn max_y(&self) -> f64 {
        self.y + self.h
    }
    pub fn mid_x(&self) -> f64 {
        self.x + self.w / 2.0
    }
    pub fn mid_y(&self) -> f64 {
        self.y + self.h / 2.0
    }
    pub fn center(&self) -> (f64, f64) {
        (self.mid_x(), self.mid_y())
    }
    pub fn width(&self) -> f64 {
        self.w
    }
    pub fn height(&self) -> f64 {
        self.h
    }
    pub fn is_landscape(&self) -> bool {
        self.w > self.h
    }

    /// Пустой прямоугольник: нулевая или отрицательная площадь.
    pub fn is_empty(&self) -> bool {
        self.w <= 0.0 || self.h <= 0.0
    }

    pub fn inset_by(&self, dx: f64, dy: f64) -> Rect {
        Rect::new(
            self.x + dx,
            self.y + dy,
            self.w - 2.0 * dx,
            self.h - 2.0 * dy,
        )
    }

    pub fn offset_by(&self, dx: f64, dy: f64) -> Rect {
        Rect::new(self.x + dx, self.y + dy, self.w, self.h)
    }

    pub fn with_origin(&self, x: f64, y: f64) -> Rect {
        Rect::new(x, y, self.w, self.h)
    }

    pub fn with_size(&self, w: f64, h: f64) -> Rect {
        Rect::new(self.x, self.y, w, h)
    }

    pub fn contains(&self, other: &Rect) -> bool {
        other.min_x() >= self.min_x()
            && other.max_x() <= self.max_x()
            && other.min_y() >= self.min_y()
            && other.max_y() <= self.max_y()
    }

    pub fn intersects(&self, other: &Rect) -> bool {
        self.min_x() < other.max_x()
            && other.min_x() < self.max_x()
            && self.min_y() < other.max_y()
            && other.min_y() < self.max_y()
    }

    pub fn intersection(&self, other: &Rect) -> Option<Rect> {
        let x = self.min_x().max(other.min_x());
        let y = self.min_y().max(other.min_y());
        let max_x = self.max_x().min(other.max_x());
        let max_y = self.max_y().min(other.max_y());
        if max_x <= x || max_y <= y {
            None
        } else {
            Some(Rect::new(x, y, max_x - x, max_y - y))
        }
    }

    pub fn area(&self) -> f64 {
        self.w.max(0.0) * self.h.max(0.0)
    }

    pub fn union(&self, other: &Rect) -> Rect {
        let x = self.min_x().min(other.min_x());
        let y = self.min_y().min(other.min_y());
        let max_x = self.max_x().max(other.max_x());
        let max_y = self.max_y().max(other.max_y());
        Rect::new(x, y, max_x - x, max_y - y)
    }

    /// Переворот по Y относительно верха основного экрана: Cocoa ↔ AX.
    /// `primary_max_y` — `NSScreen.screens[0].frame.maxY` (высота основного экрана).
    pub fn screen_flipped(&self, primary_max_y: f64) -> Rect {
        Rect::new(self.x, primary_max_y - self.max_y(), self.w, self.h)
    }

    /// Совпадение сторон с допуском.
    pub fn is_close(&self, other: &Rect, tolerance: f64) -> bool {
        (self.x - other.x).abs() <= tolerance
            && (self.y - other.y).abs() <= tolerance
            && (self.w - other.w).abs() <= tolerance
            && (self.h - other.h).abs() <= tolerance
    }

    /// Какие стороны совпадают с `other` (по умолчанию без допуска).
    pub fn shared_edges(&self, other: &Rect, tolerance: f64) -> Edge {
        let mut edges = Edge::NONE;
        if (self.min_x() - other.min_x()).abs() <= tolerance {
            edges = edges.with(Edge::LEFT);
        }
        if (self.max_x() - other.max_x()).abs() <= tolerance {
            edges = edges.with(Edge::RIGHT);
        }
        if (self.max_y() - other.max_y()).abs() <= tolerance {
            edges = edges.with(Edge::TOP);
        }
        if (self.min_y() - other.min_y()).abs() <= tolerance {
            edges = edges.with(Edge::BOTTOM);
        }
        edges
    }

    /// Доля площади прямоугольника, попавшая внутрь рамки.
    pub fn percentage_within(&self, frame: &Rect) -> f64 {
        match self.intersection(frame) {
            Some(inter) if self.area() > 0.0 => inter.area() / self.area(),
            _ => 0.0,
        }
    }
}

/// Сторона прямоугольника (битовая маска).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Edge(pub u8);

impl Edge {
    pub const NONE: Edge = Edge(0);
    pub const LEFT: Edge = Edge(1);
    pub const RIGHT: Edge = Edge(2);
    pub const TOP: Edge = Edge(4);
    pub const BOTTOM: Edge = Edge(8);

    pub fn with(self, other: Edge) -> Edge {
        Edge(self.0 | other.0)
    }
    pub fn contains(self, other: Edge) -> bool {
        self.0 & other.0 == other.0
    }
    pub fn is_corner(self) -> bool {
        let horizontal = ((self.contains(Edge::LEFT)) as u8) + ((self.contains(Edge::RIGHT)) as u8);
        let vertical = ((self.contains(Edge::TOP)) as u8) + ((self.contains(Edge::BOTTOM)) as u8);
        horizontal == 1 && vertical == 1
    }
    pub fn is_empty(self) -> bool {
        self.0 == 0
    }
}

/// Сторона половины (`HalfSplitSide`): leading — левая или верхняя, trailing — правая
/// или нижняя.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Leading,
    Trailing,
}

/// Допуск `floorDimension`: доля, умноженная на размер, может недотянуть до целого
/// на ошибку округления.
pub const FLOOR_TOLERANCE: f64 = 0.0001;

pub fn floor_dimension(value: f64) -> f64 {
    (value + FLOOR_TOLERANCE).floor()
}

/// Половина рабочей области по горизонтали (`HalfSplitFrameCalculation.horizontalRect`).
/// Доля — `Float`, как в оригинале, и в CGFloat переводится только при умножении.
pub fn horizontal_rect(visible: &Rect, side: Side, fraction: f32) -> Rect {
    let mut rect = *visible;
    rect.w = floor_dimension(visible.w * fraction as f64);
    if side == Side::Trailing {
        rect.x = visible.max_x() - rect.w;
    }
    rect
}

/// Половина рабочей области по вертикали (`HalfSplitFrameCalculation.verticalRect`).
pub fn vertical_rect(visible: &Rect, side: Side, fraction: f32) -> Rect {
    let mut rect = *visible;
    rect.h = floor_dimension(visible.h * fraction as f64);
    if side == Side::Leading {
        rect.y = visible.max_y() - rect.h;
    }
    rect
}

/// Угол: пересечение горизонтальной и вертикальной половин
/// (`HalfSplitFrameCalculation.cornerRect`).
pub fn corner_rect(
    visible: &Rect,
    horizontal_side: Side,
    vertical_side: Side,
    horizontal_fraction: f32,
    vertical_fraction: f32,
) -> Rect {
    let horizontal = horizontal_rect(visible, horizontal_side, horizontal_fraction);
    let vertical = vertical_rect(visible, vertical_side, vertical_fraction);
    Rect::new(horizontal.x, vertical.y, horizontal.w, vertical.h)
}

/// Чистая геометрия раскладки «столбики» (Rectangle 2).
pub mod columns {
    use super::Rect;

    /// Границы столбика `index` (1-based) в координатах от левого края области.
    ///
    /// Границы считаются округлением: ширины отличаются максимум на 1px,
    /// соседние столбики стыкуются пиксель в пиксель, правый край последнего
    /// совпадает с правым краем области.
    pub fn bounds(width: f64, count: u8, index: u8) -> (f64, f64) {
        let count = count.max(1) as f64;
        let index = (index.max(1) as f64).min(count);

        let min_x = if index == 1.0 {
            0.0
        } else {
            (width * (index - 1.0) / count).round()
        };
        let max_x = if index == count {
            width
        } else {
            (width * index / count).round()
        };
        (min_x, max_x)
    }

    pub fn column_width(width: f64, count: u8, index: u8) -> f64 {
        let (min_x, max_x) = bounds(width, count, index);
        max_x - min_x
    }

    pub fn widths(width: f64, count: u8) -> Vec<f64> {
        (1..=count.max(1))
            .map(|index| column_width(width, count, index))
            .collect()
    }

    /// Рект столбика внутри рабочей области экрана.
    pub fn rect(visible: &Rect, count: u8, index: u8) -> Rect {
        let (min_x, max_x) = bounds(visible.w, count, index);
        Rect::new(visible.min_x() + min_x, visible.y, max_x - min_x, visible.h)
    }

    /// Номер столбика, в котором стоит окно (допуски — как в Swift-версии).
    pub fn best_match_index(
        window: &Rect,
        candidates: &[Rect],
        x_tolerance: f64,
        width_tolerance: f64,
    ) -> Option<u8> {
        let mut best: Option<(u8, f64)> = None;
        for (offset, candidate) in candidates.iter().enumerate() {
            let dx = (candidate.min_x() - window.min_x()).abs();
            let dw = (candidate.width() - window.width()).abs();
            if dx > x_tolerance || dw > width_tolerance {
                continue;
            }
            let distance = dx + dw;
            if best.map(|(_, d)| distance < d).unwrap_or(true) {
                best = Some((offset as u8 + 1, distance));
            }
        }
        best.map(|(index, _)| index)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn column_bounds_are_rounded_and_stick_together() {
        // 1728 / 5: ширины 346/345/346/345/346, стыки без щелей.
        let widths = columns::widths(1728.0, 5);
        assert_eq!(widths, vec![346.0, 345.0, 346.0, 345.0, 346.0]);

        let mut total = 0.0;
        let mut previous_max = 0.0;
        for index in 1..=5u8 {
            let (min_x, max_x) = columns::bounds(1728.0, 5, index);
            assert_eq!(min_x, previous_max, "щель между столбиками {}", index);
            total += max_x - min_x;
            previous_max = max_x;
        }
        assert_eq!(total, 1728.0);

        // Нецелая ширина: правый край последнего столбика — ровно край области.
        let (_, max_x) = columns::bounds(999.5, 7, 7);
        assert_eq!(max_x, 999.5);
    }

    #[test]
    fn column_widths_differ_by_at_most_one_pixel() {
        for width in [999.5, 1280.0, 1440.0, 1512.0, 1728.0, 2560.0, 3840.0] {
            for count in 5..=8u8 {
                let widths = columns::widths(width, count);
                let min = widths.iter().cloned().fold(f64::MAX, f64::min);
                let max = widths.iter().cloned().fold(f64::MIN, f64::max);
                assert!(
                    max - min <= 1.0,
                    "width={} count={} ширины {:?}",
                    width,
                    count,
                    widths
                );
                assert!((widths.iter().sum::<f64>() - width).abs() < 0.001);
            }
        }
    }

    #[test]
    fn best_match_tolerates_app_side_adjustments() {
        let visible = Rect::new(0.0, 0.0, 1728.0, 1001.0);
        let candidates: Vec<Rect> = (1..=5).map(|i| columns::rect(&visible, 5, i)).collect();

        // Терминал подогнал ширину под сетку символов: 346 → 349, центрирован в слоте.
        let window = Rect::new(2.0, 0.0, 349.0, 1001.0);
        assert_eq!(
            columns::best_match_index(&window, &candidates, 6.0, 12.0),
            Some(1)
        );

        // Окно вообще не в столбике.
        let other = Rect::new(0.0, 0.0, 800.0, 600.0);
        assert_eq!(
            columns::best_match_index(&other, &candidates, 6.0, 12.0),
            None
        );
    }

    #[test]
    fn null_rect_matches_nothing_but_itself() {
        let window = Rect::new(0.0, 25.0, 864.0, 1001.0);
        assert!(Rect::NULL.is_null());
        assert!(!window.is_null());
        assert_eq!(Rect::NULL, Rect::NULL);
        assert_ne!(Rect::NULL, window);
        assert!(!window.is_close(&Rect::NULL, 2.0));
        assert!(!Rect::NULL.is_close(&Rect::NULL, 2.0));
    }

    #[test]
    fn screen_flip_roundtrip() {
        let primary_max_y = 1001.0;
        let cocoa = Rect::new(100.0, 200.0, 800.0, 400.0);
        let flipped = cocoa.screen_flipped(primary_max_y);
        assert_eq!(flipped, Rect::new(100.0, 401.0, 800.0, 400.0));
        assert_eq!(flipped.screen_flipped(primary_max_y), cocoa);
    }

    #[test]
    fn halves_and_corners() {
        let visible = Rect::new(0.0, 0.0, 1728.0, 1001.0);
        assert_eq!(horizontal_rect(&visible, Side::Leading, 0.5).w, 864.0);
        assert_eq!(horizontal_rect(&visible, Side::Trailing, 0.5).x, 864.0);
        assert_eq!(vertical_rect(&visible, Side::Leading, 0.5).y, 501.0);
        assert_eq!(vertical_rect(&visible, Side::Trailing, 0.5).y, 0.0);

        let top_left = corner_rect(&visible, Side::Leading, Side::Leading, 0.5, 0.5);
        assert_eq!(top_left, Rect::new(0.0, 501.0, 864.0, 500.0));
    }
}
