# Настройки Rectangle 2 (Rust)

Файл сгенерирован из таблицы в `src/config.rs` (`config::settings_markdown()`);
тест `config::tests::settings_doc_is_up_to_date` следит, чтобы он не отставал.
Обновить: `R2_WRITE_SETTINGS_DOC=1 cargo test settings_doc`.

Хранилище — `UserDefaults.standard` (домен `local.rectangle2rust` у приложения
из бандла; без бандла — имя исполняемого файла). Ключи и кодировка — как у
`Defaults.swift` оригинала: `OptionalBoolDefault` — целое 0 (не задано) / 1 (да) /
2 (нет); `FloatDefault`/`IntDefault` — сохранённый 0 читается как значение по
умолчанию, если оно не 0; NaN и бесконечности у `FloatDefault`/`DoubleDefault`
читаются как значение по умолчанию и в экспорт не попадают; `IntEnumDefault` —
rawValue, неизвестное значение даёт значение по умолчанию; `JSONDefault` —
компактная JSON-строка с сортировкой ключей.
«Экспорт: нет» — настройки нет в `Defaults.array`, в JSON-экспорт она не попадает.

| поле Rust | ключ Swift | тип | по умолчанию | что делает |
|---|---|---|---|---|
| `launch_on_login` | `launchOnLogin` | `bool` (BoolDefault) | `false` | Запускать при входе в систему. |
| `disabled_apps` | `disabledApps` | `Option<BTreeSet<String>>` (JSONDefault) | `None` | Bundle id приложений, для которых Rectangle выключен (пункт меню «Игнорировать …»). |
| `hide_menu_bar_icon` | `hideMenubarIcon` | `bool` (BoolDefault) | `false` | Прятать иконку в строке меню. |
| `subsequent_execution_mode` | `subsequentExecutionMode` | `SubsequentExecutionMode` (SubsequentExecutionDefault) | `Resize` | Что делает повторное выполнение того же действия (0 перебор размеров, 1 соседний экран, 2 ничего, 3 экран для ←/→ и размеры для остального, 4 перебор экранов, 5 размеры и четверти). |
| `selected_cycle_sizes` | `selectedCycleSizes` | `CycleSizes` (CycleSizesDefault) | `{}` | Размеры для перебора при повторных выполнениях (битовая маска CycleSize); действуют, только если cycle_sizes_is_changed. |
| `cycle_sizes_is_changed` | `cycleSizesIsChanged` | `bool` (BoolDefault) | `false` | Набор размеров перебора менялся; иначе действует набор по умолчанию ½ → ⅔ → ⅓. |
| `corner_cycle_expansion_axis` | `cornerCycleExpansionAxis` | `CornerCycleExpansionAxis` (IntEnumDefault) | `Horizontal` | По какой оси растёт окно в углу при повторных выполнениях (0 по горизонтали, 1 по вертикали). |
| `cooperative_corner_resize` | `cooperativeCornerResize` | `bool` (BoolDefault) | `false` | Согласованные размеры соседних окон: половины и углы подстраиваются под уже поставленные окна. |
| `window_snapping` | `windowSnapping` | `Option<bool>` (OptionalBoolDefault) | `None` | Прилипание окон при перетаскивании к краю экрана (drag-to-snap); не задано — включено. |
| `almost_maximize_height` | `almostMaximizeHeight` | `f32` (FloatDefault) | `0.0` | Высота «почти развернуть», доля экрана; 0 или больше 1 — 0,9. |
| `almost_maximize_width` | `almostMaximizeWidth` | `f32` (FloatDefault) | `0.0` | Ширина «почти развернуть», доля экрана; 0 или больше 1 — 0,9. |
| `gap_size` | `gapSize` | `f32` (FloatDefault) | `0.0` | Зазор между окнами, пикселей. |
| `skip_gap_top_edge` | `skipGapTopEdge` | `bool` (BoolDefault) | `false` | Не делать зазор у верхнего края экрана. |
| `snap_edge_margin_top` | `snapEdgeMarginTop` | `f32` (FloatDefault) | `5.0` | Полоса у верхнего края, в которой срабатывает drag-to-snap, пикселей. |
| `snap_edge_margin_bottom` | `snapEdgeMarginBottom` | `f32` (FloatDefault) | `5.0` | Полоса у нижнего края для drag-to-snap, пикселей. |
| `snap_edge_margin_left` | `snapEdgeMarginLeft` | `f32` (FloatDefault) | `5.0` | Полоса у левого края для drag-to-snap, пикселей. |
| `snap_edge_margin_right` | `snapEdgeMarginRight` | `f32` (FloatDefault) | `5.0` | Полоса у правого края для drag-to-snap, пикселей. |
| `centered_directional_move` | `centeredDirectionalMove` | `Option<bool>` (OptionalBoolDefault) | `None` | «Переместить к краю» центрирует окно по второй оси; не задано — да. |
| `resize_on_directional_move` | `resizeOnDirectionalMove` | `bool` (BoolDefault) | `false` | «Переместить к краю» ещё и меняет размер окна. |
| `move_fixed_size_to_edge` | `moveFixedSizeToEdge` | `EdgeAlignment` (IntEnumDefault) | `EdgesAndCorners` | Куда прижимать окно, которое приложение не дало растянуть (1 края и углы, 2 только углы, 3 по центру). |
| `ignored_snap_areas` | `ignoredSnapAreas` | `i64` (IntDefault) | `0` | Выключенные зоны drag-to-snap (битовая маска SnapAreaOption); старый формат, оригинал переводит его в карты зон. |
| `traverse_single_screen` | `traverseSingleScreen` | `Option<bool>` (OptionalBoolDefault) | `None` | «Следующий/предыдущий дисплей» при одном экране считает его соседом самого себя. |
| `use_cursor_screen_detection` | `useCursorScreenDetection` | `bool` (BoolDefault) | `false` | Экран для действия определять по курсору, а не по окну. Экспорт: нет. |
| `minimum_window_width` | `minimumWindowWidth` | `f64` (DoubleDefault) | `0.25` | Минимальная ширина окна при «меньше», доля экрана. |
| `minimum_window_height` | `minimumWindowHeight` | `f64` (DoubleDefault) | `0.25` | Минимальная высота окна при «меньше», доля экрана. |
| `size_offset` | `sizeOffset` | `f32` (FloatDefault) | `0.0` | Шаг «больше/меньше», пикселей; 0 и меньше — 30. |
| `width_step_size` | `widthStepSize` | `f32` (FloatDefault) | `30.0` | Шаг «шире/уже», пикселей. |
| `unsnap_restore` | `unsnapRestore` | `Option<bool>` (OptionalBoolDefault) | `None` | Возвращать прежний размер окну, которое утащили из прилипшего положения; не задано — да. |
| `unsnap_restore_from_size_change` | `unsnapRestoreFromSizeChange` | `Option<bool>` (OptionalBoolDefault) | `None` | То же для окна после «больше/меньше»: «нет» — не возвращать. Экспорт: нет. |
| `curtain_change_size` | `curtainChangeSize` | `Option<bool>` (OptionalBoolDefault) | `None` | «Больше/меньше» у окна, прижатого к краю, двигает только свободный край; не задано — да. |
| `smaller_shrinks_maximized_height` | `smallerShrinksMaximizedHeight` | `bool` (BoolDefault) | `false` | «Меньше» уменьшает и высоту окна, развёрнутого по высоте. |
| `relaunch_opens_menu` | `relaunchOpensMenu` | `bool` (BoolDefault) | `false` | Повторный запуск приложения открывает меню, а не настройки. |
| `obtain_window_on_click` | `obtainWindowOnClick` | `Option<bool>` (OptionalBoolDefault) | `None` | Drag-to-snap берёт окно под курсором уже при нажатии кнопки мыши; не задано — да. |
| `screen_edge_gap_top` | `screenEdgeGapTop` | `f32` (FloatDefault) | `0.0` | Отступ рабочей области от верхнего края экрана, пикселей. |
| `screen_edge_gap_bottom` | `screenEdgeGapBottom` | `f32` (FloatDefault) | `0.0` | Отступ рабочей области от нижнего края экрана, пикселей. |
| `screen_edge_gap_left` | `screenEdgeGapLeft` | `f32` (FloatDefault) | `0.0` | Отступ рабочей области от левого края экрана, пикселей. |
| `screen_edge_gap_right` | `screenEdgeGapRight` | `f32` (FloatDefault) | `0.0` | Отступ рабочей области от правого края экрана, пикселей. |
| `screen_edge_gaps_on_main_screen_only` | `screenEdgeGapsOnMainScreenOnly` | `bool` (BoolDefault) | `false` | Отступы от краёв — только на главном экране. |
| `screen_edge_gap_top_notch` | `screenEdgeGapTopNotch` | `f32` (FloatDefault) | `0.0` | Отступ сверху на экране с вырезом камеры вместо screen_edge_gap_top, пикселей; 0 — как у остальных. |
| `last_version` | `lastVersion` | `Option<String>` (StringDefault) | `None` | Номер сборки при прошлом запуске (для миграций между версиями). Экспорт: нет. |
| `install_version` | `installVersion` | `Option<String>` (StringDefault) | `None` | Номер сборки при первом запуске (оригиналу нужен для шорткатов по умолчанию). Экспорт: нет. |
| `show_all_actions_in_menu` | `showAllActionsInMenu` | `Option<bool>` (OptionalBoolDefault) | `None` | Показывать в меню все действия, а не только основные. |
| `show_additional_sizes_in_menu` | `showAdditionalSizesInMenu` | `Option<bool>` (OptionalBoolDefault) | `None` | Показывать в меню подменю дополнительных размеров. |
| `su_has_launched_before` | `SUHasLaunchedBefore` | `bool` (bool(forKey:), только чтение) | `false` | Sparkle: приложение уже запускалось (оригинал ключ только читает). Экспорт: нет. |
| `footprint_alpha` | `footprintAlpha` | `f32` (FloatDefault) | `0.3` | Непрозрачность подсветки будущего положения окна при drag-to-snap. |
| `footprint_border_width` | `footprintBorderWidth` | `f32` (FloatDefault) | `2.0` | Толщина рамки подсветки, пикселей. |
| `footprint_fade` | `footprintFade` | `Option<bool>` (OptionalBoolDefault) | `None` | Плавное появление подсветки; «нет» — без анимации прозрачности. |
| `footprint_color` | `footprintColor` | `Option<FootprintColor>` (JSONDefault) | `None` | Цвет подсветки; не задан — чёрный. |
| `su_enable_automatic_checks` | `SUEnableAutomaticChecks` | `bool` (BoolDefault) | `false` | Sparkle: проверять обновления автоматически. |
| `todo` | `todo` | `Option<bool>` (OptionalBoolDefault) | `None` | Режим Todo включён в настройках (появляются его пункты меню и шорткаты). |
| `todo_mode` | `todoMode` | `bool` (BoolDefault) | `false` | Режим Todo сейчас активен (приложение Todo стоит боковой панелью). |
| `todo_application` | `todoApplication` | `Option<String>` (StringDefault) | `None` | Bundle id приложения для боковой панели Todo. |
| `todo_sidebar_width` | `todoSidebarWidth` | `f32` (FloatDefault) | `400.0` | Ширина панели Todo в единицах todo_sidebar_width_unit. |
| `todo_sidebar_width_unit` | `todoSidebarWidthUnit` | `TodoSidebarWidthUnit` (IntEnumDefault) | `Pixels` | Единица ширины панели Todo (1 пиксели, 2 проценты). |
| `todo_sidebar_side` | `todoSidebarSide` | `TodoSidebarSide` (IntEnumDefault) | `Right` | Сторона панели Todo (1 справа, 2 слева). |
| `snap_modifiers` | `snapModifiers` | `i64` (IntDefault) | `0` | Модификаторы, которые надо держать для drag-to-snap (NSEventModifierFlags); 0 — не нужны. |
| `attempt_match_on_next_prev_display` | `attemptMatchOnNextPrevDisplay` | `Option<bool>` (OptionalBoolDefault) | `None` | При переносе на другой экран сохранять раскладку окна (половина остаётся половиной). |
| `alt_third_cycle` | `altThirdCycle` | `Option<bool>` (OptionalBoolDefault) | `None` | Альтернативный перебор третей; в коде оригинала больше не читается. |
| `center_half_cycles` | `centerHalfCycles` | `Option<bool>` (OptionalBoolDefault) | `None` | «Центр» перебирает размеры при каждом нажатии, в любом режиме повторов. |
| `cycling_overlap_offset` | `cyclingOverlapOffset` | `Option<bool>` (OptionalBoolDefault) | `None` | Сдвигать лесенкой окна, поставленные в одну и ту же позицию. |
| `cycling_overlap_offset_size` | `cyclingOverlapOffsetSize` | `f32` (FloatDefault) | `11.0` | Шаг лесенки, пикселей. |
| `cycling_overlap_max_cascade` | `cyclingOverlapMaxCascade` | `i64` (IntDefault) | `1` | Сколько ступеней в лесенке (оригинал ограничивает 1…5). |
| `stack_badge` | `stackBadge` | `Option<bool>` (OptionalBoolDefault) | `None` | Значок-счётчик у окон, стоящих стопкой в одной позиции. |
| `full_ignore_bundle_ids` | `fullIgnoreBundleIds` | `Option<Vec<String>>` (JSONDefault) | `None` | Приложения, которых drag-to-snap не касается вовсе; не задано — встроенный список оригинала. |
| `notified_of_problem_apps` | `notifiedOfProblemApps` | `bool` (BoolDefault) | `false` | Предупреждение о приложениях, несовместимых с drag-to-snap, уже показано. |
| `specified_height` | `specifiedHeight` | `f32` (FloatDefault) | `1050.0` | Высота «заданного размера»: до 1 — доля экрана, больше — пиксели. |
| `specified_width` | `specifiedWidth` | `f32` (FloatDefault) | `1680.0` | Ширина «заданного размера»: до 1 — доля экрана, больше — пиксели. |
| `horizontal_split_ratio` | `horizontalSplitRatio` | `f32` (FloatDefault) | `50.0` | Где делить экран на левую и правую половины, проценты. |
| `vertical_split_ratio` | `verticalSplitRatio` | `f32` (FloatDefault) | `50.0` | Где делить экран на верхнюю и нижнюю половины, проценты. |
| `move_cursor_across_displays` | `moveCursorAcrossDisplays` | `Option<bool>` (OptionalBoolDefault) | `None` | Переносить курсор вместе с окном на другой экран. |
| `move_cursor` | `moveCursor` | `Option<bool>` (OptionalBoolDefault) | `None` | Ставить курсор в центр окна после действия с клавиатуры. |
| `auto_maximize` | `autoMaximize` | `Option<bool>` (OptionalBoolDefault) | `None` | Развёрнутое окно при переносе на другой экран разворачивается и там; не задано — да. |
| `apply_gaps_to_maximize` | `applyGapsToMaximize` | `Option<bool>` (OptionalBoolDefault) | `None` | Зазоры и у «развернуть»; не задано — да. |
| `apply_gaps_to_maximize_height` | `applyGapsToMaximizeHeight` | `Option<bool>` (OptionalBoolDefault) | `None` | Зазоры и у «развернуть по высоте»; не задано — да. |
| `corner_snap_area_size` | `cornerSnapAreaSize` | `f32` (FloatDefault) | `20.0` | Размер угловой зоны drag-to-snap, пикселей. |
| `short_edge_snap_area_size` | `shortEdgeSnapAreaSize` | `f32` (FloatDefault) | `145.0` | Длина зоны у короткого края в составных зонах drag-to-snap, пикселей. |
| `cascade_all_delta_size` | `cascadeAllDeltaSize` | `f32` (FloatDefault) | `30.0` | Шаг «каскадом», пикселей. |
| `sixths_snap_area` | `sixthsSnapArea` | `Option<bool>` (OptionalBoolDefault) | `None` | Старый флаг зон «шестые» сверху и снизу; оригинал переводит его в карты зон. |
| `stage_size` | `stageSize` | `f32` (FloatDefault) | `190.0` | Сколько оставлять под полосу Stage Manager: до 1 — доля ширины, больше — пиксели, 0 и меньше — не оставлять. |
| `drag_from_stage` | `dragFromStage` | `Option<bool>` (OptionalBoolDefault) | `None` | Drag-to-snap для окон, вытаскиваемых из полосы Stage Manager; «нет» — выключить. |
| `always_account_for_stage` | `alwaysAccountForStage` | `Option<bool>` (OptionalBoolDefault) | `None` | «Центр» и «центр крупно» учитывают полосу Stage Manager всегда. |
| `landscape_snap_areas` | `landscapeSnapAreas` | `Option<SnapAreas>` (JSONDefault) | `None` | Зоны drag-to-snap для горизонтальных экранов; не задано — default_landscape_snap_areas(). |
| `portrait_snap_areas` | `portraitSnapAreas` | `Option<SnapAreas>` (JSONDefault) | `None` | Зоны drag-to-snap для вертикальных экранов; не задано — default_portrait_snap_areas(). |
| `mission_control_dragging` | `missionControlDragging` | `Option<bool>` (OptionalBoolDefault) | `None` | Не мешать перетаскиванию окна в Mission Control; «нет» — перехватывать события активно. |
| `enhanced_ui` | `enhancedUI` | `EnhancedUI` (IntEnumDefault) | `DisableEnable` | Обход AXEnhancedUserInterface (1 выключать на время действия, 2 выключать насовсем, 3 выключать при смене приложения). |
| `footprint_animation_duration_multiplier` | `footprintAnimationDurationMultiplier` | `f32` (FloatDefault) | `0.0` | Множитель длительности анимации подсветки; 0 — без анимации размера. |
| `haptic_feedback_on_snap` | `hapticFeedbackOnSnap` | `Option<bool>` (OptionalBoolDefault) | `None` | Отклик трекпада при попадании в зону drag-to-snap. |
| `mission_control_dragging_allowed_offscreen_distance` | `missionControlDraggingAllowedOffscreenDistance` | `f32` (FloatDefault) | `25.0` | На сколько пикселей окно можно увести за верх экрана, прежде чем считать это жестом Mission Control. |
| `mission_control_dragging_disallowed_duration` | `missionControlDraggingDisallowedDuration` | `i64` (IntDefault) | `250` | Сколько миллисекунд после такого жеста drag-to-snap не срабатывает. |
| `double_click_title_bar` | `doubleClickTitleBar` | `i64` (IntDefault) | `0` | Действие по двойному клику по заголовку окна: rawValue WindowAction + 1; 0 — ничего. |
| `double_click_title_bar_restore` | `doubleClickTitleBarRestore` | `Option<bool>` (OptionalBoolDefault) | `None` | Повторный двойной клик по заголовку возвращает прежний размер; не задано — да. |
| `double_click_title_bar_ignored_apps` | `doubleClickTitleBarIgnoredApps` | `Option<Vec<String>>` (JSONDefault) | `None` | Приложения, где двойной клик по заголовку не трогаем (этот же ключ читает double_click_tool_bar_ignored_apps()). |
| `ignore_drag_snap_too` | `ignoreDragSnapToo` | `Option<bool>` (OptionalBoolDefault) | `None` | «Игнорировать приложение» выключает для него и drag-to-snap; не задано — да. |
| `system_wide_mouse_down` | `systemWideMouseDown` | `Option<bool>` (OptionalBoolDefault) | `None` | Окно под курсором искать через системный AX-элемент; не задано — только для system_wide_mouse_down_apps. |
| `system_wide_mouse_down_apps` | `systemWideMouseDownApps` | `BTreeSet<String>` (JSONDefault (defaultValue)) | `{"com.microsoft.teams2", "org.languagetool.desktop"}` | Приложения, для которых окно под курсором ищется через системный AX-элемент. |
| `internal_tiling_notified` | `internalTilingNotified` | `bool` (BoolDefault) | `false` | Предупреждение о встроенной раскладке окон macOS уже показано. Экспорт: нет. |
| `screens_ordered_by_x` | `screensOrderedByX` | `ScreenOrdering` (IntEnumDefault) | `YThenMinX` | Порядок экранов (1 по середине по X, 2 по левому краю, 3 сверху вниз, затем слева направо). |
| `combined_display_mode` | `combinedDisplayMode` | `Option<bool>` (OptionalBoolDefault) | `None` | Все экраны как один большой (когда у экранов общие Spaces). Экспорт: нет. |
| `green_button_override` | `greenButtonOverride` | `bool` (BoolDefault) | `false` | Зелёная кнопка окна выполняет действие Rectangle. |
| `access_prompted` | `r2AccessPrompted` | `bool` (BoolDefault) | `false` | Rust-only: системный запрос доступа уже показывали (чтобы не спрашивать на каждом запуске). Экспорт: нет. |
| `app_columns_bundle_ids` | `r2AppColumnsBundleIds` | `Option<BTreeSet<String>>` (JSONDefault) | `None` | Rust-only: bundle id приложений, чьи окна держим столбиками (пункт меню «Держать окна «…» столбиками»). Экспорт: нет. |

## Горячие клавиши

В порте их нет (управление только мышью), поэтому не читаются и не пишутся:
шорткаты действий (ключи `leftHalf`, `columnFive1`, … — словари MASShortcut),
шорткаты Todo (`toggleTodo`, `reflowTodo`), `alternateDefaultShortcuts`,
`allowAnyShortcut`. В JSON-экспорте `shortcuts` — пустой объект, при импорте
эта секция пропускается.

## Производные значения

- `double_click_tool_bar_ignored_apps()` — `doubleClickToolBarIgnoredApps` оригинала:
тот же ключ `doubleClickTitleBarIgnoredApps`, прочитанный как множество;
пусто — `["epp.package.java"]`. Отдельного ключа нет, в экспорт не входит.
- `effective_cycle_sizes()`, `landscape_snap_areas_or_default()`,
`portrait_snap_areas_or_default()`, `ignored_snap_area_options()`.

## Служебные ключи порта

- `r2AccessPrompted` — поле `access_prompted`.
- `r2AppColumnsBundleIds` — поле `app_columns_bundle_ids` (режим «держать окна
столбиками», есть только в порте).
- `r2ConfigConfMigrated` — старый `config.conf` уже перенесён в UserDefaults.
