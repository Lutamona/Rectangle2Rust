//! Настройки приложения — полный паритет с `Defaults.swift` оригинала.
//!
//! Настройки лежат в UserDefaults под теми же ключами и в той же кодировке,
//! что у Swift-версии (правила чтения/записи — в `defaults_store`). Горячих
//! клавиш в порте нет, поэтому шорткатов и связанных с ними настроек тоже нет.
//! Таблица настроек ниже — единственный источник: из неё собираются `Config`,
//! чтение, запись, экспорт/импорт и `docs/settings.md` (тест следит, чтобы
//! документ не отставал).
//!
//! Глобальное состояние: `current()`/`with()` читают, `update()` меняет,
//! сохраняет изменения и оповещает подписчиков (`subscribe()`).

mod legacy;
mod types;

pub use legacy::migrate_legacy_text;
pub use types::*;

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::rc::Rc;
use std::sync::{Arc, Mutex, MutexGuard};

use crate::defaults_store::{
    BoolDefault, CodableDefault, Codec, CycleSizesDefault, DoubleDefault, FloatDefault, IntDefault,
    IntEnumDefault, JsonDefault, JsonDefaultWithValue, MemoryStore, OptionalBoolDefault,
    ReadOnlyBool, Store, StringDefault, SubsequentExecutionDefault, UserDefaultsStore,
};

/// Bundle id порта — домен UserDefaults приложения, запущенного из бандла.
pub const APP_BUNDLE_ID: &str = "local.rectangle2rust";

/// Rust-only: старый config.conf уже перенесён в UserDefaults.
pub const LEGACY_MIGRATED_KEY: &str = "r2ConfigConfMigrated";

/// Описание настройки для документации.
pub struct SettingInfo {
    pub field: &'static str,
    pub key: &'static str,
    pub rust_type: &'static str,
    /// Класс обёртки в `Defaults.swift`.
    pub swift_kind: &'static str,
    pub doc: &'static str,
    /// Входит в `Defaults.array`: попадает в экспорт и читается при импорте.
    pub exported: bool,
}

/// Таблица настроек: `поле: тип = по умолчанию, "ключ Swift", обёртка, в экспорте;`.
/// Порядок — как объявления `static let` в `Defaults.swift`.
macro_rules! settings {
    (
        $(
            $(#[doc = $doc:literal])*
            $field:ident: $ty:ty = $default:expr, $key:literal, $codec:ty, $exported:literal;
        )*
    ) => {
        /// Все настройки оригинала, кроме горячих клавиш. `Default` — значения при
        /// пустом UserDefaults.
        #[derive(Clone, Debug, PartialEq)]
        pub struct Config {
            $(
                $(#[doc = $doc])*
                #[doc = ""]
                #[doc = concat!("Ключ Swift: `", $key, "`.")]
                pub $field: $ty,
            )*
        }

        impl Default for Config {
            fn default() -> Self {
                Config {
                    $( $field: $default, )*
                }
            }
        }

        impl Config {
            /// Чтение из хранилища — ровно как Swift-обёртки при старте.
            pub fn load_from(store: &dyn Store) -> Config {
                let defaults = Config::default();
                Config {
                    $( $field: <$codec as Codec>::load(store, $key, &defaults.$field), )*
                }
            }

            /// Записать только изменившиеся настройки, в кодировке оригинала.
            pub fn save_changes(old: &Config, new: &Config, store: &mut dyn Store) {
                $(
                    if old.$field != new.$field {
                        <$codec as Codec>::save(store, $key, &new.$field);
                    }
                )*
            }

            /// `toCodable()` всех настроек из `Defaults.array` — для экспорта.
            pub(crate) fn exported_defaults(&self) -> Vec<(&'static str, CodableDefault)> {
                let defaults = Config::default();
                let mut exported = Vec::new();
                $(
                    if $exported {
                        exported.push((
                            $key,
                            <$codec as Codec>::to_codable(&self.$field, &defaults.$field),
                        ));
                    }
                )*
                exported
            }

            /// `load(from:)` всех настроек из `Defaults.array` — для импорта.
            pub(crate) fn import_defaults(&mut self, imported: &BTreeMap<String, CodableDefault>) {
                let defaults = Config::default();
                $(
                    if $exported {
                        if let Some(codable) = imported.get($key) {
                            self.$field = <$codec as Codec>::from_codable(
                                codable,
                                &self.$field,
                                &defaults.$field,
                            );
                        }
                    }
                )*
            }

            /// Значения по умолчанию текстом, в порядке `SETTINGS`.
            fn default_values_debug() -> Vec<String> {
                let defaults = Config::default();
                vec![ $( format!("{:?}", defaults.$field), )* ]
            }
        }

        /// Все настройки в порядке объявления — для документации.
        pub const SETTINGS: &[SettingInfo] = &[
            $(
                SettingInfo {
                    field: stringify!($field),
                    key: $key,
                    rust_type: stringify!($ty),
                    swift_kind: <$codec as Codec>::SWIFT_KIND,
                    doc: concat!($($doc),*),
                    exported: $exported,
                },
            )*
        ];
    };
}

settings! {
    /// Запускать при входе в систему.
    launch_on_login: bool = false, "launchOnLogin", BoolDefault, true;
    /// Bundle id приложений, для которых Rectangle выключен (пункт меню «Игнорировать …»).
    disabled_apps: Option<BTreeSet<String>> = None, "disabledApps", JsonDefault<BTreeSet<String>>, true;
    /// Прятать иконку в строке меню.
    hide_menu_bar_icon: bool = false, "hideMenubarIcon", BoolDefault, true;
    /// Что делает повторное выполнение того же действия (0 перебор размеров, 1 соседний экран, 2 ничего, 3 экран для ←/→ и размеры для остального, 4 перебор экранов, 5 размеры и четверти).
    subsequent_execution_mode: SubsequentExecutionMode = SubsequentExecutionMode::Resize, "subsequentExecutionMode", SubsequentExecutionDefault, true;
    /// Размеры для перебора при повторных выполнениях (битовая маска CycleSize); действуют, только если cycle_sizes_is_changed.
    selected_cycle_sizes: CycleSizes = CycleSizes::EMPTY, "selectedCycleSizes", CycleSizesDefault, true;
    /// Набор размеров перебора менялся; иначе действует набор по умолчанию ½ → ⅔ → ⅓.
    cycle_sizes_is_changed: bool = false, "cycleSizesIsChanged", BoolDefault, true;
    /// По какой оси растёт окно в углу при повторных выполнениях (0 по горизонтали, 1 по вертикали).
    corner_cycle_expansion_axis: CornerCycleExpansionAxis = CornerCycleExpansionAxis::Horizontal, "cornerCycleExpansionAxis", IntEnumDefault<CornerCycleExpansionAxis>, true;
    /// Согласованные размеры соседних окон: половины и углы подстраиваются под уже поставленные окна.
    cooperative_corner_resize: bool = false, "cooperativeCornerResize", BoolDefault, true;
    /// Прилипание окон при перетаскивании к краю экрана (drag-to-snap); не задано — включено.
    window_snapping: Option<bool> = None, "windowSnapping", OptionalBoolDefault, true;
    /// Высота «почти развернуть», доля экрана; 0 или больше 1 — 0,9.
    almost_maximize_height: f32 = 0.0, "almostMaximizeHeight", FloatDefault, true;
    /// Ширина «почти развернуть», доля экрана; 0 или больше 1 — 0,9.
    almost_maximize_width: f32 = 0.0, "almostMaximizeWidth", FloatDefault, true;
    /// Зазор между окнами, пикселей.
    gap_size: f32 = 0.0, "gapSize", FloatDefault, true;
    /// Не делать зазор у верхнего края экрана.
    skip_gap_top_edge: bool = false, "skipGapTopEdge", BoolDefault, true;
    /// Полоса у верхнего края, в которой срабатывает drag-to-snap, пикселей.
    snap_edge_margin_top: f32 = 5.0, "snapEdgeMarginTop", FloatDefault, true;
    /// Полоса у нижнего края для drag-to-snap, пикселей.
    snap_edge_margin_bottom: f32 = 5.0, "snapEdgeMarginBottom", FloatDefault, true;
    /// Полоса у левого края для drag-to-snap, пикселей.
    snap_edge_margin_left: f32 = 5.0, "snapEdgeMarginLeft", FloatDefault, true;
    /// Полоса у правого края для drag-to-snap, пикселей.
    snap_edge_margin_right: f32 = 5.0, "snapEdgeMarginRight", FloatDefault, true;
    /// «Переместить к краю» центрирует окно по второй оси; не задано — да.
    centered_directional_move: Option<bool> = None, "centeredDirectionalMove", OptionalBoolDefault, true;
    /// «Переместить к краю» ещё и меняет размер окна.
    resize_on_directional_move: bool = false, "resizeOnDirectionalMove", BoolDefault, true;
    /// Куда прижимать окно, которое приложение не дало растянуть (1 края и углы, 2 только углы, 3 по центру).
    move_fixed_size_to_edge: EdgeAlignment = EdgeAlignment::EdgesAndCorners, "moveFixedSizeToEdge", IntEnumDefault<EdgeAlignment>, true;
    /// Выключенные зоны drag-to-snap (битовая маска SnapAreaOption); старый формат, оригинал переводит его в карты зон.
    ignored_snap_areas: i64 = 0, "ignoredSnapAreas", IntDefault, true;
    /// «Следующий/предыдущий дисплей» при одном экране считает его соседом самого себя.
    traverse_single_screen: Option<bool> = None, "traverseSingleScreen", OptionalBoolDefault, true;
    /// Экран для действия определять по курсору, а не по окну.
    use_cursor_screen_detection: bool = false, "useCursorScreenDetection", BoolDefault, false;
    /// Минимальная ширина окна при «меньше», доля экрана.
    minimum_window_width: f64 = 0.25, "minimumWindowWidth", DoubleDefault, true;
    /// Минимальная высота окна при «меньше», доля экрана.
    minimum_window_height: f64 = 0.25, "minimumWindowHeight", DoubleDefault, true;
    /// Шаг «больше/меньше», пикселей; 0 и меньше — 30.
    size_offset: f32 = 0.0, "sizeOffset", FloatDefault, true;
    /// Шаг «шире/уже», пикселей.
    width_step_size: f32 = 30.0, "widthStepSize", FloatDefault, true;
    /// Возвращать прежний размер окну, которое утащили из прилипшего положения; не задано — да.
    unsnap_restore: Option<bool> = None, "unsnapRestore", OptionalBoolDefault, true;
    /// То же для окна после «больше/меньше»: «нет» — не возвращать.
    unsnap_restore_from_size_change: Option<bool> = None, "unsnapRestoreFromSizeChange", OptionalBoolDefault, false;
    /// «Больше/меньше» у окна, прижатого к краю, двигает только свободный край; не задано — да.
    curtain_change_size: Option<bool> = None, "curtainChangeSize", OptionalBoolDefault, true;
    /// «Меньше» уменьшает и высоту окна, развёрнутого по высоте.
    smaller_shrinks_maximized_height: bool = false, "smallerShrinksMaximizedHeight", BoolDefault, true;
    /// Повторный запуск приложения открывает меню, а не настройки.
    relaunch_opens_menu: bool = false, "relaunchOpensMenu", BoolDefault, true;
    /// Drag-to-snap берёт окно под курсором уже при нажатии кнопки мыши; не задано — да.
    obtain_window_on_click: Option<bool> = None, "obtainWindowOnClick", OptionalBoolDefault, true;
    /// Отступ рабочей области от верхнего края экрана, пикселей.
    screen_edge_gap_top: f32 = 0.0, "screenEdgeGapTop", FloatDefault, true;
    /// Отступ рабочей области от нижнего края экрана, пикселей.
    screen_edge_gap_bottom: f32 = 0.0, "screenEdgeGapBottom", FloatDefault, true;
    /// Отступ рабочей области от левого края экрана, пикселей.
    screen_edge_gap_left: f32 = 0.0, "screenEdgeGapLeft", FloatDefault, true;
    /// Отступ рабочей области от правого края экрана, пикселей.
    screen_edge_gap_right: f32 = 0.0, "screenEdgeGapRight", FloatDefault, true;
    /// Отступы от краёв — только на главном экране.
    screen_edge_gaps_on_main_screen_only: bool = false, "screenEdgeGapsOnMainScreenOnly", BoolDefault, true;
    /// Отступ сверху на экране с вырезом камеры вместо screen_edge_gap_top, пикселей; 0 — как у остальных.
    screen_edge_gap_top_notch: f32 = 0.0, "screenEdgeGapTopNotch", FloatDefault, true;
    /// Номер сборки при прошлом запуске (для миграций между версиями).
    last_version: Option<String> = None, "lastVersion", StringDefault, false;
    /// Номер сборки при первом запуске (оригиналу нужен для шорткатов по умолчанию).
    install_version: Option<String> = None, "installVersion", StringDefault, false;
    /// Показывать в меню все действия, а не только основные.
    show_all_actions_in_menu: Option<bool> = None, "showAllActionsInMenu", OptionalBoolDefault, true;
    /// Показывать в меню подменю дополнительных размеров.
    show_additional_sizes_in_menu: Option<bool> = None, "showAdditionalSizesInMenu", OptionalBoolDefault, true;
    /// Sparkle: приложение уже запускалось (оригинал ключ только читает).
    su_has_launched_before: bool = false, "SUHasLaunchedBefore", ReadOnlyBool, false;
    /// Непрозрачность подсветки будущего положения окна при drag-to-snap.
    footprint_alpha: f32 = 0.3, "footprintAlpha", FloatDefault, true;
    /// Толщина рамки подсветки, пикселей.
    footprint_border_width: f32 = 2.0, "footprintBorderWidth", FloatDefault, true;
    /// Плавное появление подсветки; «нет» — без анимации прозрачности.
    footprint_fade: Option<bool> = None, "footprintFade", OptionalBoolDefault, true;
    /// Цвет подсветки; не задан — чёрный.
    footprint_color: Option<FootprintColor> = None, "footprintColor", JsonDefault<FootprintColor>, true;
    /// Sparkle: проверять обновления автоматически.
    su_enable_automatic_checks: bool = false, "SUEnableAutomaticChecks", BoolDefault, true;
    /// Режим Todo включён в настройках (появляются его пункты меню и шорткаты).
    todo: Option<bool> = None, "todo", OptionalBoolDefault, true;
    /// Режим Todo сейчас активен (приложение Todo стоит боковой панелью).
    todo_mode: bool = false, "todoMode", BoolDefault, true;
    /// Bundle id приложения для боковой панели Todo.
    todo_application: Option<String> = None, "todoApplication", StringDefault, true;
    /// Ширина панели Todo в единицах todo_sidebar_width_unit.
    todo_sidebar_width: f32 = 400.0, "todoSidebarWidth", FloatDefault, true;
    /// Единица ширины панели Todo (1 пиксели, 2 проценты).
    todo_sidebar_width_unit: TodoSidebarWidthUnit = TodoSidebarWidthUnit::Pixels, "todoSidebarWidthUnit", IntEnumDefault<TodoSidebarWidthUnit>, true;
    /// Сторона панели Todo (1 справа, 2 слева).
    todo_sidebar_side: TodoSidebarSide = TodoSidebarSide::Right, "todoSidebarSide", IntEnumDefault<TodoSidebarSide>, true;
    /// Модификаторы, которые надо держать для drag-to-snap (NSEventModifierFlags); 0 — не нужны.
    snap_modifiers: i64 = 0, "snapModifiers", IntDefault, true;
    /// При переносе на другой экран сохранять раскладку окна (половина остаётся половиной).
    attempt_match_on_next_prev_display: Option<bool> = None, "attemptMatchOnNextPrevDisplay", OptionalBoolDefault, true;
    /// Альтернативный перебор третей; в коде оригинала больше не читается.
    alt_third_cycle: Option<bool> = None, "altThirdCycle", OptionalBoolDefault, true;
    /// «Центр» перебирает размеры при каждом нажатии, в любом режиме повторов.
    center_half_cycles: Option<bool> = None, "centerHalfCycles", OptionalBoolDefault, true;
    /// Сдвигать лесенкой окна, поставленные в одну и ту же позицию.
    cycling_overlap_offset: Option<bool> = None, "cyclingOverlapOffset", OptionalBoolDefault, true;
    /// Шаг лесенки, пикселей.
    cycling_overlap_offset_size: f32 = 11.0, "cyclingOverlapOffsetSize", FloatDefault, true;
    /// Сколько ступеней в лесенке (оригинал ограничивает 1…5).
    cycling_overlap_max_cascade: i64 = 1, "cyclingOverlapMaxCascade", IntDefault, true;
    /// Значок-счётчик у окон, стоящих стопкой в одной позиции.
    stack_badge: Option<bool> = None, "stackBadge", OptionalBoolDefault, true;
    /// Приложения, которых drag-to-snap не касается вовсе; не задано — встроенный список оригинала.
    full_ignore_bundle_ids: Option<Vec<String>> = None, "fullIgnoreBundleIds", JsonDefault<Vec<String>>, true;
    /// Предупреждение о приложениях, несовместимых с drag-to-snap, уже показано.
    notified_of_problem_apps: bool = false, "notifiedOfProblemApps", BoolDefault, true;
    /// Высота «заданного размера»: до 1 — доля экрана, больше — пиксели.
    specified_height: f32 = 1050.0, "specifiedHeight", FloatDefault, true;
    /// Ширина «заданного размера»: до 1 — доля экрана, больше — пиксели.
    specified_width: f32 = 1680.0, "specifiedWidth", FloatDefault, true;
    /// Где делить экран на левую и правую половины, проценты.
    horizontal_split_ratio: f32 = 50.0, "horizontalSplitRatio", FloatDefault, true;
    /// Где делить экран на верхнюю и нижнюю половины, проценты.
    vertical_split_ratio: f32 = 50.0, "verticalSplitRatio", FloatDefault, true;
    /// Переносить курсор вместе с окном на другой экран.
    move_cursor_across_displays: Option<bool> = None, "moveCursorAcrossDisplays", OptionalBoolDefault, true;
    /// Ставить курсор в центр окна после действия с клавиатуры.
    move_cursor: Option<bool> = None, "moveCursor", OptionalBoolDefault, true;
    /// Развёрнутое окно при переносе на другой экран разворачивается и там; не задано — да.
    auto_maximize: Option<bool> = None, "autoMaximize", OptionalBoolDefault, true;
    /// Зазоры и у «развернуть»; не задано — да.
    apply_gaps_to_maximize: Option<bool> = None, "applyGapsToMaximize", OptionalBoolDefault, true;
    /// Зазоры и у «развернуть по высоте»; не задано — да.
    apply_gaps_to_maximize_height: Option<bool> = None, "applyGapsToMaximizeHeight", OptionalBoolDefault, true;
    /// Размер угловой зоны drag-to-snap, пикселей.
    corner_snap_area_size: f32 = 20.0, "cornerSnapAreaSize", FloatDefault, true;
    /// Длина зоны у короткого края в составных зонах drag-to-snap, пикселей.
    short_edge_snap_area_size: f32 = 145.0, "shortEdgeSnapAreaSize", FloatDefault, true;
    /// Шаг «каскадом», пикселей.
    cascade_all_delta_size: f32 = 30.0, "cascadeAllDeltaSize", FloatDefault, true;
    /// Старый флаг зон «шестые» сверху и снизу; оригинал переводит его в карты зон.
    sixths_snap_area: Option<bool> = None, "sixthsSnapArea", OptionalBoolDefault, true;
    /// Сколько оставлять под полосу Stage Manager: до 1 — доля ширины, больше — пиксели, 0 и меньше — не оставлять.
    stage_size: f32 = 190.0, "stageSize", FloatDefault, true;
    /// Drag-to-snap для окон, вытаскиваемых из полосы Stage Manager; «нет» — выключить.
    drag_from_stage: Option<bool> = None, "dragFromStage", OptionalBoolDefault, true;
    /// «Центр» и «центр крупно» учитывают полосу Stage Manager всегда.
    always_account_for_stage: Option<bool> = None, "alwaysAccountForStage", OptionalBoolDefault, true;
    /// Зоны drag-to-snap для горизонтальных экранов; не задано — default_landscape_snap_areas().
    landscape_snap_areas: Option<SnapAreas> = None, "landscapeSnapAreas", JsonDefault<SnapAreas>, true;
    /// Зоны drag-to-snap для вертикальных экранов; не задано — default_portrait_snap_areas().
    portrait_snap_areas: Option<SnapAreas> = None, "portraitSnapAreas", JsonDefault<SnapAreas>, true;
    /// Не мешать перетаскиванию окна в Mission Control; «нет» — перехватывать события активно.
    mission_control_dragging: Option<bool> = None, "missionControlDragging", OptionalBoolDefault, true;
    /// Обход AXEnhancedUserInterface (1 выключать на время действия, 2 выключать насовсем, 3 выключать при смене приложения).
    enhanced_ui: EnhancedUI = EnhancedUI::DisableEnable, "enhancedUI", IntEnumDefault<EnhancedUI>, true;
    /// Множитель длительности анимации подсветки; 0 — без анимации размера.
    footprint_animation_duration_multiplier: f32 = 0.0, "footprintAnimationDurationMultiplier", FloatDefault, true;
    /// Отклик трекпада при попадании в зону drag-to-snap.
    haptic_feedback_on_snap: Option<bool> = None, "hapticFeedbackOnSnap", OptionalBoolDefault, true;
    /// На сколько пикселей окно можно увести за верх экрана, прежде чем считать это жестом Mission Control.
    mission_control_dragging_allowed_offscreen_distance: f32 = 25.0, "missionControlDraggingAllowedOffscreenDistance", FloatDefault, true;
    /// Сколько миллисекунд после такого жеста drag-to-snap не срабатывает.
    mission_control_dragging_disallowed_duration: i64 = 250, "missionControlDraggingDisallowedDuration", IntDefault, true;
    /// Действие по двойному клику по заголовку окна: rawValue WindowAction + 1; 0 — ничего.
    double_click_title_bar: i64 = 0, "doubleClickTitleBar", IntDefault, true;
    /// Повторный двойной клик по заголовку возвращает прежний размер; не задано — да.
    double_click_title_bar_restore: Option<bool> = None, "doubleClickTitleBarRestore", OptionalBoolDefault, true;
    /// Приложения, где двойной клик по заголовку не трогаем (этот же ключ читает double_click_tool_bar_ignored_apps()).
    double_click_title_bar_ignored_apps: Option<Vec<String>> = None, "doubleClickTitleBarIgnoredApps", JsonDefault<Vec<String>>, true;
    /// «Игнорировать приложение» выключает для него и drag-to-snap; не задано — да.
    ignore_drag_snap_too: Option<bool> = None, "ignoreDragSnapToo", OptionalBoolDefault, true;
    /// Окно под курсором искать через системный AX-элемент; не задано — только для system_wide_mouse_down_apps.
    system_wide_mouse_down: Option<bool> = None, "systemWideMouseDown", OptionalBoolDefault, true;
    /// Приложения, для которых окно под курсором ищется через системный AX-элемент.
    system_wide_mouse_down_apps: BTreeSet<String> = default_system_wide_mouse_down_apps(), "systemWideMouseDownApps", JsonDefaultWithValue<BTreeSet<String>>, true;
    /// Предупреждение о встроенной раскладке окон macOS уже показано.
    internal_tiling_notified: bool = false, "internalTilingNotified", BoolDefault, false;
    /// Порядок экранов (1 по середине по X, 2 по левому краю, 3 сверху вниз, затем слева направо).
    screens_ordered_by_x: ScreenOrdering = ScreenOrdering::YThenMinX, "screensOrderedByX", IntEnumDefault<ScreenOrdering>, true;
    /// Все экраны как один большой (когда у экранов общие Spaces).
    combined_display_mode: Option<bool> = None, "combinedDisplayMode", OptionalBoolDefault, false;
    /// Зелёная кнопка окна выполняет действие Rectangle.
    green_button_override: bool = false, "greenButtonOverride", BoolDefault, true;
    /// Rust-only: системный запрос доступа уже показывали (чтобы не спрашивать на каждом запуске).
    access_prompted: bool = false, "r2AccessPrompted", BoolDefault, false;
    /// Rust-only: bundle id приложений, чьи окна держим столбиками (пункт меню «Держать окна «…» столбиками»).
    app_columns_bundle_ids: Option<BTreeSet<String>> = None, "r2AppColumnsBundleIds", JsonDefault<BTreeSet<String>>, false;
}

impl Config {
    /// Приложение в списке «Игнорировать» (`ApplicationToggle`).
    pub fn is_app_disabled(&self, bundle_id: &str) -> bool {
        self.disabled_apps
            .as_ref()
            .is_some_and(|apps| apps.contains(bundle_id))
    }

    /// Размеры перебора, которые действуют (`cycleSizesIsChanged` ? выбранные : по умолчанию).
    pub fn effective_cycle_sizes(&self) -> CycleSizes {
        if self.cycle_sizes_is_changed {
            self.selected_cycle_sizes
        } else {
            CycleSizes::default_sizes()
        }
    }

    /// Выключенные зоны drag-to-snap как битовая маска.
    pub fn ignored_snap_area_options(&self) -> SnapAreaOption {
        SnapAreaOption(self.ignored_snap_areas)
    }

    /// Зоны drag-to-snap для горизонтального экрана с учётом значения по умолчанию.
    pub fn landscape_snap_areas_or_default(&self) -> SnapAreas {
        self.landscape_snap_areas
            .clone()
            .unwrap_or_else(default_landscape_snap_areas)
    }

    /// Зоны drag-to-snap для вертикального экрана с учётом значения по умолчанию.
    pub fn portrait_snap_areas_or_default(&self) -> SnapAreas {
        self.portrait_snap_areas
            .clone()
            .unwrap_or_else(default_portrait_snap_areas)
    }

    /// `doubleClickToolBarIgnoredApps`: тот же ключ, что у
    /// `double_click_title_bar_ignored_apps`, прочитанный как множество;
    /// пусто или не разобрать — `["epp.package.java"]`.
    pub fn double_click_tool_bar_ignored_apps(&self) -> BTreeSet<String> {
        match &self.double_click_title_bar_ignored_apps {
            Some(apps) => apps.iter().cloned().collect(),
            None => default_double_click_tool_bar_ignored_apps(),
        }
    }
}

// ---------------------------------------------------------------- старт

/// Старт: перенос config.conf, если его текст передан и перенос ещё не
/// делался. Возвращает настройки так, как их прочтёт следующий запуск, и
/// признак переноса.
fn startup(store: &mut dyn Store, legacy_text: Option<&str>) -> (Config, bool) {
    let loaded = Config::load_from(store);
    let Some(text) = legacy_text.filter(|_| !store.contains(LEGACY_MIGRATED_KEY)) else {
        return (loaded, false);
    };
    let migrated = migrate_legacy_text(text, &loaded);
    Config::save_changes(&loaded, &migrated, store);
    store.set_bool(LEGACY_MIGRATED_KEY, true);
    (Config::load_from(store), true)
}

// ---------------------------------------------------------------- глобальное состояние

struct Global {
    config: Arc<Config>,
    store: Box<dyn Store + Send>,
}

static GLOBAL: Mutex<Option<Global>> = Mutex::new(None);

/// Подписчик на изменения настроек: `(старые, новые)`.
pub type Listener = Box<dyn Fn(&Config, &Config)>;

type Subscriber = Rc<dyn Fn(&Config, &Config)>;

thread_local! {
    static SUBSCRIBERS: RefCell<Vec<Subscriber>> = const { RefCell::new(Vec::new()) };
}

fn global() -> MutexGuard<'static, Option<Global>> {
    GLOBAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn snapshot() -> Arc<Config> {
    global()
        .as_ref()
        .map(|global| global.config.clone())
        .unwrap_or_else(|| Arc::new(Config::default()))
}

/// Текущие настройки (копия). До инициализации — значения по умолчанию.
pub fn current() -> Config {
    (*snapshot()).clone()
}

/// Прочитать настройки без копирования.
pub fn with<R>(f: impl FnOnce(&Config) -> R) -> R {
    let config = snapshot();
    f(&config)
}

/// Изменить настройки: изменения сохраняются в хранилище (UserDefaults после
/// `init_from_user_defaults`, до неё — только в памяти), подписчики получают
/// старые и новые настройки. Вызывать с главного потока (там живут подписчики).
pub fn update(f: impl FnOnce(&mut Config)) {
    let mut new = current();
    f(&mut new);
    let old = {
        let mut guard = global();
        let global = guard.get_or_insert_with(|| Global {
            config: Arc::new(Config::default()),
            store: Box::new(MemoryStore::new()),
        });
        let old = global.config.clone();
        if *old == new {
            return;
        }
        Config::save_changes(&old, &new, global.store.as_mut());
        global.config = Arc::new(new.clone());
        old
    };
    let subscribers: Vec<Subscriber> = SUBSCRIBERS.with(|list| list.borrow().clone());
    for subscriber in subscribers {
        subscriber(&old, &new);
    }
}

/// Подписка на изменения настроек: `(старые, новые)`.
pub fn subscribe(callback: Listener) {
    SUBSCRIBERS.with(|list| list.borrow_mut().push(Rc::from(callback)));
}

/// Сделать хранилище текущим и загрузить из него настройки (без миграций).
pub fn init_with_store(store: Box<dyn Store + Send>) {
    let config = Config::load_from(store.as_ref());
    *global() = Some(Global {
        config: Arc::new(config),
        store,
    });
}

/// Старт приложения: настройки из `UserDefaults.standard`, при первом запуске
/// нового формата — перенос старого config.conf (файл после этого
/// переименовывается в `config.conf.migrated`).
pub fn init_from_user_defaults() {
    let mut store = UserDefaultsStore::standard();
    let legacy_path = legacy::legacy_config_path();
    let legacy_text = match &legacy_path {
        Some(path) if !store.contains(LEGACY_MIGRATED_KEY) => std::fs::read_to_string(path).ok(),
        _ => None,
    };

    let (config, migrated) = startup(&mut store, legacy_text.as_deref());
    if let (true, Some(path)) = (migrated, legacy_path) {
        rename_migrated(&path);
    }

    *global() = Some(Global {
        config: Arc::new(config),
        store: Box::new(store),
    });
}

fn rename_migrated(path: &Path) {
    let target = path.with_file_name("config.conf.migrated");
    if let Err(error) = std::fs::rename(path, &target) {
        eprintln!(
            "rectangle2rust: config.conf перенесён в настройки, но не переименован ({}): {}",
            path.display(),
            error
        );
    }
}

// ---------------------------------------------------------------- документация

/// Таблица настроек для `docs/settings.md`.
pub fn settings_markdown() -> String {
    let defaults = Config::default_values_debug();
    let clean = |text: &str| text.replace('|', "\\|");
    let mut text = String::from(
        "# Настройки Rectangle 2 (Rust)\n\
         \n\
         Файл сгенерирован из таблицы в `src/config.rs` (`config::settings_markdown()`);\n\
         тест `config::tests::settings_doc_is_up_to_date` следит, чтобы он не отставал.\n\
         Обновить: `R2_WRITE_SETTINGS_DOC=1 cargo test settings_doc`.\n\
         \n\
         Хранилище — `UserDefaults.standard` (домен `local.rectangle2rust` у приложения\n\
         из бандла; без бандла — имя исполняемого файла). Ключи и кодировка — как у\n\
         `Defaults.swift` оригинала: `OptionalBoolDefault` — целое 0 (не задано) / 1 (да) /\n\
         2 (нет); `FloatDefault`/`IntDefault` — сохранённый 0 читается как значение по\n\
         умолчанию, если оно не 0; NaN и бесконечности у `FloatDefault`/`DoubleDefault`\n\
         читаются как значение по умолчанию и в экспорт не попадают; `IntEnumDefault` —\n\
         rawValue, неизвестное значение даёт значение по умолчанию; `JSONDefault` —\n\
         компактная JSON-строка с сортировкой ключей.\n\
         «Экспорт: нет» — настройки нет в `Defaults.array`, в JSON-экспорт она не попадает.\n\
         \n\
         | поле Rust | ключ Swift | тип | по умолчанию | что делает |\n\
         |---|---|---|---|---|\n",
    );
    for (setting, default) in SETTINGS.iter().zip(defaults) {
        let export_note = if setting.exported {
            ""
        } else {
            " Экспорт: нет."
        };
        text.push_str(&format!(
            "| `{}` | `{}` | `{}` ({}) | `{}` | {}{} |\n",
            setting.field,
            setting.key,
            clean(&setting.rust_type.replace(' ', "")),
            setting.swift_kind,
            clean(&default),
            clean(setting.doc.trim()),
            export_note
        ));
    }
    text.push_str(
        "\n\
         ## Горячие клавиши\n\
         \n\
         В порте их нет (управление только мышью), поэтому не читаются и не пишутся:\n\
         шорткаты действий (ключи `leftHalf`, `columnFive1`, … — словари MASShortcut),\n\
         шорткаты Todo (`toggleTodo`, `reflowTodo`), `alternateDefaultShortcuts`,\n\
         `allowAnyShortcut`. В JSON-экспорте `shortcuts` — пустой объект, при импорте\n\
         эта секция пропускается.\n\
         \n\
         ## Производные значения\n\
         \n\
         - `double_click_tool_bar_ignored_apps()` — `doubleClickToolBarIgnoredApps` оригинала:\n\
         тот же ключ `doubleClickTitleBarIgnoredApps`, прочитанный как множество;\n\
         пусто — `[\"epp.package.java\"]`. Отдельного ключа нет, в экспорт не входит.\n\
         - `effective_cycle_sizes()`, `landscape_snap_areas_or_default()`,\n\
         `portrait_snap_areas_or_default()`, `ignored_snap_area_options()`.\n\
         \n\
         ## Служебные ключи порта\n\
         \n\
         - `r2AccessPrompted` — поле `access_prompted`.\n\
         - `r2AppColumnsBundleIds` — поле `app_columns_bundle_ids` (режим «держать окна\n\
         столбиками», есть только в порте).\n\
         - `r2ConfigConfMigrated` — старый `config.conf` уже перенесён в UserDefaults.\n",
    );
    text
}

#[cfg(test)]
mod tests;
