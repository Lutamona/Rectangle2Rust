//! Менеджер окон — порт `WindowManager.swift` и той части
//! `ShortcutManager.execute`, через которую в оригинале проходит действие из
//! любого источника (меню, URL, drag-to-snap, заголовок окна): перехват
//! раскладок всех окон и Todo, режим повторов «перебор экранов».
//!
//! Ход действия: найти окно и экраны → история (повтор? сдвинули извне?) и
//! рамка для «Восстановить» → расчёт с учётом экранов (`screen_calculation`) →
//! гэпы → сдвиг лесенкой (`overlap_offset`) → план согласованного ресайза соседей
//! (`cooperative_resize_manager`) → цепочка доводки (`movers`) → при переезде на
//! другой экран повторы, вывод окна вперёд, перенос курсора, иначе — возврат соседей
//! прошлого согласованного действия → запись истории.
//!
//! Всё — на главном потоке: история живёт там, AppKit (экраны) — тоже.

use std::time::Duration;

use crate::actions::{Action, WindowActionCategory};
use crate::ax::{self, AxElement};
use crate::calc;
use crate::config::{self, Config, EnhancedUI, SubsequentExecutionMode};
use crate::cooperative_resize::Size;
use crate::cooperative_resize_manager::{self, CleanupRequest, PlanRequest};
use crate::geometry::Rect;
use crate::log;
use crate::movers::{self, MoveParameters};
use crate::overlap_offset;
use crate::screen_calculation::{
    self, Window, WindowCalculationParameters, WindowCalculationResult,
};
use crate::screen_detection::{
    self, detect_screens, detect_screens_at_cursor, ScreenEnvironment, UsableScreens, ZERO_RECT,
};
use crate::screens::{self, Screen};
use crate::side_split_ratios;
use crate::stage::StageState;
use crate::window_history::{self, LastAction};

// ---------------------------------------------------------------- параметры

/// Откуда пришло действие (`ExecutionSource`; горячих клавиш в порте нет).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecutionSource {
    MenuItem,
    Url,
    DragToSnap,
    TitleBar,
}

/// Параметры выполнения (`ExecutionParameters`).
#[derive(Clone, Debug)]
pub struct ExecutionParameters {
    pub action: Action,
    /// Запомнить текущую рамку окна для «Восстановить» (drag-to-snap — нет:
    /// у него своя рамка до перетаскивания).
    pub update_restore_rect: bool,
    /// Явный экран: расчёт идёт на нём, как будто он единственный.
    pub screen: Option<Screen>,
    /// Явное окно; нет — окно в фокусе.
    pub window_element: Option<AxElement>,
    /// Явный номер окна; нет — узнаётся у окна.
    pub window_id: Option<u32>,
    pub source: ExecutionSource,
}

impl ExecutionParameters {
    pub fn new(action: Action, source: ExecutionSource) -> Self {
        ExecutionParameters {
            action,
            update_restore_rect: true,
            screen: None,
            window_element: None,
            window_id: None,
            source,
        }
    }

    /// Пункт меню (`postMenu`).
    pub fn menu(action: Action) -> Self {
        Self::new(action, ExecutionSource::MenuItem)
    }

    /// URL-схема (`postUrl`).
    pub fn url(action: Action) -> Self {
        Self::new(action, ExecutionSource::Url)
    }

    /// Drag-to-snap (`postSnap`): окно, его номер и экран известны, рамку для
    /// «Восстановить» не трогаем.
    pub fn snap(
        action: Action,
        window_element: Option<AxElement>,
        window_id: Option<u32>,
        screen: Screen,
    ) -> Self {
        ExecutionParameters {
            update_restore_rect: false,
            screen: Some(screen),
            window_element,
            window_id,
            ..Self::new(action, ExecutionSource::DragToSnap)
        }
    }

    /// Двойной клик по заголовку и зелёная кнопка (`postTitleBar`).
    pub fn title_bar(action: Action, window_element: Option<AxElement>) -> Self {
        ExecutionParameters {
            window_element,
            ..Self::new(action, ExecutionSource::TitleBar)
        }
    }
}

// ---------------------------------------------------------------- система

#[repr(C)]
struct CGPoint {
    x: f64,
    y: f64,
}

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGWarpMouseCursorPosition(point: CGPoint) -> i32;
}

#[link(name = "AppKit", kind = "framework")]
extern "C" {
    fn NSBeep();
}

pub(crate) fn beep() {
    unsafe { NSBeep() }
}

fn warp_cursor(center: (f64, f64)) {
    unsafe {
        CGWarpMouseCursorPosition(CGPoint {
            x: center.0,
            y: center.1,
        });
    }
}

// ---------------------------------------------------------------- подключения других этапов

/// `MultiWindowManager.execute` и `TodoManager.execute`: раскладки всех окон
/// (tile/cascade/reverse, `multi_window`) и Todo («Todo слева/справа», `todo`)
/// выполняют свои действия сами, раньше менеджера окон, в этом порядке.
fn handled_by_other_managers(params: &ExecutionParameters) -> bool {
    crate::multi_window::execute(params) || crate::todo::execute(params)
}

/// `TodoManager.isTodoWindow`: окно — боковая панель Todo. Нужна и
/// прилипанию: окно Todo прилипает к своей стороне.
pub(crate) fn is_todo_window(window_id: u32) -> bool {
    crate::todo::is_todo_window(window_id)
}

/// Экран, у которого стоит боковая панель Todo (`TodoManager.todoScreen`, когда
/// режим включён и окно Todo есть).
fn todo_screen() -> Option<u32> {
    crate::todo::sidebar_screen()
}

// ---------------------------------------------------------------- экраны

/// Снимок системы для расчёта рабочей области: Stage Manager смотрим, только
/// если под полосу вообще оставляется место.
fn environment<'a>(config: &'a Config, all_screens: &'a [Screen]) -> ScreenEnvironment<'a> {
    environment_with_todo(config, all_screens, todo_screen())
}

/// То же с заданным экраном панели Todo.
fn environment_with_todo<'a>(
    config: &'a Config,
    all_screens: &'a [Screen],
    todo_screen: Option<u32>,
) -> ScreenEnvironment<'a> {
    let primary_height = primary_height(all_screens);
    let stage = if config.stage_size > 0.0 {
        StageState::current(all_screens, primary_height)
    } else {
        StageState::default()
    };
    ScreenEnvironment {
        config,
        screens: all_screens,
        separate_spaces: screens::screens_have_separate_spaces(),
        stage,
        todo_screen,
    }
}

fn primary_height(all_screens: &[Screen]) -> f64 {
    all_screens
        .first()
        .map(|screen| screen.frame.max_y())
        .unwrap_or(0.0)
}

/// Экраны для окна (`ScreenDetection().detectScreens(using:)`); без окна — как
/// для окна в начале координат.
pub fn usable_screens(window: Option<&AxElement>) -> Option<UsableScreens> {
    let config = config::current();
    let all_screens = screens::screens();
    let frame = match window {
        Some(window) => window.frame(),
        None => Some(ZERO_RECT),
    };
    detect_screens(frame, &all_screens, &config, primary_height(&all_screens))
}

/// Рабочая область экрана для раскладок (`NSScreen.adjustedVisibleFrame`).
pub fn adjusted_visible_frame(screen: &Screen, ignore_todo: bool, ignore_stage: bool) -> Rect {
    let config = config::current();
    let all_screens = screens::screens();
    let env = environment(&config, &all_screens);
    screen_detection::adjusted_visible_frame(screen, &env, ignore_todo, ignore_stage)
}

/// Рабочая область экрана для самого Todo-режима: экран панели он знает сам
/// (`adjustedVisibleFrame` внутри `TodoManager`).
pub(crate) fn adjusted_visible_frame_with_todo(
    config: &Config,
    all_screens: &[Screen],
    screen: &Screen,
    todo_screen: Option<u32>,
    ignore_todo: bool,
) -> Rect {
    let env = environment_with_todo(config, all_screens, todo_screen);
    screen_detection::adjusted_visible_frame(screen, &env, ignore_todo, false)
}

// ---------------------------------------------------------------- выполнение

/// Выполнить действие (`ShortcutManager.execute` → `WindowManager.execute`).
pub fn execute(params: ExecutionParameters) {
    if handled_by_other_managers(&params) {
        return;
    }
    let config = config::current();
    let Some(params) = cycle_monitor(params, &config) else {
        return;
    };
    execute_window_action(params, &config);
}

/// Режим повторов «перебор экранов» (`ShortcutManager.execute`): повтор того же
/// действия выполняет его на следующем экране. Размеры, дисплеи и столбики —
/// нет (у столбиков повтор — следующий столбик). `None` — действие кончилось
/// сигналом.
fn cycle_monitor(params: ExecutionParameters, config: &Config) -> Option<ExecutionParameters> {
    let classification = params.action.classification();
    if config.subsequent_execution_mode != SubsequentExecutionMode::CycleMonitor
        || classification == Some(WindowActionCategory::Size)
        || classification == Some(WindowActionCategory::Display)
        || params.action.column_count().is_some()
    {
        return Some(params);
    }

    let window = params.window_element.clone().or_else(ax::front_window);
    let Some(window) = window else {
        beep();
        return None;
    };
    let Some(window_id) = params.window_id.or_else(|| window.get_window_id()) else {
        beep();
        return None;
    };

    let all_screens = screens::screens();
    let frame = window.frame();
    let detected = detect_screens(frame, &all_screens, config, primary_height(&all_screens));
    if is_repeat_action(params.action, window_id, frame, detected.as_ref()) {
        if let Some(next) = detected
            .and_then(|usable| usable.adjacent)
            .map(|adjacent| adjacent.next)
        {
            // Любое другое повторное поведение отменяем: забываем последнее действие.
            window_history::remove_last_action(window_id);
            return Some(ExecutionParameters {
                screen: Some(next),
                window_element: Some(window),
                window_id: Some(window_id),
                ..params
            });
        }
    }
    Some(params)
}

/// Повтор для перебора экранов (`isRepeatAction`): «развернуть» над окном,
/// которое уже во весь экран (по `visibleFrame`), или то же действие, что в
/// прошлый раз. Рамку записи не сравниваем — только действие, как в Swift.
fn is_repeat_action(
    action: Action,
    window_id: u32,
    frame: Option<Rect>,
    detected: Option<&UsableScreens>,
) -> bool {
    if action == Action::Maximize {
        if let (Some(usable), Some(frame)) = (detected, frame) {
            let visible = usable.current.visible_frame;
            if visible.w == frame.w && visible.h == frame.h {
                return true;
            }
        }
    }
    window_history::last_action(window_id).is_some_and(|last| last.action == action)
}

/// Окно сдвинули не мы (`windowMovedExternally`): записи нет, рамка не читается
/// или ушла от записанной дальше чем на 2px (система и приложения двигают окна
/// на пиксель-другой). Запись с `CGRect.null` не «близка» ни к какой рамке.
fn window_moved_externally(last: Option<&LastAction>, frame: Option<Rect>) -> bool {
    match (last, frame) {
        (Some(last), Some(frame)) => !frame.is_close(&last.rect, 2.0),
        _ => true,
    }
}

/// Всё для доводки и повторов (`ResultParameters`).
struct ResultParameters {
    window_id: Option<u32>,
    window: AxElement,
    calc_result: WindowCalculationResult,
    move_parameters: MoveParameters,
}

/// `WindowManager.execute`.
fn execute_window_action(params: ExecutionParameters, config: &Config) {
    let Some(window) = params.window_element.clone().or_else(ax::front_window) else {
        beep();
        return;
    };
    // Номер окна бывает недоступен после смены сессии (#640): действие всё равно
    // выполняется, пропускается только история (или берётся производный номер).
    let window_id = params.window_id.or_else(|| window.get_window_id());
    let action = params.action;

    if action == Action::Restore {
        let Some(window_id) = window_id else {
            beep();
            return;
        };
        if let Some(rect) = window_history::restore_rect(window_id) {
            window.set_frame(&rect);
        }
        window_history::remove_last_action(window_id);
        return;
    }

    let all_screens = screens::screens();
    let primary_height = primary_height(&all_screens);
    let current_frame = window.frame();

    // Явный экран (drag-to-snap, перебор экранов) или экран курсора задают расчёт,
    // но окно до переезда может быть и не на нём.
    let source_screens = detect_screens(current_frame, &all_screens, config, primary_height);
    let usable = match &params.screen {
        Some(screen) => Some(UsableScreens::single(screen.clone())),
        None if config.use_cursor_screen_detection => {
            screens::cursor_position().and_then(|cursor| {
                detect_screens_at_cursor(cursor, &all_screens, config, primary_height)
            })
        }
        None => source_screens.clone(),
    };
    let (Some(usable), Some(source_screens)) = (usable, source_screens) else {
        beep();
        return;
    };

    // Окно сдвинули не мы — цикл повторов начинается заново.
    let mut last_action = window_id.and_then(window_history::last_action);
    let moved_externally = window_moved_externally(last_action.as_ref(), current_frame);
    if moved_externally {
        last_action = None;
        if let Some(window_id) = window_id {
            window_history::remove_last_action(window_id);
        }
    }

    if params.update_restore_rect {
        if let (Some(window_id), Some(frame)) = (window_id, current_frame) {
            if window_history::restore_rect(window_id).is_none() || moved_externally {
                window_history::set_restore_rect(window_id, frame);
            }
        }
    }

    let ignore_todo = window_id.is_some_and(is_todo_window);

    let Some(current_frame) = current_frame else {
        beep();
        return;
    };
    if window.is_sheet() {
        beep();
        return;
    }

    let env = environment(config, &all_screens);
    let current_normalized = current_frame.screen_flipped(primary_height);
    let calc_params = WindowCalculationParameters {
        window: Window {
            id: window_id,
            rect: current_normalized,
        },
        usable_screens: &usable,
        action,
        last_action,
        ignore_todo,
    };
    let Some(mut calc_result) = screen_calculation::calculate(&calc_params, &env) else {
        beep();
        return;
    };
    if calc_result.forget_last_action {
        if let Some(window_id) = window_id {
            window_history::remove_last_action(window_id);
        }
    }

    calc_result.rect = calc::apply_gaps(
        calc_result.rect,
        calc_result.resulting_action,
        calc_result.resulting_sub_action,
        config,
    );

    // Сдвиг лесенкой: позиция уже занята другим окном — шаг в сторону. Смотрит на
    // нажатое действие, а не на итог расчёта.
    if config.cycling_overlap_offset == Some(true) && action.overlap_offset_applies() {
        let screen_frame =
            screen_detection::adjusted_visible_frame(&calc_result.screen, &env, false, false);
        calc_result.rect = overlap_offset::apply_overlap_offset_if_needed(
            calc_result.rect,
            window_id,
            &screen_frame,
            primary_height,
            config,
        );
    }

    // Рамку, в которую окно не поставить, не отдаём доводке: NaN и бесконечности,
    // а ещё пустой или отрицательный размер — гэп больше ячейки (`inset_by` у нас
    // уходит в минус, а не даёт `CGRect.null`, как в Swift).
    if !is_usable_frame(&calc_result.rect) {
        beep();
        return;
    }

    let is_fixed_size = (!window.is_resizable()
        && action.resizes(config.resize_on_directional_move))
        || window.is_system_dialog();
    let visible_frame_of_destination = calc_result.resulting_screen_frame.unwrap_or_else(|| {
        screen_detection::adjusted_visible_frame(&calc_result.screen, &env, ignore_todo, false)
    });
    let is_moved_across_displays = !source_screens.current.same_display(&calc_result.screen);

    // Согласованный ресайз: соседи подстраиваются под окно, план задаёт и его рамку.
    // Минимальный размер окна (лишний вызов AX) нужен, только когда он включён.
    let focused_window_minimum_size = if config.cooperative_corner_resize {
        window.minimum_size().map(Size::from)
    } else {
        None
    };
    let cooperative_plan =
        cooperative_resize_manager::cooperative_corner_resize_plan(&PlanRequest {
            focused_window_id: window_id,
            focused_window_is_fixed_size: is_fixed_size,
            focused_window_minimum_size,
            action,
            source: params.source,
            old_focused_frame: current_normalized,
            new_focused_frame: calc_result.rect,
            screen_frame: visible_frame_of_destination,
            destination_screen_is_current_screen: !is_moved_across_displays,
            last_action: last_action.as_ref(),
            config,
            primary_height,
        });
    match &cooperative_plan {
        Some(plan) => {
            calc_result.rect = plan.focused_frame;
            if let Some(frame) = plan.side_split_recording_frame {
                calc_result.initial_rect = frame;
            }
        }
        // `ActiveSideSplitRatios.recordSideAction` — после гэпов, по рамке до гэпов
        // (`initialRect`); с согласованным планом доля запоминается по итогу.
        None => side_split_ratios::record_side_action(
            calc_result.resulting_action,
            &calc_result.initial_rect,
            &visible_frame_of_destination,
            config,
        ),
    }

    match &cooperative_plan {
        Some(plan) if !plan.needs_application(Some(&current_normalized)) => {
            side_split_ratios::record_achieved_cooperative_action(
                plan.action,
                Some(&current_normalized),
                &plan.screen_frame,
                plan.gap_size,
                config,
            );
            log!("Согласованный ресайз: все окна уже на местах");
            record(window_id, Some(current_frame), &calc_result);
            return;
        }
        None if current_normalized == calc_result.rect => {
            // Окно уже там, куда его просят: не двигаем, но историю обновляем.
            record(window_id, Some(current_frame), &calc_result);
            return;
        }
        _ => {}
    }

    let result = ResultParameters {
        window_id,
        window: window.clone(),
        move_parameters: MoveParameters {
            action,
            initial_rect: calc_result.initial_rect,
            visible_frame: visible_frame_of_destination,
            is_fixed_size,
            primary_height,
            separate_spaces: env.separate_spaces,
            move_fixed_size_to_edge: config.move_fixed_size_to_edge,
            corner_cycle_expansion_axis: config.corner_cycle_expansion_axis,
            resize_on_directional_move: config.resize_on_directional_move,
            gap_size: config.gap_size as f64,
        },
        calc_result,
    };

    let mut resulting = match &cooperative_plan {
        Some(plan) => {
            let resulting = cooperative_resize_manager::apply_cooperative_corner_resize(
                &window,
                &mut |rect| movers::move_window(&window, rect, &result.move_parameters),
                plan,
            );
            // AX может не дать окну ужаться сильнее минимума, о котором оно не сообщало.
            side_split_ratios::record_achieved_cooperative_action(
                plan.action,
                resulting
                    .map(|rect| rect.screen_flipped(primary_height))
                    .as_ref(),
                &plan.screen_frame,
                plan.gap_size,
                config,
            );
            resulting
        }
        None => apply(&result),
    };

    if is_moved_across_displays {
        // macOS подгоняет размер под экран, на котором окно было: повторяем всей
        // цепочкой, последний раз — через 25 мс, и только потом курсор и история.
        if !same_size(&result.calc_result.rect, resulting.as_ref()) {
            resulting = apply(&result);
            if !same_size(&result.calc_result.rect, resulting.as_ref()) {
                crate::events::run_after(Duration::from_millis(25), move || {
                    let final_rect = apply(&result);
                    window_moved_across_displays(&result.window, final_rect.as_ref());
                    post_process(&result, final_rect);
                });
                return;
            }
        }
        window_moved_across_displays(&window, resulting.as_ref());
    } else {
        // Окно ушло с места прошлого согласованного действия — вернуть соседей.
        cooperative_resize_manager::apply_cooperative_corner_cleanup_if_needed(&CleanupRequest {
            focused_window_id: window_id,
            source: params.source,
            old_focused_frame: current_normalized,
            new_focused_frame: resulting.map(|rect| rect.screen_flipped(primary_height)),
            screen_frame: screen_detection::adjusted_visible_frame(
                &source_screens.current,
                &env,
                ignore_todo,
                false,
            ),
            current_action: action,
            last_action: last_action.as_ref(),
            config,
            primary_height,
        });
        resulting = window.frame();
    }

    post_process(&result, resulting);
}

/// Рамка годится для окна: все числа конечны, ширина и высота больше нуля.
fn is_usable_frame(rect: &Rect) -> bool {
    rect.x.is_finite()
        && rect.y.is_finite()
        && rect.w.is_finite()
        && rect.h.is_finite()
        && rect.w > 0.0
        && rect.h > 0.0
}

fn same_size(calculated: &Rect, resulting: Option<&Rect>) -> bool {
    resulting.is_some_and(|resulting| resulting.w == calculated.w && resulting.h == calculated.h)
}

/// Поставить окно (`apply(result:)`), если оно ещё не там. Возвращает рамку
/// окна после (AX).
fn apply(result: &ResultParameters) -> Option<Rect> {
    let new_rect = result.calc_result.rect;
    let current = result.window.frame();
    let already_there = current
        .map(|frame| frame.screen_flipped(result.move_parameters.primary_height))
        == Some(new_rect);
    if !already_there {
        movers::move_window(&result.window, &new_rect, &result.move_parameters);
    }
    result.window.frame()
}

/// Окно переехало на другой экран: вывести его вперёд и, если включено,
/// перенести курсор в центр его фактической рамки (AX — те же координаты,
/// что у `CGWarpMouseCursorPosition`).
fn window_moved_across_displays(window: &AxElement, resulting: Option<&Rect>) {
    window.bring_to_front(true);
    if config::with(|config| config.move_cursor_across_displays == Some(true)) {
        if let Some(resulting) = resulting {
            warp_cursor(resulting.center());
        }
    }
}

/// `postProcess`: записать историю. (Перенос курсора `moveCursor` в оригинале —
/// только для горячих клавиш, которых в порте нет.)
fn post_process(result: &ResultParameters, resulting: Option<Rect>) {
    record(result.window_id, resulting, &result.calc_result);
}

/// `recordAction`. Рамку прочитать не удалось — действие всё равно пишется, с
/// `CGRect.null`, как в Swift: повтор «перебора экранов» смотрит только на
/// действие и его увидит, а сама рамка ни с чем не совпадёт, так что следующее
/// действие сочтёт окно сдвинутым извне.
fn record(window_id: Option<u32>, resulting: Option<Rect>, calc_result: &WindowCalculationResult) {
    let Some(window_id) = window_id else {
        return;
    };
    window_history::with(|history| {
        history.record_action(
            window_id,
            resulting.unwrap_or(Rect::NULL),
            calc_result.resulting_action,
            calc_result.resulting_sub_action,
            true,
        )
    });
}

// ---------------------------------------------------------------- смена приложения

/// Подписка на смену активного приложения (`ApplicationToggle` оригинала):
/// нужна режиму `enhancedUI = frontmostDisable`. Запускается из
/// `subsystems::start_all`, когда есть доступ к управлению компьютером.
pub fn install(_mtm: objc2::MainThreadMarker) {
    crate::events::on_front_app_changed(|current, _previous| on_frontmost_app_changed(current.pid));
}

/// Смена активного приложения (`NSWorkspace.didActivateApplicationNotification`,
/// подключает шина событий каркаса). В режиме `enhancedUI = frontmostDisable`
/// через 50 мс у приложения в фокусе выключается `AXEnhancedUserInterface`
/// (`ApplicationToggle.receiveFrontAppChangeNote`). `pid` — активированное
/// приложение; берётся, если приложение в фокусе узнать не удалось.
pub fn on_frontmost_app_changed(pid: i32) {
    if config::with(|config| config.enhanced_ui) != EnhancedUI::FrontmostDisable {
        return;
    }
    crate::events::run_after(Duration::from_millis(50), move || {
        let app_element =
            ax::front_application_element().unwrap_or_else(|| AxElement::application(pid));
        app_element.set_enhanced_ui(false);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constructors_match_swift_sources() {
        let menu = ExecutionParameters::menu(Action::LeftHalf);
        assert_eq!(menu.source, ExecutionSource::MenuItem);
        assert!(menu.update_restore_rect);
        assert!(menu.screen.is_none() && menu.window_element.is_none() && menu.window_id.is_none());

        assert_eq!(
            ExecutionParameters::url(Action::Maximize).source,
            ExecutionSource::Url
        );

        let screen = Screen {
            id: 5,
            frame: Rect::new(0.0, 0.0, 100.0, 100.0),
            visible_frame: Rect::new(0.0, 0.0, 100.0, 90.0),
            name: String::new(),
            is_main: false,
            scale: 1.0,
            safe_area_top: 0.0,
        };
        let snap = ExecutionParameters::snap(Action::TopHalf, None, Some(9), screen);
        assert_eq!(snap.source, ExecutionSource::DragToSnap);
        assert!(!snap.update_restore_rect);
        assert_eq!(snap.window_id, Some(9));
        assert_eq!(snap.screen.map(|screen| screen.id), Some(5));

        let title_bar = ExecutionParameters::title_bar(Action::Maximize, None);
        assert_eq!(title_bar.source, ExecutionSource::TitleBar);
        assert!(title_bar.update_restore_rect);
    }

    #[test]
    fn size_comparison_needs_a_frame() {
        let calculated = Rect::new(0.0, 0.0, 800.0, 600.0);
        assert!(same_size(
            &calculated,
            Some(&Rect::new(10.0, 10.0, 800.0, 600.0))
        ));
        assert!(!same_size(
            &calculated,
            Some(&Rect::new(0.0, 0.0, 799.0, 600.0))
        ));
        assert!(!same_size(&calculated, None));
    }

    fn screen(id: u32, frame: Rect) -> Screen {
        Screen {
            id,
            frame,
            visible_frame: frame,
            name: String::new(),
            is_main: false,
            scale: 1.0,
            safe_area_top: 0.0,
        }
    }

    #[test]
    fn only_finite_non_empty_frames_reach_the_window() {
        assert!(is_usable_frame(&Rect::new(0.0, 0.0, 800.0, 600.0)));
        for rect in [
            Rect::new(f64::NAN, 0.0, 1.0, 1.0),
            Rect::new(0.0, f64::INFINITY, 1.0, 1.0),
            Rect::new(0.0, 0.0, f64::INFINITY, 1.0),
            Rect::new(0.0, 0.0, 1.0, f64::NAN),
            Rect::new(0.0, 0.0, 0.0, 600.0),
            Rect::new(0.0, 0.0, 800.0, 0.0),
            Rect::new(0.0, 0.0, -1.0, 600.0),
            Rect::new(100.0, 482.0, 50.0, -7.0),
            Rect::NULL,
        ] {
            assert!(!is_usable_frame(&rect), "{rect:?}");
        }
    }

    #[test]
    fn gap_larger_than_the_cell_is_rejected() {
        // Экран 800×600 (рабочая область 800×575), гэп 100 — «шестнадцатая» уходит
        // в минус по высоте: 575/4 ≈ 143, после гэпов 143 − 200 + 50 = −7.
        let config = Config {
            gap_size: 100.0,
            ..Config::default()
        };
        let visible = Rect::new(0.0, 0.0, 800.0, 575.0);
        let gapped = |config: &Config, action: Action| {
            let params = calc::CalcParams {
                window: Rect::new(100.0, 100.0, 400.0, 300.0),
                visible,
                visible_ignoring_stage: None,
                action,
                last: None,
                config,
                source_visible: None,
                num_screens: 1,
                primary_max_y: 600.0,
            };
            let result = calc::calculate(&params).unwrap();
            calc::apply_gaps(result.rect, result.action, result.sub_action, config)
        };
        let rect = gapped(&config, Action::TopLeftSixteenth);
        assert!(rect.h < 0.0, "{rect:?}");
        assert!(rect.x.is_finite() && rect.y.is_finite(), "{rect:?}");
        assert!(!is_usable_frame(&rect));
        // С обычным гэпом та же ячейка годится.
        let config = Config {
            gap_size: 10.0,
            ..Config::default()
        };
        assert!(is_usable_frame(&gapped(&config, Action::TopLeftSixteenth)));
    }

    #[test]
    fn unreadable_frame_after_action_is_recorded_like_swift() {
        // Номер окна только для этого теста: история — одна на поток.
        let window_id = 0x7F00_0C02;
        window_history::remove_last_action(window_id);
        let area = Rect::new(0.0, 0.0, 1728.0, 1001.0);
        let calc_result = WindowCalculationResult {
            rect: area,
            initial_rect: area,
            screen: screen(1, area),
            resulting_action: Action::LeftHalf,
            resulting_sub_action: None,
            resulting_screen_frame: None,
            forget_last_action: false,
        };

        record(Some(window_id), None, &calc_result);
        let last = window_history::last_action(window_id).expect("действие записано");
        assert_eq!(last.action, Action::LeftHalf);
        assert!(last.rect.is_null());
        assert_eq!(last.count, 1);

        // «Перебор экранов» видит повтор: сравнивается только действие.
        let frame = Some(Rect::new(0.0, 25.0, 864.0, 1001.0));
        assert!(is_repeat_action(Action::LeftHalf, window_id, frame, None));
        assert!(is_repeat_action(Action::LeftHalf, window_id, None, None));
        assert!(!is_repeat_action(Action::RightHalf, window_id, frame, None));

        // А для истории повторов окно сдвинуто извне: рамка `.null` ни к чему не близка.
        assert!(window_moved_externally(Some(&last), frame));
        assert!(window_moved_externally(Some(&last), None));

        // Счётчик растёт, как у Swift, и настоящая рамка заменяет `.null`.
        record(Some(window_id), None, &calc_result);
        assert_eq!(window_history::last_action(window_id).unwrap().count, 2);
        record(Some(window_id), frame, &calc_result);
        let last = window_history::last_action(window_id).unwrap();
        assert_eq!((last.count, Some(last.rect)), (3, frame));
        assert!(!window_moved_externally(Some(&last), frame));
        window_history::remove_last_action(window_id);
    }

    #[test]
    fn maximize_counts_as_repeat_when_window_already_fills_the_screen() {
        let window_id = 0x7F00_0C03;
        window_history::remove_last_action(window_id);
        let visible = Rect::new(0.0, 83.0, 1728.0, 1002.0);
        let usable = UsableScreens::single(Screen {
            visible_frame: visible,
            ..screen(1, Rect::new(0.0, 0.0, 1728.0, 1117.0))
        });
        let filled = Some(Rect::new(0.0, 32.0, 1728.0, 1002.0));
        assert!(is_repeat_action(
            Action::Maximize,
            window_id,
            filled,
            Some(&usable)
        ));
        assert!(!is_repeat_action(
            Action::Maximize,
            window_id,
            Some(Rect::new(0.0, 32.0, 800.0, 600.0)),
            Some(&usable)
        ));
        assert!(!is_repeat_action(
            Action::Maximize,
            window_id,
            None,
            Some(&usable)
        ));
    }

    #[test]
    fn cycle_monitor_leaves_other_modes_and_excluded_actions_alone() {
        let config = Config::default();
        let params = ExecutionParameters::menu(Action::LeftHalf);
        let same = cycle_monitor(params, &config).unwrap();
        assert!(same.screen.is_none());

        let cycle = Config {
            subsequent_execution_mode: SubsequentExecutionMode::CycleMonitor,
            ..Config::default()
        };
        // Размеры, дисплеи и столбики перебором экранов не затрагиваются: окно даже не ищется.
        for action in [
            Action::Larger,
            Action::NextDisplay,
            Action::Display(2),
            Action::Column { count: 5, index: 1 },
        ] {
            let result = cycle_monitor(ExecutionParameters::menu(action), &cycle).unwrap();
            assert!(result.screen.is_none(), "{action:?}");
            assert!(result.window_element.is_none(), "{action:?}");
        }
    }
}
