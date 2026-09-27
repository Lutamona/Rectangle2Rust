//! Чистая логика `SnappingManager.swift` — без обращения к системе:
//!
//! - зона у края экрана под курсором (`directionalLocationOfCursor`);
//! - область прилипания под курсором (`snapAreaContainingCursor`);
//! - рамка подсветки (`getBoxRect`) и точка, из которой она растёт
//!   (`getFootprintAnimationOrigin`);
//! - модификаторы (`canSnap`);
//! - слушать ли мышь: «Игнорировать приложение» (`ApplicationToggle` +
//!   `frontAppChanged`), полный экран, выключатель прилипания;
//! - возврат размера при отрыве окна (`unsnapRestore`, `getRestoreRect`);
//! - защита от случайного Mission Control (`filter`).
//!
//! Координаты курсора и экранов — Cocoa (y снизу), как у `NSEvent.mouseLocation`
//! и `NSScreen.frame`; рамки окон — AX (y сверху), как у `AccessibilityElement.frame`.

use crate::actions::{Action, WindowActionCategory};
use crate::calc::{self, LastAction};
use crate::config::{Config, Directional, SnapAreas, TodoSidebarSide};
use crate::event_monitor::MouseEventKind;
use crate::geometry::Rect;
use crate::screens::Screen;

use super::area_model::DisplayOrientation;
use super::compound::CompoundParams;

// ---------------------------------------------------------------- область

/// Область прилипания под курсором (`SnapArea`): экран, зона у края и действие.
#[derive(Clone, Debug)]
pub struct SnapArea {
    pub screen: Screen,
    pub directional: Directional,
    pub action: Action,
}

/// Экраны сравниваются как дисплеи (в Swift — один и тот же `NSScreen`).
impl PartialEq for SnapArea {
    fn eq(&self, other: &SnapArea) -> bool {
        self.screen.same_display(&other.screen)
            && self.directional == other.directional
            && self.action == other.action
    }
}

/// Настройки зон у краёв (`Defaults.snapEdgeMargin*`, `cornerSnapAreaSize`,
/// `shortEdgeSnapAreaSize`, `ignoredSnapAreas`, карты областей).
#[derive(Clone, Debug, PartialEq)]
pub struct ZoneSettings {
    pub margin_left: f64,
    pub margin_right: f64,
    pub corner_size: f64,
    /// Верхний/нижний отступ, короткие зоны и выключенные короткие зоны.
    pub compound: CompoundParams,
    pub landscape: SnapAreas,
    pub portrait: SnapAreas,
}

impl ZoneSettings {
    /// Настройки читаются при каждом обращении — правки действуют сразу (в
    /// оригинале отступы читались один раз при запуске).
    pub fn from_config(config: &Config) -> ZoneSettings {
        ZoneSettings {
            margin_left: config.snap_edge_margin_left as f64,
            margin_right: config.snap_edge_margin_right as f64,
            corner_size: config.corner_snap_area_size as f64,
            compound: CompoundParams {
                margin_top: config.snap_edge_margin_top as f64,
                margin_bottom: config.snap_edge_margin_bottom as f64,
                short_edge_size: config.short_edge_snap_area_size as f64,
                ignored: config.ignored_snap_area_options(),
            },
            landscape: config.landscape_snap_areas_or_default(),
            portrait: config.portrait_snap_areas_or_default(),
        }
    }

    fn areas(&self, orientation: DisplayOrientation) -> &SnapAreas {
        match orientation {
            DisplayOrientation::Landscape => &self.landscape,
            DisplayOrientation::Portrait => &self.portrait,
        }
    }
}

/// Зона у края экрана `frame`, в которой курсор `loc` (`directionalLocationOfCursor`).
/// Сначала углы (квадраты `отступ + cornerSnapAreaSize`), потом полосы шириной
/// в отступ у левого, правого, верхнего и нижнего края. Края экрана включены.
pub fn directional_location(
    (x, y): (f64, f64),
    frame: &Rect,
    settings: &ZoneSettings,
) -> Option<Directional> {
    if x < frame.min_x() || x > frame.max_x() || y < frame.min_y() || y > frame.max_y() {
        return None;
    }
    let corner = settings.corner_size;
    let top = settings.compound.margin_top;
    let bottom = settings.compound.margin_bottom;
    let left = settings.margin_left;
    let right = settings.margin_right;

    if x < frame.min_x() + left + corner {
        if y >= frame.max_y() - top - corner {
            return Some(Directional::Tl);
        }
        if y <= frame.min_y() + bottom + corner {
            return Some(Directional::Bl);
        }
        if x < frame.min_x() + left {
            return Some(Directional::L);
        }
    }
    if x > frame.max_x() - right - corner {
        if y >= frame.max_y() - top - corner {
            return Some(Directional::Tr);
        }
        if y <= frame.min_y() + bottom + corner {
            return Some(Directional::Br);
        }
        if x > frame.max_x() - right {
            return Some(Directional::R);
        }
    }
    if y > frame.max_y() - top {
        return Some(Directional::T);
    }
    if y < frame.min_y() + bottom {
        return Some(Directional::B);
    }
    None
}

/// Область под курсором (`snapAreaContainingCursor`): первый по порядку экран,
/// у края которого курсор и у которого эта зона включена. `todo_side` —
/// сторона панели Todo, если тащат само окно Todo в режиме Todo: тогда его
/// край даёт «Todo слева/справа». Спрашивается, как в оригинале, только когда
/// курсор уже у края экрана (и не больше раза за вызов): ответ стоит
/// AX-запроса к приложению Todo. `prior` — область, в которой курсор был до
/// этого (для составных областей).
pub fn snap_area_containing(
    loc: (f64, f64),
    screens: &[Screen],
    settings: &ZoneSettings,
    mut todo_side: impl FnMut() -> Option<TodoSidebarSide>,
    prior: Option<&SnapArea>,
) -> Option<SnapArea> {
    let mut side = None;
    for screen in screens {
        let Some(directional) = directional_location(loc, &screen.frame, settings) else {
            continue;
        };
        let area = |action| SnapArea {
            screen: screen.clone(),
            directional,
            action,
        };
        match (*side.get_or_insert_with(&mut todo_side), directional) {
            (Some(TodoSidebarSide::Left), Directional::L) => return Some(area(Action::LeftTodo)),
            (Some(TodoSidebarSide::Right), Directional::R) => return Some(area(Action::RightTodo)),
            _ => {}
        }
        let orientation = DisplayOrientation::of(&screen.frame);
        let Some(config) = settings.areas(orientation).get(&directional) else {
            continue;
        };
        if let Some(action) = config
            .action
            .and_then(|raw| i32::try_from(raw).ok())
            .and_then(Action::from_raw)
        {
            return Some(area(action));
        }
        if let Some(compound) = config.compound {
            return compound
                .snap_action(
                    loc,
                    &screen.frame,
                    prior.map(|prior| prior.action),
                    &settings.compound,
                )
                .map(area);
        }
    }
    None
}

// ---------------------------------------------------------------- подсветка

/// Рамка подсветки (`getBoxRect`, Cocoa): расчёт действия на рабочей области
/// `visible` без истории окна, затем зазоры между окнами.
pub fn footprint_rect(
    action: Action,
    window: Rect,
    visible: Rect,
    config: &Config,
    primary_max_y: f64,
) -> Option<Rect> {
    // Как `getBoxRect`: `calculateRect` действия (без истории), а не полный расчёт.
    let result = calc::calculate_rect(&calc::RectParams {
        window,
        visible,
        action,
        last: None,
        config,
        primary_max_y,
    })?;
    Some(calc::apply_gaps(
        result.rect,
        action,
        result.sub_action,
        config,
    ))
}

/// Точка, из которой подсветка вырастает при анимации
/// (`getFootprintAnimationOrigin`): угол или середина стороны у своего края.
pub fn footprint_animation_origin(directional: Directional, rect: &Rect) -> Option<(f64, f64)> {
    let point = match directional {
        Directional::Tl => (rect.min_x(), rect.max_y()),
        Directional::T => (rect.mid_x(), rect.max_y()),
        Directional::Tr => (rect.max_x(), rect.max_y()),
        Directional::L => (rect.min_x(), rect.mid_y()),
        Directional::R => (rect.max_x(), rect.mid_y()),
        Directional::Bl => (rect.min_x(), rect.min_y()),
        Directional::B => (rect.mid_x(), rect.min_y()),
        Directional::Br => (rect.max_x(), rect.min_y()),
        Directional::C => return None,
    };
    Some(point)
}

// ---------------------------------------------------------------- можно ли прилипать

/// Модификаторы (`canSnap`): если в настройках заданы (`snapModifiers` > 0),
/// зажато должно быть ровно это сочетание. `flags` — уже без битов конкретных
/// клавиш (`deviceIndependentFlagsMask`).
pub fn modifiers_allow_snap(flags: u64, snap_modifiers: i64) -> bool {
    snap_modifiers <= 0 || flags as i64 == snap_modifiers
}

/// Приложения, для которых drag-to-snap выключается совсем, если их
/// игнорируют, даже когда «Игнорировать» прилипание не выключает
/// (`fullIgnoreBundleIds` по умолчанию; сравнение — по началу bundle id).
pub const DEFAULT_FULL_IGNORE_BUNDLE_IDS: [&str; 6] = [
    "com.install4j",
    "com.mathworks.matlab",
    "com.live2d.cubism.CECubismEditorApp",
    "com.aquafold.datastudio.DataStudio",
    "com.adobe.illustrator",
    "com.adobe.AfterEffects",
];

/// Можно ли прилипание, пока впереди приложение `front_bundle_id`
/// (`ApplicationToggle` + `SnappingManager.frontAppChanged`): приложение из
/// «Игнорировать» выключает его, если `ignoreDragSnapToo` не выключено явно;
/// а если выключено — только для приложений из `fullIgnoreBundleIds`.
/// У приложения без bundle id ограничений нет.
pub fn drag_snap_allowed_for(front_bundle_id: Option<&str>, config: &Config) -> bool {
    let Some(bundle_id) = front_bundle_id else {
        return true;
    };
    if !config.is_app_disabled(bundle_id) {
        return true;
    }
    if config.ignore_drag_snap_too != Some(false) {
        return false;
    }
    let starts = |prefix: &str| bundle_id.starts_with(prefix);
    match &config.full_ignore_bundle_ids {
        Some(ids) => !ids.iter().any(|id| starts(id)),
        None => !DEFAULT_FULL_IGNORE_BUNDLE_IDS.iter().any(|id| starts(id)),
    }
}

/// Слушать ли мышь (`toggleListening`): приложение впереди не выключает
/// прилипание, переднее окно не во весь экран, прилипание не выключено и не
/// на паузе (открыт диалог импорта или экспорта настроек).
pub fn should_listen(
    allowed_for_front_app: bool,
    front_window_full_screen: bool,
    paused: bool,
    config: &Config,
) -> bool {
    allowed_for_front_app
        && !front_window_full_screen
        && !paused
        && config.window_snapping != Some(false)
}

/// Перехватывать события активно (`ActiveEventMonitor`), чтобы окно, которое
/// быстро тащат вверх, не уезжало в Mission Control: только если
/// `missionControlDragging` выключено явно.
pub fn uses_active_monitor(config: &Config) -> bool {
    config.mission_control_dragging == Some(false)
}

// ---------------------------------------------------------------- возврат размера

/// Рамка для возврата размера при отрыве (`getRestoreRect`): окно стоит там,
/// куда его последним поставил Rectangle (рамка совпадает точно), и у него есть
/// рамка «до». После «больше/меньше» — только если это не выключено.
pub fn unsnap_restore_rect(
    last_action: Option<&LastAction>,
    initial_window_rect: Option<Rect>,
    restore_rect: Option<Rect>,
    config: &Config,
) -> Option<Rect> {
    let last_action = last_action?;
    if Some(last_action.rect) != initial_window_rect {
        return None;
    }
    if last_action.action.category() == Some(WindowActionCategory::Size)
        && config.unsnap_restore_from_size_change == Some(false)
    {
        return None;
    }
    restore_rect
}

/// Рамка окна с возвращённым размером (`unsnapRestore`, macOS 12+), AX:
/// размер — прежний, место — то же, но курсор должен остаться над окном:
/// сначала пробуем прижать правый край к прежнему, потом ставим окно
/// серединой под курсор.
pub fn unsnap_frame(current: Rect, restore_size: (f64, f64), cursor: Option<(f64, f64)>) -> Rect {
    let mut frame = Rect::new(current.x, current.y, restore_size.0, restore_size.1);
    if let Some((x, y)) = cursor {
        if !contains_point(&frame, x, y) {
            frame.x = current.max_x() - frame.w;
            if !contains_point(&frame, x, y) {
                frame.x = x - frame.w / 2.0;
            }
        }
    }
    frame
}

/// `CGRect.contains(CGPoint)`: левая и нижняя границы включены, правая и верхняя — нет.
fn contains_point(rect: &Rect, x: f64, y: f64) -> bool {
    x >= rect.min_x() && x < rect.max_x() && y >= rect.min_y() && y < rect.max_y()
}

// ---------------------------------------------------------------- Mission Control

/// Защита от Mission Control при активном перехвате (`SnappingManager.filter`):
/// если окно быстро тащат за верхний край главного экрана, курсор в событии
/// опускается на пиксель, и ещё `missionControlDraggingDisallowedDuration` мс
/// верхний край держится так же.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MissionControlGuard {
    drag_prev_y: Option<f64>,
    restriction_expires_ms: u64,
}

impl MissionControlGuard {
    /// Разобрать событие. `top_edge` — верх главного экрана в координатах
    /// Quartz (`screen.frame.screenFlipped.minY`), `None` — экрана нет.
    /// Возвращает новую координату y, если событие надо опустить.
    #[allow(clippy::too_many_arguments)]
    pub fn filter(
        &mut self,
        kind: MouseEventKind,
        location_y: f64,
        delta_y: f64,
        top_edge: Option<f64>,
        allowed_offscreen_distance: f64,
        disallowed_duration_ms: i64,
        now_ms: u64,
    ) -> Option<f64> {
        match kind {
            MouseEventKind::LeftMouseUp => {
                self.drag_prev_y = None;
                None
            }
            MouseEventKind::LeftMouseDragged => {
                let min_y = top_edge?;
                let mut moved = None;
                if location_y == min_y && self.drag_prev_y == Some(min_y) {
                    if delta_y < -allowed_offscreen_distance {
                        moved = Some(min_y + 1.0);
                        self.restriction_expires_ms =
                            now_ms.saturating_add(disallowed_duration_ms.max(0) as u64);
                    } else if now_ms <= self.restriction_expires_ms {
                        moved = Some(min_y + 1.0);
                    }
                }
                self.drag_prev_y = Some(moved.unwrap_or(location_y));
                moved
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{CompoundSnapArea, SnapAreaConfig};
    use std::collections::BTreeSet;

    fn screen(id: u32, frame: Rect) -> Screen {
        Screen {
            id,
            frame,
            visible_frame: Rect::new(frame.x, frame.y, frame.w, frame.h - 25.0),
            name: String::new(),
            is_main: id == 1,
            scale: 2.0,
            safe_area_top: 0.0,
        }
    }

    fn settings() -> ZoneSettings {
        ZoneSettings::from_config(&Config::default())
    }

    #[test]
    fn corners_and_edges_of_one_screen() {
        let frame = Rect::new(0.0, 0.0, 1512.0, 982.0);
        let at = |x, y| directional_location((x, y), &frame, &settings());
        use Directional::*;
        // Углы: отступ 5 + угол 20 = 25.
        assert_eq!(at(0.0, 982.0), Some(Tl));
        assert_eq!(at(24.9, 957.0), Some(Tl));
        assert_eq!(at(0.0, 0.0), Some(Bl));
        assert_eq!(at(24.0, 25.0), Some(Bl));
        assert_eq!(at(1512.0, 982.0), Some(Tr));
        assert_eq!(at(1487.1, 0.0), Some(Br));
        // Край: полоса в отступ (строго меньше 5 от края).
        assert_eq!(at(4.9, 500.0), Some(L));
        assert_eq!(at(5.0, 500.0), None);
        assert_eq!(at(1507.1, 500.0), Some(R));
        assert_eq!(at(700.0, 977.1), Some(T));
        assert_eq!(at(700.0, 977.0), None);
        assert_eq!(at(700.0, 4.9), Some(B));
        // В полосе угла, но не у края — ничего.
        assert_eq!(at(20.0, 500.0), None);
        // Вне экрана.
        assert_eq!(at(-1.0, 500.0), None);
        assert_eq!(at(700.0, 982.5), None);
    }

    #[test]
    fn margins_and_corner_size_come_from_settings() {
        let frame = Rect::new(0.0, 0.0, 1000.0, 800.0);
        let config = Config {
            snap_edge_margin_left: 30.0,
            snap_edge_margin_top: 0.0,
            corner_snap_area_size: 100.0,
            ..Config::default()
        };
        // Сохранённый 0 у отступа читается как значение по умолчанию только
        // при загрузке; здесь 0 — честный ноль: верхней полосы нет.
        let zones = ZoneSettings::from_config(&config);
        use Directional::*;
        assert_eq!(directional_location((29.0, 400.0), &frame, &zones), Some(L));
        assert_eq!(
            directional_location((129.0, 800.0), &frame, &zones),
            Some(Tl)
        );
        assert_eq!(directional_location((129.0, 699.0), &frame, &zones), None);
        assert_eq!(directional_location((500.0, 800.0), &frame, &zones), None);
    }

    #[test]
    fn default_landscape_areas() {
        let screens = [screen(1, Rect::new(0.0, 0.0, 1512.0, 982.0))];
        let at = |x, y, prior: Option<&SnapArea>| {
            snap_area_containing((x, y), &screens, &settings(), || None, prior)
                .map(|area| area.action)
        };
        use Action::*;
        assert_eq!(at(0.0, 982.0, None), Some(TopLeft));
        assert_eq!(at(700.0, 982.0, None), Some(Maximize));
        assert_eq!(at(1512.0, 982.0, None), Some(TopRight));
        assert_eq!(at(0.0, 500.0, None), Some(LeftHalf));
        assert_eq!(at(0.0, 900.0, None), Some(TopHalf));
        assert_eq!(at(0.0, 100.0, None), Some(BottomHalf));
        assert_eq!(at(1512.0, 500.0, None), Some(RightHalf));
        assert_eq!(at(0.0, 0.0, None), Some(BottomLeft));
        assert_eq!(at(1512.0, 0.0, None), Some(BottomRight));
        assert_eq!(at(100.0, 0.0, None), Some(FirstThird));
        assert_eq!(at(756.0, 0.0, None), Some(CenterThird));
        assert_eq!(at(1400.0, 0.0, None), Some(LastThird));
        assert_eq!(at(756.0, 500.0, None), None);
    }

    #[test]
    fn default_portrait_areas() {
        let screens = [screen(2, Rect::new(0.0, 0.0, 1080.0, 1920.0))];
        let at = |x, y| {
            snap_area_containing((x, y), &screens, &settings(), || None, None)
                .map(|area| area.action)
        };
        use Action::*;
        assert_eq!(at(540.0, 1920.0), Some(Maximize));
        assert_eq!(at(0.0, 1500.0), Some(FirstThird));
        assert_eq!(at(1080.0, 300.0), Some(LastThird));
        assert_eq!(at(300.0, 0.0), Some(LeftHalf));
        assert_eq!(at(800.0, 0.0), Some(RightHalf));
    }

    #[test]
    fn two_screens_and_disabled_areas() {
        // Второй экран справа, выше основного.
        let left = screen(1, Rect::new(0.0, 0.0, 1512.0, 982.0));
        let right = screen(2, Rect::new(1512.0, 0.0, 1920.0, 1080.0));
        let screens = [left, right];
        let mut config = Config::default();
        let found = |config: &Config, x, y| {
            snap_area_containing(
                (x, y),
                &screens,
                &ZoneSettings::from_config(config),
                || None,
                None,
            )
            .map(|area| (area.screen.id, area.directional, area.action))
        };
        // Общая граница x = 1512 входит в оба экрана: берётся первый.
        assert_eq!(
            found(&config, 1512.0, 500.0),
            Some((1, Directional::R, Action::RightHalf))
        );
        assert_eq!(
            found(&config, 3432.0, 1080.0),
            Some((2, Directional::Tr, Action::TopRight))
        );
        // Правый край первого экрана выключен — ищем дальше: левый край второго.
        let mut landscape = config.landscape_snap_areas_or_default();
        landscape.remove(&Directional::R);
        config.landscape_snap_areas = Some(landscape);
        assert_eq!(
            found(&config, 1512.0, 500.0),
            Some((2, Directional::L, Action::LeftHalf))
        );
        // Действие важнее составной области; неизвестное действие — составная.
        let mut landscape = config.landscape_snap_areas_or_default();
        landscape.insert(
            Directional::B,
            SnapAreaConfig {
                compound: Some(CompoundSnapArea::Thirds),
                action: Some(Action::BottomHalf.raw() as i64),
            },
        );
        config.landscape_snap_areas = Some(landscape.clone());
        assert_eq!(
            found(&config, 100.0, 0.0),
            Some((1, Directional::B, Action::BottomHalf))
        );
        landscape.insert(
            Directional::B,
            SnapAreaConfig {
                compound: Some(CompoundSnapArea::Thirds),
                action: Some(6),
            },
        );
        config.landscape_snap_areas = Some(landscape);
        assert_eq!(
            found(&config, 100.0, 0.0),
            Some((1, Directional::B, Action::FirstThird))
        );
    }

    #[test]
    fn compound_areas_see_the_prior_area() {
        let screens = [screen(1, Rect::new(0.0, 0.0, 1512.0, 982.0))];
        let settings = settings();
        let first = snap_area_containing((100.0, 0.0), &screens, &settings, || None, None).unwrap();
        assert_eq!(first.action, Action::FirstThird);
        let toward_center =
            snap_area_containing((700.0, 0.0), &screens, &settings, || None, Some(&first)).unwrap();
        assert_eq!(toward_center.action, Action::FirstTwoThirds);
        // Верхние шестые: из угла вдоль края.
        let mut config = Config::default();
        let mut landscape = config.landscape_snap_areas_or_default();
        landscape.insert(
            Directional::T,
            SnapAreaConfig::compound(CompoundSnapArea::TopSixths),
        );
        config.landscape_snap_areas = Some(landscape);
        let settings = ZoneSettings::from_config(&config);
        let corner =
            snap_area_containing((0.0, 982.0), &screens, &settings, || None, None).unwrap();
        assert_eq!(corner.action, Action::TopLeft);
        let sixth =
            snap_area_containing((200.0, 982.0), &screens, &settings, || None, Some(&corner))
                .unwrap();
        assert_eq!(sixth.action, Action::TopLeftSixth);
        assert_eq!(
            snap_area_containing((200.0, 982.0), &screens, &settings, || None, None)
                .unwrap()
                .action,
            Action::Maximize
        );
    }

    #[test]
    fn todo_window_gets_todo_side() {
        let screens = [screen(1, Rect::new(0.0, 0.0, 1512.0, 982.0))];
        let at = |side, x| {
            snap_area_containing((x, 500.0), &screens, &settings(), || side, None)
                .map(|area| area.action)
        };
        assert_eq!(at(Some(TodoSidebarSide::Left), 0.0), Some(Action::LeftTodo));
        assert_eq!(
            at(Some(TodoSidebarSide::Left), 1512.0),
            Some(Action::RightHalf)
        );
        assert_eq!(
            at(Some(TodoSidebarSide::Right), 1512.0),
            Some(Action::RightTodo)
        );
        assert_eq!(at(None, 0.0), Some(Action::LeftHalf));
    }

    #[test]
    fn todo_side_is_asked_only_at_an_edge_and_once() {
        let screens = [
            screen(1, Rect::new(0.0, 0.0, 1512.0, 982.0)),
            screen(2, Rect::new(1512.0, 0.0, 1920.0, 1080.0)),
        ];
        let asked = std::cell::Cell::new(0);
        let ask = || {
            asked.set(asked.get() + 1);
            None
        };
        assert_eq!(
            snap_area_containing((700.0, 500.0), &screens, &settings(), ask, None),
            None
        );
        assert_eq!(asked.get(), 0, "курсор не у края — не спрашиваем");
        // Край у обоих экранов (у первого зона выключена): спрашиваем один раз.
        let mut config = Config::default();
        let mut landscape = config.landscape_snap_areas_or_default();
        landscape.remove(&Directional::R);
        config.landscape_snap_areas = Some(landscape);
        let settings = ZoneSettings::from_config(&config);
        let found = snap_area_containing((1512.0, 500.0), &screens, &settings, ask, None);
        assert_eq!(
            found.map(|area| (area.screen.id, area.action)),
            Some((2, Action::LeftHalf))
        );
        assert_eq!(asked.get(), 1);
    }

    #[test]
    fn snap_areas_compare_by_display() {
        let frame = Rect::new(0.0, 0.0, 100.0, 100.0);
        let a = SnapArea {
            screen: screen(1, frame),
            directional: Directional::L,
            action: Action::LeftHalf,
        };
        let mut moved = a.clone();
        moved.screen.frame = Rect::new(0.0, 0.0, 200.0, 100.0);
        moved.screen.is_main = false;
        assert_eq!(a, moved);
        let mut other = a.clone();
        other.screen.id = 2;
        assert_ne!(a, other);
        let mut other_action = a.clone();
        other_action.action = Action::TopHalf;
        assert_ne!(a, other_action);
    }

    /// Высота основного экрана для переворота координат в тестах.
    const PRIMARY: f64 = 1117.0;

    #[test]
    fn footprint_rect_uses_visible_frame_and_gaps() {
        let visible = Rect::new(0.0, 0.0, 1512.0, 950.0);
        let window = Rect::new(300.0, 200.0, 800.0, 600.0);
        let config = Config::default();
        assert_eq!(
            footprint_rect(Action::LeftHalf, window, visible, &config, PRIMARY),
            Some(Rect::new(0.0, 0.0, 756.0, 950.0))
        );
        assert_eq!(
            footprint_rect(Action::Maximize, window, visible, &config, PRIMARY),
            Some(visible)
        );
        let gaps = Config {
            gap_size: 10.0,
            ..Config::default()
        };
        // Левая половина с зазором 10: по 10 снаружи, 5 у общего края.
        assert_eq!(
            footprint_rect(Action::LeftHalf, window, visible, &gaps, PRIMARY),
            Some(Rect::new(10.0, 10.0, 741.0, 930.0))
        );
        // У Todo-действий в оригинале есть `calculateRect` — подсветка тоже есть.
        assert!(footprint_rect(Action::LeftTodo, window, visible, &config, PRIMARY).is_some());
    }

    #[test]
    fn footprint_grows_from_its_edge() {
        let rect = Rect::new(0.0, 0.0, 100.0, 50.0);
        use Directional::*;
        assert_eq!(footprint_animation_origin(Tl, &rect), Some((0.0, 50.0)));
        assert_eq!(footprint_animation_origin(T, &rect), Some((50.0, 50.0)));
        assert_eq!(footprint_animation_origin(R, &rect), Some((100.0, 25.0)));
        assert_eq!(footprint_animation_origin(B, &rect), Some((50.0, 0.0)));
        assert_eq!(footprint_animation_origin(Br, &rect), Some((100.0, 0.0)));
        assert_eq!(footprint_animation_origin(C, &rect), None);
    }

    #[test]
    fn modifiers_must_match_exactly() {
        const OPTION: i64 = 1 << 19;
        const CONTROL: i64 = 1 << 18;
        assert!(modifiers_allow_snap(0, 0));
        assert!(modifiers_allow_snap(OPTION as u64, 0));
        assert!(modifiers_allow_snap(OPTION as u64, OPTION));
        assert!(!modifiers_allow_snap(0, OPTION));
        assert!(!modifiers_allow_snap((OPTION | CONTROL) as u64, OPTION));
        assert!(modifiers_allow_snap(
            (OPTION | CONTROL) as u64,
            OPTION | CONTROL
        ));
    }

    #[test]
    fn ignored_apps_turn_drag_snap_off() {
        let ignored = |ids: &[&str]| Config {
            disabled_apps: Some(ids.iter().map(|id| id.to_string()).collect::<BTreeSet<_>>()),
            ..Config::default()
        };
        let config = ignored(&["com.apple.Safari", "com.mathworks.matlab"]);
        assert!(drag_snap_allowed_for(Some("com.apple.Terminal"), &config));
        assert!(drag_snap_allowed_for(None, &config));
        // По умолчанию игнор выключает и прилипание.
        assert!(!drag_snap_allowed_for(Some("com.apple.Safari"), &config));
        // Явно «не выключать»: прилипание остаётся, кроме проблемных приложений.
        let keep = Config {
            ignore_drag_snap_too: Some(false),
            ..config.clone()
        };
        assert!(drag_snap_allowed_for(Some("com.apple.Safari"), &keep));
        assert!(!drag_snap_allowed_for(Some("com.mathworks.matlab"), &keep));
        let java = Config {
            ignore_drag_snap_too: Some(false),
            ..ignored(&["com.install4j.1234-5678"])
        };
        assert!(!drag_snap_allowed_for(
            Some("com.install4j.1234-5678"),
            &java
        ));
        // Свой список вместо встроенного.
        let custom = Config {
            full_ignore_bundle_ids: Some(vec!["com.apple.Saf".to_string()]),
            ..keep
        };
        assert!(!drag_snap_allowed_for(Some("com.apple.Safari"), &custom));
        assert!(drag_snap_allowed_for(Some("com.mathworks.matlab"), &custom));
        // Проблемное приложение, которое не игнорируют, — прилипание есть.
        assert!(drag_snap_allowed_for(
            Some("com.adobe.illustrator"),
            &Config::default()
        ));
    }

    #[test]
    fn listening_needs_all_four_conditions() {
        let config = Config::default();
        assert!(should_listen(true, false, false, &config));
        assert!(!should_listen(false, false, false, &config));
        assert!(!should_listen(true, true, false, &config));
        // Пауза на время диалога снимает слежение, настройку не трогая.
        assert!(!should_listen(true, false, true, &config));
        let off = Config {
            window_snapping: Some(false),
            ..Config::default()
        };
        assert!(!should_listen(true, false, false, &off));
        let on = Config {
            window_snapping: Some(true),
            ..Config::default()
        };
        assert!(should_listen(true, false, false, &on));
        assert!(!should_listen(true, false, true, &on));

        assert!(!uses_active_monitor(&config));
        assert!(!uses_active_monitor(&Config {
            mission_control_dragging: Some(true),
            ..Config::default()
        }));
        assert!(uses_active_monitor(&Config {
            mission_control_dragging: Some(false),
            ..Config::default()
        }));
    }

    fn last(action: Action, rect: Rect) -> LastAction {
        LastAction {
            action,
            sub_action: None,
            rect,
            count: 1,
        }
    }

    #[test]
    fn restore_rect_only_for_windows_left_where_rectangle_put_them() {
        let snapped = Rect::new(0.0, 25.0, 756.0, 957.0);
        let before = Rect::new(200.0, 200.0, 800.0, 600.0);
        let config = Config::default();
        let left_half = last(Action::LeftHalf, snapped);
        assert_eq!(
            unsnap_restore_rect(Some(&left_half), Some(snapped), Some(before), &config),
            Some(before)
        );
        // Окно двигали после Rectangle — рамка не совпадает.
        assert_eq!(
            unsnap_restore_rect(
                Some(&left_half),
                Some(Rect::new(1.0, 25.0, 756.0, 957.0)),
                Some(before),
                &config
            ),
            None
        );
        assert_eq!(
            unsnap_restore_rect(None, Some(snapped), Some(before), &config),
            None
        );
        assert_eq!(
            unsnap_restore_rect(Some(&left_half), Some(snapped), None, &config),
            None
        );
        // После «почти развернуть» (размер) — пока не выключено явно.
        let almost = last(Action::AlmostMaximize, snapped);
        assert_eq!(
            unsnap_restore_rect(Some(&almost), Some(snapped), Some(before), &config),
            Some(before)
        );
        let no_size = Config {
            unsnap_restore_from_size_change: Some(false),
            ..Config::default()
        };
        assert_eq!(
            unsnap_restore_rect(Some(&almost), Some(snapped), Some(before), &no_size),
            None
        );
        assert_eq!(
            unsnap_restore_rect(Some(&left_half), Some(snapped), Some(before), &no_size),
            Some(before)
        );
    }

    #[test]
    fn unsnapped_window_stays_under_cursor() {
        // Окно левой половины (AX), тащат за заголовок у левого края.
        let current = Rect::new(40.0, 30.0, 756.0, 957.0);
        let size = (800.0, 600.0);
        assert_eq!(
            unsnap_frame(current, size, Some((100.0, 40.0))),
            Rect::new(40.0, 30.0, 800.0, 600.0)
        );
        // Тащат за правую часть заголовка: тот же правый край.
        let narrow = (300.0, 200.0);
        assert_eq!(
            unsnap_frame(current, narrow, Some((700.0, 40.0))),
            Rect::new(496.0, 30.0, 300.0, 200.0)
        );
        // Не выходит и так — окно серединой под курсор.
        assert_eq!(
            unsnap_frame(current, narrow, Some((400.0, 40.0))),
            Rect::new(250.0, 30.0, 300.0, 200.0)
        );
        // Без курсора — только размер.
        assert_eq!(
            unsnap_frame(current, narrow, None),
            Rect::new(40.0, 30.0, 300.0, 200.0)
        );
    }

    #[test]
    fn mission_control_guard_pushes_fast_upward_drags_down() {
        let mut guard = MissionControlGuard::default();
        let drag = MouseEventKind::LeftMouseDragged;
        let top = Some(0.0);
        // Первое событие у края: предыдущего нет — не трогаем.
        assert_eq!(guard.filter(drag, 0.0, -40.0, top, 25.0, 250, 1000), None);
        // Второе у края, рывок вверх больше 25 — опускаем и запрещаем на 250 мс.
        assert_eq!(
            guard.filter(drag, 0.0, -40.0, top, 25.0, 250, 1010),
            Some(1.0)
        );
        // Предыдущее стало 1 — следующее у края пропускаем.
        assert_eq!(guard.filter(drag, 0.0, -1.0, top, 25.0, 250, 1020), None);
        // Снова у края, медленно, но запрет ещё действует — опускаем.
        assert_eq!(
            guard.filter(drag, 0.0, -1.0, top, 25.0, 250, 1030),
            Some(1.0)
        );
        guard.filter(drag, 0.0, -1.0, top, 25.0, 250, 1040);
        // Запрет истёк (1010 + 250 < 1300) — медленное движение не трогаем.
        assert_eq!(guard.filter(drag, 0.0, -1.0, top, 25.0, 250, 1300), None);
        // Отпускание сбрасывает предыдущее.
        assert_eq!(
            guard.filter(MouseEventKind::LeftMouseUp, 0.0, 0.0, top, 25.0, 250, 1400),
            None
        );
        assert_eq!(guard.filter(drag, 0.0, -40.0, top, 25.0, 250, 1410), None);
        // Не у края и без экрана — ничего.
        assert_eq!(guard.filter(drag, 300.0, -40.0, top, 25.0, 250, 1420), None);
        assert_eq!(guard.filter(drag, 0.0, -40.0, None, 25.0, 250, 1430), None);
    }
}
