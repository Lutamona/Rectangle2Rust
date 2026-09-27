//! Действия (WindowAction) и их метаданные — порт `WindowAction.swift`,
//! `WindowActionCategory.swift` и `ColumnLayout.swift` из Rectangle 2.
//!
//! Таблицы свойств повторяют switch-и оригинала один к одному. Где Swift читает
//! `Defaults`, значение настройки передаётся параметром.

use crate::geometry::{Edge, Rect};

/// Раскладки «столбики», доступные пользователю. Порядок = порядок подменю в меню.
pub const SUPPORTED_COLUMN_COUNTS: [u8; 4] = [5, 6, 7, 8];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Action {
    LeftHalf,
    RightHalf,
    TopHalf,
    BottomHalf,
    CenterHalf,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
    FirstThird,
    CenterThird,
    LastThird,
    FirstTwoThirds,
    CenterTwoThirds,
    LastTwoThirds,
    FirstFourth,
    SecondFourth,
    ThirdFourth,
    LastFourth,
    FirstThreeFourths,
    CenterThreeFourths,
    LastThreeFourths,
    TopLeftSixth,
    TopCenterSixth,
    TopRightSixth,
    BottomLeftSixth,
    BottomCenterSixth,
    BottomRightSixth,
    TopLeftNinth,
    TopCenterNinth,
    TopRightNinth,
    MiddleLeftNinth,
    MiddleCenterNinth,
    MiddleRightNinth,
    BottomLeftNinth,
    BottomCenterNinth,
    BottomRightNinth,
    TopLeftThird,
    TopRightThird,
    BottomLeftThird,
    BottomRightThird,
    TopLeftEighth,
    TopCenterLeftEighth,
    TopCenterRightEighth,
    TopRightEighth,
    BottomLeftEighth,
    BottomCenterLeftEighth,
    BottomCenterRightEighth,
    BottomRightEighth,
    TopVerticalThird,
    MiddleVerticalThird,
    BottomVerticalThird,
    TopVerticalTwoThirds,
    BottomVerticalTwoThirds,
    TopLeftTwelfth,
    TopCenterLeftTwelfth,
    TopCenterRightTwelfth,
    TopRightTwelfth,
    MiddleLeftTwelfth,
    MiddleCenterLeftTwelfth,
    MiddleCenterRightTwelfth,
    MiddleRightTwelfth,
    BottomLeftTwelfth,
    BottomCenterLeftTwelfth,
    BottomCenterRightTwelfth,
    BottomRightTwelfth,
    TopLeftSixteenth,
    TopCenterLeftSixteenth,
    TopCenterRightSixteenth,
    TopRightSixteenth,
    UpperMiddleLeftSixteenth,
    UpperMiddleCenterLeftSixteenth,
    UpperMiddleCenterRightSixteenth,
    UpperMiddleRightSixteenth,
    LowerMiddleLeftSixteenth,
    LowerMiddleCenterLeftSixteenth,
    LowerMiddleCenterRightSixteenth,
    LowerMiddleRightSixteenth,
    BottomLeftSixteenth,
    BottomCenterLeftSixteenth,
    BottomCenterRightSixteenth,
    BottomRightSixteenth,
    Maximize,
    MaximizeHeight,
    AlmostMaximize,
    Center,
    CenterProminently,
    Specified,
    /// Действия над всеми окнами: метаданные есть, расчёта нет (`calc::calculate`
    /// возвращает `None`) — в оригинале их выполняет не расчёт, а `MultiWindowManager`.
    ReverseAll,
    MoveLeft,
    MoveRight,
    MoveUp,
    MoveDown,
    NextDisplay,
    PreviousDisplay,
    /// Конкретный дисплей по счёту (1-based).
    Display(u8),
    Larger,
    Smaller,
    LargerWidth,
    SmallerWidth,
    LargerHeight,
    SmallerHeight,
    HalveHeightUp,
    HalveHeightDown,
    HalveWidthLeft,
    HalveWidthRight,
    DoubleHeightUp,
    DoubleHeightDown,
    DoubleWidthLeft,
    DoubleWidthRight,
    TileAll,
    CascadeAll,
    LeftTodo,
    RightTodo,
    CascadeActiveApp,
    TileActiveApp,
    /// Столбик: сколько столбиков в раскладке и какой по счёту (1-based).
    Column {
        count: u8,
        index: u8,
    },
    Restore,
}

/// `WindowAction.active`: все действия, порядок важен — это порядок меню.
static ACTIVE: [Action; 151] = {
    use Action::*;
    [
        LeftHalf,
        RightHalf,
        CenterHalf,
        TopHalf,
        BottomHalf,
        TopLeft,
        TopRight,
        BottomLeft,
        BottomRight,
        FirstThird,
        CenterThird,
        LastThird,
        FirstTwoThirds,
        CenterTwoThirds,
        LastTwoThirds,
        TopVerticalThird,
        MiddleVerticalThird,
        BottomVerticalThird,
        TopVerticalTwoThirds,
        BottomVerticalTwoThirds,
        Maximize,
        AlmostMaximize,
        MaximizeHeight,
        Larger,
        Smaller,
        LargerWidth,
        SmallerWidth,
        LargerHeight,
        SmallerHeight,
        Center,
        CenterProminently,
        Restore,
        NextDisplay,
        PreviousDisplay,
        MoveLeft,
        MoveRight,
        MoveUp,
        MoveDown,
        FirstFourth,
        SecondFourth,
        ThirdFourth,
        LastFourth,
        FirstThreeFourths,
        CenterThreeFourths,
        LastThreeFourths,
        TopLeftSixth,
        TopCenterSixth,
        TopRightSixth,
        BottomLeftSixth,
        BottomCenterSixth,
        BottomRightSixth,
        Specified,
        ReverseAll,
        TopLeftThird,
        TopRightThird,
        BottomLeftThird,
        BottomRightThird,
        TopLeftEighth,
        TopCenterLeftEighth,
        TopCenterRightEighth,
        TopRightEighth,
        BottomLeftEighth,
        BottomCenterLeftEighth,
        BottomCenterRightEighth,
        BottomRightEighth,
        TopLeftNinth,
        TopCenterNinth,
        TopRightNinth,
        MiddleLeftNinth,
        MiddleCenterNinth,
        MiddleRightNinth,
        BottomLeftNinth,
        BottomCenterNinth,
        BottomRightNinth,
        TopLeftTwelfth,
        TopCenterLeftTwelfth,
        TopCenterRightTwelfth,
        TopRightTwelfth,
        MiddleLeftTwelfth,
        MiddleCenterLeftTwelfth,
        MiddleCenterRightTwelfth,
        MiddleRightTwelfth,
        BottomLeftTwelfth,
        BottomCenterLeftTwelfth,
        BottomCenterRightTwelfth,
        BottomRightTwelfth,
        TopLeftSixteenth,
        TopCenterLeftSixteenth,
        TopCenterRightSixteenth,
        TopRightSixteenth,
        UpperMiddleLeftSixteenth,
        UpperMiddleCenterLeftSixteenth,
        UpperMiddleCenterRightSixteenth,
        UpperMiddleRightSixteenth,
        LowerMiddleLeftSixteenth,
        LowerMiddleCenterLeftSixteenth,
        LowerMiddleCenterRightSixteenth,
        LowerMiddleRightSixteenth,
        BottomLeftSixteenth,
        BottomCenterLeftSixteenth,
        BottomCenterRightSixteenth,
        BottomRightSixteenth,
        DoubleHeightUp,
        DoubleHeightDown,
        DoubleWidthLeft,
        DoubleWidthRight,
        HalveHeightUp,
        HalveHeightDown,
        HalveWidthLeft,
        HalveWidthRight,
        TileAll,
        CascadeAll,
        LeftTodo,
        RightTodo,
        CascadeActiveApp,
        TileActiveApp,
        Display(1),
        Display(2),
        Display(3),
        Display(4),
        Display(5),
        Display(6),
        Display(7),
        Display(8),
        Display(9),
        Column { count: 5, index: 1 },
        Column { count: 5, index: 2 },
        Column { count: 5, index: 3 },
        Column { count: 5, index: 4 },
        Column { count: 5, index: 5 },
        Column { count: 6, index: 1 },
        Column { count: 6, index: 2 },
        Column { count: 6, index: 3 },
        Column { count: 6, index: 4 },
        Column { count: 6, index: 5 },
        Column { count: 6, index: 6 },
        Column { count: 7, index: 1 },
        Column { count: 7, index: 2 },
        Column { count: 7, index: 3 },
        Column { count: 7, index: 4 },
        Column { count: 7, index: 5 },
        Column { count: 7, index: 6 },
        Column { count: 7, index: 7 },
        Column { count: 8, index: 1 },
        Column { count: 8, index: 2 },
        Column { count: 8, index: 3 },
        Column { count: 8, index: 4 },
        Column { count: 8, index: 5 },
        Column { count: 8, index: 6 },
        Column { count: 8, index: 7 },
        Column { count: 8, index: 8 },
    ]
};

const DISPLAY_NAMES: [&str; 9] = [
    "displayOne",
    "displayTwo",
    "displayThree",
    "displayFour",
    "displayFive",
    "displaySix",
    "displaySeven",
    "displayEight",
    "displayNine",
];

/// rawValue первого столбика (`columnFive1`); 129 и 130 в Swift пропущены намеренно.
const FIRST_COLUMN_RAW: i32 = 131;

/// Имена столбиков по порядку rawValue: columnFive1 … columnEight8.
const COLUMN_NAMES: [&str; 26] = [
    "columnFive1",
    "columnFive2",
    "columnFive3",
    "columnFive4",
    "columnFive5",
    "columnSix1",
    "columnSix2",
    "columnSix3",
    "columnSix4",
    "columnSix5",
    "columnSix6",
    "columnSeven1",
    "columnSeven2",
    "columnSeven3",
    "columnSeven4",
    "columnSeven5",
    "columnSeven6",
    "columnSeven7",
    "columnEight1",
    "columnEight2",
    "columnEight3",
    "columnEight4",
    "columnEight5",
    "columnEight6",
    "columnEight7",
    "columnEight8",
];

/// Иконки столбиков (PNG в `packaging/icons`, рисует скрипт генерации
/// тем же кодом, что `ColumnIcon` в Swift) — в том же порядке, что `COLUMN_NAMES`.
const COLUMN_IMAGE_NAMES: [&str; 26] = [
    "column-5-1",
    "column-5-2",
    "column-5-3",
    "column-5-4",
    "column-5-5",
    "column-6-1",
    "column-6-2",
    "column-6-3",
    "column-6-4",
    "column-6-5",
    "column-6-6",
    "column-7-1",
    "column-7-2",
    "column-7-3",
    "column-7-4",
    "column-7-5",
    "column-7-6",
    "column-7-7",
    "column-8-1",
    "column-8-2",
    "column-8-3",
    "column-8-4",
    "column-8-5",
    "column-8-6",
    "column-8-7",
    "column-8-8",
];

/// Номер столбика в таблицах выше (и `rawValue - 131`); `None` — такого столбика нет.
fn column_offset(count: u8, index: u8) -> Option<usize> {
    let base = match count {
        5 => 0,
        6 => 5,
        7 => 11,
        8 => 18,
        _ => return None,
    };
    if index >= 1 && index <= count {
        Some(base + index as usize - 1)
    } else {
        None
    }
}

/// `columnDefaultDisplayName`: порядковые числительные расписаны до 8, дальше «Nth».
fn column_default_title(index: u8) -> &'static str {
    match index {
        1 => "1st Column",
        2 => "2nd Column",
        3 => "3rd Column",
        4 => "4th Column",
        5 => "5th Column",
        6 => "6th Column",
        7 => "7th Column",
        _ => "8th Column",
    }
}

/// Подпись столбика в меню — порядковым словом, как «Первая треть».
fn column_title(index: u8) -> &'static str {
    match index {
        1 => "Первый столбик",
        2 => "Второй столбик",
        3 => "Третий столбик",
        4 => "Четвёртый столбик",
        5 => "Пятый столбик",
        6 => "Шестой столбик",
        7 => "Седьмой столбик",
        _ => "Восьмой столбик",
    }
}

/// Под-действие (для повторных нажатий и гэпов) — `SubWindowAction` в Swift.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SubAction {
    LeftThird,
    CenterVerticalThird,
    RightThird,
    LeftTwoThirds,
    RightTwoThirds,

    TopThird,
    CenterHorizontalThird,
    BottomThird,
    TopTwoThirds,
    BottomTwoThirds,

    LeftFourth,
    CenterLeftFourth,
    CenterRightFourth,
    RightFourth,

    TopFourth,
    CenterTopFourth,
    CenterBottomFourth,
    BottomFourth,

    RightThreeFourths,
    BottomThreeFourths,
    LeftThreeFourths,
    TopThreeFourths,
    CenterVerticalThreeFourths,
    CenterHorizontalThreeFourths,

    CenterVerticalHalf,
    CenterHorizontalHalf,

    TopLeftSixthLandscape,
    TopCenterSixthLandscape,
    TopRightSixthLandscape,
    BottomLeftSixthLandscape,
    BottomCenterSixthLandscape,
    BottomRightSixthLandscape,

    TopLeftSixthPortrait,
    TopRightSixthPortrait,
    LeftCenterSixthPortrait,
    RightCenterSixthPortrait,
    BottomLeftSixthPortrait,
    BottomRightSixthPortrait,

    TopLeftTwoSixthsLandscape,
    TopLeftTwoSixthsPortrait,
    TopRightTwoSixthsLandscape,
    TopRightTwoSixthsPortrait,

    BottomLeftTwoSixthsLandscape,
    BottomLeftTwoSixthsPortrait,
    BottomRightTwoSixthsLandscape,
    BottomRightTwoSixthsPortrait,

    TopLeftNinth,
    TopCenterNinth,
    TopRightNinth,
    MiddleLeftNinth,
    MiddleCenterNinth,
    MiddleRightNinth,
    BottomLeftNinth,
    BottomCenterNinth,
    BottomRightNinth,

    TopLeftThird,
    TopRightThird,
    BottomLeftThird,
    BottomRightThird,

    TopLeftQuarter,
    TopRightQuarter,
    BottomLeftQuarter,
    BottomRightQuarter,

    TopLeftEighth,
    TopCenterLeftEighth,
    TopCenterRightEighth,
    TopRightEighth,
    BottomLeftEighth,
    BottomCenterLeftEighth,
    BottomCenterRightEighth,
    BottomRightEighth,

    TopLeftTwelfth,
    TopCenterLeftTwelfth,
    TopCenterRightTwelfth,
    TopRightTwelfth,
    MiddleLeftTwelfth,
    MiddleCenterLeftTwelfth,
    MiddleCenterRightTwelfth,
    MiddleRightTwelfth,
    BottomLeftTwelfth,
    BottomCenterLeftTwelfth,
    BottomCenterRightTwelfth,
    BottomRightTwelfth,

    TopLeftSixteenth,
    TopCenterLeftSixteenth,
    TopCenterRightSixteenth,
    TopRightSixteenth,
    UpperMiddleLeftSixteenth,
    UpperMiddleCenterLeftSixteenth,
    UpperMiddleCenterRightSixteenth,
    UpperMiddleRightSixteenth,
    LowerMiddleLeftSixteenth,
    LowerMiddleCenterLeftSixteenth,
    LowerMiddleCenterRightSixteenth,
    LowerMiddleRightSixteenth,
    BottomLeftSixteenth,
    BottomCenterLeftSixteenth,
    BottomCenterRightSixteenth,
    BottomRightSixteenth,

    Maximize,

    LeftTodo,
    RightTodo,
}

/// Набор краёв в записи Swift: `[.right, .bottom]` → `edges(&[R, B])`.
fn edges(list: &[Edge]) -> Edge {
    list.iter().fold(Edge::NONE, |all, edge| all.with(*edge))
}

const L: Edge = Edge::LEFT;
const R: Edge = Edge::RIGHT;
const T: Edge = Edge::TOP;
const B: Edge = Edge::BOTTOM;

impl SubAction {
    /// Все под-действия в порядке объявления в Swift.
    pub const ALL: [SubAction; 102] = {
        use SubAction::*;
        [
            LeftThird,
            CenterVerticalThird,
            RightThird,
            LeftTwoThirds,
            RightTwoThirds,
            TopThird,
            CenterHorizontalThird,
            BottomThird,
            TopTwoThirds,
            BottomTwoThirds,
            LeftFourth,
            CenterLeftFourth,
            CenterRightFourth,
            RightFourth,
            TopFourth,
            CenterTopFourth,
            CenterBottomFourth,
            BottomFourth,
            RightThreeFourths,
            BottomThreeFourths,
            LeftThreeFourths,
            TopThreeFourths,
            CenterVerticalThreeFourths,
            CenterHorizontalThreeFourths,
            CenterVerticalHalf,
            CenterHorizontalHalf,
            TopLeftSixthLandscape,
            TopCenterSixthLandscape,
            TopRightSixthLandscape,
            BottomLeftSixthLandscape,
            BottomCenterSixthLandscape,
            BottomRightSixthLandscape,
            TopLeftSixthPortrait,
            TopRightSixthPortrait,
            LeftCenterSixthPortrait,
            RightCenterSixthPortrait,
            BottomLeftSixthPortrait,
            BottomRightSixthPortrait,
            TopLeftTwoSixthsLandscape,
            TopLeftTwoSixthsPortrait,
            TopRightTwoSixthsLandscape,
            TopRightTwoSixthsPortrait,
            BottomLeftTwoSixthsLandscape,
            BottomLeftTwoSixthsPortrait,
            BottomRightTwoSixthsLandscape,
            BottomRightTwoSixthsPortrait,
            TopLeftNinth,
            TopCenterNinth,
            TopRightNinth,
            MiddleLeftNinth,
            MiddleCenterNinth,
            MiddleRightNinth,
            BottomLeftNinth,
            BottomCenterNinth,
            BottomRightNinth,
            TopLeftThird,
            TopRightThird,
            BottomLeftThird,
            BottomRightThird,
            TopLeftQuarter,
            TopRightQuarter,
            BottomLeftQuarter,
            BottomRightQuarter,
            TopLeftEighth,
            TopCenterLeftEighth,
            TopCenterRightEighth,
            TopRightEighth,
            BottomLeftEighth,
            BottomCenterLeftEighth,
            BottomCenterRightEighth,
            BottomRightEighth,
            TopLeftTwelfth,
            TopCenterLeftTwelfth,
            TopCenterRightTwelfth,
            TopRightTwelfth,
            MiddleLeftTwelfth,
            MiddleCenterLeftTwelfth,
            MiddleCenterRightTwelfth,
            MiddleRightTwelfth,
            BottomLeftTwelfth,
            BottomCenterLeftTwelfth,
            BottomCenterRightTwelfth,
            BottomRightTwelfth,
            TopLeftSixteenth,
            TopCenterLeftSixteenth,
            TopCenterRightSixteenth,
            TopRightSixteenth,
            UpperMiddleLeftSixteenth,
            UpperMiddleCenterLeftSixteenth,
            UpperMiddleCenterRightSixteenth,
            UpperMiddleRightSixteenth,
            LowerMiddleLeftSixteenth,
            LowerMiddleCenterLeftSixteenth,
            LowerMiddleCenterRightSixteenth,
            LowerMiddleRightSixteenth,
            BottomLeftSixteenth,
            BottomCenterLeftSixteenth,
            BottomCenterRightSixteenth,
            BottomRightSixteenth,
            Maximize,
            LeftTodo,
            RightTodo,
        ]
    };

    /// Имя кейса в Swift (`leftThird`, `topLeftSixthLandscape`, …).
    pub fn name(self) -> &'static str {
        use SubAction::*;
        match self {
            LeftThird => "leftThird",
            CenterVerticalThird => "centerVerticalThird",
            RightThird => "rightThird",
            LeftTwoThirds => "leftTwoThirds",
            RightTwoThirds => "rightTwoThirds",
            TopThird => "topThird",
            CenterHorizontalThird => "centerHorizontalThird",
            BottomThird => "bottomThird",
            TopTwoThirds => "topTwoThirds",
            BottomTwoThirds => "bottomTwoThirds",
            LeftFourth => "leftFourth",
            CenterLeftFourth => "centerLeftFourth",
            CenterRightFourth => "centerRightFourth",
            RightFourth => "rightFourth",
            TopFourth => "topFourth",
            CenterTopFourth => "centerTopFourth",
            CenterBottomFourth => "centerBottomFourth",
            BottomFourth => "bottomFourth",
            RightThreeFourths => "rightThreeFourths",
            BottomThreeFourths => "bottomThreeFourths",
            LeftThreeFourths => "leftThreeFourths",
            TopThreeFourths => "topThreeFourths",
            CenterVerticalThreeFourths => "centerVerticalThreeFourths",
            CenterHorizontalThreeFourths => "centerHorizontalThreeFourths",
            CenterVerticalHalf => "centerVerticalHalf",
            CenterHorizontalHalf => "centerHorizontalHalf",
            TopLeftSixthLandscape => "topLeftSixthLandscape",
            TopCenterSixthLandscape => "topCenterSixthLandscape",
            TopRightSixthLandscape => "topRightSixthLandscape",
            BottomLeftSixthLandscape => "bottomLeftSixthLandscape",
            BottomCenterSixthLandscape => "bottomCenterSixthLandscape",
            BottomRightSixthLandscape => "bottomRightSixthLandscape",
            TopLeftSixthPortrait => "topLeftSixthPortrait",
            TopRightSixthPortrait => "topRightSixthPortrait",
            LeftCenterSixthPortrait => "leftCenterSixthPortrait",
            RightCenterSixthPortrait => "rightCenterSixthPortrait",
            BottomLeftSixthPortrait => "bottomLeftSixthPortrait",
            BottomRightSixthPortrait => "bottomRightSixthPortrait",
            TopLeftTwoSixthsLandscape => "topLeftTwoSixthsLandscape",
            TopLeftTwoSixthsPortrait => "topLeftTwoSixthsPortrait",
            TopRightTwoSixthsLandscape => "topRightTwoSixthsLandscape",
            TopRightTwoSixthsPortrait => "topRightTwoSixthsPortrait",
            BottomLeftTwoSixthsLandscape => "bottomLeftTwoSixthsLandscape",
            BottomLeftTwoSixthsPortrait => "bottomLeftTwoSixthsPortrait",
            BottomRightTwoSixthsLandscape => "bottomRightTwoSixthsLandscape",
            BottomRightTwoSixthsPortrait => "bottomRightTwoSixthsPortrait",
            TopLeftNinth => "topLeftNinth",
            TopCenterNinth => "topCenterNinth",
            TopRightNinth => "topRightNinth",
            MiddleLeftNinth => "middleLeftNinth",
            MiddleCenterNinth => "middleCenterNinth",
            MiddleRightNinth => "middleRightNinth",
            BottomLeftNinth => "bottomLeftNinth",
            BottomCenterNinth => "bottomCenterNinth",
            BottomRightNinth => "bottomRightNinth",
            TopLeftThird => "topLeftThird",
            TopRightThird => "topRightThird",
            BottomLeftThird => "bottomLeftThird",
            BottomRightThird => "bottomRightThird",
            TopLeftQuarter => "topLeftQuarter",
            TopRightQuarter => "topRightQuarter",
            BottomLeftQuarter => "bottomLeftQuarter",
            BottomRightQuarter => "bottomRightQuarter",
            TopLeftEighth => "topLeftEighth",
            TopCenterLeftEighth => "topCenterLeftEighth",
            TopCenterRightEighth => "topCenterRightEighth",
            TopRightEighth => "topRightEighth",
            BottomLeftEighth => "bottomLeftEighth",
            BottomCenterLeftEighth => "bottomCenterLeftEighth",
            BottomCenterRightEighth => "bottomCenterRightEighth",
            BottomRightEighth => "bottomRightEighth",
            TopLeftTwelfth => "topLeftTwelfth",
            TopCenterLeftTwelfth => "topCenterLeftTwelfth",
            TopCenterRightTwelfth => "topCenterRightTwelfth",
            TopRightTwelfth => "topRightTwelfth",
            MiddleLeftTwelfth => "middleLeftTwelfth",
            MiddleCenterLeftTwelfth => "middleCenterLeftTwelfth",
            MiddleCenterRightTwelfth => "middleCenterRightTwelfth",
            MiddleRightTwelfth => "middleRightTwelfth",
            BottomLeftTwelfth => "bottomLeftTwelfth",
            BottomCenterLeftTwelfth => "bottomCenterLeftTwelfth",
            BottomCenterRightTwelfth => "bottomCenterRightTwelfth",
            BottomRightTwelfth => "bottomRightTwelfth",
            TopLeftSixteenth => "topLeftSixteenth",
            TopCenterLeftSixteenth => "topCenterLeftSixteenth",
            TopCenterRightSixteenth => "topCenterRightSixteenth",
            TopRightSixteenth => "topRightSixteenth",
            UpperMiddleLeftSixteenth => "upperMiddleLeftSixteenth",
            UpperMiddleCenterLeftSixteenth => "upperMiddleCenterLeftSixteenth",
            UpperMiddleCenterRightSixteenth => "upperMiddleCenterRightSixteenth",
            UpperMiddleRightSixteenth => "upperMiddleRightSixteenth",
            LowerMiddleLeftSixteenth => "lowerMiddleLeftSixteenth",
            LowerMiddleCenterLeftSixteenth => "lowerMiddleCenterLeftSixteenth",
            LowerMiddleCenterRightSixteenth => "lowerMiddleCenterRightSixteenth",
            LowerMiddleRightSixteenth => "lowerMiddleRightSixteenth",
            BottomLeftSixteenth => "bottomLeftSixteenth",
            BottomCenterLeftSixteenth => "bottomCenterLeftSixteenth",
            BottomCenterRightSixteenth => "bottomCenterRightSixteenth",
            BottomRightSixteenth => "bottomRightSixteenth",
            Maximize => "maximize",
            LeftTodo => "leftTodo",
            RightTodo => "rightTodo",
        }
    }

    /// Какие края под-действие делит с соседями (для гэпов) — `gapSharedEdge` в Swift.
    pub fn gap_shared_edge(self) -> Edge {
        use SubAction::*;
        match self {
            LeftThird => R,
            CenterVerticalThird => edges(&[R, L]),
            RightThird => L,
            LeftTwoThirds => R,
            RightTwoThirds => L,
            TopThird => B,
            CenterHorizontalThird => edges(&[T, B]),
            BottomThird => T,
            TopTwoThirds => B,
            BottomTwoThirds => T,
            LeftFourth => R,
            CenterLeftFourth => edges(&[R, L]),
            CenterRightFourth => edges(&[R, L]),
            RightFourth => L,
            TopFourth => B,
            CenterTopFourth => edges(&[T, B]),
            CenterBottomFourth => edges(&[T, B]),
            BottomFourth => T,
            RightThreeFourths => L,
            BottomThreeFourths => T,
            LeftThreeFourths => R,
            TopThreeFourths => B,
            CenterVerticalThreeFourths => edges(&[R, L]),
            CenterHorizontalThreeFourths => edges(&[T, B]),
            CenterVerticalHalf => edges(&[R, L]),
            CenterHorizontalHalf => edges(&[T, B]),
            TopLeftSixthLandscape => edges(&[R, B]),
            TopCenterSixthLandscape => edges(&[R, L, B]),
            TopRightSixthLandscape => edges(&[L, B]),
            BottomLeftSixthLandscape => edges(&[T, R]),
            BottomCenterSixthLandscape => edges(&[L, R, T]),
            BottomRightSixthLandscape => edges(&[L, T]),
            TopLeftSixthPortrait => edges(&[R, B]),
            TopRightSixthPortrait => edges(&[L, B]),
            LeftCenterSixthPortrait => edges(&[T, B, R]),
            RightCenterSixthPortrait => edges(&[L, T, B]),
            BottomLeftSixthPortrait => edges(&[T, R]),
            BottomRightSixthPortrait => edges(&[L, T]),
            TopLeftTwoSixthsLandscape => edges(&[R, B]),
            TopLeftTwoSixthsPortrait => edges(&[R, B]),
            TopRightTwoSixthsLandscape => edges(&[L, B]),
            TopRightTwoSixthsPortrait => edges(&[L, B]),
            BottomLeftTwoSixthsLandscape => edges(&[R, T]),
            BottomLeftTwoSixthsPortrait => edges(&[R, T]),
            BottomRightTwoSixthsLandscape => edges(&[L, T]),
            BottomRightTwoSixthsPortrait => edges(&[L, T]),
            TopLeftNinth => edges(&[R, B]),
            TopCenterNinth => edges(&[R, L, B]),
            TopRightNinth => edges(&[L, B]),
            MiddleLeftNinth => edges(&[T, R, B]),
            MiddleCenterNinth => edges(&[T, R, B, L]),
            MiddleRightNinth => edges(&[L, T, B]),
            BottomLeftNinth => edges(&[T, R]),
            BottomCenterNinth => edges(&[L, T, R]),
            BottomRightNinth => edges(&[L, T]),
            TopLeftThird => edges(&[R, B]),
            TopRightThird => edges(&[L, B]),
            BottomLeftThird => edges(&[R, T]),
            BottomRightThird => edges(&[L, T]),
            TopLeftQuarter => edges(&[R, B]),
            TopRightQuarter => edges(&[L, B]),
            BottomLeftQuarter => edges(&[R, T]),
            BottomRightQuarter => edges(&[L, T]),
            TopLeftEighth => edges(&[R, B]),
            TopCenterLeftEighth => edges(&[R, L, B]),
            TopCenterRightEighth => edges(&[R, L, B]),
            TopRightEighth => edges(&[L, B]),
            BottomLeftEighth => edges(&[R, T]),
            BottomCenterLeftEighth => edges(&[R, L, T]),
            BottomCenterRightEighth => edges(&[R, L, T]),
            BottomRightEighth => edges(&[L, T]),
            TopLeftTwelfth => edges(&[R, B]),
            TopCenterLeftTwelfth => edges(&[R, L, B]),
            TopCenterRightTwelfth => edges(&[R, L, B]),
            TopRightTwelfth => edges(&[L, B]),
            MiddleLeftTwelfth => edges(&[T, R, B]),
            MiddleCenterLeftTwelfth => edges(&[T, R, B, L]),
            MiddleCenterRightTwelfth => edges(&[T, R, B, L]),
            MiddleRightTwelfth => edges(&[L, T, B]),
            BottomLeftTwelfth => edges(&[T, R]),
            BottomCenterLeftTwelfth => edges(&[L, T, R]),
            BottomCenterRightTwelfth => edges(&[L, T, R]),
            BottomRightTwelfth => edges(&[L, T]),
            TopLeftSixteenth => edges(&[R, B]),
            TopCenterLeftSixteenth => edges(&[R, L, B]),
            TopCenterRightSixteenth => edges(&[R, L, B]),
            TopRightSixteenth => edges(&[L, B]),
            UpperMiddleLeftSixteenth => edges(&[T, R, B]),
            UpperMiddleCenterLeftSixteenth => edges(&[T, R, B, L]),
            UpperMiddleCenterRightSixteenth => edges(&[T, R, B, L]),
            UpperMiddleRightSixteenth => edges(&[L, T, B]),
            LowerMiddleLeftSixteenth => edges(&[T, R, B]),
            LowerMiddleCenterLeftSixteenth => edges(&[T, R, B, L]),
            LowerMiddleCenterRightSixteenth => edges(&[T, R, B, L]),
            LowerMiddleRightSixteenth => edges(&[L, T, B]),
            BottomLeftSixteenth => edges(&[T, R]),
            BottomCenterLeftSixteenth => edges(&[L, T, R]),
            BottomCenterRightSixteenth => edges(&[L, T, R]),
            BottomRightSixteenth => edges(&[L, T]),
            Maximize => Edge::NONE,
            LeftTodo => R,
            RightTodo => L,
        }
    }
}

/// Какие оси затрагивает действие (для гэпов).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Dimension(pub u8);

impl Dimension {
    pub const NONE: Dimension = Dimension(0);
    pub const HORIZONTAL: Dimension = Dimension(1);
    pub const VERTICAL: Dimension = Dimension(2);
    pub const BOTH: Dimension = Dimension(3);

    pub fn contains(self, other: Dimension) -> bool {
        self.0 & other.0 == other.0
    }
}

/// Категория действия (подменю и группы) — `WindowActionCategory` в Swift.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WindowActionCategory {
    Halves,
    Corners,
    Thirds,
    Max,
    Size,
    Display,
    Move,
    Other,
    Sixths,
    Fourths,
    Eighths,
    Ninths,
    Twelfths,
    Sixteenths,
    ColumnsFive,
    ColumnsSix,
    ColumnsSeven,
    ColumnsEight,
}

impl WindowActionCategory {
    /// Все категории в порядке объявления в Swift.
    pub const ALL: [WindowActionCategory; 18] = {
        use WindowActionCategory::*;
        [
            Halves,
            Corners,
            Thirds,
            Max,
            Size,
            Display,
            Move,
            Other,
            Sixths,
            Fourths,
            Eighths,
            Ninths,
            Twelfths,
            Sixteenths,
            ColumnsFive,
            ColumnsSix,
            ColumnsSeven,
            ColumnsEight,
        ]
    };

    /// Имя кейса в Swift.
    pub fn name(self) -> &'static str {
        use WindowActionCategory::*;
        match self {
            Halves => "halves",
            Corners => "corners",
            Thirds => "thirds",
            Max => "max",
            Size => "size",
            Display => "display",
            Move => "move",
            Other => "other",
            Sixths => "sixths",
            Fourths => "fourths",
            Eighths => "eighths",
            Ninths => "ninths",
            Twelfths => "twelfths",
            Sixteenths => "sixteenths",
            ColumnsFive => "columnsFive",
            ColumnsSix => "columnsSix",
            ColumnsSeven => "columnsSeven",
            ColumnsEight => "columnsEight",
        }
    }

    /// Порядок подменю в меню; категории без подменю — 99.
    pub fn menu_order(self) -> i32 {
        use WindowActionCategory::*;
        match self {
            Size => 0,
            Move => 1,
            Thirds => 2,
            Fourths => 3,
            Sixths => 4,
            Eighths => 5,
            Ninths => 6,
            Twelfths => 7,
            Sixteenths => 8,
            ColumnsFive => 9,
            ColumnsSix => 10,
            ColumnsSeven => 11,
            ColumnsEight => 12,
            _ => 99,
        }
    }

    /// Заголовок подменю (`displayName` в Swift) — по-русски, в едином стиле подменю.
    pub fn display_name(self) -> &'static str {
        use WindowActionCategory::*;
        match self {
            Halves => "Половины",
            Corners => "Углы",
            Thirds => "Трети",
            Max => "Развернуть",
            Size => "Размер",
            Display => "Экраны",
            Move => "Края",
            Other => "Другое",
            Sixths => "Шестые",
            Fourths => "Четверти",
            Eighths => "Восьмые",
            Ninths => "Девятые",
            Twelfths => "Двенадцатые",
            Sixteenths => "Шестнадцатые",
            ColumnsFive => "Пять столбиков",
            ColumnsSix => "Шесть столбиков",
            ColumnsSeven => "Семь столбиков",
            ColumnsEight => "Восемь столбиков",
        }
    }
}

impl Action {
    /// `WindowAction.active`: все действия в порядке меню.
    pub fn active() -> &'static [Action] {
        &ACTIVE
    }

    /// `rawValue` в Swift: под ним действие хранится в настройках оригинала.
    /// Для несуществующих `Display(n)` / `Column { .. }` — `-1`.
    pub fn raw(self) -> i32 {
        use Action::*;
        match self {
            LeftHalf => 0,
            RightHalf => 1,
            Maximize => 2,
            MaximizeHeight => 3,
            PreviousDisplay => 4,
            NextDisplay => 5,
            Larger => 8,
            Smaller => 9,
            BottomHalf => 10,
            TopHalf => 11,
            Center => 12,
            BottomLeft => 13,
            BottomRight => 14,
            TopLeft => 15,
            TopRight => 16,
            Restore => 19,
            FirstThird => 20,
            FirstTwoThirds => 21,
            CenterThird => 22,
            LastTwoThirds => 23,
            LastThird => 24,
            MoveLeft => 25,
            MoveRight => 26,
            MoveUp => 27,
            MoveDown => 28,
            AlmostMaximize => 29,
            CenterHalf => 30,
            FirstFourth => 31,
            SecondFourth => 32,
            ThirdFourth => 33,
            LastFourth => 34,
            FirstThreeFourths => 35,
            LastThreeFourths => 36,
            TopLeftSixth => 37,
            TopCenterSixth => 38,
            TopRightSixth => 39,
            BottomLeftSixth => 40,
            BottomCenterSixth => 41,
            BottomRightSixth => 42,
            Specified => 43,
            ReverseAll => 44,
            TopLeftNinth => 45,
            TopCenterNinth => 46,
            TopRightNinth => 47,
            MiddleLeftNinth => 48,
            MiddleCenterNinth => 49,
            MiddleRightNinth => 50,
            BottomLeftNinth => 51,
            BottomCenterNinth => 52,
            BottomRightNinth => 53,
            TopLeftThird => 54,
            TopRightThird => 55,
            BottomLeftThird => 56,
            BottomRightThird => 57,
            TopLeftEighth => 58,
            TopCenterLeftEighth => 59,
            TopCenterRightEighth => 60,
            TopRightEighth => 61,
            BottomLeftEighth => 62,
            BottomCenterLeftEighth => 63,
            BottomCenterRightEighth => 64,
            BottomRightEighth => 65,
            TileAll => 66,
            CascadeAll => 67,
            LeftTodo => 68,
            RightTodo => 69,
            CascadeActiveApp => 70,
            CenterProminently => 71,
            DoubleHeightUp => 72,
            DoubleHeightDown => 73,
            DoubleWidthLeft => 74,
            DoubleWidthRight => 75,
            HalveHeightUp => 76,
            HalveHeightDown => 77,
            HalveWidthLeft => 78,
            HalveWidthRight => 79,
            LargerWidth => 80,
            SmallerWidth => 81,
            LargerHeight => 82,
            SmallerHeight => 83,
            CenterTwoThirds => 84,
            CenterThreeFourths => 85,
            TileActiveApp => 86,
            TopVerticalThird => 87,
            MiddleVerticalThird => 88,
            BottomVerticalThird => 89,
            TopVerticalTwoThirds => 90,
            BottomVerticalTwoThirds => 91,
            TopLeftTwelfth => 92,
            TopCenterLeftTwelfth => 93,
            TopCenterRightTwelfth => 94,
            TopRightTwelfth => 95,
            MiddleLeftTwelfth => 96,
            MiddleCenterLeftTwelfth => 97,
            MiddleCenterRightTwelfth => 98,
            MiddleRightTwelfth => 99,
            BottomLeftTwelfth => 100,
            BottomCenterLeftTwelfth => 101,
            BottomCenterRightTwelfth => 102,
            BottomRightTwelfth => 103,
            TopLeftSixteenth => 104,
            TopCenterLeftSixteenth => 105,
            TopCenterRightSixteenth => 106,
            TopRightSixteenth => 107,
            UpperMiddleLeftSixteenth => 108,
            UpperMiddleCenterLeftSixteenth => 109,
            UpperMiddleCenterRightSixteenth => 110,
            UpperMiddleRightSixteenth => 111,
            LowerMiddleLeftSixteenth => 112,
            LowerMiddleCenterLeftSixteenth => 113,
            LowerMiddleCenterRightSixteenth => 114,
            LowerMiddleRightSixteenth => 115,
            BottomLeftSixteenth => 116,
            BottomCenterLeftSixteenth => 117,
            BottomCenterRightSixteenth => 118,
            BottomRightSixteenth => 119,
            // displayOne = 120 … displayNine = 128
            Display(number @ 1..=9) => 119 + number as i32,
            Display(_) => -1,
            Column { count, index } => match column_offset(count, index) {
                Some(offset) => FIRST_COLUMN_RAW + offset as i32,
                None => -1,
            },
        }
    }

    /// Действие по `rawValue` (`WindowAction(rawValue:)`).
    pub fn from_raw(raw: i32) -> Option<Action> {
        ACTIVE.iter().copied().find(|action| action.raw() == raw)
    }

    /// Действие по имени или старому имени (`aliasName`) — как ищет URL-схема
    /// оригинала: первое из `active`, у которого совпало одно из двух.
    pub fn from_name(name: &str) -> Option<Action> {
        ACTIVE
            .iter()
            .copied()
            .find(|action| action.alias_name() == Some(name) || action.static_name() == Some(name))
    }

    pub fn is_column(&self) -> bool {
        matches!(self, Action::Column { .. })
    }

    pub fn column_count(&self) -> Option<u8> {
        match self {
            Action::Column { count, .. } => Some(*count),
            _ => None,
        }
    }

    pub fn column_index(&self) -> Option<u8> {
        match self {
            Action::Column { index, .. } => Some(*index),
            _ => None,
        }
    }

    pub fn column_action(count: u8, index: u8) -> Option<Action> {
        if SUPPORTED_COLUMN_COUNTS.contains(&count) && index >= 1 && index <= count {
            Some(Action::Column { count, index })
        } else {
            None
        }
    }

    /// Все действия раскладки из `count` столбиков.
    pub fn column_cases(count: u8) -> Vec<Action> {
        if !SUPPORTED_COLUMN_COUNTS.contains(&count) {
            return Vec::new();
        }
        (1..=count)
            .map(|index| Action::Column { count, index })
            .collect()
    }

    /// Имя из таблиц Swift; `None` — только у несуществующих `Display(n)` / `Column`.
    fn static_name(self) -> Option<&'static str> {
        use Action::*;
        let name = match self {
            LeftHalf => "leftHalf",
            RightHalf => "rightHalf",
            TopHalf => "topHalf",
            BottomHalf => "bottomHalf",
            CenterHalf => "centerHalf",
            TopLeft => "topLeft",
            TopRight => "topRight",
            BottomLeft => "bottomLeft",
            BottomRight => "bottomRight",
            FirstThird => "firstThird",
            CenterThird => "centerThird",
            LastThird => "lastThird",
            FirstTwoThirds => "firstTwoThirds",
            CenterTwoThirds => "centerTwoThirds",
            LastTwoThirds => "lastTwoThirds",
            FirstFourth => "firstFourth",
            SecondFourth => "secondFourth",
            ThirdFourth => "thirdFourth",
            LastFourth => "lastFourth",
            FirstThreeFourths => "firstThreeFourths",
            CenterThreeFourths => "centerThreeFourths",
            LastThreeFourths => "lastThreeFourths",
            TopLeftSixth => "topLeftSixth",
            TopCenterSixth => "topCenterSixth",
            TopRightSixth => "topRightSixth",
            BottomLeftSixth => "bottomLeftSixth",
            BottomCenterSixth => "bottomCenterSixth",
            BottomRightSixth => "bottomRightSixth",
            TopLeftNinth => "topLeftNinth",
            TopCenterNinth => "topCenterNinth",
            TopRightNinth => "topRightNinth",
            MiddleLeftNinth => "middleLeftNinth",
            MiddleCenterNinth => "middleCenterNinth",
            MiddleRightNinth => "middleRightNinth",
            BottomLeftNinth => "bottomLeftNinth",
            BottomCenterNinth => "bottomCenterNinth",
            BottomRightNinth => "bottomRightNinth",
            TopLeftThird => "topLeftThird",
            TopRightThird => "topRightThird",
            BottomLeftThird => "bottomLeftThird",
            BottomRightThird => "bottomRightThird",
            TopLeftEighth => "topLeftEighth",
            TopCenterLeftEighth => "topCenterLeftEighth",
            TopCenterRightEighth => "topCenterRightEighth",
            TopRightEighth => "topRightEighth",
            BottomLeftEighth => "bottomLeftEighth",
            BottomCenterLeftEighth => "bottomCenterLeftEighth",
            BottomCenterRightEighth => "bottomCenterRightEighth",
            BottomRightEighth => "bottomRightEighth",
            TopVerticalThird => "topVerticalThird",
            MiddleVerticalThird => "middleVerticalThird",
            BottomVerticalThird => "bottomVerticalThird",
            TopVerticalTwoThirds => "topVerticalTwoThirds",
            BottomVerticalTwoThirds => "bottomVerticalTwoThirds",
            TopLeftTwelfth => "topLeftTwelfth",
            TopCenterLeftTwelfth => "topCenterLeftTwelfth",
            TopCenterRightTwelfth => "topCenterRightTwelfth",
            TopRightTwelfth => "topRightTwelfth",
            MiddleLeftTwelfth => "middleLeftTwelfth",
            MiddleCenterLeftTwelfth => "middleCenterLeftTwelfth",
            MiddleCenterRightTwelfth => "middleCenterRightTwelfth",
            MiddleRightTwelfth => "middleRightTwelfth",
            BottomLeftTwelfth => "bottomLeftTwelfth",
            BottomCenterLeftTwelfth => "bottomCenterLeftTwelfth",
            BottomCenterRightTwelfth => "bottomCenterRightTwelfth",
            BottomRightTwelfth => "bottomRightTwelfth",
            TopLeftSixteenth => "topLeftSixteenth",
            TopCenterLeftSixteenth => "topCenterLeftSixteenth",
            TopCenterRightSixteenth => "topCenterRightSixteenth",
            TopRightSixteenth => "topRightSixteenth",
            UpperMiddleLeftSixteenth => "upperMiddleLeftSixteenth",
            UpperMiddleCenterLeftSixteenth => "upperMiddleCenterLeftSixteenth",
            UpperMiddleCenterRightSixteenth => "upperMiddleCenterRightSixteenth",
            UpperMiddleRightSixteenth => "upperMiddleRightSixteenth",
            LowerMiddleLeftSixteenth => "lowerMiddleLeftSixteenth",
            LowerMiddleCenterLeftSixteenth => "lowerMiddleCenterLeftSixteenth",
            LowerMiddleCenterRightSixteenth => "lowerMiddleCenterRightSixteenth",
            LowerMiddleRightSixteenth => "lowerMiddleRightSixteenth",
            BottomLeftSixteenth => "bottomLeftSixteenth",
            BottomCenterLeftSixteenth => "bottomCenterLeftSixteenth",
            BottomCenterRightSixteenth => "bottomCenterRightSixteenth",
            BottomRightSixteenth => "bottomRightSixteenth",
            Maximize => "maximize",
            MaximizeHeight => "maximizeHeight",
            AlmostMaximize => "almostMaximize",
            Center => "center",
            CenterProminently => "centerProminently",
            Specified => "specified",
            ReverseAll => "reverseAll",
            MoveLeft => "moveLeft",
            MoveRight => "moveRight",
            MoveUp => "moveUp",
            MoveDown => "moveDown",
            NextDisplay => "nextDisplay",
            PreviousDisplay => "previousDisplay",
            Display(number @ 1..=9) => DISPLAY_NAMES[number as usize - 1],
            Display(_) => return None,
            Larger => "larger",
            Smaller => "smaller",
            LargerWidth => "largerWidth",
            SmallerWidth => "smallerWidth",
            LargerHeight => "largerHeight",
            SmallerHeight => "smallerHeight",
            HalveHeightUp => "halveHeightUp",
            HalveHeightDown => "halveHeightDown",
            HalveWidthLeft => "halveWidthLeft",
            HalveWidthRight => "halveWidthRight",
            DoubleHeightUp => "doubleHeightUp",
            DoubleHeightDown => "doubleHeightDown",
            DoubleWidthLeft => "doubleWidthLeft",
            DoubleWidthRight => "doubleWidthRight",
            TileAll => "tileAll",
            CascadeAll => "cascadeAll",
            LeftTodo => "leftTodo",
            RightTodo => "rightTodo",
            CascadeActiveApp => "cascadeActiveApp",
            TileActiveApp => "tileActiveApp",
            Column { count, index } => COLUMN_NAMES[column_offset(count, index)?],
            Restore => "restore",
        };
        Some(name)
    }

    /// Имя действия — ключ настроек, конфига и URL (`name` в Swift).
    pub fn name(&self) -> String {
        if let Some(name) = self.static_name() {
            return name.to_string();
        }
        match self {
            Action::Display(number) => format!("display{}", number),
            Action::Column { count, index } => format!("column{}x{}", count, index),
            _ => {
                unreachable!("имя есть у всех действий, кроме несуществующих дисплеев и столбиков")
            }
        }
    }

    /// Старое имя действия (`aliasName`): под ним шорткаты лежат в старых настройках
    /// и его понимает URL-схема.
    pub fn alias_name(self) -> Option<&'static str> {
        match self {
            Action::LeftHalf => Some("leftSide"),
            Action::RightHalf => Some("rightSide"),
            Action::BottomHalf => Some("bottomSide"),
            Action::TopHalf => Some("topSide"),
            Action::CenterHalf => Some("centerSection"),
            _ => None,
        }
    }

    /// Номер экрана (0-based) у действий «на дисплей N» — `displayIndex`.
    pub fn display_index(self) -> Option<usize> {
        match self {
            Action::Display(number @ 1..=9) => Some(number as usize - 1),
            _ => None,
        }
    }

    /// Подпись пункта меню (`displayName` в Swift) по-русски — перевод оригинала
    /// (`ru.lproj/Main.strings`). `None` — у действия нет пункта в меню оригинала.
    pub fn display_name(self) -> Option<&'static str> {
        use Action::*;
        let name = match self {
            LeftHalf => "Левая половина",
            RightHalf => "Правая половина",
            Maximize => "Максимизировать",
            MaximizeHeight => "Максимизировать высоту",
            PreviousDisplay => "Предыдущий экран",
            NextDisplay => "Следующий экран",
            Larger => "Увеличить",
            Smaller => "Уменьшить",
            BottomHalf => "Нижняя половина",
            TopHalf => "Верхняя половина",
            Center => "В центр",
            BottomLeft => "Внизу слева",
            BottomRight => "Внизу справа",
            TopLeft => "Слева вверху",
            TopRight => "Справа вверху",
            Restore => "Восстановить",
            FirstThird => "Первая треть",
            FirstTwoThirds => "Первые две трети",
            CenterThird => "Центральная треть",
            CenterTwoThirds => "Центральные две трети",
            LastTwoThirds => "Последние две трети",
            LastThird => "Последняя треть",
            MoveLeft => "Налево",
            MoveRight => "Направо",
            MoveUp => "Вверх",
            MoveDown => "Вниз",
            AlmostMaximize => "Почти максимизировать",
            CenterHalf => "Центральная половина",
            FirstFourth => "Первая четверть",
            SecondFourth => "Вторая четверть",
            ThirdFourth => "Третья четверть",
            LastFourth => "Последняя четверть",
            FirstThreeFourths => "Первые три четверти",
            CenterThreeFourths => "Центральные три четверти",
            LastThreeFourths => "Последние три четверти",
            TopLeftSixth => "Верхняя шестая слева",
            TopCenterSixth => "Верхняя шестая по центру",
            TopRightSixth => "Верхняя шестая справа",
            BottomLeftSixth => "Нижняя шестая слева",
            BottomCenterSixth => "Нижняя шестая по центру",
            BottomRightSixth => "Нижняя шестая справа",
            // Девятые, восьмые, двенадцатые и шестнадцатые в оригинале не переведены
            // (там английский) — подписи по образцу шестых.
            TopLeftNinth => "Верхняя девятая слева",
            TopCenterNinth => "Верхняя девятая по центру",
            TopRightNinth => "Верхняя девятая справа",
            MiddleLeftNinth => "Средняя девятая слева",
            MiddleCenterNinth => "Средняя девятая по центру",
            MiddleRightNinth => "Средняя девятая справа",
            BottomLeftNinth => "Нижняя девятая слева",
            BottomCenterNinth => "Нижняя девятая по центру",
            BottomRightNinth => "Нижняя девятая справа",
            TopLeftThird | TopRightThird | BottomLeftThird | BottomRightThird => return None,
            TopLeftEighth => "Верхняя восьмая слева",
            TopCenterLeftEighth => "Верхняя восьмая по центру слева",
            TopCenterRightEighth => "Верхняя восьмая по центру справа",
            TopRightEighth => "Верхняя восьмая справа",
            BottomLeftEighth => "Нижняя восьмая слева",
            BottomCenterLeftEighth => "Нижняя восьмая по центру слева",
            BottomCenterRightEighth => "Нижняя восьмая по центру справа",
            BottomRightEighth => "Нижняя восьмая справа",
            DoubleHeightUp | DoubleHeightDown | DoubleWidthLeft | DoubleWidthRight
            | HalveHeightUp | HalveHeightDown | HalveWidthLeft | HalveWidthRight => return None,
            Specified | ReverseAll | TileAll | CascadeAll | LeftTodo | RightTodo
            | CascadeActiveApp | TileActiveApp => return None,
            CenterProminently | LargerWidth | SmallerWidth | LargerHeight | SmallerHeight => {
                return None
            }
            TopVerticalThird
            | MiddleVerticalThird
            | BottomVerticalThird
            | TopVerticalTwoThirds
            | BottomVerticalTwoThirds => return None,
            TopLeftTwelfth => "Верхняя двенадцатая слева",
            TopCenterLeftTwelfth => "Верхняя двенадцатая по центру слева",
            TopCenterRightTwelfth => "Верхняя двенадцатая по центру справа",
            TopRightTwelfth => "Верхняя двенадцатая справа",
            MiddleLeftTwelfth => "Средняя двенадцатая слева",
            MiddleCenterLeftTwelfth => "Средняя двенадцатая по центру слева",
            MiddleCenterRightTwelfth => "Средняя двенадцатая по центру справа",
            MiddleRightTwelfth => "Средняя двенадцатая справа",
            BottomLeftTwelfth => "Нижняя двенадцатая слева",
            BottomCenterLeftTwelfth => "Нижняя двенадцатая по центру слева",
            BottomCenterRightTwelfth => "Нижняя двенадцатая по центру справа",
            BottomRightTwelfth => "Нижняя двенадцатая справа",
            TopLeftSixteenth => "Верхняя шестнадцатая слева",
            TopCenterLeftSixteenth => "Верхняя шестнадцатая по центру слева",
            TopCenterRightSixteenth => "Верхняя шестнадцатая по центру справа",
            TopRightSixteenth => "Верхняя шестнадцатая справа",
            UpperMiddleLeftSixteenth => "Верхняя средняя шестнадцатая слева",
            UpperMiddleCenterLeftSixteenth => "Верхняя средняя шестнадцатая по центру слева",
            UpperMiddleCenterRightSixteenth => "Верхняя средняя шестнадцатая по центру справа",
            UpperMiddleRightSixteenth => "Верхняя средняя шестнадцатая справа",
            LowerMiddleLeftSixteenth => "Нижняя средняя шестнадцатая слева",
            LowerMiddleCenterLeftSixteenth => "Нижняя средняя шестнадцатая по центру слева",
            LowerMiddleCenterRightSixteenth => "Нижняя средняя шестнадцатая по центру справа",
            LowerMiddleRightSixteenth => "Нижняя средняя шестнадцатая справа",
            BottomLeftSixteenth => "Нижняя шестнадцатая слева",
            BottomCenterLeftSixteenth => "Нижняя шестнадцатая по центру слева",
            BottomCenterRightSixteenth => "Нижняя шестнадцатая по центру справа",
            BottomRightSixteenth => "Нижняя шестнадцатая справа",
            Display(_) => return None,
            // В оригинале «1-й столбик» (Columns.strings); здесь порядковым словом.
            Column { count, index } => {
                column_offset(count, index)?;
                column_title(index)
            }
        };
        Some(name)
    }

    /// Заголовок пункта меню. У большинства действий это имя в Camel Case
    /// с пробелами; особые случаи перечислены явно (как в Rectangle).
    pub fn title(&self) -> String {
        use Action::*;
        match self {
            Center => return "Move to Center".to_string(),
            CenterHalf => return "Center".to_string(),
            Specified => return "Specified Size".to_string(),
            Display(number) => return format!("Display {}", number),
            Column { .. } => return self.column_default_display_name(),
            _ => {}
        }

        let name = self.name();
        let mut title = String::new();
        let mut previous_lowercase = false;
        for ch in name.chars() {
            if ch.is_ascii_uppercase() || ch.is_ascii_digit() {
                if previous_lowercase {
                    title.push(' ');
                }
                title.push(ch);
                previous_lowercase = false;
            } else {
                title.push(ch);
                previous_lowercase = true;
            }
        }
        title
    }

    /// Название столбика для меню: 1st Column, 2nd Column, …
    pub fn column_default_display_name(&self) -> String {
        match self.column_index() {
            Some(index @ 1..=8) => column_default_title(index).to_string(),
            Some(index) => format!("{}th Column", index),
            None => self.name(),
        }
    }

    /// Имя действия для столбика: columnFive1 … columnEight8.
    pub fn column_action_name(&self) -> String {
        self.name()
    }

    /// Раскладка столбиков, к которой относится действие (`columnCategory`).
    pub fn column_category(&self) -> Option<WindowActionCategory> {
        match self.column_count()? {
            5 => Some(WindowActionCategory::ColumnsFive),
            6 => Some(WindowActionCategory::ColumnsSix),
            7 => Some(WindowActionCategory::ColumnsSeven),
            8 => Some(WindowActionCategory::ColumnsEight),
            _ => None,
        }
    }

    /// Какие края столбика общие с соседями — от этого зависит, где рисуются гэпы.
    pub fn column_shared_edges(&self) -> Edge {
        match self {
            Action::Column { count, index } => {
                let mut edges = Edge::NONE;
                if *index > 1 {
                    edges = edges.with(Edge::LEFT);
                }
                if *index < *count {
                    edges = edges.with(Edge::RIGHT);
                }
                edges
            }
            _ => Edge::NONE,
        }
    }

    /// После этого действия меню ставит разделитель (`firstInGroup`).
    pub fn first_in_group(self) -> bool {
        use Action::*;
        matches!(
            self,
            LeftHalf
                | TopLeft
                | FirstThird
                | Maximize
                | AlmostMaximize
                | NextDisplay
                | MoveLeft
                | FirstFourth
                | TopLeftSixth
                | TopLeftEighth
                | TopLeftNinth
                | TopLeftTwelfth
                | TopLeftSixteenth
                | Column {
                    count: 5..=8,
                    index: 1
                }
        )
    }

    /// Меняет ли действие размер окна.
    pub fn resizes(&self, resize_on_directional_move: bool) -> bool {
        use Action::*;
        match self {
            Center | CenterProminently | NextDisplay | PreviousDisplay | Display(_) => false,
            MoveUp | MoveDown | MoveLeft | MoveRight => resize_on_directional_move,
            _ => true,
        }
    }

    /// Разрешено ли окну вылезать за пределы экрана (для BestEffortWindowMover).
    pub fn allowed_to_extend_outside_current_screen_area(&self) -> bool {
        use Action::*;
        matches!(
            self,
            DoubleHeightUp | DoubleHeightDown | DoubleWidthLeft | DoubleWidthRight
        )
    }

    /// Можно ли назначить действие на зону drag-to-snap (`isDragSnappable`).
    pub fn is_drag_snappable(self) -> bool {
        use Action::*;
        !matches!(
            self,
            Restore
                | PreviousDisplay
                | NextDisplay
                | MoveUp
                | MoveDown
                | MoveLeft
                | MoveRight
                | Specified
                | ReverseAll
                | TileAll
                | CascadeAll
                | Larger
                | Smaller
                | LargerWidth
                | SmallerWidth
                | CascadeActiveApp
                | TileActiveApp
                // девятые
                | TopLeftNinth
                | TopCenterNinth
                | TopRightNinth
                | MiddleLeftNinth
                | MiddleCenterNinth
                | MiddleRightNinth
                | BottomLeftNinth
                | BottomCenterNinth
                | BottomRightNinth
                // угловые трети
                | TopLeftThird
                | TopRightThird
                | BottomLeftThird
                | BottomRightThird
                // конкретные дисплеи и столбики
                | Display(_)
                | Column { .. }
        )
    }

    /// Имя PNG иконки пункта меню (`image` в Swift: имя в Assets или своя картинка
    /// столбика). `None` — в Swift пустая картинка `NSImage()`.
    pub fn image_name(self) -> Option<&'static str> {
        use Action::*;
        let name = match self {
            LeftHalf => "leftHalfTemplate",
            RightHalf => "rightHalfTemplate",
            Maximize => "maximizeTemplate",
            MaximizeHeight => "maximizeHeightTemplate",
            PreviousDisplay => "prevDisplayTemplate",
            NextDisplay => "nextDisplayTemplate",
            Larger => "makeLargerTemplate",
            Smaller => "makeSmallerTemplate",
            BottomHalf => "bottomHalfTemplate",
            TopHalf => "topHalfTemplate",
            Center => "centerTemplate",
            BottomLeft => "bottomLeftTemplate",
            BottomRight => "bottomRightTemplate",
            TopLeft => "topLeftTemplate",
            TopRight => "topRightTemplate",
            Restore => "restoreTemplate",
            FirstThird => "firstThirdTemplate",
            FirstTwoThirds => "firstTwoThirdsTemplate",
            CenterThird => "centerThirdTemplate",
            CenterTwoThirds => "centerTwoThirdsTemplate",
            LastTwoThirds => "lastTwoThirdsTemplate",
            LastThird => "lastThirdTemplate",
            MoveLeft => "moveLeftTemplate",
            MoveRight => "moveRightTemplate",
            MoveUp => "moveUpTemplate",
            MoveDown => "moveDownTemplate",
            AlmostMaximize => "almostMaximizeTemplate",
            CenterHalf => "halfWidthCenterTemplate",
            FirstFourth => "leftFourthTemplate",
            SecondFourth => "centerLeftFourthTemplate",
            ThirdFourth => "centerRightFourthTemplate",
            LastFourth => "rightFourthTemplate",
            FirstThreeFourths => "firstThreeFourthsTemplate",
            CenterThreeFourths => "centerThreeFourthsTemplate",
            LastThreeFourths => "lastThreeFourthsTemplate",
            TopLeftSixth => "topLeftSixthTemplate",
            TopCenterSixth => "topCenterSixthTemplate",
            TopRightSixth => "topRightSixthTemplate",
            BottomLeftSixth => "bottomLeftSixthTemplate",
            BottomCenterSixth => "bottomCenterSixthTemplate",
            BottomRightSixth => "bottomRightSixthTemplate",
            TopLeftNinth => "topLeftNinthTemplate",
            TopCenterNinth => "topCenterNinthTemplate",
            TopRightNinth => "topRightNinthTemplate",
            MiddleLeftNinth => "middleLeftNinthTemplate",
            MiddleCenterNinth => "middleCenterNinthTemplate",
            MiddleRightNinth => "middleRightNinthTemplate",
            BottomLeftNinth => "bottomLeftNinthTemplate",
            BottomCenterNinth => "bottomCenterNinthTemplate",
            BottomRightNinth => "bottomRightNinthTemplate",
            TopLeftThird | TopRightThird | BottomLeftThird | BottomRightThird => return None,
            TopLeftEighth => "tlEighthTemplate",
            TopCenterLeftEighth => "ctlEighthTemplate",
            TopCenterRightEighth => "ctrEighthTemplate",
            TopRightEighth => "trEighthTemplate",
            BottomLeftEighth => "blEighthTemplate",
            BottomCenterLeftEighth => "cblEighthTemplate",
            BottomCenterRightEighth => "cbrEighthTemplate",
            BottomRightEighth => "brEighthTemplate",
            DoubleHeightUp | DoubleHeightDown | DoubleWidthLeft | DoubleWidthRight
            | HalveHeightUp | HalveHeightDown | HalveWidthLeft | HalveWidthRight => return None,
            Specified | ReverseAll | TileAll | CascadeAll | LeftTodo | RightTodo
            | CascadeActiveApp | TileActiveApp | CenterProminently => return None,
            LargerWidth => "largerWidthTemplate",
            SmallerWidth => "smallerWidthTemplate",
            LargerHeight | SmallerHeight => return None,
            TopVerticalThird => "topThirdTemplate",
            MiddleVerticalThird => "centerThirdHorizontalTemplate",
            BottomVerticalThird => "bottomThirdTemplate",
            TopVerticalTwoThirds => "topTwoThirdsTemplate",
            BottomVerticalTwoThirds => "bottomTwoThirdsTemplate",
            TopLeftTwelfth => "topLeftTwelfthTemplate",
            TopCenterLeftTwelfth => "topCenterLeftTwelfthTemplate",
            TopCenterRightTwelfth => "topCenterRightTwelfthTemplate",
            TopRightTwelfth => "topRightTwelfthTemplate",
            MiddleLeftTwelfth => "middleLeftTwelfthTemplate",
            MiddleCenterLeftTwelfth => "middleCenterLeftTwelfthTemplate",
            MiddleCenterRightTwelfth => "middleCenterRightTwelfthTemplate",
            MiddleRightTwelfth => "middleRightTwelfthTemplate",
            BottomLeftTwelfth => "bottomLeftTwelfthTemplate",
            BottomCenterLeftTwelfth => "bottomCenterLeftTwelfthTemplate",
            BottomCenterRightTwelfth => "bottomCenterRightTwelfthTemplate",
            BottomRightTwelfth => "bottomRightTwelfthTemplate",
            TopLeftSixteenth => "topLeftSixteenthTemplate",
            TopCenterLeftSixteenth => "topCenterLeftSixteenthTemplate",
            TopCenterRightSixteenth => "topCenterRightSixteenthTemplate",
            TopRightSixteenth => "topRightSixteenthTemplate",
            UpperMiddleLeftSixteenth => "upperMiddleLeftSixteenthTemplate",
            UpperMiddleCenterLeftSixteenth => "upperMiddleCenterLeftSixteenthTemplate",
            UpperMiddleCenterRightSixteenth => "upperMiddleCenterRightSixteenthTemplate",
            UpperMiddleRightSixteenth => "upperMiddleRightSixteenthTemplate",
            LowerMiddleLeftSixteenth => "lowerMiddleLeftSixteenthTemplate",
            LowerMiddleCenterLeftSixteenth => "lowerMiddleCenterLeftSixteenthTemplate",
            LowerMiddleCenterRightSixteenth => "lowerMiddleCenterRightSixteenthTemplate",
            LowerMiddleRightSixteenth => "lowerMiddleRightSixteenthTemplate",
            BottomLeftSixteenth => "bottomLeftSixteenthTemplate",
            BottomCenterLeftSixteenth => "bottomCenterLeftSixteenthTemplate",
            BottomCenterRightSixteenth => "bottomCenterRightSixteenthTemplate",
            BottomRightSixteenth => "bottomRightSixteenthTemplate",
            Display(_) => "nextDisplayTemplate",
            Column { count, index } => COLUMN_IMAGE_NAMES[column_offset(count, index)?],
        };
        Some(name)
    }

    /// Какие края окно делит с соседями — для гэпов (`gapSharedEdge`).
    pub fn gap_shared_edge(&self, resize_on_directional_move: bool) -> Edge {
        use Action::*;
        let when_resizing = |edge: Edge| {
            if resize_on_directional_move {
                edge
            } else {
                Edge::NONE
            }
        };
        match self {
            LeftHalf => R,
            RightHalf => L,
            BottomHalf => T,
            TopHalf => B,
            BottomLeft => edges(&[T, R]),
            BottomRight => edges(&[T, L]),
            TopLeft => edges(&[B, R]),
            TopRight => edges(&[B, L]),
            MoveUp => when_resizing(B),
            MoveDown => when_resizing(T),
            MoveLeft => when_resizing(R),
            MoveRight => when_resizing(L),
            // Столбики: гэп только на внутренних границах между соседними столбиками.
            Column { .. } => self.column_shared_edges(),
            _ => Edge::NONE,
        }
    }

    /// К каким осям применяются гэпы (`gapsApplicable`). `apply_gaps_to_maximize*` —
    /// включены ли настройки (в Swift — `!userDisabled`).
    pub fn gaps_applicable(
        &self,
        resize_on_directional_move: bool,
        apply_gaps_to_maximize: bool,
        apply_gaps_to_maximize_height: bool,
    ) -> Dimension {
        use Action::*;
        match self {
            LeftHalf
            | RightHalf
            | BottomHalf
            | TopHalf
            | CenterHalf
            | BottomLeft
            | BottomRight
            | TopLeft
            | TopRight
            | FirstThird
            | FirstTwoThirds
            | CenterThird
            | CenterTwoThirds
            | LastTwoThirds
            | LastThird
            | FirstFourth
            | SecondFourth
            | ThirdFourth
            | LastFourth
            | FirstThreeFourths
            | CenterThreeFourths
            | LastThreeFourths
            | TopLeftSixth
            | TopCenterSixth
            | TopRightSixth
            | BottomLeftSixth
            | BottomCenterSixth
            | BottomRightSixth
            | TopLeftNinth
            | TopCenterNinth
            | TopRightNinth
            | MiddleLeftNinth
            | MiddleCenterNinth
            | MiddleRightNinth
            | BottomLeftNinth
            | BottomCenterNinth
            | BottomRightNinth
            | TopLeftThird
            | TopRightThird
            | BottomLeftThird
            | BottomRightThird
            | TopLeftEighth
            | TopCenterLeftEighth
            | TopCenterRightEighth
            | TopRightEighth
            | BottomLeftEighth
            | BottomCenterLeftEighth
            | BottomCenterRightEighth
            | BottomRightEighth
            | TopLeftTwelfth
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
            | BottomRightTwelfth
            | TopLeftSixteenth
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
            | BottomRightSixteenth
            | DoubleHeightUp
            | DoubleHeightDown
            | DoubleWidthLeft
            | DoubleWidthRight
            | HalveHeightUp
            | HalveHeightDown
            | HalveWidthLeft
            | HalveWidthRight
            | LeftTodo
            | RightTodo
            | TopVerticalThird
            | MiddleVerticalThird
            | BottomVerticalThird
            | TopVerticalTwoThirds
            | BottomVerticalTwoThirds
            | Column { .. } => Dimension::BOTH,
            MoveUp | MoveDown => {
                if resize_on_directional_move {
                    Dimension::VERTICAL
                } else {
                    Dimension::NONE
                }
            }
            MoveLeft | MoveRight => {
                if resize_on_directional_move {
                    Dimension::HORIZONTAL
                } else {
                    Dimension::NONE
                }
            }
            Maximize => {
                if apply_gaps_to_maximize {
                    Dimension::BOTH
                } else {
                    Dimension::NONE
                }
            }
            MaximizeHeight => {
                if apply_gaps_to_maximize_height {
                    Dimension::VERTICAL
                } else {
                    Dimension::NONE
                }
            }
            AlmostMaximize | PreviousDisplay | NextDisplay | Larger | Smaller | LargerWidth
            | SmallerWidth | LargerHeight | SmallerHeight | Center | CenterProminently
            | Restore | Specified | ReverseAll | TileAll | CascadeAll | CascadeActiveApp
            | TileActiveApp | Display(_) => Dimension::NONE,
        }
    }

    /// Перебирает ли повторное нажатие позиции (`positionCycles`).
    pub fn position_cycles(self) -> bool {
        use Action::*;
        !matches!(
            self,
            Maximize
                | AlmostMaximize
                | MaximizeHeight
                | Larger
                | Smaller
                | LargerWidth
                | SmallerWidth
                | LargerHeight
                | SmallerHeight
                | Center
                | CenterProminently
                | Restore
                | NextDisplay
                | PreviousDisplay
                | Display(_)
                | MoveLeft
                | MoveRight
                | MoveUp
                | MoveDown
                | DoubleHeightUp
                | DoubleHeightDown
                | DoubleWidthLeft
                | DoubleWidthRight
                | HalveHeightUp
                | HalveHeightDown
                | HalveWidthLeft
                | HalveWidthRight
                | ReverseAll
                | TileAll
                | CascadeAll
                | CascadeActiveApp
                | TileActiveApp
                | LeftTodo
                | RightTodo
                | Specified
        )
    }

    /// Сдвигать ли окно, которое встало ровно поверх другого (`overlapOffsetApplies`):
    /// перебираемые позиции и развёрнутое на весь экран.
    pub fn overlap_offset_applies(self) -> bool {
        self.position_cycles() || self == Action::Maximize
    }

    /// Подменю, в котором действие стоит в меню (`category`).
    pub fn category(self) -> Option<WindowActionCategory> {
        use Action::*;
        use WindowActionCategory as C;
        match self {
            FirstThird | CenterThird | LastThird | FirstTwoThirds | CenterTwoThirds
            | LastTwoThirds => Some(C::Thirds),
            FirstFourth | SecondFourth | ThirdFourth | LastFourth | FirstThreeFourths
            | CenterThreeFourths | LastThreeFourths => Some(C::Fourths),
            TopLeftSixth | TopCenterSixth | TopRightSixth | BottomLeftSixth | BottomCenterSixth
            | BottomRightSixth => Some(C::Sixths),
            TopLeftEighth
            | TopCenterLeftEighth
            | TopCenterRightEighth
            | TopRightEighth
            | BottomLeftEighth
            | BottomCenterLeftEighth
            | BottomCenterRightEighth
            | BottomRightEighth => Some(C::Eighths),
            TopLeftNinth | TopCenterNinth | TopRightNinth | MiddleLeftNinth | MiddleCenterNinth
            | MiddleRightNinth | BottomLeftNinth | BottomCenterNinth | BottomRightNinth => {
                Some(C::Ninths)
            }
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
            | BottomRightTwelfth => Some(C::Twelfths),
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
            | BottomRightSixteenth => Some(C::Sixteenths),
            MoveUp | MoveDown | MoveLeft | MoveRight => Some(C::Move),
            AlmostMaximize | MaximizeHeight | Larger | Smaller | LargerWidth | SmallerWidth
            | LargerHeight | SmallerHeight => Some(C::Size),
            Column { .. } => self.column_category(),
            _ => None,
        }
    }

    /// Группа действия для логики оригинала (`classification`): трети в портрете
    /// получают повёрнутую иконку, у size/display своя обработка повторов.
    pub fn classification(self) -> Option<WindowActionCategory> {
        use Action::*;
        use WindowActionCategory as C;
        match self {
            FirstThird | FirstTwoThirds | CenterThird | CenterTwoThirds | LastTwoThirds
            | LastThird => Some(C::Thirds),
            Smaller | Larger | SmallerWidth | LargerWidth | SmallerHeight | LargerHeight => {
                Some(C::Size)
            }
            PreviousDisplay | NextDisplay | Display(_) => Some(C::Display),
            _ => None,
        }
    }

    /// Действие, которое нужно выполнить повторно, чтобы «продолжить» (для restore и т.п.).
    pub fn is_move_to_display(&self) -> bool {
        matches!(
            self,
            Action::NextDisplay | Action::PreviousDisplay | Action::Display(_)
        )
    }
}

/// Все действия порта в порядке `WindowAction.active` оригинала.
pub fn all_actions() -> Vec<Action> {
    Action::active().to_vec()
}

/// Проверка, что «пустой» рект не просочится в AX (страховка от NaN).
pub fn sanitize(rect: Rect) -> Option<Rect> {
    let ok = rect.x.is_finite() && rect.y.is_finite() && rect.w.is_finite() && rect.h.is_finite();
    if ok && !rect.is_empty() {
        Some(rect)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    const SWIFT: &str = "WindowAction.swift";

    #[test]
    fn active_is_the_swift_list() {
        // WindowAction.swift:172-207 — 151 кейс, все кейсы enum.
        assert_eq!(Action::active().len(), 151);
        assert_eq!(Action::active()[0], Action::LeftHalf);
        assert_eq!(Action::active()[2], Action::CenterHalf);
        assert_eq!(Action::active()[52], Action::ReverseAll);
        assert_eq!(Action::active()[115], Action::TileActiveApp);
        assert_eq!(Action::active()[116], Action::Display(1));
        assert_eq!(
            *Action::active().last().unwrap(),
            Action::Column { count: 8, index: 8 }
        );
        let unique: HashSet<Action> = Action::active().iter().copied().collect();
        assert_eq!(unique.len(), 151);
    }

    #[test]
    fn raw_name_and_back() {
        let mut raws = HashSet::new();
        let mut names = HashSet::new();
        for &action in Action::active() {
            let raw = action.raw();
            assert!(raw >= 0, "{action:?}");
            assert!(raws.insert(raw), "rawValue {raw} повторяется");
            assert_eq!(Action::from_raw(raw), Some(action));

            let name = action.name();
            assert!(names.insert(name.clone()), "имя {name} повторяется");
            assert_eq!(Action::from_name(&name), Some(action));
            if let Some(alias) = action.alias_name() {
                assert_eq!(Action::from_name(alias), Some(action));
            }
        }
        // Дыры в rawValue оригинала: 6, 7, 17, 18 и 129, 130 (tileRows/tileColumns апстрима).
        for raw in [-1, 6, 7, 17, 18, 129, 130, 157] {
            assert_eq!(Action::from_raw(raw), None, "{raw}");
        }
        assert_eq!(Action::from_name("tileRows"), None);
        assert_eq!(Action::Display(10).raw(), -1);
        assert_eq!(Action::Column { count: 9, index: 1 }.raw(), -1);
        assert_eq!(Action::Column { count: 5, index: 6 }.raw(), -1);
    }

    #[test]
    fn new_actions_have_no_calculation_yet() {
        let config = crate::config::Config::default();
        let visible = Rect::new(0.0, 0.0, 1728.0, 1001.0);
        for action in [
            Action::ReverseAll,
            Action::TileAll,
            Action::CascadeAll,
            Action::CascadeActiveApp,
            Action::TileActiveApp,
        ] {
            let params = crate::calc::CalcParams {
                window: Rect::new(100.0, 100.0, 800.0, 600.0),
                visible,
                visible_ignoring_stage: None,
                action,
                last: None,
                config: &config,
                source_visible: None,
                num_screens: 1,
                primary_max_y: 1117.0,
            };
            assert!(crate::calc::calculate(&params).is_none(), "{action:?}");
        }
    }

    /// Выборочные значения — каждое с конкретной строкой оригинала.
    #[test]
    fn metadata_matches_swift_lines() {
        use Action::*;
        let lines: &[(&str, bool)] = &[
            // rawValue
            (":54 reverseAll = 44", ReverseAll.raw() == 44),
            (":96 tileActiveApp = 86", TileActiveApp.raw() == 86),
            (":138 displayNine = 128", Display(9).raw() == 128),
            (
                ":144 columnFive1 = 131",
                Column { count: 5, index: 1 }.raw() == 131,
            ),
            (
                ":169 columnEight8 = 156",
                Column { count: 8, index: 8 }.raw() == 156,
            ),
            // name / aliasName / displayIndex
            (":365 displayNine", Display(9).name() == "displayNine"),
            (
                "ColumnLayout.swift:77 columnSix4",
                Column { count: 6, index: 4 }.name() == "columnSix4",
            ),
            (
                ":376 leftHalf → leftSide",
                LeftHalf.alias_name() == Some("leftSide"),
            ),
            (
                ":380 centerHalf → centerSection",
                CenterHalf.alias_name() == Some("centerSection"),
            ),
            (":387 displayOne → 0", Display(1).display_index() == Some(0)),
            (
                ":395 displayNine → 8",
                Display(9).display_index() == Some(8),
            ),
            // displayName (русский перевод оригинала: ru.lproj/Main.strings)
            (
                ":424 larger → Eah-KL-kbn.title → «Увеличить»",
                Larger.display_name() == Some("Увеличить"),
            ),
            (
                ":436 center → 8Bg-SZ-hDO.title → «В центр»",
                Center.display_name() == Some("В центр"),
            ),
            (
                ":486 centerHalf → bRX-dV-iAR.title → «Центральная половина»",
                CenterHalf.display_name() == Some("Центральная половина"),
            ),
            (
                ":555 corner thirds → nil",
                TopRightThird.display_name().is_none(),
            ),
            (":583 tileAll → nil", TileAll.display_name().is_none()),
            (":673 displays → nil", Display(2).display_name().is_none()),
            (
                ":682 columns — пункт меню есть",
                Column { count: 7, index: 2 }.display_name() == Some("Второй столбик"),
            ),
            // firstInGroup, resizes, allowedToExtend…, isDragSnappable
            (
                ":232 almostMaximize firstInGroup",
                AlmostMaximize.first_in_group(),
            ),
            (
                ":232 columnEight1 firstInGroup",
                Column { count: 8, index: 1 }.first_in_group(),
            ),
            (
                ":232 rightHalf не firstInGroup",
                !RightHalf.first_in_group(),
            ),
            (
                ":697 moveUp resizes = настройка",
                !MoveUp.resizes(false) && MoveUp.resizes(true),
            ),
            (
                ":704 doubleWidthLeft",
                DoubleWidthLeft.allowed_to_extend_outside_current_screen_area(),
            ),
            (":713 tileAll не snappable", !TileAll.is_drag_snappable()),
            (
                ":713 largerHeight snappable (нет в списке)",
                LargerHeight.is_drag_snappable(),
            ),
            (
                ":713 leftTodo snappable (нет в списке)",
                LeftTodo.is_drag_snappable(),
            ),
            (
                ":717 topLeftThird не snappable",
                !TopLeftThird.is_drag_snappable(),
            ),
            // image
            (
                ":889 middleVerticalThird",
                MiddleVerticalThird.image_name() == Some("centerThirdHorizontalTemplate"),
            ),
            (
                ":923 displayThree",
                Display(3).image_name() == Some("nextDisplayTemplate"),
            ),
            (
                ":886 largerHeight без картинки",
                LargerHeight.image_name().is_none(),
            ),
            // gapSharedEdge, gapsApplicable
            (
                ":939 bottomLeft [.top, .right]",
                BottomLeft.gap_shared_edge(false) == edges(&[T, R]),
            ),
            (
                ":945 moveLeft",
                MoveLeft.gap_shared_edge(true) == R
                    && MoveLeft.gap_shared_edge(false) == Edge::NONE,
            ),
            (
                "ColumnLayout.swift:96-97 columnSeven1 — только правый край",
                Column { count: 7, index: 1 }.gap_shared_edge(false) == R,
            ),
            (
                ":975 leftTodo .both",
                LeftTodo.gaps_applicable(false, false, false) == Dimension::BOTH,
            ),
            (
                ":990 tileAll .none",
                TileAll.gaps_applicable(true, true, true) == Dimension::NONE,
            ),
            (
                ":987 maximize: userDisabled → .none",
                Maximize.gaps_applicable(false, false, true) == Dimension::NONE
                    && Maximize.gaps_applicable(false, true, false) == Dimension::BOTH,
            ),
            (
                ":989 maximizeHeight .vertical",
                MaximizeHeight.gaps_applicable(false, false, true) == Dimension::VERTICAL,
            ),
            // positionCycles, overlapOffsetApplies
            (
                ":1011 specified не перебирает",
                !Specified.position_cycles(),
            ),
            (":1010 leftTodo не перебирает", !LeftTodo.position_cycles()),
            (
                ":1013 centerHalf перебирает (default)",
                CenterHalf.position_cycles(),
            ),
            (
                ":1023 maximize — сдвиг при наложении",
                Maximize.overlap_offset_applies(),
            ),
            (
                ":1023 almostMaximize — нет",
                !AlmostMaximize.overlap_offset_applies(),
            ),
            // category, classification
            (
                ":1036 largerHeight .size",
                LargerHeight.category() == Some(WindowActionCategory::Size),
            ),
            (
                ":1035 moveUp .move",
                MoveUp.category() == Some(WindowActionCategory::Move),
            ),
            (
                ":1041 + ColumnLayout.swift:84 columnSix2 .columnsSix",
                Column { count: 6, index: 2 }.category() == Some(WindowActionCategory::ColumnsSix),
            ),
            (":1042 maximize без подменю", Maximize.category().is_none()),
            (
                ":1050 smallerHeight .size",
                SmallerHeight.classification() == Some(WindowActionCategory::Size),
            ),
            (
                ":1055 displayFour .display",
                Display(4).classification() == Some(WindowActionCategory::Display),
            ),
            (
                ":1056 topVerticalThird без classification",
                TopVerticalThird.classification().is_none(),
            ),
            // WindowActionCategory.swift
            (
                "WindowActionCategory.swift:21 sixteenths → 8",
                WindowActionCategory::Sixteenths.menu_order() == 8,
            ),
            (
                "WindowActionCategory.swift:26 halves → 99",
                WindowActionCategory::Halves.menu_order() == 99,
            ),
            (
                "WindowActionCategory.swift:33 halves (displayName)",
                WindowActionCategory::Halves.display_name() == "Половины",
            ),
        ];
        let failed: Vec<&str> = lines
            .iter()
            .filter(|(_, ok)| !ok)
            .map(|(line, _)| *line)
            .collect();
        assert!(lines.len() >= 20);
        assert!(failed.is_empty(), "{SWIFT}{failed:?}");
    }

    #[test]
    fn russian_titles() {
        use WindowActionCategory::*;
        let categories = [
            (Size, "Размер"),
            (Move, "Края"),
            (Thirds, "Трети"),
            (Fourths, "Четверти"),
            (Sixths, "Шестые"),
            (Eighths, "Восьмые"),
            (Ninths, "Девятые"),
            (Twelfths, "Двенадцатые"),
            (Sixteenths, "Шестнадцатые"),
            (ColumnsFive, "Пять столбиков"),
            (ColumnsSix, "Шесть столбиков"),
            (ColumnsSeven, "Семь столбиков"),
            (ColumnsEight, "Восемь столбиков"),
            (Halves, "Половины"),
            (Corners, "Углы"),
            (Max, "Развернуть"),
            (Display, "Экраны"),
            (Other, "Другое"),
        ];
        assert_eq!(categories.len(), WindowActionCategory::ALL.len());
        for (category, title) in categories {
            assert_eq!(category.display_name(), title, "{category:?}");
        }

        let ordinals = [
            "Первый столбик",
            "Второй столбик",
            "Третий столбик",
            "Четвёртый столбик",
            "Пятый столбик",
            "Шестой столбик",
            "Седьмой столбик",
            "Восьмой столбик",
        ];
        for count in SUPPORTED_COLUMN_COUNTS {
            for action in Action::column_cases(count) {
                let index = action.column_index().unwrap() as usize;
                assert_eq!(action.display_name(), Some(ordinals[index - 1]));
            }
        }
        assert_eq!(Action::Column { count: 5, index: 6 }.display_name(), None);

        // Пункт меню есть ровно у тех, у кого в Swift displayName != nil:
        // 151 − (4 угловые трети + 8 double/halve + 8 specified…tileActiveApp
        // + 5 centerProminently…smallerHeight + 5 вертикальных третей + 9 дисплеев).
        let with_title = Action::active()
            .iter()
            .filter(|action| action.display_name().is_some())
            .count();
        assert_eq!(with_title, 112);

        // Подписи, которых нет в переводе оригинала, — по образцу шестых.
        assert_eq!(
            Action::TopLeftSixth.display_name(),
            Some("Верхняя шестая слева")
        );
        assert_eq!(
            Action::TopLeftNinth.display_name(),
            Some("Верхняя девятая слева")
        );
        assert_eq!(
            Action::UpperMiddleCenterRightSixteenth.display_name(),
            Some("Верхняя средняя шестнадцатая по центру справа")
        );
    }

    /// `SubWindowAction.gapSharedEdge` целиком: WindowAction.swift:1184-1285
    /// (таблица снята со Swift скриптом, номер строки — в комментарии).
    #[test]
    fn sub_action_gap_edges_match_swift() {
        use SubAction::*;
        let expected: [(SubAction, &str, &[Edge]); 102] = [
            (LeftThird, "leftThird", &[R]),                            // :1184
            (CenterVerticalThird, "centerVerticalThird", &[R, L]),     // :1185
            (RightThird, "rightThird", &[L]),                          // :1186
            (LeftTwoThirds, "leftTwoThirds", &[R]),                    // :1187
            (RightTwoThirds, "rightTwoThirds", &[L]),                  // :1188
            (TopThird, "topThird", &[B]),                              // :1189
            (CenterHorizontalThird, "centerHorizontalThird", &[T, B]), // :1190
            (BottomThird, "bottomThird", &[T]),                        // :1191
            (TopTwoThirds, "topTwoThirds", &[B]),                      // :1192
            (BottomTwoThirds, "bottomTwoThirds", &[T]),                // :1193
            (LeftFourth, "leftFourth", &[R]),                          // :1194
            (CenterLeftFourth, "centerLeftFourth", &[R, L]),           // :1195
            (CenterRightFourth, "centerRightFourth", &[R, L]),         // :1196
            (RightFourth, "rightFourth", &[L]),                        // :1197
            (TopFourth, "topFourth", &[B]),                            // :1198
            (CenterTopFourth, "centerTopFourth", &[T, B]),             // :1199
            (CenterBottomFourth, "centerBottomFourth", &[T, B]),       // :1200
            (BottomFourth, "bottomFourth", &[T]),                      // :1201
            (RightThreeFourths, "rightThreeFourths", &[L]),            // :1202
            (BottomThreeFourths, "bottomThreeFourths", &[T]),          // :1203
            (LeftThreeFourths, "leftThreeFourths", &[R]),              // :1204
            (TopThreeFourths, "topThreeFourths", &[B]),                // :1205
            (
                CenterVerticalThreeFourths,
                "centerVerticalThreeFourths",
                &[R, L],
            ), // :1206
            (
                CenterHorizontalThreeFourths,
                "centerHorizontalThreeFourths",
                &[T, B],
            ), // :1207
            (CenterVerticalHalf, "centerVerticalHalf", &[R, L]),       // :1208
            (CenterHorizontalHalf, "centerHorizontalHalf", &[T, B]),   // :1209
            (TopLeftSixthLandscape, "topLeftSixthLandscape", &[R, B]), // :1210
            (
                TopCenterSixthLandscape,
                "topCenterSixthLandscape",
                &[R, L, B],
            ), // :1211
            (TopRightSixthLandscape, "topRightSixthLandscape", &[L, B]), // :1212
            (
                BottomLeftSixthLandscape,
                "bottomLeftSixthLandscape",
                &[T, R],
            ), // :1213
            (
                BottomCenterSixthLandscape,
                "bottomCenterSixthLandscape",
                &[L, R, T],
            ), // :1214
            (
                BottomRightSixthLandscape,
                "bottomRightSixthLandscape",
                &[L, T],
            ), // :1215
            (TopLeftSixthPortrait, "topLeftSixthPortrait", &[R, B]),   // :1216
            (TopRightSixthPortrait, "topRightSixthPortrait", &[L, B]), // :1217
            (
                LeftCenterSixthPortrait,
                "leftCenterSixthPortrait",
                &[T, B, R],
            ), // :1218
            (
                RightCenterSixthPortrait,
                "rightCenterSixthPortrait",
                &[L, T, B],
            ), // :1219
            (BottomLeftSixthPortrait, "bottomLeftSixthPortrait", &[T, R]), // :1220
            (
                BottomRightSixthPortrait,
                "bottomRightSixthPortrait",
                &[L, T],
            ), // :1221
            (
                TopLeftTwoSixthsLandscape,
                "topLeftTwoSixthsLandscape",
                &[R, B],
            ), // :1222
            (
                TopLeftTwoSixthsPortrait,
                "topLeftTwoSixthsPortrait",
                &[R, B],
            ), // :1223
            (
                TopRightTwoSixthsLandscape,
                "topRightTwoSixthsLandscape",
                &[L, B],
            ), // :1224
            (
                TopRightTwoSixthsPortrait,
                "topRightTwoSixthsPortrait",
                &[L, B],
            ), // :1225
            (
                BottomLeftTwoSixthsLandscape,
                "bottomLeftTwoSixthsLandscape",
                &[R, T],
            ), // :1226
            (
                BottomLeftTwoSixthsPortrait,
                "bottomLeftTwoSixthsPortrait",
                &[R, T],
            ), // :1227
            (
                BottomRightTwoSixthsLandscape,
                "bottomRightTwoSixthsLandscape",
                &[L, T],
            ), // :1228
            (
                BottomRightTwoSixthsPortrait,
                "bottomRightTwoSixthsPortrait",
                &[L, T],
            ), // :1229
            (TopLeftNinth, "topLeftNinth", &[R, B]),                   // :1230
            (TopCenterNinth, "topCenterNinth", &[R, L, B]),            // :1231
            (TopRightNinth, "topRightNinth", &[L, B]),                 // :1232
            (MiddleLeftNinth, "middleLeftNinth", &[T, R, B]),          // :1233
            (MiddleCenterNinth, "middleCenterNinth", &[T, R, B, L]),   // :1234
            (MiddleRightNinth, "middleRightNinth", &[L, T, B]),        // :1235
            (BottomLeftNinth, "bottomLeftNinth", &[T, R]),             // :1236
            (BottomCenterNinth, "bottomCenterNinth", &[L, T, R]),      // :1237
            (BottomRightNinth, "bottomRightNinth", &[L, T]),           // :1238
            (TopLeftThird, "topLeftThird", &[R, B]),                   // :1239
            (TopRightThird, "topRightThird", &[L, B]),                 // :1240
            (BottomLeftThird, "bottomLeftThird", &[R, T]),             // :1241
            (BottomRightThird, "bottomRightThird", &[L, T]),           // :1242
            (TopLeftQuarter, "topLeftQuarter", &[R, B]),               // :1243
            (TopRightQuarter, "topRightQuarter", &[L, B]),             // :1244
            (BottomLeftQuarter, "bottomLeftQuarter", &[R, T]),         // :1245
            (BottomRightQuarter, "bottomRightQuarter", &[L, T]),       // :1246
            (TopLeftEighth, "topLeftEighth", &[R, B]),                 // :1247
            (TopCenterLeftEighth, "topCenterLeftEighth", &[R, L, B]),  // :1248
            (TopCenterRightEighth, "topCenterRightEighth", &[R, L, B]), // :1249
            (TopRightEighth, "topRightEighth", &[L, B]),               // :1250
            (BottomLeftEighth, "bottomLeftEighth", &[R, T]),           // :1251
            (BottomCenterLeftEighth, "bottomCenterLeftEighth", &[R, L, T]), // :1252
            (
                BottomCenterRightEighth,
                "bottomCenterRightEighth",
                &[R, L, T],
            ), // :1253
            (BottomRightEighth, "bottomRightEighth", &[L, T]),         // :1254
            (TopLeftTwelfth, "topLeftTwelfth", &[R, B]),               // :1255
            (TopCenterLeftTwelfth, "topCenterLeftTwelfth", &[R, L, B]), // :1256
            (TopCenterRightTwelfth, "topCenterRightTwelfth", &[R, L, B]), // :1257
            (TopRightTwelfth, "topRightTwelfth", &[L, B]),             // :1258
            (MiddleLeftTwelfth, "middleLeftTwelfth", &[T, R, B]),      // :1259
            (
                MiddleCenterLeftTwelfth,
                "middleCenterLeftTwelfth",
                &[T, R, B, L],
            ), // :1260
            (
                MiddleCenterRightTwelfth,
                "middleCenterRightTwelfth",
                &[T, R, B, L],
            ), // :1261
            (MiddleRightTwelfth, "middleRightTwelfth", &[L, T, B]),    // :1262
            (BottomLeftTwelfth, "bottomLeftTwelfth", &[T, R]),         // :1263
            (
                BottomCenterLeftTwelfth,
                "bottomCenterLeftTwelfth",
                &[L, T, R],
            ), // :1264
            (
                BottomCenterRightTwelfth,
                "bottomCenterRightTwelfth",
                &[L, T, R],
            ), // :1265
            (BottomRightTwelfth, "bottomRightTwelfth", &[L, T]),       // :1266
            (TopLeftSixteenth, "topLeftSixteenth", &[R, B]),           // :1267
            (TopCenterLeftSixteenth, "topCenterLeftSixteenth", &[R, L, B]), // :1268
            (
                TopCenterRightSixteenth,
                "topCenterRightSixteenth",
                &[R, L, B],
            ), // :1269
            (TopRightSixteenth, "topRightSixteenth", &[L, B]),         // :1270
            (
                UpperMiddleLeftSixteenth,
                "upperMiddleLeftSixteenth",
                &[T, R, B],
            ), // :1271
            (
                UpperMiddleCenterLeftSixteenth,
                "upperMiddleCenterLeftSixteenth",
                &[T, R, B, L],
            ), // :1272
            (
                UpperMiddleCenterRightSixteenth,
                "upperMiddleCenterRightSixteenth",
                &[T, R, B, L],
            ), // :1273
            (
                UpperMiddleRightSixteenth,
                "upperMiddleRightSixteenth",
                &[L, T, B],
            ), // :1274
            (
                LowerMiddleLeftSixteenth,
                "lowerMiddleLeftSixteenth",
                &[T, R, B],
            ), // :1275
            (
                LowerMiddleCenterLeftSixteenth,
                "lowerMiddleCenterLeftSixteenth",
                &[T, R, B, L],
            ), // :1276
            (
                LowerMiddleCenterRightSixteenth,
                "lowerMiddleCenterRightSixteenth",
                &[T, R, B, L],
            ), // :1277
            (
                LowerMiddleRightSixteenth,
                "lowerMiddleRightSixteenth",
                &[L, T, B],
            ), // :1278
            (BottomLeftSixteenth, "bottomLeftSixteenth", &[T, R]),     // :1279
            (
                BottomCenterLeftSixteenth,
                "bottomCenterLeftSixteenth",
                &[L, T, R],
            ), // :1280
            (
                BottomCenterRightSixteenth,
                "bottomCenterRightSixteenth",
                &[L, T, R],
            ), // :1281
            (BottomRightSixteenth, "bottomRightSixteenth", &[L, T]),   // :1282
            (Maximize, "maximize", &[]),                               // :1283
            (LeftTodo, "leftTodo", &[R]),                              // :1284
            (RightTodo, "rightTodo", &[L]),                            // :1285
        ];
        assert_eq!(SubAction::ALL.len(), expected.len());
        for (index, (sub, name, edge_list)) in expected.iter().enumerate() {
            assert_eq!(SubAction::ALL[index], *sub, "порядок кейсов");
            assert_eq!(sub.name(), *name);
            assert_eq!(sub.gap_shared_edge(), edges(edge_list), "{name}");
        }
    }

    #[test]
    fn icons_exist_for_every_action_that_has_one() {
        let icons = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("packaging/icons");
        let mut without_icon = Vec::new();
        for &action in Action::active() {
            match action.image_name() {
                Some(name) => assert!(
                    icons.join(format!("{name}.png")).exists(),
                    "нет packaging/icons/{name}.png для {}",
                    action.name()
                ),
                None => without_icon.push(action.name()),
            }
        }
        // Ровно те, у кого в Swift `NSImage()` (WindowAction.swift:856-887).
        without_icon.sort();
        let mut expected = vec![
            "topLeftThird",
            "topRightThird",
            "bottomLeftThird",
            "bottomRightThird",
            "doubleHeightUp",
            "doubleHeightDown",
            "doubleWidthLeft",
            "doubleWidthRight",
            "halveHeightUp",
            "halveHeightDown",
            "halveWidthLeft",
            "halveWidthRight",
            "specified",
            "reverseAll",
            "tileAll",
            "cascadeAll",
            "leftTodo",
            "rightTodo",
            "cascadeActiveApp",
            "tileActiveApp",
            "centerProminently",
            "largerHeight",
            "smallerHeight",
        ];
        expected.sort();
        assert_eq!(without_icon, expected);
    }
}
