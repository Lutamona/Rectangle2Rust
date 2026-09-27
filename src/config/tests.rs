use std::cell::Cell;
use std::collections::BTreeSet;
use std::rc::Rc;

use super::*;
use crate::defaults_store::{export_json, import_json, MemoryStore, Store, Value};

// ---------------------------------------------------------------- значения по умолчанию

#[test]
fn defaults_match_swift() {
    let config = Config::default();
    // Явные значения из Defaults.swift.
    assert_eq!(config.snap_edge_margin_top, 5.0);
    assert_eq!(config.snap_edge_margin_right, 5.0);
    assert_eq!(config.width_step_size, 30.0);
    assert_eq!(config.size_offset, 0.0);
    assert_eq!(config.minimum_window_width, 0.25);
    assert_eq!(config.minimum_window_height, 0.25);
    assert_eq!(config.footprint_alpha, 0.3);
    assert_eq!(config.footprint_border_width, 2.0);
    assert_eq!(config.todo_sidebar_width, 400.0);
    assert_eq!(config.todo_sidebar_width_unit, TodoSidebarWidthUnit::Pixels);
    assert_eq!(config.todo_sidebar_side, TodoSidebarSide::Right);
    assert_eq!(config.cycling_overlap_offset_size, 11.0);
    assert_eq!(config.cycling_overlap_max_cascade, 1);
    assert_eq!(config.specified_height, 1050.0);
    assert_eq!(config.specified_width, 1680.0);
    assert_eq!(config.horizontal_split_ratio, 50.0);
    assert_eq!(config.vertical_split_ratio, 50.0);
    assert_eq!(config.corner_snap_area_size, 20.0);
    assert_eq!(config.short_edge_snap_area_size, 145.0);
    assert_eq!(config.cascade_all_delta_size, 30.0);
    assert_eq!(config.stage_size, 190.0);
    assert_eq!(config.enhanced_ui, EnhancedUI::DisableEnable);
    assert_eq!(config.footprint_animation_duration_multiplier, 0.0);
    assert_eq!(
        config.mission_control_dragging_allowed_offscreen_distance,
        25.0
    );
    assert_eq!(config.mission_control_dragging_disallowed_duration, 250);
    assert_eq!(config.screens_ordered_by_x, ScreenOrdering::YThenMinX);
    assert_eq!(
        config.move_fixed_size_to_edge,
        EdgeAlignment::EdgesAndCorners
    );
    assert_eq!(
        config.corner_cycle_expansion_axis,
        CornerCycleExpansionAxis::Horizontal
    );
    assert_eq!(
        config.subsequent_execution_mode,
        SubsequentExecutionMode::Resize
    );
    assert_eq!(config.selected_cycle_sizes, CycleSizes::EMPTY);
    assert_eq!(config.window_snapping, None);
    assert_eq!(config.disabled_apps, None);
    assert_eq!(
        config.system_wide_mouse_down_apps,
        BTreeSet::from([
            "org.languagetool.desktop".to_string(),
            "com.microsoft.teams2".to_string()
        ])
    );
    assert_eq!(
        config.double_click_tool_bar_ignored_apps(),
        BTreeSet::from(["epp.package.java".to_string()])
    );
    assert_eq!(config.effective_cycle_sizes(), CycleSizes::default_sizes());
    assert_eq!(
        config.landscape_snap_areas_or_default(),
        default_landscape_snap_areas()
    );
}

#[test]
fn default_equals_load_from_empty_store() {
    assert_eq!(Config::load_from(&MemoryStore::new()), Config::default());
}

#[test]
fn settings_table_has_unique_keys_and_fields() {
    let keys: BTreeSet<&str> = SETTINGS.iter().map(|setting| setting.key).collect();
    let fields: BTreeSet<&str> = SETTINGS.iter().map(|setting| setting.field).collect();
    assert_eq!(keys.len(), SETTINGS.len());
    assert_eq!(fields.len(), SETTINGS.len());
    // Defaults.array — 94 уникальных ключа (showAdditionalSizesInMenu там дважды),
    // без alternateDefaultShortcuts и allowAnyShortcut — горячих клавиш в порте нет.
    assert_eq!(
        SETTINGS.iter().filter(|setting| setting.exported).count(),
        92
    );
    assert!(!keys.contains("alternateDefaultShortcuts") && !keys.contains("allowAnyShortcut"));
}

// ---------------------------------------------------------------- чтение и запись

#[test]
fn optional_bool_is_stored_as_0_1_2() {
    let mut config = Config::default();
    let mut store = MemoryStore::new();

    let mut changed = config.clone();
    changed.window_snapping = Some(false);
    changed.move_cursor = Some(true);
    Config::save_changes(&config, &changed, &mut store);
    assert_eq!(store.get("windowSnapping"), Some(&Value::Integer(2)));
    assert_eq!(store.get("moveCursor"), Some(&Value::Integer(1)));
    config = changed.clone();

    changed.window_snapping = None;
    Config::save_changes(&config, &changed, &mut store);
    // nil оригинал пишет нулём, а не удаляет ключ.
    assert_eq!(store.get("windowSnapping"), Some(&Value::Integer(0)));

    // Чтение: 3 и прочие числа — «не задано».
    store.set_integer("footprintFade", 3);
    store.set_integer("autoMaximize", 2);
    let loaded = Config::load_from(&store);
    assert_eq!(loaded.window_snapping, None);
    assert_eq!(loaded.move_cursor, Some(true));
    assert_eq!(loaded.footprint_fade, None);
    assert_eq!(loaded.auto_maximize, Some(false));
}

#[test]
fn float_zero_reads_as_default_and_floats_stay_f32() {
    let mut store = MemoryStore::new();
    store.set_float("snapEdgeMarginTop", 0.0);
    store.set_float("gapSize", 0.0);
    store.set_float("stageSize", -1.0);
    store.set_float("footprintAlpha", 0.45);
    store.insert("widthStepSize", Value::String("12.5".to_string()));
    store.set_integer("cyclingOverlapMaxCascade", 0);
    store.set_double("minimumWindowWidth", 0.0);
    let config = Config::load_from(&store);
    assert_eq!(config.snap_edge_margin_top, 5.0);
    assert_eq!(config.gap_size, 0.0);
    assert_eq!(config.stage_size, -1.0);
    assert_eq!(config.footprint_alpha, 0.45_f32);
    assert_eq!(config.width_step_size, 12.5);
    assert_eq!(config.cycling_overlap_max_cascade, 1);
    // DoubleDefault: 0 сохранённый — это 0, а не значение по умолчанию.
    assert_eq!(config.minimum_window_width, 0.0);

    let mut changed = config.clone();
    changed.gap_size = 7.5;
    Config::save_changes(&config, &changed, &mut store);
    assert_eq!(store.get("gapSize"), Some(&Value::Float(7.5)));
}

#[test]
fn nan_and_infinity_read_as_default() {
    let mut store = MemoryStore::new();
    store.set_float("gapSize", f32::NAN);
    store.set_float("snapEdgeMarginTop", f32::INFINITY);
    store.set_float("footprintAlpha", f32::NEG_INFINITY);
    // Строка и Double, которые во Float не помещаются, — тоже бесконечность.
    store.insert("widthStepSize", Value::String("1e999".to_string()));
    store.insert("todoSidebarWidth", Value::Double(1e300));
    store.set_double("minimumWindowWidth", f64::NAN);
    store.set_double("minimumWindowHeight", f64::INFINITY);
    let config = Config::load_from(&store);
    assert_eq!(config, Config::default());
    assert_eq!(config.snap_edge_margin_top, 5.0);
    assert_eq!(config.todo_sidebar_width, 400.0);
    assert_eq!(config.minimum_window_height, 0.25);

    // Конечные значения читаются как раньше.
    store.set_float("gapSize", 12.0);
    store.set_double("minimumWindowWidth", 0.5);
    let config = Config::load_from(&store);
    assert_eq!(config.gap_size, 12.0);
    assert_eq!(config.minimum_window_width, 0.5);
}

#[test]
fn enums_read_unknown_raw_as_default() {
    let mut store = MemoryStore::new();
    store.set_integer("moveFixedSizeToEdge", 0);
    store.set_integer("screensOrderedByX", 2);
    store.set_integer("subsequentExecutionMode", 9);
    store.set_integer("enhancedUI", 3);
    store.set_integer("todoSidebarSide", 7);
    let config = Config::load_from(&store);
    assert_eq!(
        config.move_fixed_size_to_edge,
        EdgeAlignment::EdgesAndCorners
    );
    assert_eq!(config.screens_ordered_by_x, ScreenOrdering::MinX);
    assert_eq!(
        config.subsequent_execution_mode,
        SubsequentExecutionMode::Resize
    );
    assert_eq!(config.enhanced_ui, EnhancedUI::FrontmostDisable);
    assert_eq!(config.todo_sidebar_side, TodoSidebarSide::Right);

    let mut changed = config.clone();
    changed.move_fixed_size_to_edge = EdgeAlignment::Centered;
    changed.subsequent_execution_mode = SubsequentExecutionMode::AcrossMonitor;
    Config::save_changes(&config, &changed, &mut store);
    assert_eq!(store.get("moveFixedSizeToEdge"), Some(&Value::Integer(3)));
    assert_eq!(
        store.get("subsequentExecutionMode"),
        Some(&Value::Integer(1))
    );
}

#[test]
fn cycle_sizes_are_bits_by_raw_value() {
    let sizes: CycleSizes = [CycleSize::OneHalf, CycleSize::OneQuarter]
        .into_iter()
        .collect();
    assert_eq!(sizes.bits(), 0b01010);
    assert_eq!(CycleSizes::default_sizes().bits(), 0b00111);
    assert_eq!(CycleSizes::from_bits(-1).bits(), 0b11111);
    assert_eq!(CycleSizes::from_bits(0b100000).bits(), 0);
    assert_eq!(
        CycleSizes::from_bits(0b11111).sorted_sizes(),
        vec![
            CycleSize::OneHalf,
            CycleSize::TwoThirds,
            CycleSize::ThreeQuarters,
            CycleSize::OneQuarter,
            CycleSize::OneThird
        ]
    );
    assert_eq!(CycleSize::TwoThirds.fraction(), 2.0_f32 / 3.0);
    assert_eq!(CycleSize::matching(66.66667), Some(CycleSize::TwoThirds));
    assert_eq!(CycleSize::matching(60.0), None);

    let config = Config::default();
    let mut changed = config.clone();
    changed.cycle_sizes_is_changed = true;
    changed.selected_cycle_sizes = sizes;
    let mut store = MemoryStore::new();
    Config::save_changes(&config, &changed, &mut store);
    assert_eq!(
        store.get("selectedCycleSizes"),
        Some(&Value::Integer(0b01010))
    );
    assert_eq!(store.get("cycleSizesIsChanged"), Some(&Value::Bool(true)));
    let loaded = Config::load_from(&store);
    assert_eq!(loaded.effective_cycle_sizes(), sizes);
}

#[test]
fn json_settings_are_compact_sorted_strings() {
    let config = Config::default();
    let mut changed = config.clone();
    changed.disabled_apps = Some(BTreeSet::from(["com.b".to_string(), "com.a".to_string()]));
    changed.footprint_color = Some(FootprintColor {
        red: 0.5,
        green: 0.1,
        blue: 1.0,
        alpha: None,
    });
    let mut areas = default_landscape_snap_areas();
    areas.insert(
        Directional::B,
        SnapAreaConfig::compound(CompoundSnapArea::BottomSixths),
    );
    changed.landscape_snap_areas = Some(areas);
    let mut store = MemoryStore::new();
    Config::save_changes(&config, &changed, &mut store);

    assert_eq!(
        store.string("disabledApps").as_deref(),
        Some("[\"com.a\",\"com.b\"]")
    );
    assert_eq!(
        store.string("footprintColor").as_deref(),
        Some("{\"blue\":1,\"green\":0.1,\"red\":0.5}")
    );
    assert_eq!(
        store.string("landscapeSnapAreas").as_deref(),
        Some(
            "[1,{\"action\":15},2,{\"action\":2},3,{\"action\":16},4,{\"compound\":-2},\
             5,{\"compound\":-3},6,{\"action\":13},7,{\"compound\":-8},8,{\"action\":14}]"
        )
    );
    assert_eq!(Config::load_from(&store), changed);

    // Сброс в nil JSONEncoder пишет строкой "null"; её, как и мусор, читаем как «не задано».
    let mut reset = changed.clone();
    reset.landscape_snap_areas = None;
    Config::save_changes(&changed, &reset, &mut store);
    assert_eq!(store.string("landscapeSnapAreas").as_deref(), Some("null"));
    store.set_string("portraitSnapAreas", Some("[10,{\"action\":15}]"));
    store.set_string("fullIgnoreBundleIds", Some("{oops"));
    let loaded = Config::load_from(&store);
    assert_eq!(loaded.landscape_snap_areas, None);
    assert_eq!(loaded.portrait_snap_areas, None);
    assert_eq!(loaded.full_ignore_bundle_ids, None);
}

#[test]
fn only_changed_keys_are_written() {
    let config = Config::default();
    let mut store = MemoryStore::new();
    Config::save_changes(&config, &config.clone(), &mut store);
    assert!(store.is_empty());

    let mut changed = config.clone();
    changed.gap_size = 10.0;
    changed.todo_application = Some("com.apple.reminders".to_string());
    Config::save_changes(&config, &changed, &mut store);
    assert_eq!(
        store.keys(),
        vec!["gapSize".to_string(), "todoApplication".to_string()]
    );

    // String nil удаляет ключ.
    let mut cleared = changed.clone();
    cleared.todo_application = None;
    Config::save_changes(&changed, &cleared, &mut store);
    assert_eq!(store.keys(), vec!["gapSize".to_string()]);
}

#[test]
fn save_then_load_round_trips() {
    let config = Config::default();
    let mut changed = config.clone();
    changed.launch_on_login = true;
    changed.gap_size = 12.0;
    changed.almost_maximize_width = 0.8;
    changed.minimum_window_height = 0.1;
    changed.curtain_change_size = Some(false);
    changed.cycling_overlap_max_cascade = 4;
    changed.double_click_title_bar = 3;
    changed.todo_sidebar_width_unit = TodoSidebarWidthUnit::Pct;
    changed.screens_ordered_by_x = ScreenOrdering::MidX;
    changed.system_wide_mouse_down_apps = BTreeSet::from(["com.example".to_string()]);
    changed.double_click_title_bar_ignored_apps =
        Some(vec!["b".to_string(), "a".to_string(), "b".to_string()]);
    changed.install_version = Some("100".to_string());
    changed.access_prompted = true;

    let mut store = MemoryStore::new();
    Config::save_changes(&config, &changed, &mut store);
    assert_eq!(Config::load_from(&store), changed);
    assert_eq!(store.get("r2AccessPrompted"), Some(&Value::Bool(true)));
}

// ---------------------------------------------------------------- экспорт и импорт

#[test]
fn export_uses_json_encoder_layout() {
    let json = export_json(&Config::default(), "106");
    assert!(json.starts_with(
        "{\n  \"bundleId\" : \"com.knollsoft.Rectangle\",\n  \"defaults\" : {\n    \"SUEnableAutomaticChecks\" : {\n      \"bool\" : false\n    },"
    ));
    assert!(json.contains("\n    \"disabledApps\" : {\n\n    },"));
    assert!(json.contains("\n    \"footprintAlpha\" : {\n      \"float\" : 0.3\n    },"));
    assert!(json.contains("\n    \"minimumWindowWidth\" : {\n      \"double\" : 0.25\n    },"));
    assert!(json.contains("\n    \"windowSnapping\" : {\n      \"int\" : 0\n    }\n  },"));
    // Горячих клавиш нет: shortcuts — пустой объект.
    assert!(json.ends_with("\n  },\n  \"shortcuts\" : {\n\n  },\n  \"version\" : \"106\"\n}"));
    // Настройки не из Defaults.array в экспорт не попадают.
    assert!(!json.contains("lastVersion") && !json.contains("r2AccessPrompted"));
    assert!(!json.contains("alternateDefaultShortcuts") && !json.contains("allowAnyShortcut"));
}

#[test]
fn export_then_import_gives_same_config() {
    let config = Config {
        gap_size: 8.0,
        almost_maximize_height: 0.85,
        minimum_window_width: 0.3,
        move_cursor: Some(true),
        todo: Some(false),
        subsequent_execution_mode: SubsequentExecutionMode::CycleMonitor,
        cycle_sizes_is_changed: true,
        selected_cycle_sizes: CycleSizes::from_bits(0b11001),
        todo_application: Some("com.culturedcode.ThingsMac".to_string()),
        disabled_apps: Some(BTreeSet::from(["com.apple.Terminal".to_string()])),
        footprint_color: Some(FootprintColor {
            red: 0.2,
            green: 0.30000000000000004,
            blue: 1.0 / 3.0,
            alpha: Some(0.5),
        }),
        portrait_snap_areas: Some(default_portrait_snap_areas()),
        system_wide_mouse_down_apps: BTreeSet::new(),
        ..Config::default()
    };

    let json = export_json(&config, "106");
    let imported = import_json(&json, &Config::default()).expect("импорт своего экспорта");
    assert_eq!(imported, config);
}

#[test]
fn export_writes_defaults_instead_of_nan_and_infinity() {
    let config = Config {
        gap_size: f32::NAN,
        todo_sidebar_width: f32::INFINITY,
        stage_size: f32::NEG_INFINITY,
        minimum_window_width: f64::NAN,
        minimum_window_height: f64::NEG_INFINITY,
        ..Config::default()
    };
    let json = export_json(&config, "106");
    assert!(json.contains("\n    \"gapSize\" : {\n      \"float\" : 0\n    },"));
    assert!(json.contains("\n    \"todoSidebarWidth\" : {\n      \"float\" : 400\n    },"));
    assert!(json.contains("\n    \"minimumWindowWidth\" : {\n      \"double\" : 0.25\n    },"));
    // JSON валидный: `nan`/`inf` импорт не разобрал бы.
    let imported = import_json(&json, &config).expect("импорт своего экспорта");
    assert_eq!(imported, Config::default());
}

#[test]
fn import_rejects_what_swift_rejects() {
    let config = Config::default();
    let huge = " ".repeat(crate::defaults_store::MAX_IMPORT_SIZE + 1);
    assert!(import_json(&huge, &config).is_err());
    assert!(import_json("{}", &config).is_err());
    assert!(import_json("[1]", &config).is_err());
    assert!(import_json(
        "{\"bundleId\":\"x\",\"version\":\"1\",\"shortcuts\":{},\"defaults\":{\"gapSize\":{\"int\":true}}}",
        &config
    )
    .is_err());
}

#[test]
fn imports_real_rectangle_config() {
    // Файл в формате RectangleConfig.json оригинала: шорткаты (их порт пропускает,
    // даже битые), настройки горячих клавиш и неизвестная настройка.
    let json = r#"{
      "bundleId" : "com.knollsoft.Rectangle",
      "defaults" : {
        "alternateDefaultShortcuts" : { "bool" : true },
        "allowAnyShortcut" : { "bool" : true },
        "gapSize" : { "float" : 10 },
        "almostMaximizeWidth" : { "float" : 0.75 },
        "minimumWindowHeight" : { "float" : 0 },
        "subsequentExecutionMode" : { "int" : 7 },
        "moveFixedSizeToEdge" : { "int" : 42 },
        "applyGapsToMaximize" : { "int" : 2 },
        "footprintFade" : { "int" : 9 },
        "selectedCycleSizes" : { "int" : 26 },
        "todoApplication" : { },
        "disabledApps" : { "string" : "[\"com.b\",\"com.a\",\"com.b\"]" },
        "landscapeSnapAreas" : { "string" : "[7,{\"compound\":-4},1,{\"action\":15}]" },
        "someFutureSetting" : { "bool" : true }
      },
      "shortcuts" : {
        "leftSide" : { "keyCode" : 4, "modifierFlags" : 1966080 },
        "maximize" : { "keyCode" : -1, "modifierFlags" : -5 },
        "broken" : 5
      },
      "version" : "92"
    }"#;
    let current = Config {
        subsequent_execution_mode: SubsequentExecutionMode::AcrossMonitor,
        footprint_fade: Some(true),
        todo_application: Some("com.old".to_string()),
        move_fixed_size_to_edge: EdgeAlignment::Corners,
        ..Config::default()
    };

    let config = import_json(json, &current).expect("импорт");
    assert_eq!(config.gap_size, 10.0);
    assert_eq!(config.almost_maximize_width, 0.75);
    // Старые экспорты хранили Double-настройки во Float: 0 — значение по умолчанию.
    assert_eq!(config.minimum_window_height, 0.25);
    // Неизвестный режим повторов пропускается, неизвестный IntEnum — по умолчанию.
    assert_eq!(
        config.subsequent_execution_mode,
        SubsequentExecutionMode::AcrossMonitor
    );
    assert_eq!(
        config.move_fixed_size_to_edge,
        EdgeAlignment::EdgesAndCorners
    );
    assert_eq!(config.apply_gaps_to_maximize, Some(false));
    assert_eq!(config.footprint_fade, Some(true));
    assert_eq!(config.selected_cycle_sizes.bits(), 26);
    assert_eq!(config.todo_application, None);
    assert_eq!(
        config.disabled_apps,
        Some(BTreeSet::from(["com.a".to_string(), "com.b".to_string()]))
    );
    assert_eq!(
        config
            .landscape_snap_areas
            .as_ref()
            .map(|areas| areas.len()),
        Some(2)
    );
}

// ---------------------------------------------------------------- старт и миграция

const LEGACY_TEXT: &str = "# Rectangle 2 (Rust) — настройки
gap_size = 6
screen_edge_gap_top = 4
subsequent_execution = none
cycle_sizes = 0.5,0.75
corner_cycle_axis = vertical
horizontal_ratio = 60
centered_directional_move = true
size_offset = 30
almost_maximize_width = 0.9
almost_maximize_height = 0.8
auto_maximize = false
move_cursor = true
screen_ordering = min_x
move_fixed_size_to_edge = centered
ignored_app = com.apple.Terminal

[shortcuts]
left_half = ctrl+alt+shift+h
maximize = none
gap_size = 99
access_prompted = true
ignored_app = com.googlecode.iterm2
";

#[test]
fn legacy_text_migrates_to_fields() {
    let base = Config::default();
    let config = migrate_legacy_text(LEGACY_TEXT, &base);

    assert_eq!(config.gap_size, 6.0);
    assert_eq!(config.screen_edge_gap_top, 4.0);
    assert_eq!(
        config.subsequent_execution_mode,
        SubsequentExecutionMode::None
    );
    assert!(config.cycle_sizes_is_changed);
    assert_eq!(
        config.selected_cycle_sizes.sorted_sizes(),
        vec![CycleSize::OneHalf, CycleSize::ThreeQuarters]
    );
    assert_eq!(
        config.corner_cycle_expansion_axis,
        CornerCycleExpansionAxis::Vertical
    );
    assert_eq!(config.horizontal_split_ratio, 60.0);
    // Значения, равные прежнему поведению по умолчанию, не записываются.
    assert_eq!(config.centered_directional_move, None);
    assert_eq!(config.size_offset, 0.0);
    assert_eq!(config.almost_maximize_width, 0.0);
    assert_eq!(config.almost_maximize_height, 0.8);
    assert_eq!(config.auto_maximize, Some(false));
    assert_eq!(config.move_cursor, Some(true));
    assert_eq!(config.screens_ordered_by_x, ScreenOrdering::MinX);
    assert_eq!(config.move_fixed_size_to_edge, EdgeAlignment::Centered);
    // Эти две строки прежний save() дописывал в секцию [shortcuts] — переносим и оттуда;
    // остальное из [shortcuts] (шорткаты и прочее) пропускается.
    assert!(config.access_prompted);
    assert_eq!(
        config.disabled_apps,
        Some(BTreeSet::from([
            "com.apple.Terminal".to_string(),
            "com.googlecode.iterm2".to_string()
        ]))
    );
}

#[test]
fn default_legacy_file_writes_nothing() {
    let text =
        "gap_size = 0\nsubsequent_execution = resize\ncycle_sizes = 0.5,0.6666667,0.3333333\n\
                horizontal_ratio = 50\ncentered_directional_move = true\nsize_offset = 30\n\
                width_step_size = 30\nalmost_maximize_width = 0.9\nauto_maximize = true\n\
                move_cursor = false\nmove_cursor_across_displays = false\n[shortcuts]\n\
                # left_half = ⌥⌘←\nleft_half = ctrl+alt+left\n";
    let base = Config::default();
    let migrated = migrate_legacy_text(text, &base);
    assert_eq!(migrated, base);
}

#[test]
fn startup_migrates_once() {
    let mut store = MemoryStore::new();
    let (config, migrated) = startup(&mut store, Some(LEGACY_TEXT));
    assert!(migrated);
    assert!(store.bool(LEGACY_MIGRATED_KEY));
    assert_eq!(config.gap_size, 6.0);
    assert!(config.access_prompted);
    // Шорткаты не переносятся и в UserDefaults не пишутся.
    assert!(!store.contains("leftHalf") && !store.contains("maximize"));

    // Второй запуск: переноса нет, настройки те же.
    let (again, migrated) = startup(&mut store, Some("gap_size = 99"));
    assert!(!migrated);
    assert_eq!(again, config);
}

#[test]
fn startup_without_legacy_file_writes_nothing() {
    let mut store = MemoryStore::new();
    let (config, migrated) = startup(&mut store, None);
    assert!(!migrated);
    assert!(store.is_empty());
    assert_eq!(config, Config::default());
}

// ---------------------------------------------------------------- глобальное состояние

#[test]
fn update_saves_and_notifies_subscribers() {
    init_with_store(Box::new(MemoryStore::new()));
    let calls = Rc::new(Cell::new(0));
    let seen = calls.clone();
    subscribe(Box::new(move |old, new| {
        assert_eq!(old.gap_size, 0.0);
        assert_eq!(new.gap_size, 4.0);
        seen.set(seen.get() + 1);
    }));
    update(|config| config.gap_size = 4.0);
    assert_eq!(calls.get(), 1);
    assert_eq!(current().gap_size, 4.0);
    assert!(with(|config| config.gap_size == 4.0));
    // Без изменений подписчиков не зовём.
    update(|config| config.gap_size = 4.0);
    assert_eq!(calls.get(), 1);
}

// ---------------------------------------------------------------- документация

#[test]
fn settings_doc_is_up_to_date() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/settings.md");
    let expected = settings_markdown();
    if std::env::var("R2_WRITE_SETTINGS_DOC").is_ok() {
        std::fs::write(&path, &expected).expect("запись docs/settings.md");
        return;
    }
    let actual = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(
        actual == expected,
        "docs/settings.md отстал от таблицы настроек: R2_WRITE_SETTINGS_DOC=1 cargo test settings_doc"
    );
}
