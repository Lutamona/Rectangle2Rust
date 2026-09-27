//! Расчёты, которым нужны экраны, — `calculate(_:)` оригинала у шести классов,
//! которые его переопределяют: `NextPrevDisplayCalculation`,
//! `SpecificDisplayCalculation`, `LeftRightHalfCalculation` (режимы повторов
//! «через мониторы»), `MoveLeftRightCalculation` (переход на соседний экран),
//! `CenterCalculation` и `CenterProminentlyCalculation` (рабочая область без
//! полосы Stage Manager). Остальные действия считает `calc::calculate` на
//! рабочей области текущего экрана — как `WindowCalculation.calculate`.
//!
//! Чистые функции: экраны и настройки приходят параметрами, история не меняется.
//! Просьбу «забыть последнее действие окна», которую Swift выполняет прямо
//! в расчёте (`attemptMatchOnNextPrevDisplay`), результат передаёт флагом.

use crate::actions::{Action, SubAction};
use crate::calc::{self, CalcParams, CalcResult, LastAction};
use crate::config::SubsequentExecutionMode;
use crate::geometry::Rect;
use crate::screen_detection::{adjusted_visible_frame, ScreenEnvironment, UsableScreens};
use crate::screens::Screen;

/// Окно для расчёта (`Window`): номер и рамка в координатах Cocoa.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Window {
    pub id: Option<u32>,
    pub rect: Rect,
}

/// `WindowCalculationParameters`.
#[derive(Clone, Debug)]
pub struct WindowCalculationParameters<'a> {
    pub window: Window,
    pub usable_screens: &'a UsableScreens,
    pub action: Action,
    /// Последнее действие над окном — уже без записи, если окно сдвинули извне.
    pub last_action: Option<LastAction>,
    /// Окно — боковая панель Todo: считать рабочую область без самой панели.
    pub ignore_todo: bool,
}

impl WindowCalculationParameters<'_> {
    fn with_action(&self, action: Action) -> Self {
        WindowCalculationParameters {
            action,
            ..self.clone()
        }
    }
}

/// `WindowCalculationResult`.
#[derive(Clone, Debug, PartialEq)]
pub struct WindowCalculationResult {
    /// Куда ставить окно, Cocoa. Гэпы накладывает менеджер окон.
    pub rect: Rect,
    /// Рамка до гэпов — по ней считаются общие с экраном края для выравнивания.
    pub initial_rect: Rect,
    /// Экран, на котором окажется окно.
    pub screen: Screen,
    pub resulting_action: Action,
    pub resulting_sub_action: Option<SubAction>,
    /// Рабочая область, в которой считали, если она не обычная рабочая область
    /// экрана (центр без полосы Stage Manager).
    pub resulting_screen_frame: Option<Rect>,
    /// Забыть последнее действие окна: следующее нажатие — как первое.
    pub forget_last_action: bool,
}

impl WindowCalculationResult {
    fn new(rect: Rect, screen: Screen, resulting_action: Action) -> Self {
        WindowCalculationResult {
            rect,
            initial_rect: rect,
            screen,
            resulting_action,
            resulting_sub_action: None,
            resulting_screen_frame: None,
            forget_last_action: false,
        }
    }
}

/// Расчёт действия над окном с учётом экранов. `None` — действие здесь
/// невыполнимо (сигнал): один экран у «следующего дисплея», нет такого
/// дисплея, окно уже на нём, у действия нет расчёта.
pub fn calculate(
    params: &WindowCalculationParameters,
    env: &ScreenEnvironment,
) -> Option<WindowCalculationResult> {
    match params.action {
        Action::NextDisplay | Action::PreviousDisplay => next_prev_display(params, env),
        Action::Display(_) => specific_display(params, env),
        Action::LeftHalf | Action::RightHalf => left_right_half(params, env),
        Action::MoveLeft | Action::MoveRight => move_left_right(params, env),
        Action::Center | Action::CenterProminently => center(params, env),
        Action::Restore => None,
        _ => on_current_screen(params, env),
    }
}

/// `calculateRect` действия на заданной рабочей области.
///
/// Расчёт идёт как на ОДНОМ экране (`num_screens: 1`): переезды между экранами
/// (половины и «к краю» в режимах «через мониторы», дисплеи) решает этот модуль,
/// а calc отвечает за рамку на выбранном экране. Иначе в режиме «через мониторы и
/// размеры» перебор размеров на крайнем экране превратился бы в первую половину.
/// `visible_ignoring_stage` не нужен: «центр» сам передаёт рабочую область без
/// полосы Stage Manager.
fn rect_in(
    visible: Rect,
    action: Action,
    window: Rect,
    last: Option<&LastAction>,
    env: &ScreenEnvironment,
) -> Option<CalcResult> {
    calc::calculate(&CalcParams {
        window,
        visible,
        visible_ignoring_stage: None,
        action,
        last,
        config: env.config,
        source_visible: None,
        num_screens: 1,
        primary_max_y: env.primary_height(),
    })
}

/// `calculateRect` действия на заданной рабочей области — как оригинал в
/// переездах между экранами (центр на новом экране, первая половина на соседнем,
/// «к краю» на соседнем, повтор последнего действия). `None` — у действия в
/// оригинале нет `calculateRect` (Restore, tile/cascade/reverse).
fn rect_of(
    visible: Rect,
    action: Action,
    window: Rect,
    last: Option<&LastAction>,
    env: &ScreenEnvironment,
) -> Option<calc::RectResult> {
    calc::calculate_rect(&calc::RectParams {
        window,
        visible,
        action,
        last,
        config: env.config,
        primary_max_y: env.primary_height(),
    })
}

fn visible_frame(
    screen: &Screen,
    params: &WindowCalculationParameters,
    env: &ScreenEnvironment,
) -> Rect {
    adjusted_visible_frame(screen, env, params.ignore_todo, false)
}

/// `WindowCalculation.calculate`: расчёт на рабочей области текущего экрана.
fn on_current_screen(
    params: &WindowCalculationParameters,
    env: &ScreenEnvironment,
) -> Option<WindowCalculationResult> {
    let screen = &params.usable_screens.current;
    let result = rect_in(
        visible_frame(screen, params, env),
        params.action,
        params.window.rect,
        params.last_action.as_ref(),
        env,
    )?;
    let mut calculated = WindowCalculationResult::new(result.rect, screen.clone(), result.action);
    calculated.resulting_sub_action = result.sub_action;
    Some(calculated)
}

/// `CenterCalculation` / `CenterProminentlyCalculation`: центр считается по
/// рабочей области без полосы Stage Manager, если не включено «всегда учитывать
/// полосу». Слишком большое окно разворачивается (`resultingAction = .maximize`).
fn center(
    params: &WindowCalculationParameters,
    env: &ScreenEnvironment,
) -> Option<WindowCalculationResult> {
    let screen = &params.usable_screens.current;
    let screen_frame = (env.config.always_account_for_stage != Some(true))
        .then(|| adjusted_visible_frame(screen, env, params.ignore_todo, true));
    let visible = screen_frame.unwrap_or_else(|| visible_frame(screen, params, env));
    let result = rect_in(
        visible,
        params.action,
        params.window.rect,
        params.last_action.as_ref(),
        env,
    )?;
    let mut calculated = WindowCalculationResult::new(result.rect, screen.clone(), result.action);
    calculated.resulting_screen_frame = screen_frame;
    Some(calculated)
}

/// Центр на другом экране (`CenterCalculation.calculateRect`): слишком большое
/// окно разворачивается — тогда итоговое действие «развернуть».
fn centered_on(
    visible: Rect,
    params: &WindowCalculationParameters,
    env: &ScreenEnvironment,
) -> Option<(Rect, Action)> {
    let result = rect_of(visible, Action::Center, params.window.rect, None, env)?;
    let action = if result.resulting_action == Some(Action::Maximize) {
        Action::Maximize
    } else {
        params.action
    };
    Some((result.rect, action))
}

/// «Следующий/предыдущий дисплей» (`NextPrevDisplayCalculation`). На одном
/// экране — нет расчёта. Развёрнутое окно разворачивается и на новом экране
/// (если не выключено `autoMaximize`), остальные — по центру.
fn next_prev_display(
    params: &WindowCalculationParameters,
    env: &ScreenEnvironment,
) -> Option<WindowCalculationResult> {
    let usable = params.usable_screens;
    if usable.num_screens <= 1 {
        return None;
    }
    let adjacent = usable.adjacent.as_ref()?;
    let screen = if params.action == Action::NextDisplay {
        adjacent.next.clone()
    } else {
        adjacent.prev.clone()
    };
    let visible = visible_frame(&screen, params, env);

    if env.config.attempt_match_on_next_prev_display == Some(true) {
        return attempt_match(params, env, screen, visible);
    }

    let maximize = params
        .last_action
        .is_some_and(|last| last.action == Action::Maximize)
        && env.config.auto_maximize != Some(false);
    let (rect, action) = if maximize {
        (visible, Action::Maximize)
    } else {
        centered_on(visible, params, env)?
    };
    Some(WindowCalculationResult::new(rect, screen, action))
}

/// «Дисплей N» (`SpecificDisplayCalculation`): номер — в порядке
/// `screensOrderedByX`. Нет такого дисплея, экран один или окно уже на нём —
/// нет расчёта. Окно встаёт по центру; «развёрнутость» не переносится.
fn specific_display(
    params: &WindowCalculationParameters,
    env: &ScreenEnvironment,
) -> Option<WindowCalculationResult> {
    let usable = params.usable_screens;
    if usable.num_screens <= 1 {
        return None;
    }
    let index = params.action.display_index()?;
    let target = usable.ordered.get(index)?.clone();
    if target.same_display(&usable.current) {
        return None;
    }
    let visible = visible_frame(&target, params, env);

    if env.config.attempt_match_on_next_prev_display == Some(true) {
        return attempt_match(params, env, target, visible);
    }

    let (rect, action) = centered_on(visible, params, env)?;
    Some(WindowCalculationResult::new(rect, target, action))
}

/// `attemptMatchOnNextPrevDisplay`: на новом экране повторить последнее
/// действие (как первое нажатие), а если его нет — перенести окно
/// пропорционально (#1723). История окна забывается.
fn attempt_match(
    params: &WindowCalculationParameters,
    env: &ScreenEnvironment,
    screen: Screen,
    visible: Rect,
) -> Option<WindowCalculationResult> {
    if let Some(last) = params.last_action {
        if let Some(result) = rect_of(visible, last.action, params.window.rect, None, env) {
            let mut calculated = WindowCalculationResult::new(result.rect, screen, last.action);
            calculated.forget_last_action = true;
            return Some(calculated);
        }
    }

    let source = visible_frame(&params.usable_screens.current, params, env);
    let mapped = relative_positioned_rect(&params.window.rect, &source, &visible);
    Some(WindowCalculationResult::new(mapped, screen, params.action))
}

/// Перенести окно из рабочей области `source` в `destination` с теми же долями
/// положения и размера и не дать ему вылезти за её края
/// (`NextPrevDisplayCalculation.relativePositionedRect`).
pub fn relative_positioned_rect(window: &Rect, source: &Rect, destination: &Rect) -> Rect {
    if source.w <= 0.0 || source.h <= 0.0 {
        return *window;
    }
    let origin_x_fraction = (window.min_x() - source.min_x()) / source.w;
    let origin_y_fraction = (window.min_y() - source.min_y()) / source.h;
    let width_fraction = window.w / source.w;
    let height_fraction = window.h / source.h;

    let mut rect = Rect::new(
        destination.min_x() + origin_x_fraction * destination.w,
        destination.min_y() + origin_y_fraction * destination.h,
        width_fraction * destination.w,
        height_fraction * destination.h,
    );

    if rect.max_x() > destination.max_x() {
        rect.x = destination.max_x() - rect.w;
    }
    if rect.min_x() < destination.min_x() {
        rect.x = destination.min_x();
    }
    if rect.max_y() > destination.max_y() {
        rect.y = destination.max_y() - rect.h;
    }
    if rect.min_y() < destination.min_y() {
        rect.y = destination.min_y();
    }
    rect
}

/// Повтор того же действия над окном, которое стоит ровно там, где его оставили
/// (`isRepeatedCommand`, без допуска).
fn is_repeated_command(params: &WindowCalculationParameters, env: &ScreenEnvironment) -> bool {
    match params.last_action {
        Some(last) if last.action == params.action => {
            last.rect.screen_flipped(env.primary_height()) == params.window.rect
        }
        _ => false,
    }
}

/// `LeftRightHalfCalculation`: в режимах «через мониторы» повтор половины
/// перекидывает окно на соседний экран; в остальных — `calc` (перебор размеров
/// или всегда половина).
fn left_right_half(
    params: &WindowCalculationParameters,
    env: &ScreenEnvironment,
) -> Option<WindowCalculationResult> {
    match env.config.subsequent_execution_mode {
        SubsequentExecutionMode::AcrossMonitor => Some(across_displays(params, env)),
        SubsequentExecutionMode::AcrossAndResize if params.usable_screens.num_screens > 1 => {
            Some(across_displays(params, env))
        }
        _ => on_current_screen(params, env),
    }
}

fn across_displays(
    params: &WindowCalculationParameters,
    env: &ScreenEnvironment,
) -> WindowCalculationResult {
    let screen = params.usable_screens.current.clone();
    if params.action == Action::RightHalf {
        right_across_displays(params, env, screen)
    } else {
        left_across_displays(params, env, screen)
    }
}

fn is_across_and_resize(env: &ScreenEnvironment) -> bool {
    env.config.subsequent_execution_mode == SubsequentExecutionMode::AcrossAndResize
}

/// Левая половина; повтор — правая половина предыдущего экрана. В режиме
/// «через мониторы и размеры» с самого левого экрана — перебор размеров.
fn left_across_displays(
    params: &WindowCalculationParameters,
    env: &ScreenEnvironment,
    screen: Screen,
) -> WindowCalculationResult {
    if is_repeated_command(params, env) {
        if let Some(adjacent) = &params.usable_screens.adjacent {
            let prev = &adjacent.prev;
            let prev_is_last = params
                .usable_screens
                .ordered
                .last()
                .is_some_and(|last| last.same_display(prev));
            if is_across_and_resize(env) && prev_is_last {
                if let Some(result) = on_current_screen(params, env) {
                    return result;
                }
            } else {
                return right_across_displays(
                    &params.with_action(Action::RightHalf),
                    env,
                    prev.clone(),
                );
            }
        }
    }
    first_half(params, env, screen, Action::LeftHalf)
}

/// Правая половина; повтор — левая половина следующего экрана.
fn right_across_displays(
    params: &WindowCalculationParameters,
    env: &ScreenEnvironment,
    screen: Screen,
) -> WindowCalculationResult {
    if is_repeated_command(params, env) {
        if let Some(adjacent) = &params.usable_screens.adjacent {
            let next = &adjacent.next;
            let next_is_first = params
                .usable_screens
                .ordered
                .first()
                .is_some_and(|first| first.same_display(next));
            if is_across_and_resize(env) && next_is_first {
                if let Some(result) = on_current_screen(params, env) {
                    return result;
                }
            } else {
                return left_across_displays(
                    &params.with_action(Action::LeftHalf),
                    env,
                    next.clone(),
                );
            }
        }
    }
    first_half(params, env, screen, Action::RightHalf)
}

/// Первая половина (`calculateFirstRect`) на заданном экране.
fn first_half(
    params: &WindowCalculationParameters,
    env: &ScreenEnvironment,
    screen: Screen,
    action: Action,
) -> WindowCalculationResult {
    let visible = visible_frame(&screen, params, env);
    let rect = rect_of(visible, action, params.window.rect, None, env)
        .map(|result| result.rect)
        .unwrap_or(visible);
    WindowCalculationResult::new(rect, screen, action)
}

/// `MoveLeftRightCalculation`: в режимах «через мониторы» повтор «к левому
/// краю» переносит окно к правому краю предыдущего экрана (и наоборот).
fn move_left_right(
    params: &WindowCalculationParameters,
    env: &ScreenEnvironment,
) -> Option<WindowCalculationResult> {
    let usable = params.usable_screens;
    let can_traverse_displays =
        env.config.subsequent_execution_mode.traverses_displays() && usable.num_screens > 1;

    if can_traverse_displays && is_repeated_command(params, env) {
        let (screen, action) = if params.action == Action::MoveLeft {
            let screen = usable
                .adjacent
                .as_ref()
                .map(|adjacent| adjacent.prev.clone())
                .unwrap_or_else(|| usable.current.clone());
            (screen, Action::MoveRight)
        } else {
            let screen = usable
                .adjacent
                .as_ref()
                .map(|adjacent| adjacent.next.clone())
                .unwrap_or_else(|| usable.current.clone());
            (screen, Action::MoveLeft)
        };
        let result = rect_of(
            visible_frame(&screen, params, env),
            action,
            params.window.rect,
            params.last_action.as_ref(),
            env,
        )?;
        return Some(WindowCalculationResult::new(result.rect, screen, action));
    }

    let screen = usable.current.clone();
    let result = rect_in(
        visible_frame(&screen, params, env),
        params.action,
        params.window.rect,
        params.last_action.as_ref(),
        env,
    )?;
    Some(WindowCalculationResult::new(
        result.rect,
        screen,
        params.action,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::screen_detection::{detect_screens, AdjacentScreens};
    use crate::stage::StageState;

    fn screen(id: u32, frame: Rect, visible: Rect) -> Screen {
        Screen {
            id,
            frame,
            visible_frame: visible,
            name: format!("экран {id}"),
            is_main: false,
            scale: 2.0,
            safe_area_top: 0.0,
        }
    }

    /// Ноутбук слева (основной), монитор справа.
    fn two_screens() -> Vec<Screen> {
        vec![
            screen(
                1,
                Rect::new(0.0, 0.0, 1728.0, 1117.0),
                Rect::new(0.0, 83.0, 1728.0, 1002.0),
            ),
            screen(
                2,
                Rect::new(1728.0, 0.0, 2560.0, 1440.0),
                Rect::new(1728.0, 0.0, 2560.0, 1415.0),
            ),
        ]
    }

    fn env<'a>(config: &'a Config, screens: &'a [Screen]) -> ScreenEnvironment<'a> {
        ScreenEnvironment {
            config,
            screens,
            separate_spaces: true,
            stage: StageState::default(),
            todo_screen: None,
        }
    }

    const PRIMARY: f64 = 1117.0;

    fn usable_for(window: Rect, screens: &[Screen], config: &Config) -> UsableScreens {
        detect_screens(
            Some(window.screen_flipped(PRIMARY)),
            screens,
            config,
            PRIMARY,
        )
        .unwrap()
    }

    fn params<'a>(
        window: Rect,
        usable: &'a UsableScreens,
        action: Action,
        last: Option<LastAction>,
    ) -> WindowCalculationParameters<'a> {
        WindowCalculationParameters {
            window: Window {
                id: Some(1),
                rect: window,
            },
            usable_screens: usable,
            action,
            last_action: last,
            ignore_todo: false,
        }
    }

    /// Запись истории так, как её сделал бы менеджер окон: рамка в AX.
    fn last(action: Action, rect_cocoa: Rect, count: u32) -> LastAction {
        LastAction {
            action,
            sub_action: None,
            rect: rect_cocoa.screen_flipped(PRIMARY),
            count,
        }
    }

    #[test]
    fn next_display_centers_window_and_keeps_maximize() {
        let config = Config::default();
        let screens = two_screens();
        let env = env(&config, &screens);
        let window = Rect::new(100.0, 200.0, 800.0, 600.0);
        let usable = usable_for(window, &screens, &config);

        let result = calculate(&params(window, &usable, Action::NextDisplay, None), &env).unwrap();
        assert_eq!(result.screen.id, 2);
        assert_eq!(result.resulting_action, Action::NextDisplay);
        assert_eq!(result.rect, Rect::new(1728.0 + 880.0, 408.0, 800.0, 600.0));

        // После «развернуть» — развернуть и на новом экране.
        let maximized = screens[0].visible_frame;
        let usable = usable_for(maximized, &screens, &config);
        let history = last(Action::Maximize, maximized, 1);
        let result = calculate(
            &params(maximized, &usable, Action::NextDisplay, Some(history)),
            &env,
        )
        .unwrap();
        assert_eq!(result.rect, screens[1].visible_frame);
        assert_eq!(result.resulting_action, Action::Maximize);

        // autoMaximize выключен — по центру.
        let config = Config {
            auto_maximize: Some(false),
            ..Config::default()
        };
        let env = super::tests::env(&config, &screens);
        let result = calculate(
            &params(maximized, &usable, Action::NextDisplay, Some(history)),
            &env,
        )
        .unwrap();
        assert_eq!(result.resulting_action, Action::NextDisplay);
    }

    #[test]
    fn display_moves_need_another_screen() {
        let config = Config::default();
        let one = vec![two_screens()[0].clone()];
        let env_one = env(&config, &one);
        let window = Rect::new(100.0, 200.0, 800.0, 600.0);
        let usable = usable_for(window, &one, &config);
        for action in [
            Action::NextDisplay,
            Action::PreviousDisplay,
            Action::Display(1),
            Action::Display(2),
        ] {
            assert!(
                calculate(&params(window, &usable, action, None), &env_one).is_none(),
                "{action:?}"
            );
        }

        // Даже с «обходом одного экрана» перенос на себя не считается.
        let traverse = Config {
            traverse_single_screen: Some(true),
            ..Config::default()
        };
        let env_traverse = env(&traverse, &one);
        let usable = usable_for(window, &one, &traverse);
        assert!(calculate(
            &params(window, &usable, Action::NextDisplay, None),
            &env_traverse
        )
        .is_none());

        // Два экрана: «Дисплей 1» для окна на первом и «Дисплей 3» — нечего делать.
        let screens = two_screens();
        let env_two = env(&config, &screens);
        let usable = usable_for(window, &screens, &config);
        assert!(calculate(&params(window, &usable, Action::Display(1), None), &env_two).is_none());
        assert!(calculate(&params(window, &usable, Action::Display(3), None), &env_two).is_none());
        let result =
            calculate(&params(window, &usable, Action::Display(2), None), &env_two).unwrap();
        assert_eq!(result.screen.id, 2);
    }

    #[test]
    fn specific_display_centers_even_after_maximize() {
        let config = Config::default();
        let screens = two_screens();
        let env = env(&config, &screens);
        // Развёрнутое на ноутбуке окно меньше монитора — просто центр.
        let maximized = screens[0].visible_frame;
        let usable = usable_for(maximized, &screens, &config);
        let history = last(Action::Maximize, maximized, 1);
        let result = calculate(
            &params(maximized, &usable, Action::Display(2), Some(history)),
            &env,
        )
        .unwrap();
        assert_eq!(result.resulting_action, Action::Display(2));
        assert_eq!(result.rect.w, 1728.0);
        assert_eq!(result.rect.h, 1002.0);
        assert_eq!(result.rect.x, 1728.0 + 416.0);
    }

    #[test]
    fn attempt_match_repeats_last_action_or_maps_proportionally() {
        let config = Config {
            attempt_match_on_next_prev_display: Some(true),
            ..Config::default()
        };
        let screens = two_screens();
        let env = env(&config, &screens);

        // Окно в левой половине ноутбука → левая половина монитора, история забывается.
        let left_half = Rect::new(0.0, 83.0, 864.0, 1002.0);
        let usable = usable_for(left_half, &screens, &config);
        let history = last(Action::LeftHalf, left_half, 3);
        let result = calculate(
            &params(left_half, &usable, Action::NextDisplay, Some(history)),
            &env,
        )
        .unwrap();
        assert_eq!(result.rect, Rect::new(1728.0, 0.0, 1280.0, 1415.0));
        assert_eq!(result.resulting_action, Action::LeftHalf);
        assert!(result.forget_last_action);

        // Истории нет — те же доли на новом экране.
        let window = Rect::new(432.0, 83.0 + 250.5, 864.0, 501.0);
        let result = calculate(&params(window, &usable, Action::Display(2), None), &env).unwrap();
        assert_eq!(
            result.rect,
            Rect::new(1728.0 + 640.0, 353.75, 1280.0, 707.5)
        );
        assert_eq!(result.resulting_action, Action::Display(2));
        assert!(!result.forget_last_action);
    }

    /// Регрессия слияния calc + wm: прошлое действие — сам переход на другой экран.
    /// Оригинал повторяет его `calculateRect` (центр на новом экране), а не пищит.
    #[test]
    fn attempt_match_replays_display_move_as_center() {
        let config = Config {
            attempt_match_on_next_prev_display: Some(true),
            ..Config::default()
        };
        let screens = two_screens();
        let env = env(&config, &screens);

        let window = Rect::new(464.0, 284.0, 800.0, 600.0);
        let usable = usable_for(window, &screens, &config);
        let history = last(Action::NextDisplay, window, 1);
        let result = calculate(
            &params(window, &usable, Action::NextDisplay, Some(history)),
            &env,
        )
        .expect("повтор перехода на экран не должен пищать");
        // Центр рабочей области монитора: x = 1728 + (2560 − 800)/2, y = round((1415 − 600)/2).
        assert_eq!(result.rect, Rect::new(2608.0, 408.0, 800.0, 600.0));
        assert_eq!(result.resulting_action, Action::NextDisplay);
        assert!(result.forget_last_action);
    }

    #[test]
    fn relative_rect_is_clamped_inside_destination() {
        let source = Rect::new(0.0, 0.0, 1000.0, 1000.0);
        let destination = Rect::new(2000.0, 0.0, 500.0, 400.0);
        // Окно вылезает за правый край источника — на новом экране прижимается к краю.
        let window = Rect::new(900.0, -100.0, 300.0, 200.0);
        let rect = relative_positioned_rect(&window, &source, &destination);
        assert_eq!(rect, Rect::new(2350.0, 0.0, 150.0, 80.0));
        // Пустой источник — окно как есть.
        assert_eq!(
            relative_positioned_rect(&window, &Rect::new(0.0, 0.0, 0.0, 10.0), &destination),
            window
        );
    }

    #[test]
    fn across_monitor_moves_repeated_half_to_neighbour() {
        let config = Config {
            subsequent_execution_mode: SubsequentExecutionMode::AcrossMonitor,
            ..Config::default()
        };
        let screens = two_screens();
        let env = env(&config, &screens);

        // Первое нажатие — левая половина своего экрана (монитора).
        let window = Rect::new(2000.0, 300.0, 800.0, 600.0);
        let usable = usable_for(window, &screens, &config);
        let first = calculate(&params(window, &usable, Action::LeftHalf, None), &env).unwrap();
        assert_eq!(first.rect, Rect::new(1728.0, 0.0, 1280.0, 1415.0));
        assert_eq!(first.screen.id, 2);

        // Повтор: окно там, где его оставили, — правая половина ноутбука.
        let usable = usable_for(first.rect, &screens, &config);
        let history = last(Action::LeftHalf, first.rect, 1);
        let second = calculate(
            &params(first.rect, &usable, Action::LeftHalf, Some(history)),
            &env,
        )
        .unwrap();
        assert_eq!(second.screen.id, 1);
        assert_eq!(second.resulting_action, Action::RightHalf);
        assert_eq!(second.rect, Rect::new(864.0, 83.0, 864.0, 1002.0));

        // Окно чуть сдвинули — это не повтор, снова левая половина монитора.
        let moved = first.rect.offset_by(1.0, 0.0);
        let usable = usable_for(moved, &screens, &config);
        let again = calculate(
            &params(moved, &usable, Action::LeftHalf, Some(history)),
            &env,
        )
        .unwrap();
        assert_eq!(again.screen.id, 2);
        assert_eq!(again.resulting_action, Action::LeftHalf);
    }

    #[test]
    fn across_and_resize_cycles_sizes_at_the_outer_edge() {
        let config = Config {
            subsequent_execution_mode: SubsequentExecutionMode::AcrossAndResize,
            ..Config::default()
        };
        let screens = two_screens();
        let env = env(&config, &screens);

        // Левая половина самого левого экрана: предыдущий экран — последний
        // в порядке, поэтому вместо переезда — перебор размеров (2/3).
        let left_half = Rect::new(0.0, 83.0, 864.0, 1002.0);
        let usable = usable_for(left_half, &screens, &config);
        let history = last(Action::LeftHalf, left_half, 1);
        let result = calculate(
            &params(left_half, &usable, Action::LeftHalf, Some(history)),
            &env,
        )
        .unwrap();
        assert_eq!(result.screen.id, 1);
        assert_eq!(result.rect.w, 1152.0);
        assert_eq!(result.resulting_action, Action::LeftHalf);

        // Правая половина левого экрана → левая половина монитора.
        let right_half = Rect::new(864.0, 83.0, 864.0, 1002.0);
        let usable = usable_for(right_half, &screens, &config);
        let history = last(Action::RightHalf, right_half, 1);
        let result = calculate(
            &params(right_half, &usable, Action::RightHalf, Some(history)),
            &env,
        )
        .unwrap();
        assert_eq!(result.screen.id, 2);
        assert_eq!(result.resulting_action, Action::LeftHalf);

        // Один экран — обычный перебор размеров.
        let one = vec![screens[0].clone()];
        let env_one = super::tests::env(&config, &one);
        let usable = usable_for(left_half, &one, &config);
        let result = calculate(
            &params(
                left_half,
                &usable,
                Action::LeftHalf,
                Some(history_left(left_half)),
            ),
            &env_one,
        )
        .unwrap();
        assert_eq!(result.rect.w, 1152.0);
    }

    fn history_left(rect: Rect) -> LastAction {
        last(Action::LeftHalf, rect, 1)
    }

    #[test]
    fn across_monitor_on_single_screen_with_traversal_flips_sides() {
        let config = Config {
            subsequent_execution_mode: SubsequentExecutionMode::AcrossMonitor,
            traverse_single_screen: Some(true),
            ..Config::default()
        };
        let one = vec![two_screens()[0].clone()];
        let env = env(&config, &one);
        let left_half = Rect::new(0.0, 83.0, 864.0, 1002.0);
        let usable = usable_for(left_half, &one, &config);
        assert!(matches!(usable.adjacent, Some(AdjacentScreens { .. })));
        let result = calculate(
            &params(
                left_half,
                &usable,
                Action::LeftHalf,
                Some(history_left(left_half)),
            ),
            &env,
        )
        .unwrap();
        assert_eq!(result.resulting_action, Action::RightHalf);
        assert_eq!(result.rect, Rect::new(864.0, 83.0, 864.0, 1002.0));
    }

    #[test]
    fn move_left_repeated_goes_to_previous_screen_right_edge() {
        let config = Config {
            subsequent_execution_mode: SubsequentExecutionMode::AcrossMonitor,
            ..Config::default()
        };
        let screens = two_screens();
        let env = env(&config, &screens);

        let window = Rect::new(1728.0, 400.0, 800.0, 600.0);
        let usable = usable_for(window, &screens, &config);
        let history = last(Action::MoveLeft, window, 1);
        let result = calculate(
            &params(window, &usable, Action::MoveLeft, Some(history)),
            &env,
        )
        .unwrap();
        assert_eq!(result.screen.id, 1);
        assert_eq!(result.resulting_action, Action::MoveRight);
        assert_eq!(result.rect.x, 1728.0 - 800.0);

        // В обычном режиме — просто к левому краю своего экрана.
        let config = Config::default();
        let env = super::tests::env(&config, &screens);
        let result = calculate(
            &params(window, &usable, Action::MoveLeft, Some(history)),
            &env,
        )
        .unwrap();
        assert_eq!(result.screen.id, 2);
        assert_eq!(result.resulting_action, Action::MoveLeft);
    }

    #[test]
    fn center_ignores_stage_strip_unless_asked() {
        let screens = two_screens();
        let config = Config::default();
        let mut environment = env(&config, &screens);
        environment.stage = StageState {
            active: true,
            position: Some(crate::stage::StageStripPosition::Left),
            visible_on: vec![1],
        };
        let window = Rect::new(100.0, 200.0, 800.0, 600.0);
        let usable = usable_for(window, &screens, &config);
        let result =
            calculate(&params(window, &usable, Action::Center, None), &environment).unwrap();
        assert_eq!(
            result.resulting_screen_frame,
            Some(screens[0].visible_frame)
        );
        assert_eq!(result.rect.x, 464.0);

        let config = Config {
            always_account_for_stage: Some(true),
            ..Config::default()
        };
        let mut environment = env(&config, &screens);
        environment.stage = StageState {
            active: true,
            position: Some(crate::stage::StageStripPosition::Left),
            visible_on: vec![1],
        };
        let result =
            calculate(&params(window, &usable, Action::Center, None), &environment).unwrap();
        assert_eq!(result.resulting_screen_frame, None);
        assert_eq!(result.rect.x, 190.0 + 369.0);
    }

    #[test]
    fn other_actions_use_calc_on_current_screen() {
        let config = Config::default();
        let screens = two_screens();
        let env = env(&config, &screens);
        let window = Rect::new(2000.0, 300.0, 800.0, 600.0);
        let usable = usable_for(window, &screens, &config);
        let result = calculate(&params(window, &usable, Action::Maximize, None), &env).unwrap();
        assert_eq!(result.rect, screens[1].visible_frame);
        assert_eq!(result.initial_rect, result.rect);
        assert_eq!(result.screen.id, 2);
        assert!(calculate(&params(window, &usable, Action::Restore, None), &env).is_none());
    }
}
