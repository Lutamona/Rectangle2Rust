//! Доводка окна после расчёта — порт цепочек `WindowMover` из Rectangle.
//!
//! Обычное окно проходит три шага: `StandardWindowMover` (size → position →
//! size), `EdgeAlignmentWindowMover` (приложение ужало окно — минимальная
//! ширина, сетка символов — и его надо прижать к тому краю, который зона делит
//! с экраном) и `BestEffortWindowMover` (окно не влезло — сдвинуть внутрь
//! рабочей области). Окно фиксированного размера и системный диалог не
//! ресайзятся вовсе: `FixedSizeWindowMover` сразу ставит окно в зону по краям,
//! затем BestEffort — два шага.
//!
//! Координаты: зона и рабочая область приходят в Cocoa, окно живёт в AX. Общие
//! края считаются по перевёрнутым (AX) рамкам — в той же системе, где их
//! применяет выравнивание: `.top` там — это `maxY`, то есть визуальный низ.

use crate::actions::Action;
use crate::ax::AxElement;
use crate::config::{CornerCycleExpansionAxis, EdgeAlignment};
use crate::geometry::{Edge, Rect};

/// Окно, которое умеет цепочка доводки: прочитать и поставить рамку (AX).
pub trait MovableWindow {
    /// Рамка окна в координатах AX.
    fn frame(&self) -> Option<Rect>;
    /// Поставить рамку (AX); `adjust_size_first` — сначала размер, потом положение.
    fn set_frame(&self, rect: &Rect, adjust_size_first: bool);
}

impl MovableWindow for AxElement {
    fn frame(&self) -> Option<Rect> {
        AxElement::frame(self)
    }

    fn set_frame(&self, rect: &Rect, adjust_size_first: bool) {
        self.set_frame_ordered(rect, adjust_size_first);
    }
}

/// Что цепочке нужно знать о действии (`ResultParameters` без окна).
#[derive(Clone, Debug, PartialEq)]
pub struct MoveParameters {
    /// Нажатое действие (не итоговое): от него зависят порядок установки рамки,
    /// нужно ли выравнивание и можно ли окну вылезать за экран.
    pub action: Action,
    /// Рамка из расчёта до гэпов (Cocoa) — по ней считаются общие с экраном края.
    pub initial_rect: Rect,
    /// Рабочая область экрана назначения (Cocoa).
    pub visible_frame: Rect,
    /// Окно не ресайзится (и действие меняет размер) или это системный диалог.
    pub is_fixed_size: bool,
    /// Высота основного экрана — база переворота Cocoa ↔ AX.
    pub primary_height: f64,
    /// `NSScreen.screensHaveSeparateSpaces`.
    pub separate_spaces: bool,
    pub move_fixed_size_to_edge: EdgeAlignment,
    pub corner_cycle_expansion_axis: CornerCycleExpansionAxis,
    pub resize_on_directional_move: bool,
    pub gap_size: f64,
}

/// Порядок операций при установке рамки (`StandardWindowMover.shouldAdjustSizeFirst`).
pub fn should_adjust_size_first(action: Action, axis: CornerCycleExpansionAxis) -> bool {
    !matches!(
        (action, axis),
        (Action::TopRight, CornerCycleExpansionAxis::Horizontal)
            | (Action::BottomRight, CornerCycleExpansionAxis::Horizontal)
            | (Action::BottomLeft, CornerCycleExpansionAxis::Vertical)
            | (Action::BottomRight, CornerCycleExpansionAxis::Vertical)
    )
}

/// Выравнивание окна внутри зоны по общим с ним краям (`ClampedWindowAligner`).
/// По каждой оси: зона касается ровно одного края экрана — прижать к нему,
/// обоих или ни одного — по центру. `window` и `zone` — в одной перевёрнутой
/// (AX) системе, где `TOP` — это `maxY`.
pub fn aligned(window: Rect, zone: &Rect, shared_edges: Edge) -> Rect {
    let mut result = window;

    if window.w != zone.w {
        if shared_edges.contains(Edge::LEFT) && !shared_edges.contains(Edge::RIGHT) {
            result.x = zone.min_x();
        } else if shared_edges.contains(Edge::RIGHT) && !shared_edges.contains(Edge::LEFT) {
            result.x = zone.max_x() - window.w;
        } else {
            result.x = ((zone.w - window.w) / 2.0).round() + zone.min_x();
        }
    }

    if window.h != zone.h {
        if shared_edges.contains(Edge::TOP) && !shared_edges.contains(Edge::BOTTOM) {
            result.y = zone.max_y() - window.h;
        } else if shared_edges.contains(Edge::BOTTOM) && !shared_edges.contains(Edge::TOP) {
            result.y = zone.min_y();
        } else {
            result.y = ((zone.h - window.h) / 2.0).round() + zone.min_y();
        }
    }

    result
}

/// Какие края считать общими при выравнивании (`EdgeAlignment.alignmentEdges`).
/// Обе рамки — в одной системе координат (цепочка передаёт перевёрнутые).
pub fn alignment_edges(rect: &Rect, screen_frame: &Rect, alignment: EdgeAlignment) -> Edge {
    let shared = rect.shared_edges(screen_frame, 0.0);
    match alignment {
        EdgeAlignment::EdgesAndCorners => shared,
        EdgeAlignment::Corners => {
            if shared.is_corner() {
                shared
            } else {
                Edge::NONE
            }
        }
        EdgeAlignment::Centered => Edge::NONE,
    }
}

/// Цепочка доводки (`WindowManager.moveWindow`): `rect` — зона с гэпами, Cocoa.
pub fn move_window<W: MovableWindow>(window: &W, rect: &Rect, params: &MoveParameters) {
    if params.is_fixed_size {
        fixed_size_move(window, rect, params);
    } else {
        standard_move(window, rect, params);
        edge_alignment_move(window, rect, params);
    }
    best_effort_move(window, params);
}

/// `StandardWindowMover`: поставить рамку расчёта.
pub fn standard_move<W: MovableWindow>(window: &W, rect: &Rect, params: &MoveParameters) {
    if window.frame().is_none() {
        return;
    }
    window.set_frame(
        &rect.screen_flipped(params.primary_height),
        should_adjust_size_first(params.action, params.corner_cycle_expansion_axis),
    );
}

/// `EdgeAlignmentWindowMover`: окно ресайзилось, но приложение ужало его —
/// прижимаем к краю экрана, который делит зона. Только для действий, которые
/// меняют размер.
pub fn edge_alignment_move<W: MovableWindow>(window: &W, rect: &Rect, params: &MoveParameters) {
    if !params.action.resizes(params.resize_on_directional_move) {
        return;
    }
    align_in_zone(window, rect, params);
}

/// `FixedSizeWindowMover`: окно не меняет размер — ставим его в зону по краям.
pub fn fixed_size_move<W: MovableWindow>(window: &W, rect: &Rect, params: &MoveParameters) {
    align_in_zone(window, rect, params);
}

fn align_in_zone<W: MovableWindow>(window: &W, rect: &Rect, params: &MoveParameters) {
    let Some(current) = window.frame() else {
        return;
    };
    let height = params.primary_height;
    let shared_edges = alignment_edges(
        &params.initial_rect.screen_flipped(height),
        &params.visible_frame.screen_flipped(height),
        params.move_fixed_size_to_edge,
    );
    let adjusted = aligned(current, &rect.screen_flipped(height), shared_edges);
    if adjusted != current {
        window.set_frame(&adjusted, true);
    }
}

/// `BestEffortWindowMover`: окно не поместилось — сдвигаем внутрь рабочей
/// области. Двойные размеры могут вылезать за экран, только если у мониторов
/// общие Spaces: с отдельными Spaces окно на два экрана всё равно не видно.
pub fn best_effort_move<W: MovableWindow>(window: &W, params: &MoveParameters) {
    if params
        .action
        .allowed_to_extend_outside_current_screen_area()
        && !params.separate_spaces
    {
        return;
    }
    let Some(current) = window.frame() else {
        return;
    };
    let adjusted = best_effort_rect(
        &current,
        &params.visible_frame,
        params.gap_size,
        params.primary_height,
    );
    if adjusted != current {
        window.set_frame(&adjusted, true);
    }
}

/// Куда сдвинуть окно (`current` — AX), чтобы оно влезло в рабочую область
/// (`visible` — Cocoa). X одинаков в обеих системах; по Y окно переворачивается
/// в Cocoa и сравнивается с рабочей областью, потом переворачивается обратно.
pub fn best_effort_rect(
    current: &Rect,
    visible: &Rect,
    gap_size: f64,
    primary_height: f64,
) -> Rect {
    let mut adjusted = *current;
    if adjusted.min_x() < visible.min_x() {
        adjusted.x = visible.min_x();
    } else if adjusted.min_x() + adjusted.w > visible.min_x() + visible.w {
        adjusted.x = visible.min_x() + visible.w - adjusted.w - gap_size;
    }

    let mut flipped = adjusted.screen_flipped(primary_height);
    if flipped.min_y() < visible.min_y() {
        flipped.y = visible.min_y();
    } else if flipped.min_y() + flipped.h > visible.min_y() + visible.h {
        flipped.y = visible.min_y() + visible.h - flipped.h - gap_size;
    }
    flipped.screen_flipped(primary_height)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    const PRIMARY: f64 = 1117.0;

    /// Рабочая область ноутбука 1728×1117: меню-бар 32, док 83.
    fn visible() -> Rect {
        Rect::new(0.0, 83.0, 1728.0, 1002.0)
    }

    /// Окно-макет: подгоняет размер как настоящее приложение и считает вызовы.
    struct FakeWindow {
        frame: RefCell<Rect>,
        /// Шаг размера, как сетка символов у Терминала (`contentResizeIncrements`).
        increments: Option<(f64, f64)>,
        /// Окно без изменения размера.
        fixed: bool,
        calls: RefCell<Vec<(Rect, bool)>>,
    }

    impl FakeWindow {
        fn new(frame: Rect) -> Self {
            FakeWindow {
                frame: RefCell::new(frame),
                increments: None,
                fixed: false,
                calls: RefCell::new(Vec::new()),
            }
        }

        fn terminal(frame: Rect) -> Self {
            FakeWindow {
                increments: Some((7.0, 14.0)),
                ..FakeWindow::new(frame)
            }
        }

        fn fixed(frame: Rect) -> Self {
            FakeWindow {
                fixed: true,
                ..FakeWindow::new(frame)
            }
        }

        fn accept_size(&self, w: f64, h: f64) {
            let mut frame = self.frame.borrow_mut();
            if self.fixed {
                return;
            }
            match self.increments {
                Some((dw, dh)) => {
                    frame.w = (w / dw).floor() * dw;
                    frame.h = (h / dh).floor() * dh;
                }
                None => {
                    frame.w = w;
                    frame.h = h;
                }
            }
        }
    }

    impl MovableWindow for FakeWindow {
        fn frame(&self) -> Option<Rect> {
            Some(*self.frame.borrow())
        }

        fn set_frame(&self, rect: &Rect, adjust_size_first: bool) {
            self.calls.borrow_mut().push((*rect, adjust_size_first));
            if adjust_size_first {
                self.accept_size(rect.w, rect.h);
            }
            {
                let mut frame = self.frame.borrow_mut();
                frame.x = rect.x;
                frame.y = rect.y;
            }
            self.accept_size(rect.w, rect.h);
        }
    }

    fn params(action: Action, initial: Rect) -> MoveParameters {
        MoveParameters {
            action,
            initial_rect: initial,
            visible_frame: visible(),
            is_fixed_size: false,
            primary_height: PRIMARY,
            separate_spaces: true,
            move_fixed_size_to_edge: EdgeAlignment::EdgesAndCorners,
            corner_cycle_expansion_axis: CornerCycleExpansionAxis::Horizontal,
            resize_on_directional_move: false,
            gap_size: 0.0,
        }
    }

    /// Прогнать цепочку для зоны `zone` (Cocoa, без гэпов) и вернуть рамку окна (AX).
    fn run(window: &FakeWindow, action: Action, zone: Rect) -> Rect {
        move_window(window, &zone, &params(action, zone));
        window.frame().unwrap()
    }

    #[test]
    fn clamped_terminal_sticks_to_the_screen_edge_it_shares() {
        let start = Rect::new(300.0, 300.0, 700.0, 420.0);
        let top_of_visible = PRIMARY - visible().max_y(); // 32 — низ меню-бара
        let bottom_of_visible = PRIMARY - visible().min_y(); // 1034 — верх дока

        // Верхняя половина: 501 → 490 по высоте. Окно у меню-бара, а не у середины.
        let top_half = Rect::new(0.0, 584.0, 1728.0, 501.0);
        let frame = run(&FakeWindow::terminal(start), Action::TopHalf, top_half);
        assert_eq!(frame.h, 490.0);
        assert_eq!(frame.y, top_of_visible);
        // По ширине 1728 → 1722 и оба края общие — по центру.
        assert_eq!(frame.x, 3.0);

        // Нижняя половина: окно у дока.
        let bottom_half = Rect::new(0.0, 83.0, 1728.0, 501.0);
        let frame = run(
            &FakeWindow::terminal(start),
            Action::BottomHalf,
            bottom_half,
        );
        assert_eq!(frame.max_y(), bottom_of_visible);

        // Верхний левый угол: к меню-бару и к левому краю.
        let top_left = Rect::new(0.0, 584.0, 864.0, 501.0);
        let frame = run(&FakeWindow::terminal(start), Action::TopLeft, top_left);
        assert_eq!((frame.x, frame.y), (0.0, top_of_visible));

        // Нижний правый угол: к доку и к правому краю.
        let bottom_right = Rect::new(864.0, 83.0, 864.0, 501.0);
        let frame = run(
            &FakeWindow::terminal(start),
            Action::BottomRight,
            bottom_right,
        );
        assert_eq!(frame.max_x(), 1728.0);
        assert_eq!(frame.max_y(), bottom_of_visible);
    }

    #[test]
    fn left_half_of_terminal_is_centered_vertically() {
        // Оба вертикальных края общие — окно по центру по высоте, у левого края.
        let start = Rect::new(300.0, 300.0, 700.0, 420.0);
        let left_half = Rect::new(0.0, 83.0, 864.0, 1002.0);
        let frame = run(&FakeWindow::terminal(start), Action::LeftHalf, left_half);
        assert_eq!(frame.w, 861.0);
        assert_eq!(frame.h, 994.0);
        assert_eq!(frame.x, 0.0);
        assert_eq!(frame.y, 32.0 + 4.0);
    }

    #[test]
    fn fixed_size_window_is_placed_without_resizing() {
        let start = Rect::new(300.0, 300.0, 520.0, 360.0);
        let window = FakeWindow::fixed(start);
        let left_half = Rect::new(0.0, 83.0, 864.0, 1002.0);
        let mut p = params(Action::LeftHalf, left_half);
        p.is_fixed_size = true;
        move_window(&window, &left_half, &p);

        let calls = window.calls.borrow();
        // Один сеттер: сразу нужное место, размер окна свой (без попытки растянуть до 864).
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0.w, 520.0);
        let frame = window.frame().unwrap();
        assert_eq!(frame.x, 0.0);
        assert_eq!(frame.y, 32.0 + ((1002.0 - 360.0) / 2.0_f64).round());
        assert_eq!((frame.w, frame.h), (520.0, 360.0));
    }

    #[test]
    fn shared_edges_come_from_the_rect_before_gaps() {
        // Гэп 10: зона отстоит от краёв, но общие края — у рамки до гэпов,
        // поэтому ужатое окно прижимается к левому краю зоны, а не центрируется.
        let initial = Rect::new(0.0, 83.0, 864.0, 1002.0);
        let with_gaps = Rect::new(10.0, 93.0, 849.0, 982.0);
        let window = FakeWindow::terminal(Rect::new(300.0, 300.0, 700.0, 420.0));
        let mut p = params(Action::LeftHalf, initial);
        p.gap_size = 10.0;
        move_window(&window, &with_gaps, &p);
        let frame = window.frame().unwrap();
        assert_eq!(frame.w, 847.0);
        assert_eq!(frame.x, 10.0);
    }

    #[test]
    fn edge_alignment_is_skipped_for_actions_that_keep_size() {
        // «Центр» не ресайзит: окно, которое приложение подогнало под свою
        // сетку (703×425 → 700×420), остаётся там, куда его поставили.
        let window = FakeWindow::terminal(Rect::new(0.0, 32.0, 700.0, 420.0));
        let zone = Rect::new(514.0, 374.0, 703.0, 425.0);
        move_window(&window, &zone, &params(Action::Center, zone));
        assert_eq!(window.calls.borrow().len(), 1);
        let frame = window.frame().unwrap();
        assert_eq!((frame.x, frame.y), (514.0, PRIMARY - 374.0 - 425.0));

        // «Левая половина» то же окно довыравнивает — второй сеттер.
        let window = FakeWindow::terminal(Rect::new(0.0, 32.0, 700.0, 420.0));
        move_window(&window, &zone, &params(Action::LeftHalf, zone));
        assert_eq!(window.calls.borrow().len(), 2);
    }

    #[test]
    fn frame_already_right_needs_no_extra_setters() {
        // Обычное окно приняло рамку: ни выравнивания, ни сдвига — один сеттер.
        let window = FakeWindow::new(Rect::new(300.0, 300.0, 700.0, 420.0));
        let left_half = Rect::new(0.0, 83.0, 864.0, 1002.0);
        let frame = run(&window, Action::LeftHalf, left_half);
        assert_eq!(frame, left_half.screen_flipped(PRIMARY));
        assert_eq!(window.calls.borrow().len(), 1);
    }

    #[test]
    fn corners_set_position_first_when_they_grow_to_the_left() {
        assert!(!should_adjust_size_first(
            Action::TopRight,
            CornerCycleExpansionAxis::Horizontal
        ));
        assert!(should_adjust_size_first(
            Action::TopRight,
            CornerCycleExpansionAxis::Vertical
        ));
        assert!(!should_adjust_size_first(
            Action::BottomLeft,
            CornerCycleExpansionAxis::Vertical
        ));
        assert!(should_adjust_size_first(
            Action::LeftHalf,
            CornerCycleExpansionAxis::Horizontal
        ));
    }

    #[test]
    fn double_width_is_pulled_back_on_screen_with_separate_spaces() {
        // Экран 1728, окно x=900 w=800 → «двойная ширина вправо» дала w=1600.
        let window = FakeWindow::new(Rect::new(900.0, 100.0, 800.0, 600.0));
        let doubled = Rect::new(900.0, 417.0, 1600.0, 600.0);
        let mut p = params(Action::DoubleWidthRight, doubled);
        p.visible_frame = Rect::new(0.0, 0.0, 1728.0, 1117.0);
        move_window(&window, &doubled, &p);
        assert_eq!(window.frame().unwrap().x, 128.0);

        // Общие Spaces — окну можно вылезать на соседний экран.
        let window = FakeWindow::new(Rect::new(900.0, 100.0, 800.0, 600.0));
        p.separate_spaces = false;
        move_window(&window, &doubled, &p);
        assert_eq!(window.frame().unwrap().x, 900.0);
    }

    #[test]
    fn best_effort_rect_moves_window_inside_visible_frame() {
        let visible = Rect::new(0.0, 83.0, 1728.0, 1002.0);
        // Вылез вправо и вниз (под док).
        let current = Rect::new(1500.0, 900.0, 400.0, 300.0);
        let adjusted = best_effort_rect(&current, &visible, 0.0, PRIMARY);
        assert_eq!(adjusted.x, 1328.0);
        assert_eq!(adjusted.max_y(), PRIMARY - 83.0);
        // С гэпом у правого и верхнего краёв остаётся зазор.
        let adjusted = best_effort_rect(
            &Rect::new(1500.0, 0.0, 400.0, 300.0),
            &visible,
            10.0,
            PRIMARY,
        );
        assert_eq!(adjusted.x, 1318.0);
        assert_eq!(adjusted.y, 42.0);
        // Уже внутри — без изменений.
        let inside = Rect::new(100.0, 100.0, 400.0, 300.0);
        assert_eq!(best_effort_rect(&inside, &visible, 0.0, PRIMARY), inside);
    }

    #[test]
    fn edge_alignment_modes() {
        // Верхняя левая четверть в AX-координатах: общие края — левый и `BOTTOM`
        // (в AX это minY, то есть визуальный верх).
        let visible_ax = visible().screen_flipped(PRIMARY);
        let top_left_ax = Rect::new(0.0, 584.0, 864.0, 501.0).screen_flipped(PRIMARY);
        assert_eq!(
            alignment_edges(&top_left_ax, &visible_ax, EdgeAlignment::EdgesAndCorners),
            Edge::LEFT.with(Edge::BOTTOM)
        );
        assert_eq!(
            alignment_edges(&top_left_ax, &visible_ax, EdgeAlignment::Corners),
            Edge::LEFT.with(Edge::BOTTOM)
        );
        let left_half_ax = Rect::new(0.0, 83.0, 864.0, 1002.0).screen_flipped(PRIMARY);
        // Три общих края — не угол: в режиме «только углы» центрируем.
        assert!(alignment_edges(&left_half_ax, &visible_ax, EdgeAlignment::Corners).is_empty());
        assert!(alignment_edges(&top_left_ax, &visible_ax, EdgeAlignment::Centered).is_empty());
    }

    #[test]
    fn clamped_height_is_centered_when_both_edges_shared() {
        let zone = Rect::new(0.0, 33.0, 864.0, 1001.0);
        let window = Rect::new(0.0, 33.0, 864.0, 1000.0);
        let shared = Edge::LEFT.with(Edge::TOP).with(Edge::BOTTOM);
        let result = aligned(window, &zone, shared);
        // (1001 − 1000) / 2 = 0,5 → round → 1: окно встаёт на y = 34.
        assert_eq!(result.y, 34.0);
    }

    #[test]
    fn column_in_the_middle_is_centered() {
        let zone = Rect::new(346.0, 33.0, 345.0, 1001.0);
        let window = Rect::new(346.0, 33.0, 342.0, 1000.0);
        let result = aligned(window, &zone, Edge::NONE);
        assert_eq!(result.x, 348.0); // (345-342)/2 = 1.5 → round = 2
        assert_eq!(result.y, 34.0);
    }
}
