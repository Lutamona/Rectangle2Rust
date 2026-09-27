//! Старый текстовый config.conf прошлых версий порта и его перенос в
//! UserDefaults. Разбор строк — тот же, что был у прежнего `config::load()`;
//! секция `[shortcuts]` пропускается: горячих клавиш в порте больше нет.

use std::collections::BTreeSet;
use std::path::PathBuf;

use super::{
    Config, CornerCycleExpansionAxis, CycleSize, CycleSizes, EdgeAlignment, ScreenOrdering,
    SubsequentExecutionMode,
};

/// Где лежал config.conf (как искал прежний порт: сначала `~/.config`).
/// `None` — файла нет.
pub(super) fn legacy_config_path() -> Option<PathBuf> {
    let home = std::env::var("HOME").ok()?;
    [
        PathBuf::from(&home).join(".config/rectangle2rust/config.conf"),
        PathBuf::from(&home).join("Library/Application Support/Rectangle2Rust/config.conf"),
    ]
    .into_iter()
    .find(|path| path.is_file())
}

fn parse_bool(value: &str) -> bool {
    matches!(value, "true" | "yes" | "1" | "on")
}

fn parse_f32(value: &str) -> Option<f32> {
    value
        .parse::<f32>()
        .ok()
        .filter(|number| number.is_finite())
}

/// OptionalBool из старого флага: совпадает с поведением «не задано» — не задано.
fn optional_bool(value: bool, when_not_set: bool) -> Option<bool> {
    (value != when_not_set).then_some(value)
}

/// Float, у которого 0 в оригинале значит «подставить `fallback` в расчёте»:
/// значение, равное подстановке, переносится как 0.
fn float_with_fallback(value: f32, fallback: f32) -> f32 {
    if value == fallback {
        0.0
    } else {
        value
    }
}

/// Размеры перебора из списка долей (`0.5,0.6666667,0.3333333`). Доли,
/// которых нет среди CycleSize, пропускаются; порядок задаёт оригинал.
fn apply_cycle_sizes(config: &mut Config, value: &str) {
    let sizes: CycleSizes = value
        .split(',')
        .filter_map(|part| parse_f32(part.trim()))
        .filter_map(|fraction| CycleSize::matching(fraction * 100.0))
        .collect();
    if sizes.is_empty() {
        return;
    }
    if sizes == CycleSizes::default_sizes() {
        config.cycle_sizes_is_changed = false;
        config.selected_cycle_sizes = CycleSizes::EMPTY;
    } else {
        config.cycle_sizes_is_changed = true;
        config.selected_cycle_sizes = sizes;
    }
}

fn apply_setting(
    config: &mut Config,
    key: &str,
    value: &str,
    disabled_apps: &mut BTreeSet<String>,
) {
    let number = || parse_f32(value);
    match key {
        "gap_size" => config.gap_size = number().unwrap_or(config.gap_size),
        "skip_gap_top_edge" => config.skip_gap_top_edge = parse_bool(value),
        "screen_edge_gap_top" => config.screen_edge_gap_top = number().unwrap_or(0.0),
        "screen_edge_gap_bottom" => config.screen_edge_gap_bottom = number().unwrap_or(0.0),
        "screen_edge_gap_left" => config.screen_edge_gap_left = number().unwrap_or(0.0),
        "screen_edge_gap_right" => config.screen_edge_gap_right = number().unwrap_or(0.0),
        "screen_edge_gaps_main_only" => {
            config.screen_edge_gaps_on_main_screen_only = parse_bool(value)
        }
        "apply_gaps_to_maximize" => {
            config.apply_gaps_to_maximize = optional_bool(parse_bool(value), true)
        }
        "apply_gaps_to_maximize_height" => {
            config.apply_gaps_to_maximize_height = optional_bool(parse_bool(value), true)
        }
        "subsequent_execution" => {
            let mode = match value {
                "resize" => Some(SubsequentExecutionMode::Resize),
                "none" => Some(SubsequentExecutionMode::None),
                "across_monitor" => Some(SubsequentExecutionMode::AcrossMonitor),
                "across_and_resize" => Some(SubsequentExecutionMode::AcrossAndResize),
                "cycle_monitor" => Some(SubsequentExecutionMode::CycleMonitor),
                "resize_and_cycle_quadrants" => {
                    Some(SubsequentExecutionMode::ResizeAndCycleQuadrants)
                }
                _ => None,
            };
            if let Some(mode) = mode {
                config.subsequent_execution_mode = mode;
            }
        }
        "cycle_sizes" => apply_cycle_sizes(config, value),
        "corner_cycle_axis" => {
            config.corner_cycle_expansion_axis = match value {
                "vertical" => CornerCycleExpansionAxis::Vertical,
                _ => CornerCycleExpansionAxis::Horizontal,
            }
        }
        // В старом конфиге доли задавались процентами — как horizontalSplitRatio.
        "horizontal_ratio" => {
            if let Some(percent) = number() {
                config.horizontal_split_ratio = percent;
            }
        }
        "vertical_ratio" => {
            if let Some(percent) = number() {
                config.vertical_split_ratio = percent;
            }
        }
        "resize_on_directional_move" => config.resize_on_directional_move = parse_bool(value),
        "centered_directional_move" => {
            config.centered_directional_move = optional_bool(parse_bool(value), true)
        }
        "size_offset" => {
            if let Some(size) = number() {
                config.size_offset = float_with_fallback(size, 30.0);
            }
        }
        "width_step_size" => config.width_step_size = number().unwrap_or(config.width_step_size),
        "curtain_change_size" => {
            config.curtain_change_size = optional_bool(parse_bool(value), true)
        }
        "smaller_shrinks_maximized_height" => {
            config.smaller_shrinks_maximized_height = parse_bool(value)
        }
        "almost_maximize_width" => {
            if let Some(fraction) = number() {
                config.almost_maximize_width = float_with_fallback(fraction, 0.9);
            }
        }
        "almost_maximize_height" => {
            if let Some(fraction) = number() {
                config.almost_maximize_height = float_with_fallback(fraction, 0.9);
            }
        }
        "auto_maximize" => config.auto_maximize = optional_bool(parse_bool(value), true),
        "specified_width" => config.specified_width = number().unwrap_or(config.specified_width),
        "specified_height" => config.specified_height = number().unwrap_or(config.specified_height),
        "screen_ordering" => {
            config.screens_ordered_by_x = match value {
                "mid_x" => ScreenOrdering::MidX,
                "min_x" => ScreenOrdering::MinX,
                _ => ScreenOrdering::YThenMinX,
            }
        }
        "move_cursor" => config.move_cursor = optional_bool(parse_bool(value), false),
        "move_cursor_across_displays" => {
            config.move_cursor_across_displays = optional_bool(parse_bool(value), false)
        }
        "move_fixed_size_to_edge" => {
            config.move_fixed_size_to_edge = match value {
                "corners" => EdgeAlignment::Corners,
                "centered" => EdgeAlignment::Centered,
                _ => EdgeAlignment::EdgesAndCorners,
            }
        }
        "access_prompted" => config.access_prompted = parse_bool(value),
        "ignored_app" => {
            let bundle_id = value.trim();
            if !bundle_id.is_empty() {
                disabled_apps.insert(bundle_id.to_string());
            }
        }
        _ => {}
    }
}

/// Перенос настроек из текста старого config.conf поверх `base`.
///
/// Значения, равные прежнему поведению по умолчанию, не записываются
/// (OptionalBool остаётся «не задано», `size_offset`/`almost_maximize_*` — 0).
/// Секция `[shortcuts]` пропускается, кроме строк `ignored_app` и
/// `access_prompted`: прежний `save()` дописывал их в конец файла — то есть в
/// эту секцию, где старый разбор их уже не видел.
pub fn migrate_legacy_text(text: &str, base: &Config) -> Config {
    let mut config = base.clone();
    let mut disabled_apps = BTreeSet::new();

    let mut in_shortcuts = false;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with("//") {
            continue;
        }
        if line.starts_with('[') {
            in_shortcuts = line.contains("shortcuts");
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim();

        if in_shortcuts && !matches!(key, "ignored_app" | "access_prompted") {
            continue;
        }
        apply_setting(&mut config, key, value, &mut disabled_apps);
    }

    if !disabled_apps.is_empty() {
        let mut apps = config.disabled_apps.take().unwrap_or_default();
        apps.extend(disabled_apps);
        config.disabled_apps = Some(apps);
    }
    config
}
