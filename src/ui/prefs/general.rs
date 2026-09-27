//! Вкладка «Основные» (`SettingsViewController` оригинала, docs/ui-spec.md §6).
//!
//! Контролы и их порядок — как в storyboard и вставках кодом, без того, что
//! нужно только горячим клавишам («Разрешить любое сочетание клавиш», поля
//! Toggle/Reflow Todo). Каждый контрол при показе читает свою настройку и при
//! изменении пишет её через `config::update`; побочные эффекты (иконка в строке
//! меню, пересборка меню, перезагрузка подсистем) делают подписчики настроек.
//!
//! Содержимое лежит в прокрутке: если окно не помещается по высоте на экран,
//! оно становится ниже, а вкладка прокручивается (в оригинале прокрутки нет,
//! docs/ui-spec.md §18.5).

use std::cell::{Cell, RefCell};
use std::path::Path;
use std::rc::Rc;
use std::time::Duration;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{define_class, msg_send, sel, MainThreadMarker, MainThreadOnly, Message};
use objc2_app_kit::{
    NSAlert, NSAlertFirstButtonReturn, NSAlertStyle, NSAnimatablePropertyContainer, NSApplication,
    NSBezelStyle, NSBorderType, NSButton, NSCellImagePosition, NSColor, NSControl,
    NSControlTextEditingDelegate, NSEventType, NSFont, NSLayoutAttribute,
    NSLayoutConstraintOrientation, NSModalResponseOK, NSOpenPanel, NSPopUpButton, NSSavePanel,
    NSScreen, NSScrollView, NSSlider, NSStackView, NSStackViewDistribution, NSText,
    NSTextAlignment, NSTextField, NSTextFieldDelegate, NSUserInterfaceLayoutOrientation, NSView,
    NSViewController, NSWorkspace,
};
use objc2_foundation::{NSArray, NSLocale, NSNotification, NSProcessInfo, NSURL};

use super::logic::{self, NumberInput, NumberSetting, Separators};
use crate::config::{self, Config, CornerCycleExpansionAxis, CycleSize, SubsequentExecutionMode};
use crate::config::{TodoSidebarSide, TodoSidebarWidthUnit};
use crate::ui::controls::{self, ns, DEFAULT_HIGH, DEFAULT_LOW, REQUIRED};
use crate::{app_delegate, launch_on_login, log, snapping};

/// Ширина вкладки и колонки настроек (storyboard: `width = 850`, колонка 500).
pub const VIEW_WIDTH: f64 = 850.0;
const COLUMN_WIDTH: f64 = 500.0;
/// Отступы колонки сверху и снизу (`top + 20`, `bottom + 26`).
const TOP_INSET: f64 = 20.0;
const BOTTOM_INSET: f64 = 26.0;
const GAP_SLIDER_MAX: i32 = 100;
const STAGE_SLIDER_MAX: i32 = 250;

// ---------------------------------------------------------------- цель действий

define_class!(
    /// Цель действий контролов вкладки и поповера «Ещё» и делегат числовых
    /// полей — то, чем в оригинале был `SettingsViewController`.
    #[unsafe(super(NSObject))]
    #[name = "R2PrefsGeneralTarget"]
    #[thread_kind = MainThreadOnly]
    pub struct Target;

    unsafe impl NSObjectProtocol for Target {}

    unsafe impl NSControlTextEditingDelegate for Target {
        #[unsafe(method(controlTextDidChange:))]
        fn control_text_did_change(&self, notification: &NSNotification) {
            if let Some(field) = notification_field(notification) {
                number_text_changed(&field);
            }
        }

        #[unsafe(method(controlTextDidEndEditing:))]
        fn control_text_did_end_editing(&self, notification: &NSNotification) {
            if let Some(field) = notification_field(notification) {
                number_editing_ended(&field);
            }
        }

        #[unsafe(method(control:textShouldEndEditing:))]
        fn control_text_should_end_editing(&self, control: &NSControl, _editor: &NSText) -> bool {
            number_may_end_editing(control)
        }
    }

    unsafe impl NSTextFieldDelegate for Target {}

    impl Target {
        #[unsafe(method(toggleLaunchOnLogin:))]
        fn toggle_launch_on_login(&self, sender: &NSButton) {
            let on = controls::is_on(sender);
            launch_on_login::set_enabled(on);
            if launch_on_login::is_enabled() != on {
                log!("Запуск при входе в систему: система не приняла «{}»", if on { "вкл." } else { "выкл." });
            }
            write(|config| config.launch_on_login = on);
        }

        #[unsafe(method(toggleHideMenuBarIcon:))]
        fn toggle_hide_menu_bar_icon(&self, sender: &NSButton) {
            // Иконку прячет/показывает подписка на настройки (app_delegate → menu).
            let on = controls::is_on(sender);
            write(|config| config.hide_menu_bar_icon = on);
        }

        #[unsafe(method(setSubsequentExecutionBehavior:))]
        fn set_subsequent_execution_behavior(&self, sender: &NSPopUpButton) {
            let tag = sender.selectedTag();
            let Some(mode) = SubsequentExecutionMode::from_raw(tag as i64) else {
                log!("Повторяющиеся команды: нет режима с tag {tag}");
                return;
            };
            write(|config| config.subsequent_execution_mode = mode);
            with_tab(|tab| tab.update_cycle_sizes_view(&config::current(), true));
        }

        #[unsafe(method(didCheckCycleSizeCheckbox:))]
        fn did_check_cycle_size_checkbox(&self, sender: &NSButton) {
            let Some(size) = CycleSize::from_raw(sender.tag() as i64) else {
                log!("Размер перебора: нет размера с tag {}", sender.tag());
                return;
            };
            let on = controls::is_on(sender);
            write(|config| logic::toggle_cycle_size(config, size, on));
        }

        #[unsafe(method(setCornerCycleExpansionAxis:))]
        fn set_corner_cycle_expansion_axis(&self, sender: &NSButton) {
            let Some(axis) = CornerCycleExpansionAxis::from_raw(sender.tag() as i64) else {
                log!("Ось углов: нет оси с tag {}", sender.tag());
                return;
            };
            write(|config| config.corner_cycle_expansion_axis = axis);
            with_tab(|tab| tab.update_corner_axis(axis));
        }

        #[unsafe(method(toggleCooperativeCornerResize:))]
        fn toggle_cooperative_corner_resize(&self, sender: &NSButton) {
            let on = controls::is_on(sender);
            write(|config| config.cooperative_corner_resize = on);
        }

        #[unsafe(method(gapSliderChanged:))]
        fn gap_slider_changed(&self, sender: &NSSlider) {
            let position = sender.intValue();
            with_tab(|tab| tab.gap_label.setStringValue(&ns(&logic::slider_label(position))));
            // Пишется только по отпусканию мыши или с клавиатуры, не на каждый шаг.
            if !is_final_slider_event(self.mtm()) {
                return;
            }
            let gap = logic::gap_from_slider(position);
            if config::with(|config| config.gap_size) != gap {
                write(|config| config.gap_size = gap);
                with_tab(|tab| {
                    tab.skip_gap_top_edge
                        .setHidden(!logic::shows_skip_gap_top_edge(&config::current()));
                    tab.relayout(true);
                });
            }
        }

        #[unsafe(method(toggleSkipGapTopEdge:))]
        fn toggle_skip_gap_top_edge(&self, sender: &NSButton) {
            let on = controls::is_on(sender);
            write(|config| config.skip_gap_top_edge = on);
        }

        #[unsafe(method(toggleCursorMove:))]
        fn toggle_cursor_move(&self, sender: &NSButton) {
            let on = controls::is_on(sender);
            write(|config| config.move_cursor_across_displays = Some(on));
        }

        #[unsafe(method(toggleUseCursorScreenDetection:))]
        fn toggle_use_cursor_screen_detection(&self, sender: &NSButton) {
            let on = controls::is_on(sender);
            write(|config| config.use_cursor_screen_detection = on);
        }

        #[unsafe(method(toggleDoubleClickTitleBar:))]
        fn toggle_double_click_title_bar(&self, sender: &NSButton) {
            let on = controls::is_on(sender);
            if on && logic::double_click_conflicts_with_system(
                    crate::title_bar::system_double_click_action().as_deref(),
                ) {
                warn_double_click_conflict(self.mtm());
            }
            // Настройка пишется в любом случае, как в оригинале.
            write(|config| config.double_click_title_bar = logic::double_click_title_bar_value(on));
        }

        #[unsafe(method(toggleAutoMaximize:))]
        fn toggle_auto_maximize(&self, sender: &NSButton) {
            let on = controls::is_on(sender);
            write(|config| config.auto_maximize = Some(on));
        }

        #[unsafe(method(toggleGreenButtonOverride:))]
        fn toggle_green_button_override(&self, sender: &NSButton) {
            let on = controls::is_on(sender);
            write(|config| config.green_button_override = on);
        }

        #[unsafe(method(toggleCombinedDisplayMode:))]
        fn toggle_combined_display_mode(&self, sender: &NSButton) {
            let on = controls::is_on(sender);
            write(|config| config.combined_display_mode = Some(on));
        }

        #[unsafe(method(toggleTodoMode:))]
        fn toggle_todo_mode(&self, sender: &NSButton) {
            let on = controls::is_on(sender);
            write(|config| config.todo = Some(on));
            with_tab(|tab| tab.update_todo_view(&config::current(), true));
        }

        #[unsafe(method(showTodoModeHelp:))]
        fn show_todo_mode_help(&self, _sender: Option<&AnyObject>) {
            super::about_todo::show(self.mtm());
        }

        #[unsafe(method(setTodoWidthUnit:))]
        fn set_todo_width_unit(&self, sender: &NSPopUpButton) {
            let tag = sender.selectedTag();
            let Some(unit) = TodoSidebarWidthUnit::from_raw(tag as i64) else {
                log!("Единица ширины Todo: нет единицы с tag {tag}");
                return;
            };
            // Как `TodoManager.setTodoSidebarWidthUnit`: ширина пересчитывается по экрану
            // Todo-окна (рабочая область с отступами и Stage Manager), единица и ширина
            // записываются в настройки — поле показывает, что получилось.
            let width = crate::todo::set_sidebar_width_unit(unit);
            with_tab(|tab| {
                tab.todo_width
                    .setStringValue(&ns(&NumberSetting::TodoSidebarWidth.display(width, &separators())));
            });
        }

        #[unsafe(method(setTodoAppSide:))]
        fn set_todo_app_side(&self, sender: &NSPopUpButton) {
            let tag = sender.selectedTag();
            let Some(side) = TodoSidebarSide::from_raw(tag as i64) else {
                log!("Сторона Todo: нет стороны с tag {tag}");
                return;
            };
            write(|config| config.todo_sidebar_side = side);
        }

        #[unsafe(method(stageSliderChanged:))]
        fn stage_slider_changed(&self, sender: &NSSlider) {
            with_tab(|tab| {
                if let Some((_, label)) = &tab.stage {
                    label.setStringValue(&ns(&logic::slider_label(sender.intValue())));
                }
            });
            if !is_final_slider_event(self.mtm()) {
                return;
            }
            let value = logic::stage_size_from_slider(sender.floatValue());
            if config::with(|config| config.stage_size) != value {
                write(|config| config.stage_size = value);
            }
        }

        #[unsafe(method(restoreDefaults:))]
        fn restore_defaults(&self, _sender: Option<&AnyObject>) {
            if confirm_restore_defaults(self.mtm()) {
                write(|config| {
                    config.landscape_snap_areas = None;
                    config.portrait_snap_areas = None;
                });
            }
        }

        #[unsafe(method(exportConfig:))]
        fn export_config(&self, _sender: Option<&AnyObject>) {
            // Пока открыта панель, прилипание на паузе (`windowSnapping(false)` оригинала).
            snapping::paused(|| export_with_panel(self.mtm()));
        }

        #[unsafe(method(importConfig:))]
        fn import_config(&self, _sender: Option<&AnyObject>) {
            snapping::paused(|| import_with_panel(self.mtm()));
        }

        #[unsafe(method(showExtraSettings:))]
        fn show_extra_settings(&self, sender: &NSButton) {
            super::extras::show(self, sender);
        }

        // ---- поповер «Ещё»

        #[unsafe(method(toggleShowAdditionalSizesInMenu:))]
        fn toggle_show_additional_sizes_in_menu(&self, sender: &NSButton) {
            // Меню перестраивает себя само по подписке на настройки.
            let on = controls::is_on(sender);
            write(|config| config.show_additional_sizes_in_menu = Some(on));
        }

        #[unsafe(method(toggleCyclingOverlapOffset:))]
        fn toggle_cycling_overlap_offset(&self, sender: &NSButton) {
            let on = controls::is_on(sender);
            write(|config| config.cycling_overlap_offset = Some(on));
        }

        #[unsafe(method(toggleStackBadge:))]
        fn toggle_stack_badge(&self, sender: &NSButton) {
            let on = controls::is_on(sender);
            write(|config| config.stack_badge = Some(on));
        }

        #[unsafe(method(didSelectHalfSplitRatioPreset:))]
        fn did_select_half_split_ratio_preset(&self, sender: &NSPopUpButton) {
            super::extras::split_preset_selected(sender);
        }
    }
);

impl Target {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm);
        unsafe { msg_send![this, init] }
    }
}

/// Поле ввода — отправитель уведомления редактирования.
fn notification_field(notification: &NSNotification) -> Option<Retained<NSTextField>> {
    notification.object()?.downcast::<NSTextField>().ok()
}

/// Последнее событие — отпускание мыши или клавиша: ползунок отпустили.
fn is_final_slider_event(mtm: MainThreadMarker) -> bool {
    NSApplication::sharedApplication(mtm)
        .currentEvent()
        .is_some_and(|event| {
            matches!(
                event.r#type(),
                NSEventType::LeftMouseUp | NSEventType::KeyDown
            )
        })
}

// ---------------------------------------------------------------- запись настроек

thread_local! {
    /// Настройки сейчас меняет само окно: свой подписчик их не перечитывает.
    static WRITING: Cell<bool> = const { Cell::new(false) };
    static TAB: RefCell<Option<Rc<GeneralTab>>> = const { RefCell::new(None) };
    static TARGET: RefCell<Option<Retained<Target>>> = const { RefCell::new(None) };
}

/// Изменить настройки из окна: `config::update`, но без перечитывания контролов
/// (контрол, который их поменял, уже показывает новое значение).
pub(super) fn write(change: impl FnOnce(&mut Config)) {
    WRITING.with(|writing| writing.set(true));
    config::update(change);
    WRITING.with(|writing| writing.set(false));
}

fn with_tab(f: impl FnOnce(&GeneralTab)) {
    let tab = TAB.with(|slot| slot.borrow().clone());
    if let Some(tab) = tab {
        f(&tab);
    }
}

/// Цель действий (одна на процесс).
pub(super) fn target(mtm: MainThreadMarker) -> Retained<Target> {
    TARGET.with(|slot| {
        slot.borrow_mut()
            .get_or_insert_with(|| Target::new(mtm))
            .clone()
    })
}

/// Настройки поменяли не из окна (импорт, меню, ссылка, иконку убрали из
/// строки меню): показать новые значения.
pub(super) fn config_changed(_old: &Config, new: &Config) {
    if WRITING.with(Cell::get) {
        return;
    }
    with_tab(|tab| tab.apply(new, Refresh::External));
    super::extras::apply(new);
}

/// Разделители чисел текущей локали.
pub(super) fn separators() -> Separators {
    let locale = NSLocale::currentLocale();
    Separators {
        decimal: locale.decimalSeparator().to_string(),
        grouping: locale.groupingSeparator().to_string(),
    }
}

// ---------------------------------------------------------------- вкладка

/// Что заставило перечитать контролы.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Refresh {
    /// Окно только что создано.
    Initial,
    /// Импорт конфига (`configImported`): как при создании, но с тем, что уже есть.
    Import,
    /// Настройку поменяли снаружи или окно показывают снова.
    External,
}

pub(super) struct GeneralTab {
    mtm: MainThreadMarker,
    root: Retained<NSScrollView>,
    document: Retained<NSView>,
    launch_on_login: Retained<NSButton>,
    hide_menu_bar_icon: Retained<NSButton>,
    subsequent_execution: Retained<NSPopUpButton>,
    cycle_sizes_view: Retained<NSStackView>,
    cycle_size_checkboxes: Vec<(CycleSize, Retained<NSButton>)>,
    corner_axis_buttons: Vec<(CornerCycleExpansionAxis, Retained<NSButton>)>,
    cooperative_corner_resize: Option<Retained<NSButton>>,
    gap_slider: Retained<NSSlider>,
    gap_label: Retained<NSTextField>,
    skip_gap_top_edge: Retained<NSButton>,
    cursor_across: Retained<NSButton>,
    use_cursor_screen_detection: Retained<NSButton>,
    double_click_title_bar: Retained<NSButton>,
    auto_maximize: Retained<NSButton>,
    green_button_override: Retained<NSButton>,
    combined_display_mode: Option<Retained<NSButton>>,
    todo: Retained<NSButton>,
    todo_view: Retained<NSStackView>,
    todo_width: Retained<NSTextField>,
    todo_width_unit: Retained<NSPopUpButton>,
    todo_side: Retained<NSPopUpButton>,
    /// Ползунок и подпись Stage Manager (блока нет до macOS 13).
    stage: Option<(Retained<NSSlider>, Retained<NSTextField>)>,
}

/// Создать вкладку (один раз на процесс) и вернуть её контроллер.
pub(super) fn build(mtm: MainThreadMarker) -> Retained<NSViewController> {
    let tab = Rc::new(GeneralTab::new(mtm));
    let controller = NSViewController::new(mtm);
    controller.setView(&tab.root);
    tab.apply(&config::current(), Refresh::Initial);
    TAB.with(|slot| *slot.borrow_mut() = Some(tab));
    controller
}

/// Высота, при которой вкладка видна целиком.
pub(super) fn content_height() -> f64 {
    let mut height = 0.0;
    with_tab(|tab| {
        tab.document.layoutSubtreeIfNeeded();
        height = tab.document.fittingSize().height;
    });
    height
}

/// Перечитать контролы из настроек (показ окна снова).
pub(super) fn refresh() {
    with_tab(|tab| tab.apply(&config::current(), Refresh::External));
}

/// После импорта конфига — как `configImported` у оригинала.
pub(super) fn refresh_after_import() {
    with_tab(|tab| tab.apply(&config::current(), Refresh::Import));
    super::extras::apply(&config::current());
}

impl GeneralTab {
    fn new(mtm: MainThreadMarker) -> GeneralTab {
        let target = target(mtm);
        let target: &AnyObject = &target;
        let config = config::current();
        let app_name = app_delegate::app_name();

        // 1. «Запуск при входе в систему» и версия справа.
        let launch_on_login = controls::checkbox(
            mtm,
            "Запуск при входе в систему",
            target,
            sel!(toggleLaunchOnLogin:),
        );
        launch_on_login.setContentHuggingPriority_forOrientation(
            DEFAULT_LOW,
            NSLayoutConstraintOrientation::Horizontal,
        );
        let version = controls::label(mtm, &version_text());
        version.setTextColor(Some(&NSColor::secondaryLabelColor()));
        version.setContentHuggingPriority_forOrientation(
            DEFAULT_LOW + 1.0,
            NSLayoutConstraintOrientation::Horizontal,
        );
        version.setContentCompressionResistancePriority_forOrientation(
            DEFAULT_HIGH + 1.0,
            NSLayoutConstraintOrientation::Horizontal,
        );
        let launch_row = controls::row(mtm, 10.0, &[&launch_on_login, &version]);
        launch_row
            .heightAnchor()
            .constraintEqualToConstant(16.0)
            .setActive(true);

        // 2–3. Иконка в строке меню.
        let hide_menu_bar_icon = controls::checkbox(
            mtm,
            "Скрыть значок из строки меню",
            target,
            sel!(toggleHideMenuBarIcon:),
        );
        let hide_icon_note = controls::note(
            mtm,
            &format!(
                "Если значок строки меню скрыт, перезапустите программу {app_name} из Finder, чтобы открыть меню."
            ),
            COLUMN_WIDTH,
        );

        // 4. Обновлений в сборке нет: вместо контролов — пояснение (4c).
        let updates_note = controls::note(
            mtm,
            "Автообновления нет. Новые версии выходят на GitHub — пункт меню «Проверить обновления…» откроет страницу релизов.",
            420.0,
        );

        // 6. Повторяющиеся команды.
        let repeated_label = controls::label(mtm, "Повторяющиеся команды");
        repeated_label.setContentCompressionResistancePriority_forOrientation(
            REQUIRED,
            NSLayoutConstraintOrientation::Horizontal,
        );
        let items: Vec<(&str, isize)> = logic::SUBSEQUENT_EXECUTION_ITEMS
            .iter()
            .map(|(title, mode)| (*title, mode.raw() as isize))
            .collect();
        let subsequent_execution =
            controls::popup(mtm, &items, target, sel!(setSubsequentExecutionBehavior:));
        // Шрифт попапа в storyboard — HelveticaNeue 12.
        if let Some(font) = objc2_app_kit::NSFont::fontWithName_size(&ns("HelveticaNeue"), 12.0) {
            subsequent_execution.setFont(Some(&font));
        }
        let repeated_row = controls::row(mtm, 10.0, &[&repeated_label, &subsequent_execution]);

        // 7. Размеры перебора (§6.3).
        let cycle_size_checkboxes: Vec<(CycleSize, Retained<NSButton>)> = CycleSize::SORTED
            .iter()
            .map(|size| {
                let checkbox =
                    controls::checkbox(mtm, size.title(), target, sel!(didCheckCycleSizeCheckbox:));
                checkbox.setTag(size.raw() as isize);
                checkbox.setRefusesFirstResponder(true);
                (*size, checkbox)
            })
            .collect();
        let sizes_views: Vec<&NSView> = cycle_size_checkboxes
            .iter()
            .map(|(_, checkbox)| checkbox.as_ref())
            .collect();
        let sizes_row = controls::row(mtm, 8.0, &sizes_views);
        let axis_label = controls::label(mtm, "Углы при повторе растут:");
        axis_label.setContentCompressionResistancePriority_forOrientation(
            REQUIRED,
            NSLayoutConstraintOrientation::Horizontal,
        );
        let corner_axis_buttons: Vec<(CornerCycleExpansionAxis, Retained<NSButton>)> = [
            (CornerCycleExpansionAxis::Horizontal, "по горизонтали"),
            (CornerCycleExpansionAxis::Vertical, "по вертикали"),
        ]
        .into_iter()
        .map(|(axis, title)| {
            let button = controls::radio(mtm, title, target, sel!(setCornerCycleExpansionAxis:));
            button.setTag(axis.raw() as isize);
            button.setRefusesFirstResponder(true);
            (axis, button)
        })
        .collect();
        let axis_row = controls::row(
            mtm,
            8.0,
            &[
                &axis_label,
                &corner_axis_buttons[0].1,
                &corner_axis_buttons[1].1,
            ],
        );
        let cycle_sizes_view = controls::column(mtm, 8.0, &[&sizes_row, &axis_row]);
        // Фича скрыта в оригинале: флажок есть, только если уже включена.
        let cooperative_corner_resize =
            logic::shows_cooperative_corner_resize(&config).then(|| {
                let checkbox = controls::checkbox(
                    mtm,
                    "Менять размер соседних окон при переборе половин и углов",
                    target,
                    sel!(toggleCooperativeCornerResize:),
                );
                checkbox.setRefusesFirstResponder(true);
                cycle_sizes_view.addArrangedSubview(&checkbox);
                checkbox
            });

        // 8. Промежутки между окнами.
        let gap_title = controls::label(mtm, "Промежутки между окнами");
        gap_title.setContentHuggingPriority_forOrientation(
            DEFAULT_LOW + 1.0,
            NSLayoutConstraintOrientation::Horizontal,
        );
        let gap_slider = controls::slider(
            mtm,
            0.0,
            f64::from(GAP_SLIDER_MAX),
            target,
            sel!(gapSliderChanged:),
        );
        let gap_label = controls::label(mtm, "0 px");
        gap_label.setContentHuggingPriority_forOrientation(
            DEFAULT_LOW + 1.0,
            NSLayoutConstraintOrientation::Horizontal,
        );
        let gap_row = controls::stack(
            mtm,
            NSUserInterfaceLayoutOrientation::Horizontal,
            NSLayoutAttribute::Top,
            10.0,
            &[&gap_title, &gap_slider, &gap_label],
        );

        // 9, 11–13. Флажки.
        let skip_gap_top_edge = controls::checkbox(
            mtm,
            "Без промежутка у верхнего края экрана",
            target,
            sel!(toggleSkipGapTopEdge:),
        );
        let cursor_across = controls::checkbox(
            mtm,
            "Перемещать курсор вместе с окном между экранами",
            target,
            sel!(toggleCursorMove:),
        );
        let use_cursor_screen_detection = controls::checkbox(
            mtm,
            "Определять экран по положению курсора",
            target,
            sel!(toggleUseCursorScreenDetection:),
        );
        let double_click_title_bar = controls::checkbox(
            mtm,
            "Дважды щелкните по заголовку окна, чтобы увеличить/уменьшить его размер",
            target,
            sel!(toggleDoubleClickTitleBar:),
        );

        // 13.1–13.6: вставки кодом после двойного клика (§6.4).
        let auto_maximize = controls::checkbox(
            mtm,
            "Сохранять максимизацию при переносе окна на другой экран",
            target,
            sel!(toggleAutoMaximize:),
        );
        let green_button_override = controls::checkbox(
            mtm,
            "Зелёная кнопка максимизирует окно вместо полноэкранного режима",
            target,
            sel!(toggleGreenButtonOverride:),
        );
        let green_button_note = controls::note(
            mtm,
            "Чтобы получить обычное поведение macOS, удерживайте любую клавишу-модификатор или используйте меню окна",
            COLUMN_WIDTH,
        );
        let combined_display_mode = logic::shows_combined_display_mode(
            NSScreen::screensHaveSeparateSpaces(mtm),
        )
        .then(|| {
            controls::checkbox(
                mtm,
                "Считать несколько дисплеев одним",
                target,
                sel!(toggleCombinedDisplayMode:),
            )
        });

        // 15–17. Todo.
        let todo = controls::checkbox(
            mtm,
            "Показывать Todo режим в меню",
            target,
            sel!(toggleTodoMode:),
        );
        todo.setContentHuggingPriority_forOrientation(
            DEFAULT_HIGH,
            NSLayoutConstraintOrientation::Horizontal,
        );
        let todo_help = controls::push_button(mtm, "ⓘ", target, sel!(showTodoModeHelp:));
        todo_help.setBezelStyle(NSBezelStyle::SmallSquare);
        todo_help.setBordered(false);
        todo_help.setContentHuggingPriority_forOrientation(
            REQUIRED,
            NSLayoutConstraintOrientation::Horizontal,
        );
        let todo_spacer = NSView::new(mtm);
        todo_spacer.setTranslatesAutoresizingMaskIntoConstraints(false);
        todo_spacer.setContentHuggingPriority_forOrientation(
            1.0,
            NSLayoutConstraintOrientation::Horizontal,
        );
        let todo_row = controls::row(mtm, 5.0, &[&todo, &todo_help, &todo_spacer]);
        let todo_note = controls::note(
            mtm,
            "Держать выбранное приложение постоянно видимым в правой части основного экрана",
            COLUMN_WIDTH,
        );

        let todo_width_label = controls::label(mtm, "Ширина окна Todo приложения");
        todo_width_label.setAlignment(NSTextAlignment::Right);
        let todo_width = NSTextField::textFieldWithString(&ns("400"), mtm);
        todo_width.setAlignment(NSTextAlignment::Right);
        todo_width.setTranslatesAutoresizingMaskIntoConstraints(false);
        controls::fix_width(&todo_width, 87.0);
        // SAFETY: цель живёт до выхода (`TARGET`), делегат поля — слабая ссылка.
        unsafe { todo_width.setDelegate(Some(ProtocolObject::from_ref(&*self::target(mtm)))) };
        let todo_width_unit = controls::popup(
            mtm,
            &[
                (
                    TodoSidebarWidthUnit::Pixels.description(),
                    TodoSidebarWidthUnit::Pixels.raw() as isize,
                ),
                (
                    TodoSidebarWidthUnit::Pct.description(),
                    TodoSidebarWidthUnit::Pct.raw() as isize,
                ),
            ],
            target,
            sel!(setTodoWidthUnit:),
        );
        todo_width_unit.setFont(Some(&NSFont::messageFontOfSize(13.0)));
        let todo_side_label = controls::label(mtm, "Расположение Todo приложения");
        let todo_side = controls::popup(
            mtm,
            &[
                ("Слева", TodoSidebarSide::Left.raw() as isize),
                ("Справа", TodoSidebarSide::Right.raw() as isize),
            ],
            target,
            sel!(setTodoAppSide:),
        );
        todo_side.setFont(Some(&NSFont::messageFontOfSize(13.0)));
        // В storyboard ширина и сторона — одна строка, но по-русски подписи
        // длиннее и в 500 pt она не помещается: сторона — второй строкой,
        // подписи выровнены вправо, как у строк Toggle/Reflow Todo оригинала.
        todo_side_label.setAlignment(NSTextAlignment::Right);
        let todo_width_row = controls::row(
            mtm,
            8.0,
            &[&todo_width_label, &todo_width, &todo_width_unit],
        );
        let todo_side_row = controls::row(mtm, 8.0, &[&todo_side_label, &todo_side]);
        let todo_view = controls::column(mtm, 8.0, &[&todo_width_row, &todo_side_row]);
        // Ограничение между строками — когда у подписей уже есть общий предок.
        todo_side_label
            .widthAnchor()
            .constraintEqualToAnchor(&todo_width_label.widthAnchor())
            .setActive(true);

        // 19. Stage Manager (с macOS 13).
        let stage_view_and_controls = logic::shows_stage(
            NSProcessInfo::processInfo()
                .operatingSystemVersion()
                .majorVersion,
        )
        .then(|| {
            let title = controls::label(mtm, "Stage Manager область последних приложений");
            let slider = controls::slider(
                mtm,
                0.0,
                f64::from(STAGE_SLIDER_MAX),
                target,
                sel!(stageSliderChanged:),
            );
            let stage_label = controls::label(mtm, "190 px");
            stage_label.setContentHuggingPriority_forOrientation(
                DEFAULT_LOW + 1.0,
                NSLayoutConstraintOrientation::Horizontal,
            );
            let stage_row = controls::stack(
                mtm,
                NSUserInterfaceLayoutOrientation::Horizontal,
                NSLayoutAttribute::Top,
                8.0,
                &[&title, &slider, &stage_label],
            );
            let note = controls::note(
                mtm,
                "Если область слишком мала, последние приложения будут скрыты",
                COLUMN_WIDTH,
            );
            let separator = controls::separator(mtm);
            let view = controls::column(mtm, 8.0, &[&stage_row, &note, &separator]);
            // Строки блока — на всю ширину, ползунок забирает остаток.
            view.setHuggingPriority_forOrientation(
                REQUIRED,
                NSLayoutConstraintOrientation::Horizontal,
            );
            (view, (slider, stage_label))
        });

        // 20. Сброс, импорт, экспорт.
        let restore = controls::push_button(
            mtm,
            "Сбросить области прилипания",
            target,
            sel!(restoreDefaults:),
        );
        let import = controls::push_button(mtm, "Импорт", target, sel!(importConfig:));
        let export = controls::push_button(mtm, "Экспорт", target, sel!(exportConfig:));
        for (button, symbol) in [
            (&import, "square.and.arrow.down"),
            (&export, "square.and.arrow.up"),
        ] {
            button.setImage(controls::symbol(symbol).as_deref());
            button.setImagePosition(NSCellImagePosition::ImageLeft);
        }
        let import_export = controls::stack(
            mtm,
            NSUserInterfaceLayoutOrientation::Horizontal,
            NSLayoutAttribute::Top,
            10.0,
            &[&import, &export],
        );
        let buttons_row = controls::stack(
            mtm,
            NSUserInterfaceLayoutOrientation::Horizontal,
            NSLayoutAttribute::Top,
            0.0,
            &[&restore, &import_export],
        );
        buttons_row.setDistribution(NSStackViewDistribution::EqualSpacing);

        // 21. «⋯» — поповер «Ещё» (§7).
        let extras = controls::push_button(mtm, "⋯", target, sel!(showExtraSettings:));
        extras.setImage(controls::symbol("ellipsis.viewfinder").as_deref());
        extras.setImagePosition(NSCellImagePosition::ImageLeading);
        let extras_stack = controls::column(mtm, 0.0, &[&extras]);

        // Колонка настроек сверху вниз.
        let column = controls::column(mtm, 10.0, &[]);
        column.setDetachesHiddenViews(true);
        // Строки растягиваются на всю ширину колонки (storyboard: hugging 1000).
        column
            .setHuggingPriority_forOrientation(REQUIRED, NSLayoutConstraintOrientation::Horizontal);
        let mut views: Vec<Retained<NSView>> = vec![
            into_view(&launch_row),
            into_view(&hide_menu_bar_icon),
            into_view(&hide_icon_note),
            into_view(&updates_note),
            into_view(&controls::separator(mtm)),
            into_view(&repeated_row),
            into_view(&cycle_sizes_view),
            into_view(&gap_row),
            into_view(&skip_gap_top_edge),
            into_view(&cursor_across),
            into_view(&use_cursor_screen_detection),
            into_view(&double_click_title_bar),
            into_view(&auto_maximize),
            into_view(&green_button_override),
            into_view(&green_button_note),
        ];
        if let Some(combined) = &combined_display_mode {
            views.push(into_view(&controls::separator(mtm)));
            views.push(into_view(combined));
            views.push(into_view(&controls::note(
                mtm,
                "При нескольких дисплеях работает с ними как с одним. Для этого в Системных настройках → «Рабочий стол и Dock» должен быть выключен параметр «Дисплеи с разными рабочими пространствами Spaces».",
                COLUMN_WIDTH,
            )));
        }
        views.extend([
            into_view(&controls::separator(mtm)),
            into_view(&todo_row),
            into_view(&todo_note),
            into_view(&todo_view),
            into_view(&controls::separator(mtm)),
        ]);
        if let Some((stage_view, _)) = &stage_view_and_controls {
            views.push(into_view(stage_view));
        }
        views.extend([into_view(&buttons_row), into_view(&extras_stack)]);
        for view in &views {
            column.addArrangedSubview(view);
        }
        controls::activate(
            column.widthAnchor().constraintEqualToConstant(COLUMN_WIDTH),
            999.0,
        );
        column
            .widthAnchor()
            .constraintGreaterThanOrEqualToConstant(COLUMN_WIDTH)
            .setActive(true);

        // Прокрутка: документ прижат к верху, ширина — по ширине прокрутки.
        let document = controls::FlippedView::new(mtm);
        document.setTranslatesAutoresizingMaskIntoConstraints(false);
        document.addSubview(&column);
        column
            .topAnchor()
            .constraintEqualToAnchor_constant(&document.topAnchor(), TOP_INSET)
            .setActive(true);
        document
            .bottomAnchor()
            .constraintEqualToAnchor_constant(&column.bottomAnchor(), BOTTOM_INSET)
            .setActive(true);
        column
            .centerXAnchor()
            .constraintEqualToAnchor(&document.centerXAnchor())
            .setActive(true);
        controls::activate(
            document.widthAnchor().constraintEqualToConstant(VIEW_WIDTH),
            DEFAULT_LOW,
        );

        let root = NSScrollView::new(mtm);
        root.setBorderType(NSBorderType::NoBorder);
        root.setDrawsBackground(false);
        root.setHasVerticalScroller(true);
        root.setAutohidesScrollers(true);
        root.setDocumentView(Some(&document));
        let clip = root.contentView();
        document
            .topAnchor()
            .constraintEqualToAnchor(&clip.topAnchor())
            .setActive(true);
        document
            .leadingAnchor()
            .constraintEqualToAnchor(&clip.leadingAnchor())
            .setActive(true);
        document
            .widthAnchor()
            .constraintEqualToAnchor(&clip.widthAnchor())
            .setActive(true);
        root.setFrameSize(objc2_foundation::NSSize::new(VIEW_WIDTH, 600.0));

        GeneralTab {
            mtm,
            root,
            document: into_view(&document),
            launch_on_login,
            hide_menu_bar_icon,
            subsequent_execution,
            cycle_sizes_view,
            cycle_size_checkboxes,
            corner_axis_buttons,
            cooperative_corner_resize,
            gap_slider,
            gap_label,
            skip_gap_top_edge,
            cursor_across,
            use_cursor_screen_detection,
            double_click_title_bar,
            auto_maximize,
            green_button_override,
            combined_display_mode,
            todo,
            todo_view,
            todo_width,
            todo_width_unit,
            todo_side,
            stage: stage_view_and_controls.map(|(_, controls)| controls),
        }
    }

    /// Показать настройки в контролах.
    fn apply(&self, config: &Config, refresh: Refresh) {
        controls::set_on(&self.launch_on_login, config.launch_on_login);
        controls::set_on(&self.hide_menu_bar_icon, config.hide_menu_bar_icon);
        self.subsequent_execution
            .selectItemWithTag(config.subsequent_execution_mode.raw() as isize);
        self.update_cycle_sizes_view(config, false);

        let gap = logic::slider_position(config.gap_size, GAP_SLIDER_MAX);
        self.gap_slider.setIntValue(gap);
        self.gap_label
            .setStringValue(&ns(&logic::slider_label(self.gap_slider.intValue())));
        controls::set_on(&self.skip_gap_top_edge, config.skip_gap_top_edge);
        self.skip_gap_top_edge
            .setHidden(!logic::shows_skip_gap_top_edge(config));

        controls::set_on(
            &self.cursor_across,
            config.move_cursor_across_displays == Some(true),
        );
        if refresh != Refresh::External {
            self.use_cursor_screen_detection
                .setHidden(!logic::shows_use_cursor_screen_detection(config));
        }
        controls::set_on(
            &self.use_cursor_screen_detection,
            config.use_cursor_screen_detection,
        );
        controls::set_on(
            &self.double_click_title_bar,
            logic::double_click_title_bar_on(config.double_click_title_bar),
        );
        controls::set_on(&self.auto_maximize, logic::auto_maximize_on(config));
        controls::set_on(&self.green_button_override, config.green_button_override);
        if let Some(combined) = &self.combined_display_mode {
            controls::set_on(combined, config.combined_display_mode == Some(true));
        }

        controls::set_on(&self.todo, config.todo == Some(true));
        if self.todo_width.currentEditor().is_none() {
            self.todo_width
                .setStringValue(&ns(&NumberSetting::TodoSidebarWidth
                    .display(config.todo_sidebar_width, &separators())));
        }
        self.todo_width_unit
            .selectItemWithTag(config.todo_sidebar_width_unit.raw() as isize);
        self.todo_side
            .selectItemWithTag(config.todo_sidebar_side.raw() as isize);
        self.update_todo_view(config, false);

        if let Some((slider, label)) = &self.stage {
            slider.setIntValue(logic::slider_position(config.stage_size, STAGE_SLIDER_MAX));
            label.setStringValue(&ns(&logic::slider_label(slider.intValue())));
        }
        self.relayout(false);
    }

    /// Флажки и радиокнопки перебора размеров; блок виден при режимах,
    /// которые перебирают размеры.
    fn update_cycle_sizes_view(&self, config: &Config, animated: bool) {
        let shown = logic::shows_cycle_sizes(config);
        if shown {
            let sizes = config.effective_cycle_sizes();
            for (size, checkbox) in &self.cycle_size_checkboxes {
                controls::set_on(checkbox, sizes.contains(*size));
            }
            self.update_corner_axis(config.corner_cycle_expansion_axis);
            if let Some(checkbox) = &self.cooperative_corner_resize {
                controls::set_on(checkbox, config.cooperative_corner_resize);
            }
        }
        self.set_block_visible(&self.cycle_sizes_view, shown, animated);
    }

    fn update_corner_axis(&self, axis: CornerCycleExpansionAxis) {
        for (button_axis, button) in &self.corner_axis_buttons {
            controls::set_on(button, *button_axis == axis);
        }
    }

    fn update_todo_view(&self, config: &Config, animated: bool) {
        self.set_block_visible(&self.todo_view, logic::shows_todo_block(config), animated);
    }

    /// Показать или скрыть блок; при действии пользователя — с анимацией
    /// прозрачности и высоты окна (0,3 с, как `setVisibility` оригинала).
    fn set_block_visible(&self, view: &NSView, shown: bool, animated: bool) {
        if view.isHidden() == !shown {
            return;
        }
        view.setHidden(!shown);
        if shown && animated {
            view.setAlphaValue(0.0);
            objc2_app_kit::NSAnimationContext::beginGrouping();
            let context = objc2_app_kit::NSAnimationContext::currentContext();
            context.setDuration(0.3);
            view.animator().setAlphaValue(1.0);
            objc2_app_kit::NSAnimationContext::endGrouping();
        } else {
            view.setAlphaValue(1.0);
        }
        if animated {
            self.relayout(true);
        }
    }

    /// Пересчитать высоту и подогнать под неё окно.
    fn relayout(&self, animated: bool) {
        self.document.layoutSubtreeIfNeeded();
        if self.root.window().is_some() {
            super::fit_window(self.mtm, animated);
        }
    }
}

fn into_view<T: AsRef<NSView>>(view: &T) -> Retained<NSView> {
    view.as_ref().retain()
}

/// Версия справа от «Запуск при входе в систему»: `v<версия> (<сборка>)`.
fn version_text() -> String {
    let short = app_delegate::info_string("CFBundleShortVersionString")
        .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string());
    logic::version_text(&short, app_delegate::bundle_version().as_deref())
}

// ---------------------------------------------------------------- числовые поля

/// Все поля ввода окна: вкладка и поповер.
fn number_setting(field: &NSTextField) -> Option<NumberSetting> {
    let on_tab = TAB.with(|slot| {
        slot.borrow()
            .as_ref()
            .is_some_and(|tab| std::ptr::eq(&*tab.todo_width, field))
    });
    if on_tab {
        return Some(NumberSetting::TodoSidebarWidth);
    }
    super::extras::number_setting(field)
}

/// Ввод поменялся: сохранить через 0,5 с, если за это время он не менялся
/// (`Debounce` оригинала).
fn number_text_changed(field: &NSTextField) {
    let Some(setting) = number_setting(field) else {
        return;
    };
    let NumberInput::Valid(value) = setting
        .rules()
        .parse(&field.stringValue().to_string(), &separators())
    else {
        return;
    };
    let field = field.retain();
    crate::events::run_after(Duration::from_millis(500), move || {
        let current = setting
            .rules()
            .parse(&field.stringValue().to_string(), &separators());
        if current == NumberInput::Valid(value) {
            save_number(setting, value as f32);
        }
    });
}

/// Ввод закончен: пустое поле получает запасное значение, верное — сохраняется
/// и показывается так, как его показывает форматтер.
fn number_editing_ended(field: &NSTextField) {
    let Some(setting) = number_setting(field) else {
        return;
    };
    let separators = separators();
    let value = match setting
        .rules()
        .parse(&field.stringValue().to_string(), &separators)
    {
        NumberInput::Empty => setting.fallback(),
        NumberInput::Valid(value) => value as f32,
        NumberInput::Invalid => return,
    };
    save_number(setting, value);
    field.setStringValue(&ns(&setting.display(value, &separators)));
}

/// Неверный ввод не отпускает фокус — как поле с `NumberFormatter`.
fn number_may_end_editing(control: &NSControl) -> bool {
    let Ok(field) = control.retain().downcast::<NSTextField>() else {
        return true;
    };
    let Some(setting) = number_setting(&field) else {
        return true;
    };
    let valid = setting
        .rules()
        .parse(&field.stringValue().to_string(), &separators())
        != NumberInput::Invalid;
    if !valid {
        objc2_app_kit::NSBeep();
    }
    valid
}

fn save_number(setting: NumberSetting, value: f32) {
    if config::with(|config| setting.get(config)) != value {
        write(|config| setting.set(config, value));
    }
    // `defaultsSetAction`: попап доли сторон выбирает пункт по новому значению.
    super::extras::number_saved(setting);
}

// ---------------------------------------------------------------- диалоги

/// Экспорт: панель сохранения (`json`, имя `RectangleConfig`) и запись файла.
fn export_with_panel(mtm: MainThreadMarker) {
    let panel = NSSavePanel::savePanel(mtm);
    #[allow(deprecated)]
    panel.setAllowedFileTypes(Some(&NSArray::from_retained_slice(&[ns("json")])));
    panel.setNameFieldStringValue(&ns("RectangleConfig"));
    if panel.runModal() != NSModalResponseOK {
        return;
    }
    let Some(path) = panel.URL().and_then(|url| url.path()) else {
        return;
    };
    if let Err(error) = super::export_config_file(Path::new(&path.to_string())) {
        log!("Экспорт настроек: {error}");
    }
}

/// Импорт: панель открытия (`json`) и загрузка файла.
fn import_with_panel(mtm: MainThreadMarker) {
    let panel = NSOpenPanel::openPanel(mtm);
    #[allow(deprecated)]
    panel.setAllowedFileTypes(Some(&NSArray::from_retained_slice(&[ns("json")])));
    if panel.runModal() != NSModalResponseOK {
        return;
    }
    let Some(path) = panel.URL().and_then(|url| url.path()) else {
        return;
    };
    if let Err(error) = super::import_config_file(Path::new(&path.to_string())) {
        log!("Импорт настроек: {error}");
    }
}

/// Алерт «Конфликт с системными настройками» (§15, №1).
fn warn_double_click_conflict(mtm: MainThreadMarker) {
    let alert = NSAlert::new(mtm);
    alert.setAlertStyle(NSAlertStyle::Warning);
    alert.setMessageText(&ns("Конфликт с системными настройками"));
    alert.setInformativeText(&ns(&format!(
        "Чтобы позволить {} управлять функцией двойного щелчка в заголовке, необходимо отключить соответствующую настройку macOS.",
        app_delegate::app_name()
    )));
    alert.addButtonWithTitle(&ns("Открыть Системные настройки"));
    alert.addButtonWithTitle(&ns("Закрыть"));
    if alert.runModal() == NSAlertFirstButtonReturn {
        if let Some(url) =
            NSURL::URLWithString(&ns("x-apple.systempreferences:com.apple.preference.dock"))
        {
            NSWorkspace::sharedWorkspace().openURL(&url);
        }
    }
}

/// Подтверждение сброса областей прилипания (в оригинале — выбор набора
/// шорткатов Rectangle/Spectacle и Cancel, §15 №2).
fn confirm_restore_defaults(mtm: MainThreadMarker) -> bool {
    let alert = NSAlert::new(mtm);
    alert.setAlertStyle(NSAlertStyle::Warning);
    alert.setMessageText(&ns("Области прилипания по умолчанию"));
    alert.setInformativeText(&ns(
        "Вернуть области прилипания к значениям по умолчанию? Остальные настройки не изменятся.",
    ));
    alert.addButtonWithTitle(&ns("Восстановить"));
    alert.addButtonWithTitle(&ns("Отмена"));
    alert.runModal() == NSAlertFirstButtonReturn
}
