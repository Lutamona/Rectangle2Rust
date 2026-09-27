//! Типы настроек: перечисления с rawValue как в Swift, размеры перебора,
//! области drag-to-snap, цвет подсветки.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::BitOr;

use crate::defaults_store::{IntEnum, JsonCodable};
use crate::json::{self, Json};

// ---------------------------------------------------------------- перечисления

/// Перечисление с целочисленным rawValue, как `enum X: Int` в Swift.
macro_rules! int_enum {
    (
        $(#[$meta:meta])*
        pub enum $name:ident $(= $default:ident)? {
            $( $(#[$variant_meta:meta])* $variant:ident = $raw:literal ),* $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
        #[repr(i64)]
        pub enum $name {
            $( $(#[$variant_meta])* $variant = $raw ),*
        }

        impl $name {
            pub const ALL: &'static [$name] = &[$($name::$variant),*];

            pub fn from_raw(raw: i64) -> Option<Self> {
                match raw {
                    $( $raw => Some($name::$variant), )*
                    _ => None,
                }
            }

            pub fn raw(self) -> i64 {
                self as i64
            }
        }

        impl IntEnum for $name {
            fn from_raw(raw: i64) -> Option<Self> {
                $name::from_raw(raw)
            }

            fn raw(self) -> i64 {
                $name::raw(self)
            }
        }

        $(
            impl Default for $name {
                fn default() -> Self {
                    $name::$default
                }
            }
        )?
    };
}

int_enum! {
    /// Что делает повторное выполнение того же действия (`SubsequentExecutionMode.swift`).
    pub enum SubsequentExecutionMode = Resize {
        /// Перебирать размеры, как Spectacle.
        Resize = 0,
        /// Переносить окно на соседний экран.
        AcrossMonitor = 1,
        /// Ничего не делать.
        None = 2,
        /// Влево/вправо — на соседний экран, остальное — перебор размеров.
        AcrossAndResize = 3,
        /// Перебирать экраны.
        CycleMonitor = 4,
        /// Перебор размеров и позиций четвертей.
        ResizeAndCycleQuadrants = 5,
    }
}

impl SubsequentExecutionMode {
    pub fn resizes(self) -> bool {
        matches!(
            self,
            SubsequentExecutionMode::Resize
                | SubsequentExecutionMode::AcrossAndResize
                | SubsequentExecutionMode::ResizeAndCycleQuadrants
        )
    }

    pub fn cycles_quadrant_positions(self) -> bool {
        self == SubsequentExecutionMode::ResizeAndCycleQuadrants
    }

    pub fn traverses_displays(self) -> bool {
        matches!(
            self,
            SubsequentExecutionMode::AcrossMonitor | SubsequentExecutionMode::AcrossAndResize
        )
    }
}

int_enum! {
    /// Ось, по которой растёт окно в углу при повторных выполнениях (`CycleSize.swift`).
    pub enum CornerCycleExpansionAxis = Horizontal {
        Horizontal = 0,
        Vertical = 1,
    }
}

int_enum! {
    /// Как выравнивать окно, которое приложение не дало растянуть (`WindowMover.swift`).
    pub enum EdgeAlignment = EdgesAndCorners {
        /// По общим с экраном краям и углам.
        EdgesAndCorners = 1,
        /// Только по углам.
        Corners = 2,
        /// По центру зоны.
        Centered = 3,
    }
}

int_enum! {
    /// Обход AXEnhancedUserInterface при перемещении окна (`AccessibilityElement.swift`).
    pub enum EnhancedUI = DisableEnable {
        /// Выключать на время каждого перемещения и включать обратно.
        DisableEnable = 1,
        /// Выключать и не включать обратно.
        DisableOnly = 2,
        /// Выключать при каждой смене активного приложения.
        FrontmostDisable = 3,
    }
}

int_enum! {
    /// С какой стороны экрана боковая панель Todo (`TodoManager.swift`).
    pub enum TodoSidebarSide = Right {
        Right = 1,
        Left = 2,
    }
}

int_enum! {
    /// В чём задана ширина панели Todo (`TodoManager.swift`).
    pub enum TodoSidebarWidthUnit = Pixels {
        Pixels = 1,
        Pct = 2,
    }
}

impl TodoSidebarWidthUnit {
    /// Подпись единицы, как `description` в Swift.
    pub fn description(self) -> &'static str {
        match self {
            TodoSidebarWidthUnit::Pixels => "px",
            TodoSidebarWidthUnit::Pct => "%",
        }
    }
}

int_enum! {
    /// Порядок экранов для «следующий/предыдущий дисплей» (`ScreenDetection.swift`).
    pub enum ScreenOrdering = YThenMinX {
        /// По середине по X.
        MidX = 1,
        /// По левому краю.
        MinX = 2,
        /// Сверху вниз, затем слева направо.
        YThenMinX = 3,
    }
}

int_enum! {
    /// Зона у края экрана для drag-to-snap (`SnapAreaModel.swift`).
    pub enum Directional {
        Tl = 1,
        T = 2,
        Tr = 3,
        L = 4,
        R = 5,
        Bl = 6,
        B = 7,
        Br = 8,
        C = 9,
    }
}

impl Directional {
    /// Зоны, которые настраиваются (`Directional.cases` — без центра).
    pub const CASES: [Directional; 8] = [
        Directional::Tl,
        Directional::T,
        Directional::Tr,
        Directional::L,
        Directional::R,
        Directional::Bl,
        Directional::B,
        Directional::Br,
    ];
}

int_enum! {
    /// Составная зона drag-to-snap (`CompoundSnapArea.swift`).
    pub enum CompoundSnapArea {
        LeftTopBottomHalf = -2,
        RightTopBottomHalf = -3,
        Thirds = -4,
        PortraitThirdsSide = -5,
        Halves = -6,
        TopSixths = -7,
        BottomSixths = -8,
        Fourths = -9,
        PortraitTopBottomHalves = -10,
        TopEighths = -11,
        BottomEighths = -12,
    }
}

// ---------------------------------------------------------------- размеры перебора

int_enum! {
    /// Размер в переборе при повторных выполнениях (`CycleSize.swift`).
    pub enum CycleSize {
        TwoThirds = 0,
        OneHalf = 1,
        OneThird = 2,
        OneQuarter = 3,
        ThreeQuarters = 4,
    }
}

impl CycleSize {
    /// Допуск при сравнении процентов (`matchingTolerance`).
    pub const MATCHING_TOLERANCE: f32 = 0.001;

    /// Порядок перебора: с ½ вверх по размеру, затем меньшие —
    /// ½, ⅔, ¾, ¼, ⅓ (`sortedSizes`).
    pub const SORTED: [CycleSize; 5] = [
        CycleSize::OneHalf,
        CycleSize::TwoThirds,
        CycleSize::ThreeQuarters,
        CycleSize::OneQuarter,
        CycleSize::OneThird,
    ];

    /// Доля во Float, как в Swift (`2 / 3` считается во Float).
    pub fn fraction(self) -> f32 {
        match self {
            CycleSize::TwoThirds => 2.0 / 3.0,
            CycleSize::OneHalf => 1.0 / 2.0,
            CycleSize::OneThird => 1.0 / 3.0,
            CycleSize::OneQuarter => 1.0 / 4.0,
            CycleSize::ThreeQuarters => 3.0 / 4.0,
        }
    }

    pub fn percent_value(self) -> f32 {
        self.fraction() * 100.0
    }

    pub fn title(self) -> &'static str {
        match self {
            CycleSize::TwoThirds => "⅔",
            CycleSize::OneHalf => "½",
            CycleSize::OneThird => "⅓",
            CycleSize::OneQuarter => "¼",
            CycleSize::ThreeQuarters => "¾",
        }
    }

    pub fn matches(self, percent_value: f32, tolerance: f32) -> bool {
        (self.percent_value() - percent_value).abs() <= tolerance
    }

    /// Размер по проценту (`CycleSize.matching(percentValue:)`).
    pub fn matching(percent_value: f32) -> Option<CycleSize> {
        CycleSize::SORTED
            .into_iter()
            .find(|size| size.matches(percent_value, Self::MATCHING_TOLERANCE))
    }
}

/// Набор размеров перебора — битовая маска по rawValue (`selectedCycleSizes`).
#[derive(Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct CycleSizes(u8);

impl std::fmt::Debug for CycleSizes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let titles: Vec<&str> = self
            .sorted_sizes()
            .into_iter()
            .map(CycleSize::title)
            .collect();
        write!(f, "{{{}}}", titles.join(", "))
    }
}

impl CycleSizes {
    pub const EMPTY: CycleSizes = CycleSizes(0);

    /// `CycleSize.fromBits`: неизвестные биты отбрасываются.
    pub fn from_bits(bits: i64) -> CycleSizes {
        let mut sizes = CycleSizes::EMPTY;
        for size in CycleSize::ALL {
            if (bits >> size.raw()) & 1 == 1 {
                sizes.insert(*size);
            }
        }
        sizes
    }

    /// `Set<CycleSize>.toBits()`.
    pub fn bits(self) -> i64 {
        self.0 as i64
    }

    /// Набор по умолчанию: ½, ⅔, ⅓ (`CycleSize.defaultSizes`).
    pub fn default_sizes() -> CycleSizes {
        [
            CycleSize::OneHalf,
            CycleSize::TwoThirds,
            CycleSize::OneThird,
        ]
        .into_iter()
        .collect()
    }

    pub fn contains(self, size: CycleSize) -> bool {
        self.0 & (1 << size.raw()) != 0
    }

    pub fn insert(&mut self, size: CycleSize) {
        self.0 |= 1 << size.raw();
    }

    pub fn remove(&mut self, size: CycleSize) {
        self.0 &= !(1 << size.raw());
    }

    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Выбранные размеры в порядке перебора (`sortedSizes` с фильтром по набору).
    pub fn sorted_sizes(self) -> Vec<CycleSize> {
        CycleSize::SORTED
            .into_iter()
            .filter(|size| self.contains(*size))
            .collect()
    }
}

impl FromIterator<CycleSize> for CycleSizes {
    fn from_iter<I: IntoIterator<Item = CycleSize>>(iter: I) -> Self {
        let mut sizes = CycleSizes::EMPTY;
        for size in iter {
            sizes.insert(size);
        }
        sizes
    }
}

// ---------------------------------------------------------------- drag-to-snap

/// Зоны drag-to-snap, которые пользователь выключил (`SnapAreaOption`, битовая маска).
/// Старый формат настройки `ignoredSnapAreas`; оригинал переводит его в карты зон.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct SnapAreaOption(pub i64);

impl SnapAreaOption {
    pub const NONE: SnapAreaOption = SnapAreaOption(0);
    pub const TOP: SnapAreaOption = SnapAreaOption(1 << 0);
    pub const BOTTOM: SnapAreaOption = SnapAreaOption(1 << 1);
    pub const LEFT: SnapAreaOption = SnapAreaOption(1 << 2);
    pub const RIGHT: SnapAreaOption = SnapAreaOption(1 << 3);
    pub const TOP_LEFT: SnapAreaOption = SnapAreaOption(1 << 4);
    pub const TOP_RIGHT: SnapAreaOption = SnapAreaOption(1 << 5);
    pub const BOTTOM_LEFT: SnapAreaOption = SnapAreaOption(1 << 6);
    pub const BOTTOM_RIGHT: SnapAreaOption = SnapAreaOption(1 << 7);
    pub const TOP_LEFT_SHORT: SnapAreaOption = SnapAreaOption(1 << 8);
    pub const TOP_RIGHT_SHORT: SnapAreaOption = SnapAreaOption(1 << 9);
    pub const BOTTOM_LEFT_SHORT: SnapAreaOption = SnapAreaOption(1 << 10);
    pub const BOTTOM_RIGHT_SHORT: SnapAreaOption = SnapAreaOption(1 << 11);
    pub const ALL: SnapAreaOption = SnapAreaOption((1 << 12) - 1);

    pub const fn contains(self, other: SnapAreaOption) -> bool {
        self.0 & other.0 == other.0
    }
}

impl BitOr for SnapAreaOption {
    type Output = SnapAreaOption;

    fn bitor(self, other: SnapAreaOption) -> SnapAreaOption {
        SnapAreaOption(self.0 | other.0)
    }
}

/// Проверка rawValue `WindowAction` из Swift: оригинал отвергает весь JSON
/// зон, если в нём действие, которого нет в перечислении. Проверка — по
/// перечислению оригинала, а не по таблице порта (сюда входят и действия, которые
/// в области не ставятся: reverseAll, tileAll, cascadeAll и т.д.).
pub fn is_swift_window_action_raw(raw: i64) -> bool {
    matches!(raw, 0..=5 | 8..=16 | 19..=128 | 131..=156)
}

/// rawValue действий из Swift, нужные для зон по умолчанию.
const SWIFT_MAXIMIZE: i64 = 2;
const SWIFT_BOTTOM_LEFT: i64 = 13;
const SWIFT_BOTTOM_RIGHT: i64 = 14;
const SWIFT_TOP_LEFT: i64 = 15;
const SWIFT_TOP_RIGHT: i64 = 16;

/// Что делает зона drag-to-snap (`SnapAreaConfig`): составная зона или действие.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SnapAreaConfig {
    pub compound: Option<CompoundSnapArea>,
    /// rawValue `WindowAction` из Swift (действие хранится числом).
    pub action: Option<i64>,
}

impl SnapAreaConfig {
    pub fn action(raw: i64) -> Self {
        SnapAreaConfig {
            compound: None,
            action: Some(raw),
        }
    }

    pub fn compound(compound: CompoundSnapArea) -> Self {
        SnapAreaConfig {
            compound: Some(compound),
            action: None,
        }
    }
}

/// Карта зон (`[Directional: SnapAreaConfig]`).
pub type SnapAreas = BTreeMap<Directional, SnapAreaConfig>;

/// Зоны по умолчанию для горизонтального экрана (`SnapAreaModel.defaultLandscape`).
pub fn default_landscape_snap_areas() -> SnapAreas {
    BTreeMap::from([
        (Directional::Tl, SnapAreaConfig::action(SWIFT_TOP_LEFT)),
        (Directional::T, SnapAreaConfig::action(SWIFT_MAXIMIZE)),
        (Directional::Tr, SnapAreaConfig::action(SWIFT_TOP_RIGHT)),
        (
            Directional::L,
            SnapAreaConfig::compound(CompoundSnapArea::LeftTopBottomHalf),
        ),
        (
            Directional::R,
            SnapAreaConfig::compound(CompoundSnapArea::RightTopBottomHalf),
        ),
        (Directional::Bl, SnapAreaConfig::action(SWIFT_BOTTOM_LEFT)),
        (
            Directional::B,
            SnapAreaConfig::compound(CompoundSnapArea::Thirds),
        ),
        (Directional::Br, SnapAreaConfig::action(SWIFT_BOTTOM_RIGHT)),
    ])
}

/// Зоны по умолчанию для вертикального экрана (`SnapAreaModel.defaultPortrait`).
pub fn default_portrait_snap_areas() -> SnapAreas {
    BTreeMap::from([
        (Directional::Tl, SnapAreaConfig::action(SWIFT_TOP_LEFT)),
        (Directional::T, SnapAreaConfig::action(SWIFT_MAXIMIZE)),
        (Directional::Tr, SnapAreaConfig::action(SWIFT_TOP_RIGHT)),
        (
            Directional::L,
            SnapAreaConfig::compound(CompoundSnapArea::PortraitThirdsSide),
        ),
        (
            Directional::R,
            SnapAreaConfig::compound(CompoundSnapArea::PortraitThirdsSide),
        ),
        (Directional::Bl, SnapAreaConfig::action(SWIFT_BOTTOM_LEFT)),
        (
            Directional::B,
            SnapAreaConfig::compound(CompoundSnapArea::Halves),
        ),
        (Directional::Br, SnapAreaConfig::action(SWIFT_BOTTOM_RIGHT)),
    ])
}

impl JsonCodable for SnapAreaConfig {
    fn to_json(&self) -> Json {
        let mut pairs = Vec::new();
        if let Some(action) = self.action {
            pairs.push(("action".to_string(), Json::from_i64(action)));
        }
        if let Some(compound) = self.compound {
            pairs.push(("compound".to_string(), Json::from_i64(compound.raw())));
        }
        Json::Object(pairs)
    }

    fn from_json(value: &Json) -> Result<Self, String> {
        if !matches!(value, Json::Object(_)) {
            return Err("SnapAreaConfig: ожидался объект".to_string());
        }
        let compound = match value.get("compound") {
            None | Some(Json::Null) => None,
            Some(raw) => {
                let raw = json::decode_i64(raw)?;
                Some(
                    CompoundSnapArea::from_raw(raw)
                        .ok_or_else(|| format!("нет CompoundSnapArea с rawValue {}", raw))?,
                )
            }
        };
        let action = match value.get("action") {
            None | Some(Json::Null) => None,
            Some(raw) => {
                let raw = json::decode_i64(raw)?;
                if !is_swift_window_action_raw(raw) {
                    return Err(format!("нет WindowAction с rawValue {}", raw));
                }
                Some(raw)
            }
        };
        Ok(SnapAreaConfig { compound, action })
    }
}

/// Словарь с ключом-перечислением Swift кодирует массивом `[ключ, значение, …]`
/// (порядок у Swift случайный, здесь — по возрастанию ключа).
impl JsonCodable for SnapAreas {
    fn to_json(&self) -> Json {
        let mut items = Vec::new();
        for (directional, config) in self {
            items.push(Json::from_i64(directional.raw()));
            items.push(config.to_json());
        }
        Json::Array(items)
    }

    fn from_json(value: &Json) -> Result<Self, String> {
        let Json::Array(items) = value else {
            return Err("карта зон: ожидался массив".to_string());
        };
        if items.len() % 2 != 0 {
            return Err("карта зон: нечётное число элементов".to_string());
        }
        let mut areas = BTreeMap::new();
        for pair in items.chunks(2) {
            let raw = json::decode_i64(&pair[0])?;
            let directional = Directional::from_raw(raw)
                .ok_or_else(|| format!("нет Directional с rawValue {}", raw))?;
            areas.insert(directional, SnapAreaConfig::from_json(&pair[1])?);
        }
        Ok(areas)
    }
}

// ---------------------------------------------------------------- цвет подсветки

/// Цвет подсветки при drag-to-snap (`CodableColor`, компоненты CGFloat).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FootprintColor {
    pub red: f64,
    pub green: f64,
    pub blue: f64,
    pub alpha: Option<f64>,
}

impl JsonCodable for FootprintColor {
    fn to_json(&self) -> Json {
        let mut pairs = vec![
            ("red".to_string(), Json::from_f64(self.red)),
            ("green".to_string(), Json::from_f64(self.green)),
            ("blue".to_string(), Json::from_f64(self.blue)),
        ];
        if let Some(alpha) = self.alpha {
            pairs.push(("alpha".to_string(), Json::from_f64(alpha)));
        }
        Json::Object(pairs)
    }

    fn from_json(value: &Json) -> Result<Self, String> {
        if !matches!(value, Json::Object(_)) {
            return Err("CodableColor: ожидался объект".to_string());
        }
        let component = |name: &str| -> Result<f64, String> {
            let value = value
                .get(name)
                .ok_or_else(|| format!("CodableColor: нет ключа {}", name))?;
            json::decode_f64(value)
        };
        let alpha = match value.get("alpha") {
            None | Some(Json::Null) => None,
            Some(alpha) => Some(json::decode_f64(alpha)?),
        };
        Ok(FootprintColor {
            red: component("red")?,
            green: component("green")?,
            blue: component("blue")?,
            alpha,
        })
    }
}

/// Приложения, которым по умолчанию нужен «системный» mouseDown
/// (`systemWideMouseDownApps`).
pub fn default_system_wide_mouse_down_apps() -> BTreeSet<String> {
    BTreeSet::from([
        "org.languagetool.desktop".to_string(),
        "com.microsoft.teams2".to_string(),
    ])
}

/// Приложения, у которых двойной клик по панели инструментов не трогаем
/// (`doubleClickToolBarIgnoredApps`, если ключ пуст).
pub fn default_double_click_tool_bar_ignored_apps() -> BTreeSet<String> {
    BTreeSet::from(["epp.package.java".to_string()])
}
