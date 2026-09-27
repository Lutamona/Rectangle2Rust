//! Составные области — порт `Snapping/CompoundSnapArea/*.swift`.
//!
//! Составная область выбирает действие по положению курсора вдоль края (трети,
//! четверти, половины у коротких краёв) и по тому, какое действие было перед
//! этим (`priorSnapArea`): так из угла ведут вдоль края к шестым и восьмым, а
//! от крайней трети к центру — к двум третям. Числа — как в Swift: `floor` у
//! долей экрана, границы включены там же, где у оригинала.

use crate::actions::Action;
use crate::config::{CompoundSnapArea, Directional, SnapAreaOption};
use crate::geometry::Rect;

use super::area_model::DisplayOrientation;

/// Настройки, которые составные области читают у `Defaults`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CompoundParams {
    /// `snapEdgeMarginTop`.
    pub margin_top: f64,
    /// `snapEdgeMarginBottom`.
    pub margin_bottom: f64,
    /// `shortEdgeSnapAreaSize`.
    pub short_edge_size: f64,
    /// `ignoredSnapAreas`: выключенные короткие зоны у краёв.
    pub ignored: SnapAreaOption,
}

impl Default for CompoundParams {
    /// Значения по умолчанию оригинала.
    fn default() -> Self {
        CompoundParams {
            margin_top: 5.0,
            margin_bottom: 5.0,
            short_edge_size: 145.0,
            ignored: SnapAreaOption::NONE,
        }
    }
}

impl CompoundSnapArea {
    /// Подпись в попапе вкладки «Области прилипания» (`displayName`) —
    /// русский перевод оригинала, где он есть.
    pub fn display_name(self) -> &'static str {
        match self {
            CompoundSnapArea::LeftTopBottomHalf => {
                "Левая половина, верхняя/нижняя половина возле углов"
            }
            CompoundSnapArea::RightTopBottomHalf => {
                "Правая половина, верхняя/нижняя половина возле углов"
            }
            CompoundSnapArea::Thirds => "Трети, перетащите к центру на две трети",
            CompoundSnapArea::PortraitThirdsSide => "Трети, верхняя/нижняя половина возле углов",
            CompoundSnapArea::Halves => "Левая или правая половина",
            CompoundSnapArea::TopSixths => "Верхние шестые от углов; максимизировать",
            CompoundSnapArea::BottomSixths => "Нижние шестые от углов; трети",
            CompoundSnapArea::Fourths => "Четверти-колонны",
            CompoundSnapArea::PortraitTopBottomHalves => "Верхняя или нижняя половина",
            CompoundSnapArea::TopEighths => "Верхние восьмые от углов; максимизировать",
            CompoundSnapArea::BottomEighths => "Нижние восьмые от углов; трети",
        }
    }

    /// У каких краёв область имеет смысл (`compatibleDirectionals`).
    pub fn compatible_directionals(self) -> &'static [Directional] {
        use Directional::{B, L, R, T};
        match self {
            CompoundSnapArea::LeftTopBottomHalf => &[L],
            CompoundSnapArea::RightTopBottomHalf => &[R],
            CompoundSnapArea::Thirds => &[T, B],
            CompoundSnapArea::PortraitThirdsSide => &[L, R],
            CompoundSnapArea::Halves => &[T, B],
            CompoundSnapArea::TopSixths => &[T],
            CompoundSnapArea::BottomSixths => &[B],
            CompoundSnapArea::Fourths => &[T, B],
            CompoundSnapArea::PortraitTopBottomHalves => &[L, R],
            CompoundSnapArea::TopEighths => &[T],
            CompoundSnapArea::BottomEighths => &[B],
        }
    }

    /// Для каких экранов (`compatibleOrientation`).
    pub fn compatible_orientations(self) -> &'static [DisplayOrientation] {
        use DisplayOrientation::{Landscape, Portrait};
        match self {
            CompoundSnapArea::LeftTopBottomHalf
            | CompoundSnapArea::RightTopBottomHalf
            | CompoundSnapArea::Halves => &[Portrait, Landscape],
            CompoundSnapArea::PortraitThirdsSide | CompoundSnapArea::PortraitTopBottomHalves => {
                &[Portrait]
            }
            CompoundSnapArea::Thirds
            | CompoundSnapArea::TopSixths
            | CompoundSnapArea::BottomSixths
            | CompoundSnapArea::Fourths
            | CompoundSnapArea::TopEighths
            | CompoundSnapArea::BottomEighths => &[Landscape],
        }
    }

    /// Действие области для курсора `loc` (Cocoa) на экране с рамкой `frame`
    /// (`calculation.snapArea(cursorLocation:screen:directional:priorSnapArea:)`).
    /// `prior` — действие области, в которой курсор был до этого.
    pub fn snap_action(
        self,
        loc: (f64, f64),
        frame: &Rect,
        prior: Option<Action>,
        params: &CompoundParams,
    ) -> Option<Action> {
        match self {
            CompoundSnapArea::LeftTopBottomHalf => Some(side_top_bottom_half(
                loc,
                frame,
                params,
                SnapAreaOption::BOTTOM_LEFT_SHORT,
                SnapAreaOption::TOP_LEFT_SHORT,
                Action::LeftHalf,
            )),
            CompoundSnapArea::RightTopBottomHalf => Some(side_top_bottom_half(
                loc,
                frame,
                params,
                SnapAreaOption::BOTTOM_RIGHT_SHORT,
                SnapAreaOption::TOP_RIGHT_SHORT,
                Action::RightHalf,
            )),
            CompoundSnapArea::Thirds => thirds(loc, frame, prior),
            CompoundSnapArea::PortraitThirdsSide => portrait_side_thirds(loc, frame, prior, params),
            CompoundSnapArea::Halves => Some(left_right_halves(loc, frame)),
            CompoundSnapArea::TopSixths => Some(top_sixths(loc, frame, prior)),
            CompoundSnapArea::BottomSixths => bottom_sixths(loc, frame, prior),
            CompoundSnapArea::Fourths => fourths(loc, frame, prior),
            CompoundSnapArea::PortraitTopBottomHalves => top_bottom_halves(loc, frame, params),
            CompoundSnapArea::TopEighths => Some(top_eighths(loc, frame, prior)),
            CompoundSnapArea::BottomEighths => bottom_eighths(loc, frame, prior),
        }
    }
}

/// `LeftTopBottomHalfCalculation` / `RightTopBottomHalfCalculation`: у коротких
/// краёв — верхняя/нижняя половина, иначе половина своей стороны.
fn side_top_bottom_half(
    (_, y): (f64, f64),
    frame: &Rect,
    params: &CompoundParams,
    bottom_short: SnapAreaOption,
    top_short: SnapAreaOption,
    side: Action,
) -> Action {
    if y <= frame.min_y() + params.margin_bottom + params.short_edge_size
        && !params.ignored.contains(bottom_short)
    {
        return Action::BottomHalf;
    }
    if y >= frame.max_y() - params.margin_top - params.short_edge_size
        && !params.ignored.contains(top_short)
    {
        return Action::TopHalf;
    }
    side
}

/// `LeftRightHalvesCompoundCalculation`.
fn left_right_halves((x, _): (f64, f64), frame: &Rect) -> Action {
    if x < frame.max_x() - frame.w / 2.0 {
        Action::LeftHalf
    } else {
        Action::RightHalf
    }
}

/// `TopBottomHalvesCalculation`.
fn top_bottom_halves((x, y): (f64, f64), frame: &Rect, params: &CompoundParams) -> Option<Action> {
    let half_height = (frame.h / 2.0).floor();
    if y <= frame.min_y() + params.margin_bottom + params.short_edge_size {
        let option = if x < frame.mid_x() {
            SnapAreaOption::BOTTOM_LEFT_SHORT
        } else {
            SnapAreaOption::BOTTOM_RIGHT_SHORT
        };
        if !params.ignored.contains(option) {
            return Some(Action::BottomHalf);
        }
    }
    if y >= frame.max_y() - params.margin_top - params.short_edge_size {
        let option = if x < frame.mid_x() {
            SnapAreaOption::TOP_LEFT_SHORT
        } else {
            SnapAreaOption::TOP_RIGHT_SHORT
        };
        if !params.ignored.contains(option) {
            return Some(Action::TopHalf);
        }
    }
    if y >= frame.min_y() && y <= frame.min_y() + half_height {
        return Some(Action::BottomHalf);
    }
    if y > frame.min_y() + half_height && y <= frame.max_y() {
        return Some(Action::TopHalf);
    }
    None
}

/// Средняя треть: от крайней трети — две трети с её стороны
/// (общая часть `ThirdsCompoundCalculation` и `PortraitSideThirdsCompoundCalculation`).
fn center_or_two_thirds(prior: Option<Action>) -> Action {
    match prior {
        Some(Action::FirstThird | Action::FirstTwoThirds) => Action::FirstTwoThirds,
        Some(Action::LastThird | Action::LastTwoThirds) => Action::LastTwoThirds,
        _ => Action::CenterThird,
    }
}

/// `ThirdsCompoundCalculation`.
fn thirds((x, _): (f64, f64), frame: &Rect, prior: Option<Action>) -> Option<Action> {
    let third_width = (frame.w / 3.0).floor();
    if x <= frame.min_x() + third_width {
        return Some(Action::FirstThird);
    }
    if x >= frame.min_x() + third_width && x <= frame.max_x() - third_width {
        return Some(center_or_two_thirds(prior));
    }
    if x >= frame.min_x() + third_width {
        return Some(Action::LastThird);
    }
    None
}

/// `PortraitSideThirdsCompoundCalculation`: трети по высоте, у коротких краёв —
/// верхняя/нижняя половина.
fn portrait_side_thirds(
    (x, y): (f64, f64),
    frame: &Rect,
    prior: Option<Action>,
    params: &CompoundParams,
) -> Option<Action> {
    let third_height = (frame.h / 3.0).floor();
    if y <= frame.min_y() + params.margin_bottom + params.short_edge_size {
        let option = if x < frame.mid_x() {
            SnapAreaOption::BOTTOM_LEFT_SHORT
        } else {
            SnapAreaOption::BOTTOM_RIGHT_SHORT
        };
        if !params.ignored.contains(option) {
            return Some(Action::BottomHalf);
        }
    }
    if y >= frame.max_y() - params.margin_top - params.short_edge_size {
        let option = if x < frame.mid_x() {
            SnapAreaOption::TOP_LEFT_SHORT
        } else {
            SnapAreaOption::TOP_RIGHT_SHORT
        };
        if !params.ignored.contains(option) {
            return Some(Action::TopHalf);
        }
    }
    if y >= frame.min_y() && y <= frame.min_y() + third_height {
        return Some(Action::LastThird);
    }
    if y >= frame.min_y() + third_height && y <= frame.max_y() - third_height {
        return Some(center_or_two_thirds(prior));
    }
    if y >= frame.min_y() + third_height && y <= frame.max_y() {
        return Some(Action::FirstThird);
    }
    None
}

/// `FourthsColumnCompoundCalculation`.
fn fourths((x, _): (f64, f64), frame: &Rect, prior: Option<Action>) -> Option<Action> {
    let quarter_width = (frame.w / 4.0).floor();
    if x <= frame.min_x() + quarter_width {
        return Some(Action::FirstFourth);
    }
    if x >= frame.min_x() + quarter_width && x <= frame.max_x() - quarter_width * 2.0 {
        return Some(match prior {
            Some(Action::FirstFourth | Action::FirstThreeFourths) => Action::FirstThreeFourths,
            Some(Action::ThirdFourth | Action::LastThreeFourths | Action::CenterHalf) => {
                Action::CenterHalf
            }
            _ => Action::SecondFourth,
        });
    }
    if x >= frame.min_x() + quarter_width * 2.0 && x <= frame.max_x() - quarter_width {
        return Some(match prior {
            Some(Action::LastFourth | Action::LastThreeFourths) => Action::LastThreeFourths,
            Some(Action::SecondFourth | Action::FirstThreeFourths | Action::CenterHalf) => {
                Action::CenterHalf
            }
            _ => Action::ThirdFourth,
        });
    }
    if x >= frame.min_x() + quarter_width * 2.0 {
        return Some(Action::LastFourth);
    }
    None
}

/// `TopSixthsCompoundCalculation`: без предыдущей области — развернуть; из угла
/// вдоль края — шестые.
fn top_sixths((x, _): (f64, f64), frame: &Rect, prior: Option<Action>) -> Action {
    let Some(prior) = prior else {
        return Action::Maximize;
    };
    let third_width = (frame.w / 3.0).floor();
    if x <= frame.min_x() + third_width
        && matches!(
            prior,
            Action::TopLeft | Action::TopLeftSixth | Action::TopCenterSixth
        )
    {
        return Action::TopLeftSixth;
    }
    if x >= frame.max_x() - third_width
        && matches!(
            prior,
            Action::TopRight | Action::TopRightSixth | Action::TopCenterSixth
        )
    {
        return Action::TopRightSixth;
    }
    if matches!(
        prior,
        Action::TopLeftSixth | Action::TopRightSixth | Action::TopCenterSixth
    ) {
        Action::TopCenterSixth
    } else {
        Action::Maximize
    }
}

/// `BottomSixthsCompoundCalculation`: без предыдущей области — трети.
/// Правая шестая проверяется от `minX + ⅓`, как в оригинале.
fn bottom_sixths((x, y): (f64, f64), frame: &Rect, prior: Option<Action>) -> Option<Action> {
    let Some(prior_action) = prior else {
        return thirds((x, y), frame, prior);
    };
    let third_width = (frame.w / 3.0).floor();
    if x <= frame.min_x() + third_width
        && matches!(
            prior_action,
            Action::BottomLeft | Action::BottomLeftSixth | Action::BottomCenterSixth
        )
    {
        return Some(Action::BottomLeftSixth);
    }
    if x >= frame.min_x() + third_width
        && x <= frame.max_x() - third_width
        && matches!(
            prior_action,
            Action::BottomRightSixth | Action::BottomLeftSixth | Action::BottomCenterSixth
        )
    {
        return Some(Action::BottomCenterSixth);
    }
    if x >= frame.min_x() + third_width
        && matches!(
            prior_action,
            Action::BottomRight | Action::BottomRightSixth | Action::BottomCenterSixth
        )
    {
        return Some(Action::BottomRightSixth);
    }
    thirds((x, y), frame, prior)
}

/// `TopEighthsCompoundCalculation`.
fn top_eighths((x, _): (f64, f64), frame: &Rect, prior: Option<Action>) -> Action {
    use Action::*;
    let Some(prior) = prior else {
        return Maximize;
    };
    let quarter_width = (frame.w / 4.0).floor();
    if x <= frame.min_x() + quarter_width
        && matches!(prior, TopLeft | TopLeftEighth | TopCenterLeftEighth)
    {
        return TopLeftEighth;
    }
    if x >= frame.min_x() + quarter_width
        && x <= frame.mid_x()
        && matches!(
            prior,
            TopLeftEighth | TopCenterLeftEighth | TopCenterRightEighth
        )
    {
        return TopCenterLeftEighth;
    }
    if x >= frame.mid_x()
        && x <= frame.max_x() - quarter_width
        && matches!(
            prior,
            TopCenterLeftEighth | TopCenterRightEighth | TopRightEighth
        )
    {
        return TopCenterRightEighth;
    }
    if x >= frame.max_x() - quarter_width
        && matches!(prior, TopRight | TopRightEighth | TopCenterRightEighth)
    {
        return TopRightEighth;
    }
    Maximize
}

/// `BottomEighthsCompoundCalculation`: без предыдущей области и вне восьмых — трети.
fn bottom_eighths((x, y): (f64, f64), frame: &Rect, prior: Option<Action>) -> Option<Action> {
    use Action::*;
    let Some(prior_action) = prior else {
        return thirds((x, y), frame, prior);
    };
    let quarter_width = (frame.w / 4.0).floor();
    if x <= frame.min_x() + quarter_width
        && matches!(
            prior_action,
            BottomLeft | BottomLeftEighth | BottomCenterLeftEighth
        )
    {
        return Some(BottomLeftEighth);
    }
    if x >= frame.min_x() + quarter_width
        && x <= frame.mid_x()
        && matches!(
            prior_action,
            BottomLeftEighth | BottomCenterLeftEighth | BottomCenterRightEighth
        )
    {
        return Some(BottomCenterLeftEighth);
    }
    if x >= frame.mid_x()
        && x <= frame.max_x() - quarter_width
        && matches!(
            prior_action,
            BottomCenterLeftEighth | BottomCenterRightEighth | BottomRightEighth
        )
    {
        return Some(BottomCenterRightEighth);
    }
    if x >= frame.max_x() - quarter_width
        && matches!(
            prior_action,
            BottomRight | BottomRightEighth | BottomCenterRightEighth
        )
    {
        return Some(BottomRightEighth);
    }
    thirds((x, y), frame, prior)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Экран 1512×982 в начале координат, как у встроенного дисплея.
    fn frame() -> Rect {
        Rect::new(0.0, 0.0, 1512.0, 982.0)
    }

    fn params() -> CompoundParams {
        CompoundParams::default()
    }

    /// Провести курсор по точкам вдоль края: каждое следующее решение видит
    /// предыдущее (`priorSnapArea`), как при перетаскивании.
    fn walk(
        area: CompoundSnapArea,
        frame: &Rect,
        start: Option<Action>,
        points: &[(f64, f64)],
    ) -> Vec<Option<Action>> {
        let mut prior = start;
        points
            .iter()
            .map(|&point| {
                let action = area.snap_action(point, frame, prior, &params());
                prior = action;
                action
            })
            .collect()
    }

    #[test]
    fn thirds_switch_to_two_thirds_toward_center() {
        let frame = frame();
        // ⅓ ширины = floor(504) = 504.
        use Action::*;
        let area = CompoundSnapArea::Thirds;
        // Вход с левой трети, к центру — первые две трети, дальше вправо — последняя треть.
        assert_eq!(
            walk(
                area,
                &frame,
                None,
                &[(10.0, 2.0), (504.0, 2.0), (700.0, 2.0), (1100.0, 2.0)]
            ),
            vec![
                Some(FirstThird),
                Some(FirstThird),
                Some(FirstTwoThirds),
                Some(LastThird)
            ]
        );
        // Сразу в центр — центральная треть; справа к центру — последние две трети.
        assert_eq!(
            walk(area, &frame, None, &[(756.0, 2.0)]),
            vec![Some(CenterThird)]
        );
        assert_eq!(
            walk(
                area,
                &frame,
                None,
                &[(1500.0, 2.0), (1008.0, 2.0), (600.0, 2.0)]
            ),
            vec![Some(LastThird), Some(LastTwoThirds), Some(LastTwoThirds)]
        );
        // Граница: x = minX + ⅓ ещё первая треть (`<=`), maxX − ⅓ — ещё центр.
        assert_eq!(
            area.snap_action((504.0, 0.0), &frame, None, &params()),
            Some(FirstThird)
        );
        assert_eq!(
            area.snap_action((1008.0, 0.0), &frame, None, &params()),
            Some(CenterThird)
        );
        assert_eq!(
            area.snap_action((1008.5, 0.0), &frame, None, &params()),
            Some(LastThird)
        );
    }

    #[test]
    fn thirds_use_floor_on_odd_widths_and_screen_offset() {
        // Второй экран правее: x отсчитывается от его minX.
        let frame = Rect::new(1512.0, -200.0, 1000.0, 1400.0);
        // floor(1000/3) = 333.
        let area = CompoundSnapArea::Thirds;
        assert_eq!(
            area.snap_action((1512.0 + 333.0, 0.0), &frame, None, &params()),
            Some(Action::FirstThird)
        );
        assert_eq!(
            area.snap_action((1512.0 + 333.5, 0.0), &frame, None, &params()),
            Some(Action::CenterThird)
        );
        assert_eq!(
            area.snap_action((1512.0 + 667.0, 0.0), &frame, None, &params()),
            Some(Action::CenterThird)
        );
        assert_eq!(
            area.snap_action((1512.0 + 667.5, 0.0), &frame, None, &params()),
            Some(Action::LastThird)
        );
    }

    #[test]
    fn fourths_grow_to_three_fourths_and_center_half() {
        use Action::*;
        let frame = Rect::new(0.0, 0.0, 1600.0, 900.0);
        let area = CompoundSnapArea::Fourths;
        // Четверть = 400. Слева направо: 1-я → ¾ слева → центр половина → последняя.
        assert_eq!(
            walk(
                area,
                &frame,
                None,
                &[(100.0, 0.0), (500.0, 0.0), (900.0, 0.0), (1300.0, 0.0)]
            ),
            vec![
                Some(FirstFourth),
                Some(FirstThreeFourths),
                Some(CenterHalf),
                Some(LastFourth)
            ]
        );
        // Справа налево: последняя → ¾ справа → центр половина → первая.
        assert_eq!(
            walk(
                area,
                &frame,
                None,
                &[(1500.0, 0.0), (1100.0, 0.0), (700.0, 0.0), (300.0, 0.0)]
            ),
            vec![
                Some(LastFourth),
                Some(LastThreeFourths),
                Some(CenterHalf),
                Some(FirstFourth)
            ]
        );
        // Сразу во вторую/третью колонку — вторая/третья четверть.
        assert_eq!(
            area.snap_action((500.0, 0.0), &frame, None, &params()),
            Some(SecondFourth)
        );
        assert_eq!(
            area.snap_action((1100.0, 0.0), &frame, None, &params()),
            Some(ThirdFourth)
        );
        // Граница minX + 2·¼ = 800 относится ко второй колонке (проверяется раньше).
        assert_eq!(
            area.snap_action((800.0, 0.0), &frame, None, &params()),
            Some(SecondFourth)
        );
    }

    #[test]
    fn side_halves_turn_into_top_and_bottom_near_corners() {
        use Action::*;
        let frame = frame();
        let left = CompoundSnapArea::LeftTopBottomHalf;
        // Короткая зона: 5 + 145 = 150 от нижнего и верхнего края.
        assert_eq!(
            left.snap_action((0.0, 150.0), &frame, None, &params()),
            Some(BottomHalf)
        );
        assert_eq!(
            left.snap_action((0.0, 150.5), &frame, None, &params()),
            Some(LeftHalf)
        );
        assert_eq!(
            left.snap_action((0.0, 832.0), &frame, None, &params()),
            Some(TopHalf)
        );
        assert_eq!(
            left.snap_action((0.0, 831.5), &frame, None, &params()),
            Some(LeftHalf)
        );
        let right = CompoundSnapArea::RightTopBottomHalf;
        assert_eq!(
            right.snap_action((1511.0, 491.0), &frame, None, &params()),
            Some(RightHalf)
        );
        assert_eq!(
            right.snap_action((1511.0, 10.0), &frame, None, &params()),
            Some(BottomHalf)
        );

        // Выключенные короткие зоны (`ignoredSnapAreas`) — только половина стороны.
        let ignored = CompoundParams {
            ignored: SnapAreaOption::BOTTOM_LEFT_SHORT | SnapAreaOption::TOP_LEFT_SHORT,
            ..params()
        };
        assert_eq!(
            left.snap_action((0.0, 10.0), &frame, None, &ignored),
            Some(LeftHalf)
        );
        assert_eq!(
            left.snap_action((0.0, 970.0), &frame, None, &ignored),
            Some(LeftHalf)
        );
        assert_eq!(
            right.snap_action((1511.0, 10.0), &frame, None, &ignored),
            Some(BottomHalf)
        );

        // Размер короткой зоны берётся из настроек.
        let short = CompoundParams {
            short_edge_size: 50.0,
            ..params()
        };
        assert_eq!(
            left.snap_action((0.0, 100.0), &frame, None, &short),
            Some(LeftHalf)
        );
        assert_eq!(
            left.snap_action((0.0, 55.0), &frame, None, &short),
            Some(BottomHalf)
        );
    }

    #[test]
    fn halves_split_at_middle() {
        let frame = Rect::new(-1080.0, 0.0, 1080.0, 1920.0);
        let area = CompoundSnapArea::Halves;
        assert_eq!(
            area.snap_action((-541.0, 0.0), &frame, None, &params()),
            Some(Action::LeftHalf)
        );
        assert_eq!(
            area.snap_action((-540.0, 0.0), &frame, None, &params()),
            Some(Action::RightHalf)
        );
    }

    #[test]
    fn portrait_sides_are_thirds_by_height() {
        use Action::*;
        // Вертикальный экран 1080×1920: ⅓ высоты = 640.
        let frame = Rect::new(0.0, 0.0, 1080.0, 1920.0);
        let area = CompoundSnapArea::PortraitThirdsSide;
        // У коротких краёв — половины.
        assert_eq!(
            area.snap_action((0.0, 100.0), &frame, None, &params()),
            Some(BottomHalf)
        );
        assert_eq!(
            area.snap_action((0.0, 1850.0), &frame, None, &params()),
            Some(TopHalf)
        );
        // Снизу вверх: нижняя треть (последняя) → к центру — последние две трети → верхняя.
        assert_eq!(
            walk(
                area,
                &frame,
                None,
                &[(0.0, 300.0), (0.0, 900.0), (0.0, 1500.0)]
            ),
            vec![Some(LastThird), Some(LastTwoThirds), Some(FirstThird)]
        );
        assert_eq!(
            walk(area, &frame, None, &[(0.0, 1500.0), (0.0, 1000.0)]),
            vec![Some(FirstThird), Some(FirstTwoThirds)]
        );
        assert_eq!(
            area.snap_action((0.0, 960.0), &frame, None, &params()),
            Some(CenterThird)
        );
    }

    #[test]
    fn portrait_top_bottom_halves() {
        use Action::*;
        let frame = Rect::new(0.0, 0.0, 1080.0, 1920.0);
        let area = CompoundSnapArea::PortraitTopBottomHalves;
        assert_eq!(
            area.snap_action((1079.0, 960.0), &frame, None, &params()),
            Some(BottomHalf)
        );
        assert_eq!(
            area.snap_action((1079.0, 960.5), &frame, None, &params()),
            Some(TopHalf)
        );
        // Выключенные короткие зоны слева — решает половина высоты.
        let ignored = CompoundParams {
            ignored: SnapAreaOption::BOTTOM_LEFT_SHORT | SnapAreaOption::TOP_LEFT_SHORT,
            ..params()
        };
        assert_eq!(
            area.snap_action((0.0, 20.0), &frame, None, &ignored),
            Some(BottomHalf)
        );
        assert_eq!(
            area.snap_action((0.0, 1900.0), &frame, None, &ignored),
            Some(TopHalf)
        );
        // Вне экрана по высоте (короткие зоны выключены) — ничего.
        assert_eq!(
            area.snap_action((0.0, 1921.0), &frame, None, &ignored),
            None
        );
    }

    #[test]
    fn sixths_come_from_corners() {
        use Action::*;
        let frame = Rect::new(0.0, 0.0, 1800.0, 1000.0);
        let top = CompoundSnapArea::TopSixths;
        // Без предыдущей области — развернуть.
        assert_eq!(
            top.snap_action((100.0, 999.0), &frame, None, &params()),
            Some(Maximize)
        );
        // Из левого верхнего угла вдоль края: левая шестая → центр → правая.
        assert_eq!(
            walk(
                top,
                &frame,
                Some(TopLeft),
                &[(300.0, 999.0), (900.0, 999.0), (1700.0, 999.0)]
            ),
            vec![
                Some(TopLeftSixth),
                Some(TopCenterSixth),
                Some(TopRightSixth)
            ]
        );
        // Из другой области (не угол, не шестая) — развернуть.
        assert_eq!(
            top.snap_action((300.0, 999.0), &frame, Some(LeftHalf), &params()),
            Some(Maximize)
        );

        let bottom = CompoundSnapArea::BottomSixths;
        // Без предыдущей области — трети.
        assert_eq!(
            bottom.snap_action((100.0, 0.0), &frame, None, &params()),
            Some(FirstThird)
        );
        assert_eq!(
            walk(
                bottom,
                &frame,
                Some(BottomLeft),
                &[(100.0, 0.0), (900.0, 0.0), (1700.0, 0.0)]
            ),
            vec![
                Some(BottomLeftSixth),
                Some(BottomCenterSixth),
                Some(BottomRightSixth)
            ]
        );
        // Причуда оригинала: из правого нижнего угла в середину — всё ещё правая шестая.
        assert_eq!(
            bottom.snap_action((900.0, 0.0), &frame, Some(BottomRight), &params()),
            Some(BottomRightSixth)
        );
        // Из чужой области — трети с учётом предыдущей.
        assert_eq!(
            bottom.snap_action((900.0, 0.0), &frame, Some(FirstThird), &params()),
            Some(FirstTwoThirds)
        );
    }

    #[test]
    fn eighths_come_from_corners() {
        use Action::*;
        let frame = Rect::new(0.0, 0.0, 1600.0, 1000.0);
        let top = CompoundSnapArea::TopEighths;
        assert_eq!(
            top.snap_action((100.0, 999.0), &frame, None, &params()),
            Some(Maximize)
        );
        assert_eq!(
            walk(
                top,
                &frame,
                Some(TopLeft),
                &[
                    (100.0, 999.0),
                    (500.0, 999.0),
                    (900.0, 999.0),
                    (1300.0, 999.0)
                ]
            ),
            vec![
                Some(TopLeftEighth),
                Some(TopCenterLeftEighth),
                Some(TopCenterRightEighth),
                Some(TopRightEighth)
            ]
        );
        // Перепрыгнуть колонку нельзя: из левой восьмой сразу в правую половину — развернуть.
        assert_eq!(
            top.snap_action((1300.0, 999.0), &frame, Some(TopLeftEighth), &params()),
            Some(Maximize)
        );

        let bottom = CompoundSnapArea::BottomEighths;
        assert_eq!(
            bottom.snap_action((100.0, 0.0), &frame, None, &params()),
            Some(FirstThird)
        );
        assert_eq!(
            walk(
                bottom,
                &frame,
                Some(BottomRight),
                &[(1500.0, 0.0), (1100.0, 0.0), (700.0, 0.0), (300.0, 0.0)]
            ),
            vec![
                Some(BottomRightEighth),
                Some(BottomCenterRightEighth),
                Some(BottomCenterLeftEighth),
                Some(BottomLeftEighth)
            ]
        );
    }

    #[test]
    fn compatibility_matches_swift() {
        use Directional::*;
        use DisplayOrientation::*;
        assert_eq!(CompoundSnapArea::ALL.len(), 11);
        assert_eq!(CompoundSnapArea::Thirds.compatible_directionals(), &[T, B]);
        assert_eq!(
            CompoundSnapArea::PortraitThirdsSide.compatible_directionals(),
            &[L, R]
        );
        assert_eq!(
            CompoundSnapArea::Halves.compatible_orientations(),
            &[Portrait, Landscape]
        );
        assert_eq!(
            CompoundSnapArea::TopEighths.compatible_orientations(),
            &[Landscape]
        );
        for area in CompoundSnapArea::ALL {
            assert!(!area.display_name().is_empty());
            assert!(area.raw() <= -2);
        }
    }
}
