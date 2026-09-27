//! Сеточные раскладки: трети, четверти, три четверти, шестые, восьмые, девятые,
//! двенадцатые, шестнадцатые, угловые трети, вертикальные трети — порт
//! `WindowCalculation/*Calculation.swift` и `*Repeated.swift` из Rectangle 2.
//!
//! Прямоугольник ячейки зависит только от рабочей области и её ориентации
//! (`OrientationAware`: `landscapeRect`/`portraitRect`), у каждой ячейки своё
//! под-действие. Повторное нажатие переходит к соседней ячейке по под-действию
//! прошлого нажатия (`nextCalculation`) — у некоторых раскладок и тогда, когда прошлое
//! действие было другим, но окно встало в ячейку этого действия.
//!
//! Математика буквально как в Swift: `floor` там, где он есть, и его отсутствие там,
//! где его нет (ширина центральной трети — `width / 3.0` без `floor`, отступ —
//! `floor(width / 3.0)`), и тот же порядок операций.

use super::repeated::{center_half_landscape, center_half_portrait};
use super::{LastAction, RectParams, RectResult};
use crate::actions::{Action, SubAction};
use crate::config::SubsequentExecutionMode;
use crate::geometry::Rect;

/// Ячейка с под-действием: `var rect = visibleFrameOfScreen` и новые поля.
fn cell(x: f64, y: f64, w: f64, h: f64, sub_action: SubAction) -> Option<RectResult> {
    Some(RectResult::with_sub(Rect::new(x, y, w, h), sub_action))
}

/// Ячейка на горизонтальной рабочей области (`landscapeRect`). `None` — не сеточное
/// действие.
fn landscape_rect(action: Action, v: &Rect) -> Option<RectResult> {
    use Action::*;
    use SubAction as S;
    let (min_x, min_y, max_y, width, height) = (v.min_x(), v.min_y(), v.max_y(), v.w, v.h);
    match action {
        // Трети.
        FirstThird => cell(v.x, v.y, (width / 3.0).floor(), height, S::LeftThird),
        CenterThird => cell(
            min_x + (width / 3.0).floor(),
            min_y,
            width / 3.0,
            height,
            S::CenterVerticalThird,
        ),
        LastThird => {
            let w = (width / 3.0).floor();
            cell(v.x + width - w, v.y, w, height, S::RightThird)
        }
        FirstTwoThirds => cell(
            v.x,
            v.y,
            (width * 2.0 / 3.0).floor(),
            height,
            S::LeftTwoThirds,
        ),
        CenterTwoThirds => cell(
            min_x + (width / 3.0).floor() / 2.0,
            min_y,
            width / 3.0 * 2.0,
            height,
            S::CenterVerticalThird,
        ),
        LastTwoThirds => {
            let w = (width * 2.0 / 3.0).floor();
            cell(min_x + width - w, v.y, w, height, S::RightTwoThirds)
        }

        // Четверти и три четверти.
        FirstFourth => cell(v.x, v.y, (width / 4.0).floor(), height, S::LeftFourth),
        SecondFourth => {
            let w = (width / 4.0).floor();
            cell(min_x + w, v.y, w, height, S::CenterLeftFourth)
        }
        ThirdFourth => {
            let w = (width / 4.0).floor();
            cell(min_x + w * 2.0, v.y, w, height, S::CenterRightFourth)
        }
        LastFourth => {
            let w = (width / 4.0).floor();
            cell(v.x + width - w, v.y, w, height, S::RightFourth)
        }
        FirstThreeFourths => cell(
            v.x,
            v.y,
            (width * 3.0 / 4.0).floor(),
            height,
            S::LeftThreeFourths,
        ),
        CenterThreeFourths => cell(
            min_x + (width / 4.0).floor() / 2.0,
            min_y,
            width / 4.0 * 3.0,
            height,
            S::CenterVerticalThreeFourths,
        ),
        LastThreeFourths => {
            let w = (width * 3.0 / 4.0).floor();
            cell(min_x + width - w, v.y, w, height, S::RightThreeFourths)
        }

        // Шестые: 3 столбца × 2 ряда.
        TopLeftSixth => {
            let (w, h) = ((width / 3.0).floor(), (height / 2.0).floor());
            cell(v.x, max_y - h, w, h, S::TopLeftSixthLandscape)
        }
        TopCenterSixth => {
            let (w, h) = ((width / 3.0).floor(), (height / 2.0).floor());
            cell(min_x + w, max_y - h, w, h, S::TopCenterSixthLandscape)
        }
        TopRightSixth => {
            let (w, h) = ((width / 3.0).floor(), (height / 2.0).floor());
            cell(
                min_x + width - w,
                max_y - h,
                w,
                h,
                S::TopRightSixthLandscape,
            )
        }
        BottomLeftSixth => {
            let (w, h) = ((width / 3.0).floor(), (height / 2.0).floor());
            cell(v.x, v.y, w, h, S::BottomLeftSixthLandscape)
        }
        BottomCenterSixth => {
            let (w, h) = ((width / 3.0).floor(), (height / 2.0).floor());
            cell(v.x + w, v.y, w, h, S::BottomCenterSixthLandscape)
        }
        BottomRightSixth => {
            let (w, h) = ((width / 3.0).floor(), (height / 2.0).floor());
            cell(v.x + width - w, v.y, w, h, S::BottomRightSixthLandscape)
        }

        // Девятые: 3 × 3.
        TopLeftNinth | TopCenterNinth | TopRightNinth | MiddleLeftNinth | MiddleCenterNinth
        | MiddleRightNinth | BottomLeftNinth | BottomCenterNinth | BottomRightNinth => {
            ninth_rect(action, v)
        }

        // Угловые трети: ⅔ ширины × ½ высоты.
        TopLeftThird => cell(
            min_x,
            max_y - height / 2.0,
            (2.0 * width / 3.0).floor(),
            (height / 2.0).floor(),
            S::TopLeftThird,
        ),
        TopRightThird => {
            let h = (height / 2.0).floor();
            cell(
                min_x + width / 3.0,
                max_y - h,
                (2.0 * width / 3.0).floor(),
                h,
                S::TopRightThird,
            )
        }
        BottomLeftThird => cell(
            min_x,
            min_y,
            (2.0 * width / 3.0).floor(),
            (height / 2.0).floor(),
            S::BottomLeftThird,
        ),
        BottomRightThird => cell(
            min_x + width / 3.0,
            min_y,
            (2.0 * width / 3.0).floor(),
            (height / 2.0).floor(),
            S::BottomRightThird,
        ),

        // Восьмые: 4 столбца × 2 ряда.
        TopLeftEighth
        | TopCenterLeftEighth
        | TopCenterRightEighth
        | TopRightEighth
        | BottomLeftEighth
        | BottomCenterLeftEighth
        | BottomCenterRightEighth
        | BottomRightEighth => {
            let (w, h) = ((width / 4.0).floor(), (height / 2.0).floor());
            let (x, y, sub_action) = match action {
                TopLeftEighth => (min_x, max_y - h, S::TopLeftEighth),
                TopCenterLeftEighth => (min_x + w, max_y - h, S::TopCenterLeftEighth),
                TopCenterRightEighth => (min_x + 2.0 * w, max_y - h, S::TopCenterRightEighth),
                TopRightEighth => (min_x + 3.0 * w, max_y - h, S::TopRightEighth),
                BottomLeftEighth => (min_x, min_y, S::BottomLeftEighth),
                BottomCenterLeftEighth => (min_x + w, min_y, S::BottomCenterLeftEighth),
                BottomCenterRightEighth => (min_x + 2.0 * w, min_y, S::BottomCenterRightEighth),
                _ => (min_x + 3.0 * w, min_y, S::BottomRightEighth),
            };
            cell(x, y, w, h, sub_action)
        }

        // Двенадцатые: 4 столбца × 3 ряда.
        TopLeftTwelfth
        | TopCenterLeftTwelfth
        | TopCenterRightTwelfth
        | TopRightTwelfth
        | MiddleLeftTwelfth
        | MiddleCenterLeftTwelfth
        | MiddleCenterRightTwelfth
        | MiddleRightTwelfth
        | BottomLeftTwelfth
        | BottomCenterLeftTwelfth
        | BottomCenterRightTwelfth
        | BottomRightTwelfth => {
            let (w, h) = ((width / 4.0).floor(), (height / 3.0).floor());
            let quarter = (width / 4.0).floor();
            let (x, y, sub_action) = match action {
                TopLeftTwelfth => (min_x, max_y - h, S::TopLeftTwelfth),
                TopCenterLeftTwelfth => (min_x + w, max_y - h, S::TopCenterLeftTwelfth),
                TopCenterRightTwelfth => {
                    (min_x + quarter * 2.0, max_y - h, S::TopCenterRightTwelfth)
                }
                TopRightTwelfth => (min_x + quarter * 3.0, max_y - h, S::TopRightTwelfth),
                MiddleLeftTwelfth => (min_x, min_y + h, S::MiddleLeftTwelfth),
                MiddleCenterLeftTwelfth => (min_x + w, min_y + h, S::MiddleCenterLeftTwelfth),
                MiddleCenterRightTwelfth => (
                    min_x + quarter * 2.0,
                    min_y + h,
                    S::MiddleCenterRightTwelfth,
                ),
                MiddleRightTwelfth => (min_x + quarter * 3.0, min_y + h, S::MiddleRightTwelfth),
                BottomLeftTwelfth => (min_x, min_y, S::BottomLeftTwelfth),
                BottomCenterLeftTwelfth => (min_x + w, min_y, S::BottomCenterLeftTwelfth),
                BottomCenterRightTwelfth => {
                    (min_x + quarter * 2.0, min_y, S::BottomCenterRightTwelfth)
                }
                _ => (min_x + quarter * 3.0, min_y, S::BottomRightTwelfth),
            };
            cell(x, y, w, h, sub_action)
        }

        // Шестнадцатые: 4 × 4.
        TopLeftSixteenth
        | TopCenterLeftSixteenth
        | TopCenterRightSixteenth
        | TopRightSixteenth
        | UpperMiddleLeftSixteenth
        | UpperMiddleCenterLeftSixteenth
        | UpperMiddleCenterRightSixteenth
        | UpperMiddleRightSixteenth
        | LowerMiddleLeftSixteenth
        | LowerMiddleCenterLeftSixteenth
        | LowerMiddleCenterRightSixteenth
        | LowerMiddleRightSixteenth
        | BottomLeftSixteenth
        | BottomCenterLeftSixteenth
        | BottomCenterRightSixteenth
        | BottomRightSixteenth => sixteenth_rect(action, v),

        _ => None,
    }
}

/// Ячейка на вертикальной (или квадратной) рабочей области (`portraitRect`). `None` —
/// не сеточное действие.
fn portrait_rect(action: Action, v: &Rect) -> Option<RectResult> {
    use Action::*;
    use SubAction as S;
    let (min_x, min_y, max_y, width, height) = (v.min_x(), v.min_y(), v.max_y(), v.w, v.h);
    match action {
        // Трети — по высоте.
        FirstThird => {
            let h = (height / 3.0).floor();
            cell(v.x, min_y + height - h, width, h, S::TopThird)
        }
        CenterThird => cell(
            min_x,
            min_y + (height / 3.0).floor(),
            width,
            height / 3.0,
            S::CenterHorizontalThird,
        ),
        LastThird => cell(v.x, v.y, width, (height / 3.0).floor(), S::BottomThird),
        FirstTwoThirds => {
            let h = (height * 2.0 / 3.0).floor();
            cell(v.x, v.y + height - h, width, h, S::TopTwoThirds)
        }
        CenterTwoThirds => cell(
            min_x,
            min_y + (height / 3.0).floor() / 2.0,
            width,
            height / 3.0 * 2.0,
            S::CenterHorizontalThird,
        ),
        LastTwoThirds => cell(
            v.x,
            v.y,
            width,
            (height * 2.0 / 3.0).floor(),
            S::BottomTwoThirds,
        ),

        // Четверти и три четверти — по высоте.
        FirstFourth => {
            let h = (height / 4.0).floor();
            cell(v.x, v.y + height - h, width, h, S::TopFourth)
        }
        SecondFourth => {
            let h = (height / 4.0).floor();
            cell(v.x, min_y + height - h * 2.0, width, h, S::CenterTopFourth)
        }
        ThirdFourth => {
            let h = (height / 4.0).floor();
            cell(
                v.x,
                min_y + height - h * 3.0,
                width,
                h,
                S::CenterBottomFourth,
            )
        }
        LastFourth => cell(v.x, v.y, width, (height / 4.0).floor(), S::BottomFourth),
        FirstThreeFourths => {
            let h = (height * 3.0 / 4.0).floor();
            cell(v.x, v.y + height - h, width, h, S::TopThreeFourths)
        }
        CenterThreeFourths => cell(
            min_x,
            min_y + (height / 4.0).floor() / 2.0,
            width,
            height / 4.0 * 3.0,
            S::CenterHorizontalThreeFourths,
        ),
        LastThreeFourths => cell(
            v.x,
            v.y,
            width,
            (height * 3.0 / 4.0).floor(),
            S::BottomThreeFourths,
        ),

        // Шестые: 2 столбца × 3 ряда. «Верхняя центральная» — левая в среднем ряду,
        // «нижняя центральная» — правая в среднем ряду.
        TopLeftSixth => {
            let (w, h) = ((width / 2.0).floor(), (height / 3.0).floor());
            cell(v.x, max_y - h, w, h, S::TopLeftSixthPortrait)
        }
        TopCenterSixth => {
            let (w, h) = ((width / 2.0).floor(), (height / 3.0).floor());
            cell(v.x, min_y + h, w, h, S::LeftCenterSixthPortrait)
        }
        TopRightSixth => {
            let (w, h) = ((width / 2.0).floor(), (height / 3.0).floor());
            cell(min_x + w, max_y - h, w, h, S::TopRightSixthPortrait)
        }
        BottomLeftSixth => {
            let (w, h) = ((width / 2.0).floor(), (height / 3.0).floor());
            cell(v.x, v.y, w, h, S::BottomLeftSixthPortrait)
        }
        BottomCenterSixth => {
            let (w, h) = ((width / 2.0).floor(), (height / 3.0).floor());
            cell(v.x + w, v.y + h, w, h, S::RightCenterSixthPortrait)
        }
        BottomRightSixth => {
            let (w, h) = ((width / 2.0).floor(), (height / 3.0).floor());
            cell(v.x + width - w, v.y, w, h, S::BottomRightSixthPortrait)
        }

        TopLeftNinth | TopCenterNinth | TopRightNinth | MiddleLeftNinth | MiddleCenterNinth
        | MiddleRightNinth | BottomLeftNinth | BottomCenterNinth | BottomRightNinth => {
            ninth_rect(action, v)
        }

        // Угловые трети: ½ ширины × ⅔ высоты.
        TopLeftThird => cell(
            min_x,
            max_y - height / 3.0,
            (width / 2.0).floor(),
            (2.0 * height / 3.0).floor(),
            S::TopLeftThird,
        ),
        TopRightThird => cell(
            min_x + width / 2.0,
            max_y - height / 3.0,
            (width / 2.0).floor(),
            (2.0 * height / 3.0).floor(),
            S::TopRightThird,
        ),
        BottomLeftThird => cell(
            min_x,
            min_y,
            (width / 2.0).floor(),
            (2.0 * height / 3.0).floor(),
            S::BottomLeftThird,
        ),
        BottomRightThird => cell(
            min_x + width / 2.0,
            min_y,
            (width / 2.0).floor(),
            (2.0 * height / 3.0).floor(),
            S::BottomRightThird,
        ),

        // Восьмые: 2 столбца × 4 ряда, имена идут по строкам.
        TopLeftEighth
        | TopCenterLeftEighth
        | TopCenterRightEighth
        | TopRightEighth
        | BottomLeftEighth
        | BottomCenterLeftEighth
        | BottomCenterRightEighth
        | BottomRightEighth => {
            let (w, h) = ((width / 2.0).floor(), (height / 4.0).floor());
            let (x, y, sub_action) = match action {
                TopLeftEighth => (min_x, max_y - h, S::TopLeftEighth),
                TopCenterLeftEighth => (min_x + w, max_y - h, S::TopCenterLeftEighth),
                TopCenterRightEighth => (min_x, max_y - h * 2.0, S::TopCenterRightEighth),
                TopRightEighth => (min_x + w, max_y - h * 2.0, S::TopRightEighth),
                BottomLeftEighth => (min_x, max_y - h * 3.0, S::BottomLeftEighth),
                BottomCenterLeftEighth => (min_x + w, max_y - h * 3.0, S::BottomCenterLeftEighth),
                BottomCenterRightEighth => (min_x, min_y, S::BottomCenterRightEighth),
                _ => (min_x + w, min_y, S::BottomRightEighth),
            };
            cell(x, y, w, h, sub_action)
        }

        // Двенадцатые: 3 столбца × 4 ряда, имена идут по строкам.
        TopLeftTwelfth
        | TopCenterLeftTwelfth
        | TopCenterRightTwelfth
        | TopRightTwelfth
        | MiddleLeftTwelfth
        | MiddleCenterLeftTwelfth
        | MiddleCenterRightTwelfth
        | MiddleRightTwelfth
        | BottomLeftTwelfth
        | BottomCenterLeftTwelfth
        | BottomCenterRightTwelfth
        | BottomRightTwelfth => {
            let (w, h) = ((width / 3.0).floor(), (height / 4.0).floor());
            let third = (width / 3.0).floor();
            let (x, y, sub_action) = match action {
                TopLeftTwelfth => (min_x, max_y - h, S::TopLeftTwelfth),
                TopCenterLeftTwelfth => (min_x + w, max_y - h, S::TopCenterLeftTwelfth),
                TopCenterRightTwelfth => (min_x + third * 2.0, max_y - h, S::TopCenterRightTwelfth),
                TopRightTwelfth => (min_x, max_y - 2.0 * h, S::TopRightTwelfth),
                MiddleLeftTwelfth => (min_x + w, max_y - 2.0 * h, S::MiddleLeftTwelfth),
                MiddleCenterLeftTwelfth => {
                    (min_x + 2.0 * w, max_y - 2.0 * h, S::MiddleCenterLeftTwelfth)
                }
                MiddleCenterRightTwelfth => (min_x, max_y - 3.0 * h, S::MiddleCenterRightTwelfth),
                MiddleRightTwelfth => (min_x + w, max_y - 3.0 * h, S::MiddleRightTwelfth),
                BottomLeftTwelfth => (min_x + 2.0 * w, max_y - 3.0 * h, S::BottomLeftTwelfth),
                BottomCenterLeftTwelfth => (min_x, min_y, S::BottomCenterLeftTwelfth),
                BottomCenterRightTwelfth => (min_x + w, min_y, S::BottomCenterRightTwelfth),
                _ => (min_x + third * 2.0, min_y, S::BottomRightTwelfth),
            };
            cell(x, y, w, h, sub_action)
        }

        TopLeftSixteenth
        | TopCenterLeftSixteenth
        | TopCenterRightSixteenth
        | TopRightSixteenth
        | UpperMiddleLeftSixteenth
        | UpperMiddleCenterLeftSixteenth
        | UpperMiddleCenterRightSixteenth
        | UpperMiddleRightSixteenth
        | LowerMiddleLeftSixteenth
        | LowerMiddleCenterLeftSixteenth
        | LowerMiddleCenterRightSixteenth
        | LowerMiddleRightSixteenth
        | BottomLeftSixteenth
        | BottomCenterLeftSixteenth
        | BottomCenterRightSixteenth
        | BottomRightSixteenth => sixteenth_rect(action, v),

        _ => None,
    }
}

/// Девятые: сетка 3 × 3 в обеих ориентациях.
fn ninth_rect(action: Action, v: &Rect) -> Option<RectResult> {
    use Action::*;
    use SubAction as S;
    let (w, h) = ((v.w / 3.0).floor(), (v.h / 3.0).floor());
    let (min_x, min_y, max_y) = (v.min_x(), v.min_y(), v.max_y());
    let right = min_x + v.w - w;
    let (x, y, sub_action) = match action {
        TopLeftNinth => (min_x, max_y - h, S::TopLeftNinth),
        TopCenterNinth => (min_x + w, max_y - h, S::TopCenterNinth),
        TopRightNinth => (right, max_y - h, S::TopRightNinth),
        MiddleLeftNinth => (min_x, min_y + h, S::MiddleLeftNinth),
        MiddleCenterNinth => (min_x + w, min_y + h, S::MiddleCenterNinth),
        MiddleRightNinth => (right, min_y + h, S::MiddleRightNinth),
        BottomLeftNinth => (min_x, min_y, S::BottomLeftNinth),
        BottomCenterNinth => (min_x + w, min_y, S::BottomCenterNinth),
        BottomRightNinth => (right, min_y, S::BottomRightNinth),
        _ => return None,
    };
    cell(x, y, w, h, sub_action)
}

/// Шестнадцатые: сетка 4 × 4 в обеих ориентациях.
fn sixteenth_rect(action: Action, v: &Rect) -> Option<RectResult> {
    use Action::*;
    use SubAction as S;
    let (w, h) = ((v.w / 4.0).floor(), (v.h / 4.0).floor());
    let (min_x, min_y, max_y) = (v.min_x(), v.min_y(), v.max_y());
    let (top, upper, lower) = (max_y - h, max_y - h * 2.0, min_y + h);
    let (x, y, sub_action) = match action {
        TopLeftSixteenth => (min_x, top, S::TopLeftSixteenth),
        TopCenterLeftSixteenth => (min_x + w, top, S::TopCenterLeftSixteenth),
        TopCenterRightSixteenth => (min_x + w * 2.0, top, S::TopCenterRightSixteenth),
        TopRightSixteenth => (min_x + w * 3.0, top, S::TopRightSixteenth),
        UpperMiddleLeftSixteenth => (min_x, upper, S::UpperMiddleLeftSixteenth),
        UpperMiddleCenterLeftSixteenth => (min_x + w, upper, S::UpperMiddleCenterLeftSixteenth),
        UpperMiddleCenterRightSixteenth => {
            (min_x + w * 2.0, upper, S::UpperMiddleCenterRightSixteenth)
        }
        UpperMiddleRightSixteenth => (min_x + w * 3.0, upper, S::UpperMiddleRightSixteenth),
        LowerMiddleLeftSixteenth => (min_x, lower, S::LowerMiddleLeftSixteenth),
        LowerMiddleCenterLeftSixteenth => (min_x + w, lower, S::LowerMiddleCenterLeftSixteenth),
        LowerMiddleCenterRightSixteenth => {
            (min_x + w * 2.0, lower, S::LowerMiddleCenterRightSixteenth)
        }
        LowerMiddleRightSixteenth => (min_x + w * 3.0, lower, S::LowerMiddleRightSixteenth),
        BottomLeftSixteenth => (min_x, min_y, S::BottomLeftSixteenth),
        BottomCenterLeftSixteenth => (min_x + w, min_y, S::BottomCenterLeftSixteenth),
        BottomCenterRightSixteenth => (min_x + w * 2.0, min_y, S::BottomCenterRightSixteenth),
        BottomRightSixteenth => (min_x + w * 3.0, min_y, S::BottomRightSixteenth),
        _ => return None,
    };
    cell(x, y, w, h, sub_action)
}

/// Ячейка по ориентации рабочей области (`orientationBasedRect`).
fn orientation_based_rect(action: Action, v: &Rect) -> Option<RectResult> {
    if v.is_landscape() {
        landscape_rect(action, v)
    } else {
        portrait_rect(action, v)
    }
}

/// Блок из двух шестых, в который перебираются центральные шестые
/// (`TopRightTwoSixthsCalculation` и соседи).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TwoSixths {
    TopRight,
    TopLeft,
    BottomRight,
    BottomLeft,
}

/// `orientationBasedRect` блока из двух шестых.
fn two_sixths_rect(block: TwoSixths, v: &Rect) -> RectResult {
    use SubAction as S;
    let landscape = v.is_landscape();
    let (w, h) = if landscape {
        ((v.w * 2.0 / 3.0).floor(), (v.h / 2.0).floor())
    } else {
        ((v.w / 2.0).floor(), (v.h * 2.0 / 3.0).floor())
    };
    let (x, y, sub_action) = match (block, landscape) {
        (TwoSixths::TopRight, true) => {
            (v.max_x() - w, v.max_y() - h, S::TopRightTwoSixthsLandscape)
        }
        (TwoSixths::TopRight, false) => {
            (v.max_x() - w, v.max_y() - h, S::TopRightTwoSixthsPortrait)
        }
        (TwoSixths::TopLeft, true) => (v.x, v.max_y() - h, S::TopLeftTwoSixthsLandscape),
        (TwoSixths::TopLeft, false) => (v.x, v.max_y() - h, S::TopLeftTwoSixthsPortrait),
        (TwoSixths::BottomRight, true) => (v.max_x() - w, v.y, S::BottomRightTwoSixthsLandscape),
        (TwoSixths::BottomRight, false) => (v.max_x() - w, v.y, S::BottomRightTwoSixthsPortrait),
        (TwoSixths::BottomLeft, true) => (v.x, v.y, S::BottomLeftTwoSixthsLandscape),
        (TwoSixths::BottomLeft, false) => (v.x, v.y, S::BottomLeftTwoSixthsPortrait),
    };
    RectResult::with_sub(Rect::new(x, y, w, h), sub_action)
}

/// Вертикальные трети — без учёта ориентации (`calculateTopThird` и соседи).
fn vertical_third_rect(action: Action, v: &Rect) -> Option<RectResult> {
    use Action::*;
    use SubAction as S;
    match action {
        TopVerticalThird => {
            let h = (v.h / 3.0).floor();
            cell(v.x, v.min_y() + v.h - h, v.w, h, S::TopThird)
        }
        MiddleVerticalThird => cell(
            v.min_x(),
            v.min_y() + (v.h / 3.0).floor(),
            v.w,
            v.h / 3.0,
            S::CenterVerticalThird,
        ),
        BottomVerticalThird => cell(v.x, v.min_y(), v.w, (v.h / 3.0).floor(), S::BottomThird),
        TopVerticalTwoThirds => {
            let h = (v.h * 2.0 / 3.0).floor();
            cell(v.x, v.y + v.h - h, v.w, h, S::TopTwoThirds)
        }
        BottomVerticalTwoThirds => cell(
            v.x,
            v.min_y(),
            v.w,
            (v.h * 2.0 / 3.0).floor(),
            S::BottomTwoThirds,
        ),
        _ => None,
    }
}

// ---------------------------------------------------------------- повторные нажатия

/// История для перебора: режим повторов не «ничего», и у прошлого нажатия есть
/// под-действие (общий `guard` расчётов с перебором).
fn history<'a>(p: &RectParams<'a>) -> Option<(&'a LastAction, SubAction)> {
    if p.config.subsequent_execution_mode == SubsequentExecutionMode::None {
        return None;
    }
    let last = p.last?;
    Some((last, last.sub_action?))
}

/// Прямоугольник сеточного действия с учётом повторных нажатий (`calculateRect`).
/// `action` — действие, чей расчёт вызывают (при переходах оригинал зовёт расчёт
/// соседнего действия с теми же параметрами).
pub(super) fn calculate_rect(p: &RectParams, action: Action) -> Option<RectResult> {
    use Action::*;
    let v = &p.visible;
    match action {
        FirstThird => first_third(p),
        LastThird => last_third(p),
        FirstTwoThirds => first_two_thirds(p),
        LastTwoThirds => last_two_thirds(p),
        CenterThird | CenterTwoThirds | CenterThreeFourths => orientation_based_rect(action, v),
        FirstFourth => first_fourth(p),
        SecondFourth => second_fourth(p),
        ThirdFourth => third_fourth(p),
        LastFourth => last_fourth(p),
        FirstThreeFourths => first_three_fourths(p),
        LastThreeFourths => last_three_fourths(p),
        TopVerticalThird => top_vertical_third(p),
        MiddleVerticalThird => vertical_third_rect(MiddleVerticalThird, v),
        BottomVerticalThird => bottom_vertical_third(p),
        TopVerticalTwoThirds => top_vertical_two_thirds(p),
        BottomVerticalTwoThirds => bottom_vertical_two_thirds(p),
        TopLeftSixth | BottomLeftSixth => corner_sixth(p, action, Direction::Right),
        TopRightSixth | BottomRightSixth => corner_sixth(p, action, Direction::Left),
        TopCenterSixth => top_center_sixth(p),
        BottomCenterSixth => bottom_center_sixth(p),
        _ => {
            let family = family_of(action)?;
            walk(p, action, family)
        }
    }
}

/// `FirstThirdCalculation.calculateRect`.
fn first_third(p: &RectParams) -> Option<RectResult> {
    use SubAction::*;
    let Some((last, sub_action)) = history(p) else {
        return orientation_based_rect(Action::FirstThird, &p.visible);
    };
    match (last.action, sub_action) {
        (Action::FirstThird, TopThird | LeftThird) | (Action::LastThird, TopThird | LeftThird) => {
            orientation_based_rect(Action::CenterThird, &p.visible)
        }
        (Action::FirstThird, CenterHorizontalThird | CenterVerticalThird) => last_third(p),
        _ => orientation_based_rect(Action::FirstThird, &p.visible),
    }
}

/// `LastThirdCalculation.calculateRect`.
fn last_third(p: &RectParams) -> Option<RectResult> {
    use SubAction::*;
    let Some((last, sub_action)) = history(p) else {
        return orientation_based_rect(Action::LastThird, &p.visible);
    };
    match (last.action, sub_action) {
        (Action::LastThird, BottomThird | RightThird)
        | (Action::FirstThird, BottomThird | RightThird) => {
            orientation_based_rect(Action::CenterThird, &p.visible)
        }
        (Action::LastThird, CenterHorizontalThird | CenterVerticalThird) => first_third(p),
        _ => orientation_based_rect(Action::LastThird, &p.visible),
    }
}

/// `FirstTwoThirdsCalculation.calculateRect`: смотрит только на под-действие.
fn first_two_thirds(p: &RectParams) -> Option<RectResult> {
    match history(p) {
        Some((_, SubAction::LeftTwoThirds | SubAction::TopTwoThirds)) => {
            orientation_based_rect(Action::LastTwoThirds, &p.visible)
        }
        _ => orientation_based_rect(Action::FirstTwoThirds, &p.visible),
    }
}

/// `LastTwoThirdsCalculation.calculateRect`.
fn last_two_thirds(p: &RectParams) -> Option<RectResult> {
    match history(p) {
        Some((_, SubAction::RightTwoThirds | SubAction::BottomTwoThirds)) => {
            orientation_based_rect(Action::FirstTwoThirds, &p.visible)
        }
        _ => orientation_based_rect(Action::LastTwoThirds, &p.visible),
    }
}

/// `FirstFourthCalculation.calculateRect`: перебор — только когда нажата сама первая
/// четверть (к ней переходят и из последней).
fn first_fourth(p: &RectParams) -> Option<RectResult> {
    use SubAction::*;
    let own = || orientation_based_rect(Action::FirstFourth, &p.visible);
    let Some((last, sub_action)) = history(p) else {
        return own();
    };
    if p.action != Action::FirstFourth {
        return own();
    }
    match (last.action, sub_action) {
        (Action::FirstFourth, TopFourth | LeftFourth)
        | (Action::LastFourth, LeftFourth | TopFourth) => second_fourth(p),
        (Action::FirstFourth, CenterTopFourth | CenterLeftFourth) => third_fourth(p),
        (Action::FirstFourth, CenterBottomFourth | CenterRightFourth) => last_fourth(p),
        _ => own(),
    }
}

/// `SecondFourthCalculation.calculateRect`: вторая четверть → три четверти справа →
/// центр-половина.
fn second_fourth(p: &RectParams) -> Option<RectResult> {
    use SubAction::*;
    let v = &p.visible;
    match history(p) {
        Some((last, sub_action)) if last.action == Action::SecondFourth => match sub_action {
            CenterLeftFourth => landscape_rect(Action::LastThreeFourths, v),
            CenterTopFourth => portrait_rect(Action::LastThreeFourths, v),
            RightThreeFourths => Some(center_half_landscape(v, 0.5)),
            BottomThreeFourths => Some(center_half_portrait(v, 0.5)),
            _ => orientation_based_rect(Action::SecondFourth, v),
        },
        _ => orientation_based_rect(Action::SecondFourth, v),
    }
}

/// `ThirdFourthCalculation.calculateRect`: третья четверть → три четверти слева →
/// центр-половина.
fn third_fourth(p: &RectParams) -> Option<RectResult> {
    use SubAction::*;
    let v = &p.visible;
    match history(p) {
        Some((last, sub_action)) if last.action == Action::ThirdFourth => match sub_action {
            CenterRightFourth => landscape_rect(Action::FirstThreeFourths, v),
            CenterBottomFourth => portrait_rect(Action::FirstThreeFourths, v),
            LeftThreeFourths => Some(center_half_landscape(v, 0.5)),
            TopThreeFourths => Some(center_half_portrait(v, 0.5)),
            _ => orientation_based_rect(Action::ThirdFourth, v),
        },
        _ => orientation_based_rect(Action::ThirdFourth, v),
    }
}

/// `LastFourthCalculation.calculateRect`.
fn last_fourth(p: &RectParams) -> Option<RectResult> {
    use SubAction::*;
    let Some((last, sub_action)) = history(p) else {
        return orientation_based_rect(Action::LastFourth, &p.visible);
    };
    match (last.action, sub_action) {
        (Action::LastFourth, BottomFourth | RightFourth)
        | (Action::FirstFourth, BottomFourth | RightFourth) => third_fourth(p),
        (Action::LastFourth, CenterBottomFourth | CenterRightFourth) => second_fourth(p),
        (Action::LastFourth, CenterTopFourth | CenterLeftFourth) => first_fourth(p),
        _ => orientation_based_rect(Action::LastFourth, &p.visible),
    }
}

/// `FirstThreeFourthsCalculation.calculateRect`: смотрит только на под-действие.
fn first_three_fourths(p: &RectParams) -> Option<RectResult> {
    match history(p) {
        Some((_, SubAction::LeftThreeFourths | SubAction::TopThreeFourths)) => {
            orientation_based_rect(Action::LastThreeFourths, &p.visible)
        }
        _ => orientation_based_rect(Action::FirstThreeFourths, &p.visible),
    }
}

/// `LastThreeFourthsCalculation.calculateRect`.
fn last_three_fourths(p: &RectParams) -> Option<RectResult> {
    match history(p) {
        Some((_, SubAction::RightThreeFourths | SubAction::BottomThreeFourths)) => {
            orientation_based_rect(Action::FirstThreeFourths, &p.visible)
        }
        _ => orientation_based_rect(Action::LastThreeFourths, &p.visible),
    }
}

/// `TopVerticalThirdCalculation.calculateRect`.
fn top_vertical_third(p: &RectParams) -> Option<RectResult> {
    use SubAction::*;
    let v = &p.visible;
    let Some((last, sub_action)) = history(p) else {
        return vertical_third_rect(Action::TopVerticalThird, v);
    };
    match (last.action, sub_action) {
        (Action::TopVerticalThird, TopThird) | (Action::BottomVerticalThird, TopThird) => {
            vertical_third_rect(Action::MiddleVerticalThird, v)
        }
        (Action::TopVerticalThird, CenterVerticalThird) => bottom_vertical_third(p),
        _ => vertical_third_rect(Action::TopVerticalThird, v),
    }
}

/// `BottomVerticalThirdCalculation.calculateRect`.
fn bottom_vertical_third(p: &RectParams) -> Option<RectResult> {
    use SubAction::*;
    let v = &p.visible;
    let Some((last, sub_action)) = history(p) else {
        return vertical_third_rect(Action::BottomVerticalThird, v);
    };
    match (last.action, sub_action) {
        (Action::BottomVerticalThird, CenterVerticalThird) => top_vertical_third(p),
        (Action::BottomVerticalThird, BottomThird) | (Action::TopVerticalThird, BottomThird) => {
            vertical_third_rect(Action::MiddleVerticalThird, v)
        }
        _ => vertical_third_rect(Action::BottomVerticalThird, v),
    }
}

/// `TopVerticalTwoThirdsCalculation.calculateRect`: смотрит только на под-действие.
fn top_vertical_two_thirds(p: &RectParams) -> Option<RectResult> {
    match history(p) {
        Some((_, SubAction::TopTwoThirds)) => bottom_vertical_two_thirds(p),
        _ => vertical_third_rect(Action::TopVerticalTwoThirds, &p.visible),
    }
}

/// `BottomVerticalTwoThirdsCalculation.calculateRect`.
fn bottom_vertical_two_thirds(p: &RectParams) -> Option<RectResult> {
    match history(p) {
        Some((_, SubAction::BottomTwoThirds)) => top_vertical_two_thirds(p),
        _ => vertical_third_rect(Action::BottomVerticalTwoThirds, &p.visible),
    }
}

// ---------------------------------------------------------------- шестые

/// Направление перебора (`Direction`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Direction {
    Left,
    Right,
}

/// Угловые шестые (`TopLeftSixthCalculation` и соседи): левые перебирают вперёд, правые
/// назад. Перебор продолжается и после другого действия, если окно стоит в этой ячейке.
fn corner_sixth(p: &RectParams, action: Action, direction: Direction) -> Option<RectResult> {
    use SubAction::*;
    let own = || orientation_based_rect(action, &p.visible);
    let Some((last, sub_action)) = history(p) else {
        return own();
    };
    let (landscape, portrait) = match action {
        Action::TopLeftSixth => (TopLeftSixthLandscape, TopLeftSixthPortrait),
        Action::TopRightSixth => (TopRightSixthLandscape, TopRightSixthPortrait),
        Action::BottomLeftSixth => (BottomLeftSixthLandscape, BottomLeftSixthPortrait),
        _ => (BottomRightSixthLandscape, BottomRightSixthPortrait),
    };
    if last.action != action && sub_action != landscape && sub_action != portrait {
        return own();
    }
    match next_sixth(sub_action, direction) {
        Some(next) => orientation_based_rect(next, &p.visible),
        None => own(),
    }
}

/// Следующая шестая (`SixthsRepeated.nextCalculation`).
fn next_sixth(sub_action: SubAction, direction: Direction) -> Option<Action> {
    use Action::*;
    use SubAction as S;
    let next = match (direction, sub_action) {
        (Direction::Left, S::TopLeftSixthLandscape) => BottomRightSixth,
        (Direction::Left, S::TopCenterSixthLandscape) => TopLeftSixth,
        (Direction::Left, S::TopRightSixthLandscape) => TopCenterSixth,
        (Direction::Left, S::BottomLeftSixthLandscape) => TopRightSixth,
        (Direction::Left, S::BottomCenterSixthLandscape) => BottomLeftSixth,
        (Direction::Left, S::BottomRightSixthLandscape) => BottomCenterSixth,
        (Direction::Left, S::TopLeftSixthPortrait) => BottomRightSixth,
        (Direction::Left, S::TopRightSixthPortrait) => TopLeftSixth,
        (Direction::Left, S::LeftCenterSixthPortrait) => TopRightSixth,
        (Direction::Left, S::RightCenterSixthPortrait) => TopCenterSixth,
        (Direction::Left, S::BottomLeftSixthPortrait) => BottomCenterSixth,
        (Direction::Left, S::BottomRightSixthPortrait) => BottomLeftSixth,
        (Direction::Right, S::TopLeftSixthLandscape) => TopCenterSixth,
        (Direction::Right, S::TopCenterSixthLandscape) => TopRightSixth,
        (Direction::Right, S::TopRightSixthLandscape) => BottomLeftSixth,
        (Direction::Right, S::BottomLeftSixthLandscape) => BottomCenterSixth,
        (Direction::Right, S::BottomCenterSixthLandscape) => BottomRightSixth,
        (Direction::Right, S::BottomRightSixthLandscape) => TopLeftSixth,
        (Direction::Right, S::TopLeftSixthPortrait) => TopRightSixth,
        (Direction::Right, S::TopRightSixthPortrait) => TopCenterSixth,
        (Direction::Right, S::LeftCenterSixthPortrait) => BottomCenterSixth,
        (Direction::Right, S::RightCenterSixthPortrait) => BottomLeftSixth,
        (Direction::Right, S::BottomLeftSixthPortrait) => BottomRightSixth,
        (Direction::Right, S::BottomRightSixthPortrait) => TopLeftSixth,
        _ => return None,
    };
    Some(next)
}

/// `TopCenterSixthCalculation.calculateRect`: шестая → две шестых справа → две слева.
fn top_center_sixth(p: &RectParams) -> Option<RectResult> {
    use SubAction::*;
    let v = &p.visible;
    let block = match history(p) {
        Some((_, sub_action)) if p.action == Action::TopCenterSixth => match sub_action {
            TopCenterSixthLandscape => Some(TwoSixths::TopRight),
            LeftCenterSixthPortrait => Some(TwoSixths::BottomLeft),
            TopRightTwoSixthsLandscape | BottomLeftTwoSixthsPortrait => Some(TwoSixths::TopLeft),
            _ => None,
        },
        _ => None,
    };
    match block {
        Some(block) => Some(two_sixths_rect(block, v)),
        None => orientation_based_rect(Action::TopCenterSixth, v),
    }
}

/// `BottomCenterSixthCalculation.calculateRect`.
fn bottom_center_sixth(p: &RectParams) -> Option<RectResult> {
    use SubAction::*;
    let v = &p.visible;
    let block = match history(p) {
        Some((_, sub_action)) if p.action == Action::BottomCenterSixth => match sub_action {
            BottomCenterSixthLandscape | RightCenterSixthPortrait => Some(TwoSixths::BottomRight),
            BottomRightTwoSixthsLandscape => Some(TwoSixths::BottomLeft),
            BottomRightTwoSixthsPortrait => Some(TwoSixths::TopRight),
            _ => None,
        },
        _ => None,
    };
    match block {
        Some(block) => Some(two_sixths_rect(block, v)),
        None => orientation_based_rect(Action::BottomCenterSixth, v),
    }
}

// ---------------------------------------------------------------- восьмые и прочие сетки

/// Семейство ячеек в порядке перебора: под-действие ячейки и действие, которое её
/// ставит. Перебор идёт вперёд (`nextCalculation(direction: .right)`), по кругу.
type Family = &'static [(SubAction, Action)];

const CORNER_THIRDS: Family = &[
    (SubAction::TopLeftThird, Action::TopLeftThird),
    (SubAction::TopRightThird, Action::TopRightThird),
    (SubAction::BottomLeftThird, Action::BottomLeftThird),
    (SubAction::BottomRightThird, Action::BottomRightThird),
];

const EIGHTHS: Family = &[
    (SubAction::TopLeftEighth, Action::TopLeftEighth),
    (SubAction::TopCenterLeftEighth, Action::TopCenterLeftEighth),
    (
        SubAction::TopCenterRightEighth,
        Action::TopCenterRightEighth,
    ),
    (SubAction::TopRightEighth, Action::TopRightEighth),
    (SubAction::BottomLeftEighth, Action::BottomLeftEighth),
    (
        SubAction::BottomCenterLeftEighth,
        Action::BottomCenterLeftEighth,
    ),
    (
        SubAction::BottomCenterRightEighth,
        Action::BottomCenterRightEighth,
    ),
    (SubAction::BottomRightEighth, Action::BottomRightEighth),
];

const NINTHS: Family = &[
    (SubAction::TopLeftNinth, Action::TopLeftNinth),
    (SubAction::TopCenterNinth, Action::TopCenterNinth),
    (SubAction::TopRightNinth, Action::TopRightNinth),
    (SubAction::MiddleLeftNinth, Action::MiddleLeftNinth),
    (SubAction::MiddleCenterNinth, Action::MiddleCenterNinth),
    (SubAction::MiddleRightNinth, Action::MiddleRightNinth),
    (SubAction::BottomLeftNinth, Action::BottomLeftNinth),
    (SubAction::BottomCenterNinth, Action::BottomCenterNinth),
    (SubAction::BottomRightNinth, Action::BottomRightNinth),
];

const TWELFTHS: Family = &[
    (SubAction::TopLeftTwelfth, Action::TopLeftTwelfth),
    (
        SubAction::TopCenterLeftTwelfth,
        Action::TopCenterLeftTwelfth,
    ),
    (
        SubAction::TopCenterRightTwelfth,
        Action::TopCenterRightTwelfth,
    ),
    (SubAction::TopRightTwelfth, Action::TopRightTwelfth),
    (SubAction::MiddleLeftTwelfth, Action::MiddleLeftTwelfth),
    (
        SubAction::MiddleCenterLeftTwelfth,
        Action::MiddleCenterLeftTwelfth,
    ),
    (
        SubAction::MiddleCenterRightTwelfth,
        Action::MiddleCenterRightTwelfth,
    ),
    (SubAction::MiddleRightTwelfth, Action::MiddleRightTwelfth),
    (SubAction::BottomLeftTwelfth, Action::BottomLeftTwelfth),
    (
        SubAction::BottomCenterLeftTwelfth,
        Action::BottomCenterLeftTwelfth,
    ),
    (
        SubAction::BottomCenterRightTwelfth,
        Action::BottomCenterRightTwelfth,
    ),
    (SubAction::BottomRightTwelfth, Action::BottomRightTwelfth),
];

const SIXTEENTHS: Family = &[
    (SubAction::TopLeftSixteenth, Action::TopLeftSixteenth),
    (
        SubAction::TopCenterLeftSixteenth,
        Action::TopCenterLeftSixteenth,
    ),
    (
        SubAction::TopCenterRightSixteenth,
        Action::TopCenterRightSixteenth,
    ),
    (SubAction::TopRightSixteenth, Action::TopRightSixteenth),
    (
        SubAction::UpperMiddleLeftSixteenth,
        Action::UpperMiddleLeftSixteenth,
    ),
    (
        SubAction::UpperMiddleCenterLeftSixteenth,
        Action::UpperMiddleCenterLeftSixteenth,
    ),
    (
        SubAction::UpperMiddleCenterRightSixteenth,
        Action::UpperMiddleCenterRightSixteenth,
    ),
    (
        SubAction::UpperMiddleRightSixteenth,
        Action::UpperMiddleRightSixteenth,
    ),
    (
        SubAction::LowerMiddleLeftSixteenth,
        Action::LowerMiddleLeftSixteenth,
    ),
    (
        SubAction::LowerMiddleCenterLeftSixteenth,
        Action::LowerMiddleCenterLeftSixteenth,
    ),
    (
        SubAction::LowerMiddleCenterRightSixteenth,
        Action::LowerMiddleCenterRightSixteenth,
    ),
    (
        SubAction::LowerMiddleRightSixteenth,
        Action::LowerMiddleRightSixteenth,
    ),
    (SubAction::BottomLeftSixteenth, Action::BottomLeftSixteenth),
    (
        SubAction::BottomCenterLeftSixteenth,
        Action::BottomCenterLeftSixteenth,
    ),
    (
        SubAction::BottomCenterRightSixteenth,
        Action::BottomCenterRightSixteenth,
    ),
    (
        SubAction::BottomRightSixteenth,
        Action::BottomRightSixteenth,
    ),
];

fn family_of(action: Action) -> Option<Family> {
    [CORNER_THIRDS, EIGHTHS, NINTHS, TWELFTHS, SIXTEENTHS]
        .into_iter()
        .find(|family| family.iter().any(|(_, cell_action)| *cell_action == action))
}

/// Угловые трети, восьмые, девятые, двенадцатые, шестнадцатые (`TopLeftEighthCalculation`
/// и соседи): повтор того же действия ставит окно в следующую ячейку по кругу.
fn walk(p: &RectParams, action: Action, family: Family) -> Option<RectResult> {
    let own = || orientation_based_rect(action, &p.visible);
    let Some((last, sub_action)) = history(p) else {
        return own();
    };
    if last.action != action {
        return own();
    }
    match family
        .iter()
        .position(|(cell_sub, _)| *cell_sub == sub_action)
    {
        Some(index) => orientation_based_rect(family[(index + 1) % family.len()].1, &p.visible),
        None => own(),
    }
}
