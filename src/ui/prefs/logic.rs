//! Логика окна настроек без AppKit: что показывать при каких настройках, как
//! контролы переводятся в настройки и обратно, числа в полях ввода. Всё здесь
//! проверяется обычными тестами; окна (`general`, `extras`) только вызывают.

use crate::actions::Action;
use crate::config::{is_swift_window_action_raw, Config, CycleSize, SubsequentExecutionMode};
use crate::geometry::Rect;

// ---------------------------------------------------------------- повторные команды

/// Пункты попапа «Повторяющиеся команды» в порядке storyboard; tag — rawValue режима.
pub const SUBSEQUENT_EXECUTION_ITEMS: [(&str, SubsequentExecutionMode); 6] = [
    ("ничего не делать", SubsequentExecutionMode::None),
    (
        "переключаться между дисплеями",
        SubsequentExecutionMode::CycleMonitor,
    ),
    (
        "переключаться между ½, ⅔ и ⅓ для половин",
        SubsequentExecutionMode::Resize,
    ),
    (
        "перейти к соседнему дисплею слева или справа",
        SubsequentExecutionMode::AcrossMonitor,
    ),
    (
        "перейти к соседнему дисплею слева/справа или переключать размер половин",
        SubsequentExecutionMode::AcrossAndResize,
    ),
    (
        "переключать углы по кругу и размер половин",
        SubsequentExecutionMode::ResizeAndCycleQuadrants,
    ),
];

// ---------------------------------------------------------------- что показывать

/// Блок размеров перебора (½ ⅔ ¾ ¼ ⅓ и ось углов) — при режимах, которые
/// перебирают размеры (`SubsequentExecutionMode.resizes`).
pub fn shows_cycle_sizes(config: &Config) -> bool {
    config.subsequent_execution_mode.resizes()
}

/// «Без промежутка у верхнего края» — только когда промежутки есть.
pub fn shows_skip_gap_top_edge(config: &Config) -> bool {
    config.gap_size != 0.0
}

/// Ширина и сторона Todo — только при включённом «Показывать Todo режим в меню».
pub fn shows_todo_block(config: &Config) -> bool {
    config.todo == Some(true)
}

/// «Определять экран по положению курсора» — скрытая настройка: флажок виден
/// только тем, у кого она уже включена (проверяется при создании окна и после
/// импорта, как `initializeToggles`).
pub fn shows_use_cursor_screen_detection(config: &Config) -> bool {
    config.use_cursor_screen_detection
}

/// «Менять размер соседних окон…» — фича скрыта в оригинале («Holding off on
/// showing…»): флажок добавляется, только если настройка уже включена при
/// создании окна.
pub fn shows_cooperative_corner_resize(config: &Config) -> bool {
    config.cooperative_corner_resize
}

/// «Считать несколько дисплеев одним» — только при выключенном системном
/// «Дисплеи с разными рабочими пространствами Spaces».
pub fn shows_combined_display_mode(screens_have_separate_spaces: bool) -> bool {
    !screens_have_separate_spaces
}

/// Блок Stage Manager — с macOS 13 (`StageUtil.stageCapable`).
pub fn shows_stage(macos_major: isize) -> bool {
    macos_major >= 13
}

// ---------------------------------------------------------------- флажки ↔ настройки

/// «Сохранять максимизацию…»: включено, пока настройку явно не выключили.
pub fn auto_maximize_on(config: &Config) -> bool {
    config.auto_maximize != Some(false)
}

/// Флажок двойного клика по заголовку включён, если в настройке записано
/// существующее действие (`WindowAction(rawValue: value - 1) != nil`).
pub fn double_click_title_bar_on(value: i64) -> bool {
    is_swift_window_action_raw(value - 1)
}

/// Значение `doubleClickTitleBar` для флажка: «максимизировать» (rawValue + 1)
/// или 0 — ничего.
pub fn double_click_title_bar_value(on: bool) -> i64 {
    if on {
        i64::from(Action::Maximize.raw()) + 1
    } else {
        0
    }
}

/// Системная настройка двойного клика по заголовку мешает Rectangle, если она
/// не «Ничего» (`AppleActionOnDoubleClick` ≠ "None").
pub fn double_click_conflicts_with_system(system_action: Option<&str>) -> bool {
    system_action != Some("None")
}

/// Флажок размера перебора: первое изменение сначала записывает набор по
/// умолчанию, потом правку (`didCheckCycleSizeCheckbox`).
pub fn toggle_cycle_size(config: &mut Config, size: CycleSize, on: bool) {
    if !config.cycle_sizes_is_changed {
        config.selected_cycle_sizes = crate::config::CycleSizes::default_sizes();
    }
    config.cycle_sizes_is_changed = true;
    if on {
        config.selected_cycle_sizes.insert(size);
    } else {
        config.selected_cycle_sizes.remove(size);
    }
}

// ---------------------------------------------------------------- ползунки

/// Положение ползунка для значения настройки: `Int32(value)` (отбрасывание
/// дробной части), зажатое в пределы ползунка.
pub fn slider_position(value: f32, max: i32) -> i32 {
    (value as i32).clamp(0, max)
}

/// Подпись справа от ползунка.
pub fn slider_label(position: i32) -> String {
    format!("{position} px")
}

/// Промежуток между окнами из положения ползунка: оригинал пишет целые пиксели.
pub fn gap_from_slider(position: i32) -> f32 {
    position as f32
}

/// Область Stage Manager из значения ползунка: 0 хранится как −1, иначе
/// сохранённый 0 читался бы как значение по умолчанию (190).
pub fn stage_size_from_slider(value: f32) -> f32 {
    if value == 0.0 {
        -1.0
    } else {
        value
    }
}

// ---------------------------------------------------------------- доли сторон

/// Пункт попапа доли сторон для значения: размер, если процент совпадает с
/// ним (±0,001), иначе «Другое» (`None`) — тогда рядом видно поле ввода.
pub fn half_split_preset(percent: f32) -> Option<CycleSize> {
    CycleSize::matching(percent)
}

// ---------------------------------------------------------------- числовые поля

/// Правила поля ввода — как `NumberFormatter` у полей оригинала.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NumberRules {
    /// Дробные значения разрешены (`allowsFloats`).
    pub allows_floats: bool,
    /// Сколько знаков после запятой показывать (`maximumFractionDigits`).
    pub max_fraction_digits: usize,
    /// Разряды разделяются (`numberStyle = .decimal`).
    pub grouping: bool,
    pub minimum: Option<f64>,
    pub maximum: Option<f64>,
}

/// Разделители чисел текущей локали (`NSLocale.current`).
#[derive(Clone, Debug, PartialEq)]
pub struct Separators {
    pub decimal: String,
    pub grouping: String,
}

impl Separators {
    /// Разделители `en_US`.
    #[cfg(test)]
    pub fn english() -> Separators {
        Separators {
            decimal: ".".to_string(),
            grouping: ",".to_string(),
        }
    }
}

/// Что введено в поле.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum NumberInput {
    /// Поле пустое: при окончании ввода подставляется запасное значение.
    Empty,
    Valid(f64),
    /// Не число или вне пределов: поле не отпускает фокус, как с форматтером.
    Invalid,
}

impl NumberRules {
    /// Разбор ввода. Кроме разделителей локали принимается и точка как
    /// десятичный разделитель, если в локали это запятая; пробелы между
    /// разрядами — любые (в `ru` разделитель — неразрывный пробел).
    pub fn parse(&self, text: &str, separators: &Separators) -> NumberInput {
        let text = text.trim();
        if text.is_empty() {
            return NumberInput::Empty;
        }
        let grouping_is_space = separators.grouping.chars().all(char::is_whitespace);
        let mut normalized = String::with_capacity(text.len());
        let mut rest = text;
        while let Some(ch) = rest.chars().next() {
            if !separators.decimal.is_empty() && rest.starts_with(separators.decimal.as_str()) {
                normalized.push('.');
                rest = &rest[separators.decimal.len()..];
                continue;
            }
            if self.grouping
                && !separators.grouping.is_empty()
                && rest.starts_with(separators.grouping.as_str())
            {
                rest = &rest[separators.grouping.len()..];
                continue;
            }
            if self.grouping && grouping_is_space && ch.is_whitespace() {
                rest = &rest[ch.len_utf8()..];
                continue;
            }
            if ch == '.' && separators.decimal != "." && separators.grouping != "." {
                normalized.push('.');
            } else if ch.is_ascii_digit() || ((ch == '-' || ch == '+') && normalized.is_empty()) {
                normalized.push(ch);
            } else {
                return NumberInput::Invalid;
            }
            rest = &rest[ch.len_utf8()..];
        }
        let digits = normalized.trim_start_matches(['-', '+']);
        if digits.is_empty() || digits == "." {
            return NumberInput::Invalid;
        }
        if !self.allows_floats && digits.contains('.') {
            return NumberInput::Invalid;
        }
        if digits.matches('.').count() > 1 {
            return NumberInput::Invalid;
        }
        let Ok(value) = normalized.parse::<f64>() else {
            return NumberInput::Invalid;
        };
        if !value.is_finite()
            || self.minimum.is_some_and(|minimum| value < minimum)
            || self.maximum.is_some_and(|maximum| value > maximum)
        {
            return NumberInput::Invalid;
        }
        NumberInput::Valid(value)
    }

    /// Текст числа, как его показывает форматтер: не больше
    /// `max_fraction_digits` знаков после запятой, без хвостовых нулей, с
    /// разделителями локали.
    pub fn format(&self, value: f64, separators: &Separators) -> String {
        let digits = if self.allows_floats {
            self.max_fraction_digits
        } else {
            0
        };
        let fixed = format!("{:.*}", digits, value.abs());
        let (integer, fraction) = fixed.split_once('.').unwrap_or((fixed.as_str(), ""));
        let fraction = fraction.trim_end_matches('0');
        let negative = value < 0.0 && (integer != "0" || !fraction.is_empty());

        let mut text = String::new();
        if negative {
            text.push('-');
        }
        if self.grouping {
            let length = integer.len();
            for (index, digit) in integer.chars().enumerate() {
                if index > 0 && (length - index) % 3 == 0 {
                    text.push_str(&separators.grouping);
                }
                text.push(digit);
            }
        } else {
            text.push_str(integer);
        }
        if !fraction.is_empty() {
            text.push_str(&separators.decimal);
            text.push_str(fraction);
        }
        text
    }
}

/// Целое как `String(Int(value))` у оригинала: дробная часть отбрасывается.
pub fn integer_text(value: f32) -> String {
    (value as i64).to_string()
}

/// Числовая настройка, которую правят полем ввода (`AutoSaveFloatField`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NumberSetting {
    /// «Ширина окна Todo приложения».
    TodoSidebarWidth,
    /// «Шаг ширины (px)» в поповере «Ещё».
    WidthStep,
    /// «По горизонтали (Л/П, %)».
    HorizontalSplitRatio,
    /// «По вертикали (В/Н, %)».
    VerticalSplitRatio,
}

impl NumberSetting {
    /// Форматтер поля.
    pub fn rules(self) -> NumberRules {
        match self {
            // NumberFormatter из storyboard: decimal, до 3 знаков после запятой.
            NumberSetting::TodoSidebarWidth => NumberRules {
                allows_floats: true,
                max_fraction_digits: 3,
                grouping: true,
                minimum: None,
                maximum: None,
            },
            // allowsFloats = false, minimum = 1.
            NumberSetting::WidthStep => NumberRules {
                allows_floats: false,
                max_fraction_digits: 0,
                grouping: false,
                minimum: Some(1.0),
                maximum: None,
            },
            // allowsFloats = false, 1…99.
            NumberSetting::HorizontalSplitRatio | NumberSetting::VerticalSplitRatio => {
                NumberRules {
                    allows_floats: false,
                    max_fraction_digits: 0,
                    grouping: false,
                    minimum: Some(1.0),
                    maximum: Some(99.0),
                }
            }
        }
    }

    /// Что подставить в опустевшее поле (`fallbackValue`: 30, у долей — 50).
    pub fn fallback(self) -> f32 {
        match self {
            NumberSetting::TodoSidebarWidth | NumberSetting::WidthStep => 30.0,
            NumberSetting::HorizontalSplitRatio | NumberSetting::VerticalSplitRatio => 50.0,
        }
    }

    pub fn get(self, config: &Config) -> f32 {
        match self {
            NumberSetting::TodoSidebarWidth => config.todo_sidebar_width,
            NumberSetting::WidthStep => config.width_step_size,
            NumberSetting::HorizontalSplitRatio => config.horizontal_split_ratio,
            NumberSetting::VerticalSplitRatio => config.vertical_split_ratio,
        }
    }

    pub fn set(self, config: &mut Config, value: f32) {
        match self {
            NumberSetting::TodoSidebarWidth => config.todo_sidebar_width = value,
            NumberSetting::WidthStep => config.width_step_size = value,
            NumberSetting::HorizontalSplitRatio => config.horizontal_split_ratio = value,
            NumberSetting::VerticalSplitRatio => config.vertical_split_ratio = value,
        }
    }

    /// Текст поля для значения настройки: ширина Todo — через форматтер, у
    /// целых полей — `String(Int(value))`.
    pub fn display(self, value: f32, separators: &Separators) -> String {
        match self {
            NumberSetting::TodoSidebarWidth => self.rules().format(f64::from(value), separators),
            _ => integer_text(value),
        }
    }
}

// ---------------------------------------------------------------- окно

/// Рамка окна под новую высоту содержимого: верхний край остаётся на месте,
/// окно не выше видимой области экрана и не вылезает за неё сверху и снизу
/// (`clampWindowToScreen` оригинала). `chrome` — заголовок с панелью вкладок.
pub fn fit_window_frame(
    frame: Rect,
    chrome: f64,
    width: f64,
    content_height: f64,
    visible: Option<Rect>,
) -> Rect {
    let mut height = content_height + chrome;
    if let Some(visible) = visible {
        height = height.min(visible.h);
    }
    let mut y = frame.max_y() - height;
    if let Some(visible) = visible {
        if y < visible.y {
            y = visible.y;
        }
        if y + height > visible.max_y() {
            y = visible.max_y() - height;
        }
    }
    Rect::new(frame.x, y, width, height)
}

// ---------------------------------------------------------------- прочее

/// Версия справа от «Запуск при входе в систему»: `v1.100 (106)`.
pub fn version_text(short_version: &str, build: Option<&str>) -> String {
    match build {
        Some(build) if !build.is_empty() => format!("v{short_version} ({build})"),
        _ => format!("v{short_version}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{CornerCycleExpansionAxis, CycleSizes};

    fn russian() -> Separators {
        Separators {
            decimal: ",".to_string(),
            grouping: "\u{a0}".to_string(),
        }
    }

    #[test]
    fn subsequent_items_follow_storyboard_order_and_tags() {
        let tags: Vec<i64> = SUBSEQUENT_EXECUTION_ITEMS
            .iter()
            .map(|(_, mode)| mode.raw())
            .collect();
        assert_eq!(tags, vec![2, 4, 0, 1, 3, 5]);
        // Все режимы на месте, каждый один раз.
        let mut sorted = tags.clone();
        sorted.sort();
        assert_eq!(sorted, vec![0, 1, 2, 3, 4, 5]);
    }

    #[test]
    fn cycle_sizes_block_follows_resizing_modes() {
        let mut config = Config::default();
        for (mode, shown) in [
            (SubsequentExecutionMode::Resize, true),
            (SubsequentExecutionMode::AcrossMonitor, false),
            (SubsequentExecutionMode::None, false),
            (SubsequentExecutionMode::AcrossAndResize, true),
            (SubsequentExecutionMode::CycleMonitor, false),
            (SubsequentExecutionMode::ResizeAndCycleQuadrants, true),
        ] {
            config.subsequent_execution_mode = mode;
            assert_eq!(shows_cycle_sizes(&config), shown, "{mode:?}");
        }
    }

    #[test]
    fn dependent_controls_visibility() {
        let mut config = Config::default();
        assert!(!shows_skip_gap_top_edge(&config));
        config.gap_size = 8.0;
        assert!(shows_skip_gap_top_edge(&config));

        assert!(!shows_todo_block(&config));
        config.todo = Some(false);
        assert!(!shows_todo_block(&config));
        config.todo = Some(true);
        assert!(shows_todo_block(&config));

        assert!(!shows_use_cursor_screen_detection(&config));
        config.use_cursor_screen_detection = true;
        assert!(shows_use_cursor_screen_detection(&config));

        assert!(!shows_cooperative_corner_resize(&config));
        config.cooperative_corner_resize = true;
        assert!(shows_cooperative_corner_resize(&config));

        assert!(shows_combined_display_mode(false));
        assert!(!shows_combined_display_mode(true));

        assert!(!shows_stage(12));
        assert!(shows_stage(13));
        assert!(shows_stage(27));
    }

    #[test]
    fn auto_maximize_is_on_unless_explicitly_off() {
        let mut config = Config::default();
        assert!(auto_maximize_on(&config));
        config.auto_maximize = Some(true);
        assert!(auto_maximize_on(&config));
        config.auto_maximize = Some(false);
        assert!(!auto_maximize_on(&config));
    }

    #[test]
    fn double_click_title_bar_maps_to_maximize() {
        assert_eq!(double_click_title_bar_value(true), 3);
        assert_eq!(double_click_title_bar_value(false), 0);
        assert!(double_click_title_bar_on(3));
        assert!(!double_click_title_bar_on(0));
        // Любое существующее действие считается включённым флажком.
        assert!(double_click_title_bar_on(1));
        // rawValue 6 и 7 в Swift не заняты.
        assert!(!double_click_title_bar_on(7));
        assert!(!double_click_title_bar_on(-5));
    }

    #[test]
    fn system_double_click_setting_conflicts_unless_none() {
        assert!(!double_click_conflicts_with_system(Some("None")));
        assert!(double_click_conflicts_with_system(Some("Maximize")));
        assert!(double_click_conflicts_with_system(Some("Minimize")));
        assert!(double_click_conflicts_with_system(None));
    }

    #[test]
    fn first_cycle_size_change_starts_from_defaults() {
        let mut config = Config::default();
        assert_eq!(config.effective_cycle_sizes(), CycleSizes::default_sizes());
        toggle_cycle_size(&mut config, CycleSize::ThreeQuarters, true);
        assert!(config.cycle_sizes_is_changed);
        assert_eq!(
            config.effective_cycle_sizes().sorted_sizes(),
            vec![
                CycleSize::OneHalf,
                CycleSize::TwoThirds,
                CycleSize::ThreeQuarters,
                CycleSize::OneThird
            ]
        );
        toggle_cycle_size(&mut config, CycleSize::OneHalf, false);
        assert_eq!(
            config.effective_cycle_sizes().sorted_sizes(),
            vec![
                CycleSize::TwoThirds,
                CycleSize::ThreeQuarters,
                CycleSize::OneThird
            ]
        );
        // Ось углов — отдельная настройка, флажки размеров её не трогают.
        assert_eq!(
            config.corner_cycle_expansion_axis,
            CornerCycleExpansionAxis::Horizontal
        );
    }

    #[test]
    fn sliders() {
        assert_eq!(slider_position(0.0, 100), 0);
        assert_eq!(slider_position(7.9, 100), 7);
        assert_eq!(slider_position(150.0, 100), 100);
        // Stage: −1 («не оставлять») показывается как 0.
        assert_eq!(slider_position(-1.0, 250), 0);
        assert_eq!(slider_label(190), "190 px");
        assert_eq!(gap_from_slider(12), 12.0);
        assert_eq!(stage_size_from_slider(0.0), -1.0);
        assert_eq!(stage_size_from_slider(187.5), 187.5);
    }

    #[test]
    fn half_split_presets() {
        assert_eq!(half_split_preset(50.0), Some(CycleSize::OneHalf));
        assert_eq!(
            half_split_preset(CycleSize::TwoThirds.percent_value()),
            Some(CycleSize::TwoThirds)
        );
        assert_eq!(half_split_preset(75.0), Some(CycleSize::ThreeQuarters));
        assert_eq!(half_split_preset(25.0), Some(CycleSize::OneQuarter));
        // Целые 67 и 33 — не ⅔ и ⅓: в попапе «Другое».
        assert_eq!(half_split_preset(67.0), None);
        assert_eq!(half_split_preset(33.0), None);
    }

    #[test]
    fn todo_width_field_formats_like_decimal_formatter() {
        let rules = NumberSetting::TodoSidebarWidth.rules();
        let en = Separators::english();
        assert_eq!(rules.format(400.0, &en), "400");
        assert_eq!(rules.format(1200.0, &en), "1,200");
        assert_eq!(rules.format(33.333332, &en), "33.333");
        assert_eq!(rules.format(12.5, &en), "12.5");
        assert_eq!(rules.format(0.0004, &en), "0");
        assert_eq!(rules.format(-0.0001, &en), "0");
        assert_eq!(rules.format(-5.25, &en), "-5.25");
        assert_eq!(rules.format(1_234_567.0, &en), "1,234,567");
        let ru = russian();
        assert_eq!(rules.format(1200.5, &ru), "1\u{a0}200,5");
        assert_eq!(NumberSetting::TodoSidebarWidth.display(400.0, &ru), "400");
    }

    #[test]
    fn todo_width_field_parses_locale_numbers() {
        let rules = NumberSetting::TodoSidebarWidth.rules();
        let ru = russian();
        assert_eq!(rules.parse("400", &ru), NumberInput::Valid(400.0));
        assert_eq!(rules.parse(" 12,5 ", &ru), NumberInput::Valid(12.5));
        // Точку тоже понимаем, раз в локали запятая.
        assert_eq!(rules.parse("12.5", &ru), NumberInput::Valid(12.5));
        assert_eq!(rules.parse("1 200", &ru), NumberInput::Valid(1200.0));
        assert_eq!(rules.parse("1\u{a0}200", &ru), NumberInput::Valid(1200.0));
        assert_eq!(rules.parse("", &ru), NumberInput::Empty);
        assert_eq!(rules.parse("   ", &ru), NumberInput::Empty);
        assert_eq!(rules.parse("abc", &ru), NumberInput::Invalid);
        assert_eq!(rules.parse("1,2,3", &ru), NumberInput::Invalid);
        assert_eq!(rules.parse(",", &ru), NumberInput::Invalid);
        assert_eq!(rules.parse("-", &ru), NumberInput::Invalid);
        let en = Separators::english();
        assert_eq!(rules.parse("1,200.25", &en), NumberInput::Valid(1200.25));
        assert_eq!(rules.parse("-7", &en), NumberInput::Valid(-7.0));
        assert_eq!(rules.parse("7-", &en), NumberInput::Invalid);
    }

    #[test]
    fn integer_fields_reject_floats_and_out_of_range() {
        let en = Separators::english();
        let step = NumberSetting::WidthStep.rules();
        assert_eq!(step.parse("30", &en), NumberInput::Valid(30.0));
        assert_eq!(step.parse("1", &en), NumberInput::Valid(1.0));
        assert_eq!(step.parse("0", &en), NumberInput::Invalid);
        assert_eq!(step.parse("2.5", &en), NumberInput::Invalid);
        assert_eq!(step.parse("1,000", &en), NumberInput::Invalid);
        assert_eq!(step.parse("5000", &en), NumberInput::Valid(5000.0));

        let ratio = NumberSetting::HorizontalSplitRatio.rules();
        assert_eq!(ratio.parse("99", &en), NumberInput::Valid(99.0));
        assert_eq!(ratio.parse("100", &en), NumberInput::Invalid);
        assert_eq!(ratio.parse("0", &en), NumberInput::Invalid);
        assert_eq!(ratio.parse("", &en), NumberInput::Empty);
    }

    #[test]
    fn integer_fields_display_truncated_values() {
        let en = Separators::english();
        assert_eq!(NumberSetting::WidthStep.display(30.0, &en), "30");
        assert_eq!(
            NumberSetting::HorizontalSplitRatio.display(CycleSize::TwoThirds.percent_value(), &en),
            "66"
        );
        assert_eq!(integer_text(49.9), "49");
    }

    #[test]
    fn number_settings_read_write_and_fallbacks() {
        let mut config = Config::default();
        for (setting, fallback) in [
            (NumberSetting::TodoSidebarWidth, 30.0),
            (NumberSetting::WidthStep, 30.0),
            (NumberSetting::HorizontalSplitRatio, 50.0),
            (NumberSetting::VerticalSplitRatio, 50.0),
        ] {
            assert_eq!(setting.fallback(), fallback);
            setting.set(&mut config, 42.0);
            assert_eq!(setting.get(&config), 42.0);
        }
        assert_eq!(config.todo_sidebar_width, 42.0);
        assert_eq!(config.width_step_size, 42.0);
        assert_eq!(config.horizontal_split_ratio, 42.0);
        assert_eq!(config.vertical_split_ratio, 42.0);
    }

    #[test]
    fn window_keeps_top_edge_and_fits_screen() {
        let visible = Rect::new(0.0, 70.0, 1728.0, 1010.0);
        // Растёт вниз от верхнего края.
        let frame = Rect::new(245.0, 500.0, 850.0, 400.0);
        let fitted = fit_window_frame(frame, 80.0, 850.0, 500.0, Some(visible));
        assert_eq!(fitted, Rect::new(245.0, 320.0, 850.0, 580.0));
        // Упёрлось в низ — поднимается.
        let fitted = fit_window_frame(frame, 80.0, 850.0, 800.0, Some(visible));
        assert_eq!(fitted, Rect::new(245.0, 70.0, 850.0, 880.0));
        // Выше экрана — высота по экрану, содержимое прокручивается.
        let fitted = fit_window_frame(frame, 80.0, 850.0, 2000.0, Some(visible));
        assert_eq!(fitted, Rect::new(245.0, 70.0, 850.0, 1010.0));
        // Верх выше видимой области — окно опускается.
        let high = Rect::new(245.0, 900.0, 850.0, 400.0);
        let fitted = fit_window_frame(high, 80.0, 850.0, 300.0, Some(visible));
        assert_eq!(fitted, Rect::new(245.0, 700.0, 850.0, 380.0));
        // Без экрана — только высота.
        let fitted = fit_window_frame(frame, 80.0, 850.0, 500.0, None);
        assert_eq!(fitted, Rect::new(245.0, 320.0, 850.0, 580.0));
    }

    #[test]
    fn version_label() {
        assert_eq!(version_text("1.100", Some("106")), "v1.100 (106)");
        assert_eq!(version_text("0.1.0", None), "v0.1.0");
        assert_eq!(version_text("0.1.0", Some("")), "v0.1.0");
    }
}
