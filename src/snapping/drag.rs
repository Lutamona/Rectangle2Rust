//! Перетаскивание окна — `SnappingManager.handle(event:)` оригинала.
//!
//! Нажатие: запомнить окно под курсором, его номер и рамку. Перетаскивание:
//! окно сдвинулось с места (а не меняет размер) — значит, его тащат; вернуть
//! ему размер, если его отрывают от места, куда его поставил Rectangle;
//! найти область под курсором и показать подсветку. Отпускание: выполнить
//! действие области (`postSnap`) — или, если события перетаскивания не успели
//! за окном, проверить область по итоговому положению.
//!
//! Всё, что касается системы, — через `SnapSystem`: настоящая реализация в
//! `snapping/mod.rs`, в тестах — подставная.

use std::cell::Cell;

use crate::actions::Action;
use crate::calc::LastAction;
use crate::config::{Config, TodoSidebarSide};
use crate::event_monitor::{MouseEvent, MouseEventKind};
use crate::geometry::Rect;
use crate::screens::Screen;

use super::zones::{self, SnapArea, ZoneSettings};

/// Сколько раз при перетаскивании пытаться узнать номер окна и не чаще
/// какого интервала (секунды).
const WINDOW_ID_ATTEMPTS: u32 = 20;
const WINDOW_ID_ATTEMPT_INTERVAL: f64 = 0.1;

/// Всё, что автомату перетаскивания нужно от системы.
pub(crate) trait SnapSystem {
    type Window: Clone;

    /// `AccessibilityElement.getWindowElementUnderCursor()`.
    fn window_under_cursor(&mut self) -> Option<Self::Window>;
    /// `getWindowId()`.
    fn window_id(&mut self, window: &Self::Window) -> Option<u32>;
    /// Рамка окна, AX.
    fn frame(&mut self, window: &Self::Window) -> Option<Rect>;
    /// `setFrame(_, adjustSizeFirst: false)`, AX.
    fn set_frame(&mut self, window: &Self::Window, frame: Rect);
    /// `NSEvent.mouseLocation`, Cocoa.
    fn cursor(&mut self) -> Option<(f64, f64)>;
    /// `NSScreen.screens`.
    fn screens(&mut self) -> Vec<Screen>;
    /// Окно — в полосе Stage Manager (`StageUtil.getStageStripWindowGroup`).
    fn in_stage_strip(&mut self, window_id: u32) -> bool;
    /// `TodoManager.isTodoWindow`.
    fn is_todo_window(&mut self, window_id: u32) -> bool;
    /// `getBoxRect`: рамка подсветки области для окна с рамкой `window_frame` (AX).
    fn footprint_rect(
        &mut self,
        area: &SnapArea,
        window_frame: Rect,
        window_id: Option<u32>,
        config: &Config,
    ) -> Option<Rect>;
    /// Показать подсветку в `rect` (Cocoa).
    fn show_footprint(&mut self, area: &SnapArea, rect: Rect, config: &Config);
    /// `box?.orderOut(nil)`.
    fn hide_footprint(&mut self);
    /// `NSHapticFeedbackManager…perform(.alignment)`.
    fn haptic_feedback(&mut self);
    /// `action.postSnap(windowElement:windowId:screen:)`.
    fn execute(
        &mut self,
        action: Action,
        window: Option<Self::Window>,
        window_id: Option<u32>,
        screen: Screen,
    );
    /// `windowHistory.lastRectangleActions[windowId]`.
    fn last_action(&mut self, window_id: u32) -> Option<LastAction>;
    fn remove_last_action(&mut self, window_id: u32);
    /// `windowHistory.restoreRects[windowId]`.
    fn restore_rect(&mut self, window_id: u32) -> Option<Rect>;
    /// Запомнить (или забыть при `None`) рамку для «Восстановить».
    fn set_restore_rect(&mut self, window_id: u32, rect: Option<Rect>);
}

/// Состояние одного перетаскивания (поля `SnappingManager`).
pub(crate) struct DragState<W> {
    window: Option<W>,
    window_id: Option<u32>,
    window_id_attempt: u32,
    last_window_id_attempt: Option<f64>,
    window_moving: bool,
    initial_window_rect: Option<Rect>,
    current_snap_area: Option<SnapArea>,
    /// Тащат ли окно Todo: (номер окна, ответ). Ответ стоит AX-запроса к
    /// приложению Todo, поэтому спрашивается один раз за перетаскивание — когда
    /// курсор впервые у края экрана.
    todo_window: Cell<Option<(u32, bool)>>,
}

impl<W> Default for DragState<W> {
    /// Ничего не тащат.
    fn default() -> Self {
        DragState {
            window: None,
            window_id: None,
            window_id_attempt: 0,
            last_window_id_attempt: None,
            window_moving: false,
            initial_window_rect: None,
            current_snap_area: None,
            todo_window: Cell::new(None),
        }
    }
}

impl<W: Clone> DragState<W> {
    /// Забыть перетаскивание (слежение выключили или сменили посреди него).
    /// `true` — окно было над областью: её подсветку надо погасить.
    pub(crate) fn reset(&mut self) -> bool {
        std::mem::take(self).current_snap_area.is_some()
    }

    /// `handle(event:)`.
    pub(crate) fn handle<S>(&mut self, system: &mut S, event: &MouseEvent, config: &Config)
    where
        S: SnapSystem<Window = W>,
    {
        match event.kind {
            MouseEventKind::LeftMouseDown => self.mouse_down(system, config),
            MouseEventKind::LeftMouseUp => self.mouse_up(system, event, config),
            MouseEventKind::LeftMouseDragged => self.mouse_dragged(system, event, config),
            _ => {}
        }
    }

    fn mouse_down<S: SnapSystem<Window = W>>(&mut self, system: &mut S, config: &Config) {
        if config.obtain_window_on_click == Some(false) {
            return;
        }
        self.window = system.window_under_cursor();
        self.window_id = self
            .window
            .as_ref()
            .and_then(|window| system.window_id(window));
        self.initial_window_rect = self.window.as_ref().and_then(|window| system.frame(window));
    }

    fn mouse_up<S: SnapSystem<Window = W>>(
        &mut self,
        system: &mut S,
        event: &MouseEvent,
        config: &Config,
    ) {
        if let Some(area) = self.current_snap_area.take() {
            system.hide_footprint();
            system.execute(
                area.action,
                self.window.clone(),
                self.window_id,
                area.screen,
            );
        } else if let (Some(current), Some(initial)) = (
            self.window.as_ref().and_then(|window| system.frame(window)),
            self.initial_window_rect,
        ) {
            // Окно сдвинули так быстро, что события перетаскивания не застали
            // его в движении: подсветки не было, но прилипание по месту отпускания
            // всё равно срабатывает.
            if same_size(&current, &initial) && !same_origin(&current, &initial) {
                if let Some(window_id) = self.window_id {
                    self.unsnap_restore(system, window_id, current, event.location, config);
                }
                if let Some(area) = self.snap_area_containing_cursor(system, None, config) {
                    system.hide_footprint();
                    if self.can_snap(system, event, config) {
                        system.execute(
                            area.action,
                            self.window.clone(),
                            self.window_id,
                            area.screen,
                        );
                    }
                }
            }
        }
        *self = DragState::default();
    }

    fn mouse_dragged<S: SnapSystem<Window = W>>(
        &mut self,
        system: &mut S,
        event: &MouseEvent,
        config: &Config,
    ) {
        if self.window_id.is_none() && self.window_id_attempt < WINDOW_ID_ATTEMPTS {
            if let Some(last) = self.last_window_id_attempt {
                if event.timestamp - last < WINDOW_ID_ATTEMPT_INTERVAL {
                    return;
                }
            }
            if self.window.is_none() {
                self.window = system.window_under_cursor();
            }
            self.window_id = self
                .window
                .as_ref()
                .and_then(|window| system.window_id(window));
            self.initial_window_rect = self.window.as_ref().and_then(|window| system.frame(window));
            self.window_id_attempt += 1;
            self.last_window_id_attempt = Some(event.timestamp);
        }
        let Some(current) = self.window.as_ref().and_then(|window| system.frame(window)) else {
            return;
        };

        if !self.window_moving {
            match self.initial_window_rect {
                // Сдвинулось без изменения размера (или размер поменялся так,
                // что общих краёв меньше двух — это не растягивание за край).
                Some(initial)
                    if same_size(&current, &initial)
                        || shared_edge_count(&current, &initial) < 2 =>
                {
                    if !same_origin(&current, &initial) {
                        self.window_moving = true;
                        if let Some(window_id) = self.window_id {
                            self.unsnap_restore(system, window_id, current, event.location, config);
                        }
                    }
                }
                // Окно растягивают: оно больше не там, куда его поставил Rectangle.
                _ => {
                    if let Some(window_id) = self.window_id {
                        system.remove_last_action(window_id);
                    }
                }
            }
        }
        if !self.window_moving {
            return;
        }

        if !self.can_snap(system, event, config) {
            if self.current_snap_area.take().is_some() {
                system.hide_footprint();
            }
            return;
        }

        let found =
            self.snap_area_containing_cursor(system, self.current_snap_area.as_ref(), config);
        let Some(area) = found else {
            if self.current_snap_area.take().is_some() {
                system.hide_footprint();
            }
            return;
        };
        if self.current_snap_area.as_ref() == Some(&area) {
            return;
        }
        if config.haptic_feedback_on_snap == Some(true) {
            system.haptic_feedback();
        }
        if let Some(rect) = system.footprint_rect(&area, current, self.window_id, config) {
            system.show_footprint(&area, rect, config);
        }
        self.current_snap_area = Some(area);
    }

    /// `canSnap`: модификаторы и полоса Stage Manager.
    fn can_snap<S: SnapSystem<Window = W>>(
        &self,
        system: &mut S,
        event: &MouseEvent,
        config: &Config,
    ) -> bool {
        if !zones::modifiers_allow_snap(event.device_independent_flags(), config.snap_modifiers) {
            return false;
        }
        if let Some(window_id) = self.window_id {
            if system.in_stage_strip(window_id) {
                return false;
            }
        }
        true
    }

    /// `snapAreaContainingCursor`.
    fn snap_area_containing_cursor<S: SnapSystem<Window = W>>(
        &self,
        system: &mut S,
        prior: Option<&SnapArea>,
        config: &Config,
    ) -> Option<SnapArea> {
        let loc = system.cursor()?;
        let screens = system.screens();
        zones::snap_area_containing(
            loc,
            &screens,
            &ZoneSettings::from_config(config),
            || self.todo_side(system, config),
            prior,
        )
    }

    /// Сторона панели Todo, если тащат окно Todo в режиме Todo.
    fn todo_side<S: SnapSystem<Window = W>>(
        &self,
        system: &mut S,
        config: &Config,
    ) -> Option<TodoSidebarSide> {
        let window_id = self.window_id?;
        if !(config.todo == Some(true) && config.todo_mode) {
            return None;
        }
        let is_todo = match self.todo_window.get() {
            Some((known_id, is_todo)) if known_id == window_id => is_todo,
            _ => {
                let is_todo = system.is_todo_window(window_id);
                self.todo_window.set(Some((window_id, is_todo)));
                is_todo
            }
        };
        is_todo.then_some(config.todo_sidebar_side)
    }

    /// `unsnapRestore`: окно отрывают от места, куда его поставил Rectangle, —
    /// вернуть ему прежний размер; иначе запомнить рамку до перетаскивания
    /// для «Восстановить».
    fn unsnap_restore<S: SnapSystem<Window = W>>(
        &self,
        system: &mut S,
        window_id: u32,
        current: Rect,
        cursor: Option<(f64, f64)>,
        config: &Config,
    ) {
        if config.unsnap_restore == Some(false) {
            return;
        }
        let last_action = system.last_action(window_id);
        let restore = system.restore_rect(window_id);
        match zones::unsnap_restore_rect(
            last_action.as_ref(),
            self.initial_window_rect,
            restore,
            config,
        ) {
            Some(restore) => {
                if let Some(window) = &self.window {
                    let frame = zones::unsnap_frame(current, (restore.w, restore.h), cursor);
                    system.set_frame(window, frame);
                }
                system.remove_last_action(window_id);
            }
            None => system.set_restore_rect(window_id, self.initial_window_rect),
        }
    }
}

fn same_size(a: &Rect, b: &Rect) -> bool {
    a.w == b.w && a.h == b.h
}

fn same_origin(a: &Rect, b: &Rect) -> bool {
    a.x == b.x && a.y == b.y
}

/// `numSharedEdges(withRect:)`: сколько сторон совпадает точно.
fn shared_edge_count(a: &Rect, b: &Rect) -> usize {
    [
        a.min_x() == b.min_x(),
        a.max_x() == b.max_x(),
        a.min_y() == b.min_y(),
        a.max_y() == b.max_y(),
    ]
    .into_iter()
    .filter(|&shared| shared)
    .count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calc::LastAction;
    use crate::config::{CompoundSnapArea, Directional, SnapAreaConfig};
    use crate::window_history::WindowHistory;
    use std::collections::{HashMap, HashSet};

    const PRIMARY_HEIGHT: f64 = 982.0;
    const OPTION: u64 = 1 << 19;

    #[derive(Clone, Debug, PartialEq)]
    enum Call {
        Show(Directional, Action, Rect),
        Hide,
        Haptic,
        Execute(Action, Option<u32>, Option<u32>, u32),
        SetFrame(u32, Rect),
    }

    /// Подставная система: одно или несколько окон с рамками (AX), курсор,
    /// экраны; записывает, что с ней делали.
    struct Fake {
        under_cursor: Option<u32>,
        frames: HashMap<u32, Rect>,
        ids: HashMap<u32, Option<u32>>,
        cursor: (f64, f64),
        screens: Vec<Screen>,
        stage_strip: HashSet<u32>,
        todo: HashSet<u32>,
        history: WindowHistory,
        calls: Vec<Call>,
        lookups: usize,
        /// Сколько раз спросили, не окно ли это Todo (AX-запрос к приложению Todo).
        todo_checks: usize,
    }

    impl Fake {
        fn new() -> Fake {
            let frame = Rect::new(0.0, 0.0, 1512.0, PRIMARY_HEIGHT);
            Fake {
                under_cursor: Some(7),
                frames: HashMap::from([(7, Rect::new(300.0, 200.0, 800.0, 600.0))]),
                ids: HashMap::new(),
                cursor: (700.0, 500.0),
                screens: vec![Screen {
                    id: 1,
                    frame,
                    visible_frame: Rect::new(0.0, 0.0, 1512.0, 950.0),
                    name: String::new(),
                    is_main: true,
                    scale: 2.0,
                    safe_area_top: 0.0,
                }],
                stage_strip: HashSet::new(),
                todo: HashSet::new(),
                history: WindowHistory::default(),
                calls: Vec::new(),
                lookups: 0,
                todo_checks: 0,
            }
        }

        /// Курсор тащит окно: окно едет вместе с курсором (заголовок под ним).
        fn drag_to(&mut self, x: f64, y: f64) {
            let (dx, dy) = (x - self.cursor.0, y - self.cursor.1);
            self.cursor = (x, y);
            if let Some(frame) = self.frames.get_mut(&7) {
                frame.x += dx;
                frame.y -= dy;
            }
        }

        fn take_calls(&mut self) -> Vec<Call> {
            std::mem::take(&mut self.calls)
        }
    }

    impl SnapSystem for Fake {
        type Window = u32;

        fn window_under_cursor(&mut self) -> Option<u32> {
            self.lookups += 1;
            self.under_cursor
        }
        fn window_id(&mut self, window: &u32) -> Option<u32> {
            self.ids.get(window).copied().unwrap_or(Some(*window))
        }
        fn frame(&mut self, window: &u32) -> Option<Rect> {
            self.frames.get(window).copied()
        }
        fn set_frame(&mut self, window: &u32, frame: Rect) {
            self.frames.insert(*window, frame);
            self.calls.push(Call::SetFrame(*window, frame));
        }
        fn cursor(&mut self) -> Option<(f64, f64)> {
            Some(self.cursor)
        }
        fn screens(&mut self) -> Vec<Screen> {
            self.screens.clone()
        }
        fn in_stage_strip(&mut self, window_id: u32) -> bool {
            self.stage_strip.contains(&window_id)
        }
        fn is_todo_window(&mut self, window_id: u32) -> bool {
            self.todo_checks += 1;
            self.todo.contains(&window_id)
        }
        fn footprint_rect(
            &mut self,
            area: &SnapArea,
            window_frame: Rect,
            _window_id: Option<u32>,
            config: &Config,
        ) -> Option<Rect> {
            zones::footprint_rect(
                area.action,
                window_frame.screen_flipped(PRIMARY_HEIGHT),
                area.screen.visible_frame,
                config,
                PRIMARY_HEIGHT,
            )
        }
        fn show_footprint(&mut self, area: &SnapArea, rect: Rect, _config: &Config) {
            self.calls
                .push(Call::Show(area.directional, area.action, rect));
        }
        fn hide_footprint(&mut self) {
            self.calls.push(Call::Hide);
        }
        fn haptic_feedback(&mut self) {
            self.calls.push(Call::Haptic);
        }
        fn execute(
            &mut self,
            action: Action,
            window: Option<u32>,
            window_id: Option<u32>,
            screen: Screen,
        ) {
            self.calls
                .push(Call::Execute(action, window, window_id, screen.id));
        }
        fn last_action(&mut self, window_id: u32) -> Option<LastAction> {
            self.history.last_actions.get(&window_id).copied()
        }
        fn remove_last_action(&mut self, window_id: u32) {
            self.history.last_actions.remove(&window_id);
        }
        fn restore_rect(&mut self, window_id: u32) -> Option<Rect> {
            self.history.restore_rects.get(&window_id).copied()
        }
        fn set_restore_rect(&mut self, window_id: u32, rect: Option<Rect>) {
            match rect {
                Some(rect) => self.history.restore_rects.insert(window_id, rect),
                None => self.history.restore_rects.remove(&window_id),
            };
        }
    }

    fn event(kind: MouseEventKind, timestamp: f64, flags: u64, fake: &Fake) -> MouseEvent {
        MouseEvent {
            kind,
            timestamp,
            modifier_flags: flags,
            location: Some((fake.cursor.0, PRIMARY_HEIGHT - fake.cursor.1)),
            delta_y: 0.0,
            click_count: 1,
        }
    }

    /// Прогон: нажать, протащить по точкам, отпустить.
    struct Run {
        fake: Fake,
        drag: DragState<u32>,
        config: Config,
        time: f64,
        flags: u64,
    }

    impl Run {
        fn new(config: Config) -> Run {
            Run {
                fake: Fake::new(),
                drag: DragState::default(),
                config,
                time: 100.0,
                flags: 0,
            }
        }

        fn send(&mut self, kind: MouseEventKind) {
            self.time += 0.016;
            let event = event(kind, self.time, self.flags, &self.fake);
            self.drag.handle(&mut self.fake, &event, &self.config);
        }

        fn down(&mut self) {
            self.send(MouseEventKind::LeftMouseDown);
        }

        fn drag(&mut self, x: f64, y: f64) {
            self.fake.drag_to(x, y);
            self.send(MouseEventKind::LeftMouseDragged);
        }

        fn up(&mut self) {
            self.send(MouseEventKind::LeftMouseUp);
        }
    }

    fn left_half_rect() -> Rect {
        Rect::new(0.0, 0.0, 756.0, 950.0)
    }

    #[test]
    fn dragging_to_the_left_edge_snaps_left_half() {
        let mut run = Run::new(Config::default());
        run.down();
        run.drag(500.0, 500.0);
        run.drag(200.0, 500.0);
        assert!(run.fake.take_calls().is_empty(), "до края подсветки нет");
        run.drag(0.0, 500.0);
        assert_eq!(
            run.fake.take_calls(),
            vec![Call::Show(
                Directional::L,
                Action::LeftHalf,
                left_half_rect()
            )]
        );
        // Та же область — ничего не повторяется.
        run.drag(2.0, 480.0);
        assert!(run.fake.take_calls().is_empty());
        run.up();
        assert_eq!(
            run.fake.take_calls(),
            vec![
                Call::Hide,
                Call::Execute(Action::LeftHalf, Some(7), Some(7), 1)
            ]
        );
        // Перетаскивание не отменяет «Восстановить»: рамка до него запомнена.
        assert_eq!(
            run.fake.history.restore_rects.get(&7),
            Some(&Rect::new(300.0, 200.0, 800.0, 600.0))
        );
    }

    #[test]
    fn leaving_the_area_hides_footprint_and_nothing_happens_on_release() {
        let mut run = Run::new(Config::default());
        run.down();
        run.drag(0.0, 500.0);
        run.drag(300.0, 500.0);
        run.up();
        assert_eq!(
            run.fake.take_calls(),
            vec![
                Call::Show(Directional::L, Action::LeftHalf, left_half_rect()),
                Call::Hide,
            ]
        );
    }

    #[test]
    fn corner_then_edge_and_thirds_along_the_bottom() {
        let mut run = Run::new(Config::default());
        run.down();
        run.drag(0.0, 982.0);
        run.drag(700.0, 982.0);
        run.drag(100.0, 0.0);
        run.drag(700.0, 0.0);
        let calls = run.fake.take_calls();
        let shown: Vec<(Directional, Action)> = calls
            .iter()
            .filter_map(|call| match call {
                Call::Show(directional, action, _) => Some((*directional, *action)),
                _ => None,
            })
            .collect();
        assert_eq!(
            shown,
            vec![
                (Directional::Tl, Action::TopLeft),
                (Directional::T, Action::Maximize),
                (Directional::B, Action::FirstThird),
                (Directional::B, Action::FirstTwoThirds),
            ]
        );
        run.up();
        assert_eq!(
            run.fake.take_calls(),
            vec![
                Call::Hide,
                Call::Execute(Action::FirstTwoThirds, Some(7), Some(7), 1)
            ]
        );
    }

    #[test]
    fn sixths_from_the_corner_when_configured() {
        let mut config = Config::default();
        let mut landscape = config.landscape_snap_areas_or_default();
        landscape.insert(
            Directional::T,
            SnapAreaConfig::compound(CompoundSnapArea::TopSixths),
        );
        config.landscape_snap_areas = Some(landscape);
        let mut run = Run::new(config);
        run.down();
        run.drag(1512.0, 982.0);
        run.drag(1300.0, 982.0);
        run.drag(700.0, 982.0);
        run.up();
        let executed: Vec<Call> = run
            .fake
            .take_calls()
            .into_iter()
            .filter(|call| matches!(call, Call::Execute(..)))
            .collect();
        assert_eq!(
            executed,
            vec![Call::Execute(Action::TopCenterSixth, Some(7), Some(7), 1)]
        );
    }

    #[test]
    fn modifiers_gate_the_snap() {
        let config = Config {
            snap_modifiers: OPTION as i64,
            ..Config::default()
        };
        let mut run = Run::new(config);
        run.down();
        run.drag(0.0, 500.0);
        assert!(run.fake.take_calls().is_empty(), "без ⌥ подсветки нет");
        run.flags = OPTION;
        run.drag(1.0, 500.0);
        assert_eq!(
            run.fake.take_calls(),
            vec![Call::Show(
                Directional::L,
                Action::LeftHalf,
                left_half_rect()
            )]
        );
        // Отпустили ⌥ посреди — подсветка гаснет.
        run.flags = 0;
        run.drag(2.0, 500.0);
        assert_eq!(run.fake.take_calls(), vec![Call::Hide]);
        // Отпускание в зоне без ⌥: подсветку гасят (как оригинал), окно не трогают.
        run.up();
        assert_eq!(run.fake.take_calls(), vec![Call::Hide]);
    }

    #[test]
    fn window_in_stage_strip_does_not_snap() {
        let mut run = Run::new(Config::default());
        run.fake.stage_strip.insert(7);
        run.down();
        run.drag(0.0, 500.0);
        assert!(run.fake.take_calls().is_empty());
        run.up();
        assert_eq!(run.fake.take_calls(), vec![Call::Hide]);
    }

    #[test]
    fn resizing_is_not_moving_and_forgets_the_last_action() {
        let mut run = Run::new(Config::default());
        run.fake.history.record_action(
            7,
            Rect::new(300.0, 200.0, 800.0, 600.0),
            Action::Center,
            None,
            true,
        );
        run.down();
        // Тянут правый нижний угол: левый и верхний край на месте.
        run.fake
            .frames
            .insert(7, Rect::new(300.0, 200.0, 900.0, 700.0));
        run.fake.cursor = (0.0, 500.0);
        run.send(MouseEventKind::LeftMouseDragged);
        run.up();
        assert!(
            run.fake.take_calls().is_empty(),
            "растягивание не прилипает"
        );
        assert!(!run.fake.history.last_actions.contains_key(&7));
    }

    #[test]
    fn resizing_by_the_left_edge_to_the_screen_edge_does_not_snap() {
        let mut run = Run::new(Config::default());
        run.down();
        // Левый край окна тянут к левому краю экрана: правый, верхний и нижний
        // края на месте.
        for x in [250.0, 100.0, 0.0] {
            run.fake
                .frames
                .insert(7, Rect::new(x, 200.0, 1100.0 - x, 600.0));
            run.fake.cursor = (x, 500.0);
            run.send(MouseEventKind::LeftMouseDragged);
        }
        run.up();
        assert!(run.fake.take_calls().is_empty());
        // Без событий перетаскивания — тоже: размер другой, это не перенос.
        let mut run = Run::new(Config::default());
        run.down();
        run.fake
            .frames
            .insert(7, Rect::new(0.0, 200.0, 1100.0, 600.0));
        run.fake.cursor = (0.0, 500.0);
        run.up();
        assert!(run.fake.take_calls().is_empty());
    }

    #[test]
    fn unsnap_restores_size_under_the_cursor() {
        let snapped = Rect::new(0.0, 32.0, 756.0, 950.0);
        let before = Rect::new(300.0, 200.0, 800.0, 600.0);
        let mut run = Run::new(Config::default());
        run.fake.frames.insert(7, snapped);
        run.fake.cursor = (400.0, PRIMARY_HEIGHT - 40.0);
        run.fake.history.restore_rects.insert(7, before);
        run.fake
            .history
            .record_action(7, snapped, Action::LeftHalf, None, true);
        run.down();
        run.drag(420.0, PRIMARY_HEIGHT - 45.0);
        let calls = run.fake.take_calls();
        // Окно сдвинулось на (20, 5): тот же левый край, курсор над окном.
        assert_eq!(
            calls,
            vec![Call::SetFrame(7, Rect::new(20.0, 37.0, 800.0, 600.0))]
        );
        assert!(!run.fake.history.last_actions.contains_key(&7));
        // Дальше — обычное перетаскивание без повторного возврата.
        run.drag(600.0, 500.0);
        run.up();
        assert!(run.fake.take_calls().is_empty());
    }

    #[test]
    fn unsnap_restore_can_be_turned_off() {
        let snapped = Rect::new(0.0, 32.0, 756.0, 950.0);
        let config = Config {
            unsnap_restore: Some(false),
            ..Config::default()
        };
        let mut run = Run::new(config);
        run.fake.frames.insert(7, snapped);
        run.fake
            .history
            .restore_rects
            .insert(7, Rect::new(300.0, 200.0, 800.0, 600.0));
        run.fake
            .history
            .record_action(7, snapped, Action::LeftHalf, None, true);
        run.down();
        run.drag(750.0, 500.0);
        assert!(run.fake.take_calls().is_empty());
        // Ничего не трогали: ни рамку «до», ни последнее действие.
        assert!(run.fake.history.last_actions.contains_key(&7));
    }

    #[test]
    fn window_not_placed_by_rectangle_remembers_its_frame() {
        let mut run = Run::new(Config::default());
        run.down();
        run.drag(750.0, 500.0);
        assert_eq!(
            run.fake.history.restore_rects.get(&7),
            Some(&Rect::new(300.0, 200.0, 800.0, 600.0))
        );
        assert!(run.fake.take_calls().is_empty());
    }

    #[test]
    fn fast_drop_without_drag_events_still_snaps() {
        let mut run = Run::new(Config::default());
        run.down();
        // Событие перетаскивания пришло, когда окно ещё не двигалось.
        run.send(MouseEventKind::LeftMouseDragged);
        // Окно уже у левого края, курсор в зоне — и сразу отпускание.
        run.fake.drag_to(0.0, 500.0);
        run.up();
        assert_eq!(
            run.fake.take_calls(),
            vec![
                Call::Hide,
                Call::Execute(Action::LeftHalf, Some(7), Some(7), 1)
            ]
        );
    }

    #[test]
    fn fast_drop_respects_modifiers() {
        let config = Config {
            snap_modifiers: OPTION as i64,
            ..Config::default()
        };
        let mut run = Run::new(config);
        run.down();
        run.fake.drag_to(0.0, 500.0);
        run.up();
        // Подсветку гасят (её и не было), но окно не трогают.
        assert_eq!(run.fake.take_calls(), vec![Call::Hide]);
    }

    #[test]
    fn haptic_feedback_only_when_enabled() {
        let config = Config {
            haptic_feedback_on_snap: Some(true),
            ..Config::default()
        };
        let mut run = Run::new(config);
        run.down();
        run.drag(0.0, 500.0);
        run.drag(0.0, 982.0);
        let haptics = run
            .fake
            .take_calls()
            .into_iter()
            .filter(|call| *call == Call::Haptic)
            .count();
        assert_eq!(haptics, 2);

        let mut run = Run::new(Config::default());
        run.down();
        run.drag(0.0, 500.0);
        assert!(!run.fake.take_calls().contains(&Call::Haptic));
    }

    #[test]
    fn todo_window_snaps_to_its_side() {
        let config = Config {
            todo: Some(true),
            todo_mode: true,
            ..Config::default()
        };
        let mut run = Run::new(config);
        run.fake.todo.insert(7);
        run.down();
        run.drag(1512.0, 500.0);
        run.up();
        let calls = run.fake.take_calls();
        // Как `getBoxRect`: у `rightTodo` есть `calculateRect` — подсветка панели
        // Todo (400 px справа), затем действие.
        assert_eq!(
            calls,
            vec![
                Call::Show(
                    Directional::R,
                    Action::RightTodo,
                    Rect::new(1112.0, 0.0, 400.0, 950.0)
                ),
                Call::Hide,
                Call::Execute(Action::RightTodo, Some(7), Some(7), 1)
            ]
        );
    }

    #[test]
    fn todo_window_is_asked_about_only_at_the_edge_once_per_drag() {
        let config = Config {
            todo: Some(true),
            todo_mode: true,
            ..Config::default()
        };
        let mut run = Run::new(config);
        run.down();
        for x in [650.0, 600.0, 400.0, 200.0] {
            run.drag(x, 500.0);
        }
        assert_eq!(run.fake.todo_checks, 0, "вдали от краёв не спрашиваем");
        run.drag(0.0, 500.0);
        assert_eq!(run.fake.todo_checks, 1, "у края — спросили");
        // У края дальше, ушли от него и вернулись — ответ уже известен.
        run.drag(1.0, 480.0);
        run.drag(300.0, 500.0);
        run.drag(0.0, 400.0);
        run.up();
        assert_eq!(run.fake.todo_checks, 1);
        // Новое перетаскивание — вопрос заново, и снова только у края.
        run.down();
        run.drag(400.0, 500.0);
        assert_eq!(run.fake.todo_checks, 1);
        run.drag(0.0, 500.0);
        assert_eq!(run.fake.todo_checks, 2);
        run.up();

        // Режим Todo выключен — не спрашиваем вовсе.
        let mut run = Run::new(Config::default());
        run.down();
        run.drag(0.0, 500.0);
        run.up();
        assert_eq!(run.fake.todo_checks, 0);
    }

    #[test]
    fn window_found_on_drag_when_not_obtained_on_click() {
        let config = Config {
            obtain_window_on_click: Some(false),
            ..Config::default()
        };
        let mut run = Run::new(config);
        run.down();
        assert_eq!(run.fake.lookups, 0);
        // Первое перетаскивание: окно, номер и рамка — только сейчас,
        // поэтому окно ещё «не сдвинулось».
        run.drag(500.0, 500.0);
        assert_eq!(run.fake.lookups, 1);
        run.drag(0.0, 500.0);
        run.up();
        assert_eq!(
            run.fake.take_calls(),
            vec![
                Call::Show(Directional::L, Action::LeftHalf, left_half_rect()),
                Call::Hide,
                Call::Execute(Action::LeftHalf, Some(7), Some(7), 1),
            ]
        );
    }

    #[test]
    fn window_id_lookups_are_throttled_and_limited() {
        let mut run = Run::new(Config::default());
        run.fake.ids.insert(7, None);
        run.down();
        assert_eq!(run.fake.lookups, 1);
        // Номера нет: окно уже есть, ищем только номер — не чаще раза в 0,1 с.
        for _ in 0..10 {
            run.drag(run.fake.cursor.0 - 1.0, 500.0);
        }
        // 10 событий по 16 мс: попытки в 0 мс и после 0,1 с — две.
        assert_eq!(run.drag.window_id_attempt, 2);
        assert_eq!(run.fake.lookups, 1, "окно под курсором больше не ищем");
        for _ in 0..400 {
            run.drag(run.fake.cursor.0, 500.0);
        }
        assert_eq!(run.drag.window_id_attempt, WINDOW_ID_ATTEMPTS);
        run.up();
        assert_eq!(run.drag.window_id_attempt, 0);
    }

    #[test]
    fn reset_tells_whether_a_footprint_was_shown() {
        let mut run = Run::new(Config::default());
        run.down();
        run.drag(500.0, 500.0);
        assert!(!run.drag.reset());
        run.down();
        run.drag(0.0, 500.0);
        assert!(run.drag.reset());
        // После сброса отпускание ничего не делает.
        run.fake.take_calls();
        run.up();
        assert!(run.fake.take_calls().is_empty());
    }

    #[test]
    fn no_window_under_cursor_means_nothing() {
        let mut run = Run::new(Config::default());
        run.fake.under_cursor = None;
        run.down();
        run.drag(0.0, 500.0);
        run.up();
        assert!(run.fake.take_calls().is_empty());
    }

    #[test]
    fn shared_edges_are_counted_exactly() {
        let a = Rect::new(0.0, 0.0, 100.0, 100.0);
        assert_eq!(shared_edge_count(&a, &a), 4);
        assert_eq!(shared_edge_count(&a, &Rect::new(0.0, 0.0, 120.0, 130.0)), 2);
        assert_eq!(
            shared_edge_count(&a, &Rect::new(10.0, 10.0, 100.0, 100.0)),
            0
        );
        assert_eq!(shared_edge_count(&a, &Rect::new(0.0, 5.0, 100.0, 100.0)), 2);
    }
}
