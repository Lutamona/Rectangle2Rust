//! Чтение рамок по правилам `CGRect` и сравнения по правилам Swift — чтобы геометрия
//! согласованного ресайза и сдвига лесенкой совпадала с оригиналом до бита.
//!
//! `CGRect.minX`/`maxX`/`width` смотрят на прямоугольник, приведённый к положительным
//! размерам: у рамки с отрицательной шириной `minX` — это `x + width`. Такие рамки
//! в оригинале бывают (сосед, прижатый к гэпу у края экрана, получает рамку «наоборот»),
//! поэтому здесь — не поля `Rect`, а эти функции. `intersects`/`contains`/`inset_by`
//! проверены против CoreGraphics на миллионах случайных рамок, включая нулевые и
//! отрицательные размеры.

use crate::geometry::Rect;

/// `CGRect.minX`.
pub fn min_x(rect: &Rect) -> f64 {
    if rect.w < 0.0 {
        rect.x + rect.w
    } else {
        rect.x
    }
}

/// `CGRect.maxX`.
pub fn max_x(rect: &Rect) -> f64 {
    if rect.w < 0.0 {
        rect.x
    } else {
        rect.x + rect.w
    }
}

/// `CGRect.minY`.
pub fn min_y(rect: &Rect) -> f64 {
    if rect.h < 0.0 {
        rect.y + rect.h
    } else {
        rect.y
    }
}

/// `CGRect.maxY`.
pub fn max_y(rect: &Rect) -> f64 {
    if rect.h < 0.0 {
        rect.y
    } else {
        rect.y + rect.h
    }
}

/// `CGRect.width` — без знака.
pub fn width(rect: &Rect) -> f64 {
    rect.w.abs()
}

/// `CGRect.height` — без знака.
pub fn height(rect: &Rect) -> f64 {
    rect.h.abs()
}

/// `CGRect.intersects(_:)`: полуоткрытые отрезки `[min, max)`; у нулевой стороны отрезок —
/// точка. Касание краями — не пересечение.
pub fn intersects(a: &Rect, b: &Rect) -> bool {
    spans_intersect(min_x(a), max_x(a), min_x(b), max_x(b))
        && spans_intersect(min_y(a), max_y(a), min_y(b), max_y(b))
}

fn spans_intersect(a_min: f64, a_max: f64, b_min: f64, b_max: f64) -> bool {
    let start = swift_max(a_min, b_min);
    let in_span =
        |min: f64, max: f64| min <= start && (start < max || (max == min && start == min));
    in_span(a_min, a_max) && in_span(b_min, b_max)
}

/// `CGRect.contains(_: CGRect)`.
pub fn contains(outer: &Rect, inner: &Rect) -> bool {
    min_x(inner) >= min_x(outer)
        && max_x(inner) <= max_x(outer)
        && min_y(inner) >= min_y(outer)
        && max_y(inner) <= max_y(outer)
}

/// `CGRect.insetBy(dx:dy:)`: у рамки, приведённой к положительным размерам; «съели»
/// целиком — `CGRect.null` (`None`).
pub fn inset_by(rect: &Rect, dx: f64, dy: f64) -> Option<Rect> {
    let w = width(rect) - 2.0 * dx;
    let h = height(rect) - 2.0 * dy;
    if w < 0.0 || h < 0.0 {
        return None;
    }
    Some(Rect::new(min_x(rect) + dx, min_y(rect) + dy, w, h))
}

/// `Swift.min(x, y)`: `y < x ? y : x`.
pub fn swift_min(x: f64, y: f64) -> f64 {
    if y < x {
        y
    } else {
        x
    }
}

/// `Swift.max(x, y)`: `y >= x ? y : x`.
pub fn swift_max(x: f64, y: f64) -> f64 {
    if y >= x {
        y
    } else {
        x
    }
}

/// `Sequence.max(by:)`: из равных остаётся первый.
pub fn swift_max_by<T>(
    items: impl IntoIterator<Item = T>,
    are_in_increasing_order: impl Fn(&T, &T) -> bool,
) -> Option<T> {
    let mut items = items.into_iter();
    let mut result = items.next()?;
    for item in items {
        if are_in_increasing_order(&result, &item) {
            result = item;
        }
    }
    Some(result)
}

/// `Sequence.min(by:)`: из равных остаётся первый.
pub fn swift_min_by<T>(
    items: impl IntoIterator<Item = T>,
    are_in_increasing_order: impl Fn(&T, &T) -> bool,
) -> Option<T> {
    let mut items = items.into_iter();
    let mut result = items.next()?;
    for item in items {
        if are_in_increasing_order(&item, &result) {
            result = item;
        }
    }
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn negative_sizes_are_read_like_cgrect() {
        // `var r = CGRect(1, 2, 3, 4); r.size.width = -5` → minX -4, maxX 1, width 5.
        let rect = Rect::new(1.0, 2.0, -5.0, 4.0);
        assert_eq!((min_x(&rect), max_x(&rect), width(&rect)), (-4.0, 1.0, 5.0));
    }

    #[test]
    fn intersection_rules_match_core_graphics() {
        let a = Rect::new(0.0, 0.0, 10.0, 10.0);
        // Значения — из CGRect.intersects (swiftc, macOS 27).
        let cases = [
            (Rect::new(10.0, 0.0, 10.0, 10.0), false),
            (Rect::new(5.0, 5.0, 0.0, 0.0), true),
            (Rect::new(0.0, 0.0, 0.0, 10.0), true),
            (Rect::new(10.0, 0.0, 0.0, 10.0), false),
            (Rect::new(0.0, 0.0, 0.0, 0.0), true),
            (Rect::new(5.0, 10.0, 3.0, 0.0), false),
            (Rect::new(20.0, 0.0, -10.0, 10.0), false),
            (Rect::new(20.0, 0.0, -11.0, 10.0), true),
        ];
        for (b, expected) in cases {
            assert_eq!(intersects(&a, &b), expected, "{b:?}");
            assert_eq!(intersects(&b, &a), expected, "{b:?}");
        }
        let point = Rect::new(3.0, 3.0, 0.0, 0.0);
        assert!(intersects(&point, &point));
    }

    #[test]
    fn inset_and_contains() {
        let expanded = inset_by(&Rect::new(5.0, 5.0, -4.0, 2.0), -1.0, -1.0);
        assert_eq!(expanded, Some(Rect::new(0.0, 4.0, 6.0, 4.0)));
        assert_eq!(inset_by(&Rect::new(5.0, 5.0, 2.0, 2.0), 2.0, 0.0), None);

        let bounds = Rect::new(-4.0, -4.0, 18.0, 18.0);
        assert!(contains(&bounds, &Rect::new(-4.0, -4.0, 18.0, 18.0)));
        assert!(contains(&bounds, &Rect::new(3.0, 3.0, 0.0, 0.0)));
        assert!(!contains(&bounds, &Rect::new(-5.0, 0.0, 2.0, 2.0)));
    }

    #[test]
    fn swift_min_max_keep_argument_order() {
        assert!(swift_max(0.0, -0.0).is_sign_negative());
        assert!(swift_min(-0.0, 0.0).is_sign_negative());
        let values = [(1, 3.0), (2, 5.0), (3, 5.0), (4, 1.0)];
        let max = swift_max_by(values, |lhs, rhs| lhs.1 < rhs.1).unwrap();
        assert_eq!(max.0, 2);
        let min = swift_min_by(values, |lhs, rhs| lhs.1 < rhs.1).unwrap();
        assert_eq!(min.0, 4);
    }
}
