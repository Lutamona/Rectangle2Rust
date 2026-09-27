//! Согласованный ресайз в конвейере действия — порт
//! `WindowManager+CooperativeCornerResize.swift`: найти соседние окна на экране,
//! составить план (`cooperative_resize::plan`), поставить окна в нужном порядке,
//! проверить, куда они встали на самом деле, и при нужде сделать повторный проход; после
//! ухода окна в фокусе с прежнего места вернуть соседей к размерам из настроек (`cleanup`).
//!
//! Порядок: растущее окно в фокусе сначала освобождает место (сперва соседи за краем,
//! потом окна его полосы, потом оно само), сжимающееся — наоборот. Соседям пишется
//! история: растущим вместе с окном — то же действие, соседям за краем — действие их
//! стороны, чтобы их следующий повтор продолжил перебор.
//!
//! Окна перечисляются через AX (`AccessibilityElement.getAllWindowElements`). Для живых
//! проверок есть отладочное ограничение `restrict_windows_to_pid`: тогда видны только окна
//! одного процесса. Приложение его не ставит.

mod cycle_targets;

#[cfg(test)]
mod tests;

pub use cycle_targets::{
    adjacent_cycle_action, cleanup_destination_allows_source_resize, cleanup_source_frame,
    cleanup_target_frame, cooperative_history_action,
    cycle_look_ahead_target_for_minimum_restricted_adjacent, cycle_target_frames,
    frame_is_adjacent_to_cleanup_source, observed_cooperative_source_frame, CycleContext,
    CycleLookAheadTarget, CycleTargetFrame,
};

use std::collections::HashMap;
use std::sync::atomic::{AtomicI32, Ordering};

use crate::actions::Action;
use crate::ax::{self, AxElement};
use crate::calc::LastAction;
use crate::config::{Config, CornerCycleExpansionAxis as Axis};
use crate::cooperative_resize::cg::{intersects, swift_max};
use crate::cooperative_resize::{
    capture_tolerance, correction_plan, detection_tolerance,
    focused_frame_resolving_realized_corner_boundary, focused_window_is_expanding,
    frame_needs_application, is_compatible_repeated_resize_action, is_corner_action, plan,
    resize_axis, resize_moved_edge, Adjustment, AdjustmentKind, Candidate, CorrectionParams,
    MovedEdge, Plan, PlanParams, Size,
};
use crate::geometry::Rect;
use crate::log;
use crate::window_history;
use crate::window_manager::ExecutionSource;

/// Насколько фактическая рамка может отличаться от плана (`layoutTolerance`).
pub const LAYOUT_TOLERANCE: f64 = 4.0;

/// Описания проходов для журнала (`actionDescription`).
const REPEATED_DESCRIPTION: &str = "повтор с согласованным ресайзом";
const INITIAL_DESCRIPTION: &str = "первое нажатие угла или половины рядом с соседями";
const CLEANUP_DESCRIPTION: &str = "возврат соседей после ухода окна в фокусе";
const CLEANUP_SETTLING_DESCRIPTION: &str = "повторный проход возврата соседей";

// ---------------------------------------------------------------- окна

/// Окно, которое двигает согласованный ресайз.
pub trait CooperativeWindow: Clone {
    /// Рамка, координаты AX; `None` — не читается (`CGRect.null`).
    fn frame(&self) -> Option<Rect>;
    /// Поставить рамку, координаты AX (`AccessibilityElement.setFrame`).
    fn set_frame(&self, rect: &Rect);
}

impl CooperativeWindow for AxElement {
    fn frame(&self) -> Option<Rect> {
        AxElement::frame(self)
    }

    fn set_frame(&self, rect: &Rect) {
        AxElement::set_frame(self, rect);
    }
}

/// Окно-сосед, найденное на экране: номер, рамка (Cocoa) и минимальный размер.
#[derive(Clone, Debug)]
pub struct NeighborWindow<W> {
    pub id: u32,
    pub window: W,
    pub frame: Rect,
    pub minimum_size: Option<Size>,
}

impl<W> NeighborWindow<W> {
    fn candidate(&self) -> Candidate {
        Candidate {
            id: self.id,
            frame: self.frame,
            minimum_size: self.minimum_size,
        }
    }
}

/// Отладочное ограничение: окна только процесса с этим pid; 0 — все окна.
static WINDOW_SCOPE_PID: AtomicI32 = AtomicI32::new(0);

/// Только для проверок и примеров: согласованный ресайз и сдвиг лесенкой видят окна
/// одного процесса (`Some(pid)`) — так живая проверка не заденет окна пользователя.
/// `None` — все окна, как в приложении (оно это ограничение не ставит).
pub fn restrict_windows_to_pid(pid: Option<i32>) {
    WINDOW_SCOPE_PID.store(pid.unwrap_or(0), Ordering::SeqCst);
}

/// Все окна (`AccessibilityElement.getAllWindowElements()`) — с отладочным ограничением,
/// если оно стоит.
pub(crate) fn scoped_window_elements() -> Vec<AxElement> {
    match WINDOW_SCOPE_PID.load(Ordering::SeqCst) {
        0 => ax::all_window_elements(),
        pid => AxElement::application(pid)
            .window_elements()
            .unwrap_or_default(),
    }
}

/// Окна, которые может подвинуть согласованный ресайз: настоящие окна (не листы), не
/// свёрнутые, не во весь экран, приложение не скрыто, размер меняется, рамка задевает
/// рабочую область (`screen_frame_ax` — координаты AX). `exclude` — окно в фокусе.
///
/// Рамка читается один раз: Swift перечитывает её для кандидата, но между чтениями
/// ничего не двигается. У повторного номера окна, как в словаре Swift, остаётся последнее.
fn neighbor_windows(
    exclude: Option<u32>,
    screen_frame_ax: &Rect,
    primary_height: f64,
) -> Vec<NeighborWindow<AxElement>> {
    let mut windows: Vec<NeighborWindow<AxElement>> = Vec::new();
    for element in scoped_window_elements() {
        let Some(id) = element.get_window_id() else {
            continue;
        };
        if Some(id) == exclude
            || !element.is_window()
            || element.is_minimized()
            || element.is_full_screen() == Some(true)
            || element.is_hidden() == Some(true)
            || element.is_sheet()
            || !element.is_resizable()
        {
            continue;
        }
        let Some(frame) = element.frame() else {
            continue;
        };
        if !intersects(screen_frame_ax, &frame) {
            continue;
        }

        let neighbor = NeighborWindow {
            id,
            frame: frame.screen_flipped(primary_height),
            minimum_size: element.minimum_size().map(Size::from),
            window: element,
        };
        match windows.iter_mut().find(|window| window.id == id) {
            Some(existing) => *existing = neighbor,
            None => windows.push(neighbor),
        }
    }
    windows
}

/// Минимальный размер соседа, который его не сообщает: настройки «меньше» оригинал
/// здесь читает как пиксели, не меньше одного.
fn configured_minimum_size(config: &Config) -> Size {
    Size::new(
        swift_max(1.0, config.minimum_window_width),
        swift_max(1.0, config.minimum_window_height),
    )
}

fn log_lines(lines: &[String]) {
    for line in lines {
        log!("{line}");
    }
}

// ---------------------------------------------------------------- план с окнами

/// Сосед в плане: окно и его новая рамка (`CooperativeCornerWindowAdjustment`).
#[derive(Clone, Debug)]
pub struct WindowAdjustment<W> {
    pub window: W,
    pub id: u32,
    /// Рамка, от которой считался план (Cocoa).
    pub old_frame: Rect,
    pub new_frame: Rect,
    pub kind: AdjustmentKind,
}

/// Окна для рамок плана (`applicationAdjustments(for:elementsById:)`): окна, которых нет
/// среди `windows`, пропускаются.
fn window_adjustments<W: Clone>(
    adjustments: &[Adjustment],
    windows: &[NeighborWindow<W>],
) -> Vec<WindowAdjustment<W>> {
    adjustments
        .iter()
        .filter_map(|adjustment| {
            let window = windows.iter().find(|window| window.id == adjustment.id)?;
            Some(WindowAdjustment {
                window: window.window.clone(),
                id: adjustment.id,
                old_frame: adjustment.old_frame,
                new_frame: adjustment.new_frame,
                kind: adjustment.kind,
            })
        })
        .collect()
}

/// План с окнами (`CooperativeCornerApplicationPlan`).
#[derive(Clone, Debug)]
pub struct ApplicationPlan<W> {
    pub old_focused_frame: Rect,
    pub requested_focused_frame: Rect,
    /// Куда поставить окно в фокусе (Cocoa).
    pub focused_frame: Rect,
    pub screen_frame: Rect,
    pub candidates: Vec<Candidate>,
    pub axis: Axis,
    pub detection_tolerance: f64,
    pub capture_tolerance: f64,
    pub layout_tolerance: f64,
    pub minimum_size: Size,
    pub focused_minimum_size: Option<Size>,
    pub gap_size: f64,
    pub moved_edge: MovedEdge,
    pub action: Action,
    pub candidate_discovery_frame: Rect,
    pub action_description: &'static str,
    pub adjustments: Vec<WindowAdjustment<W>>,
    /// Рамка до гэпов для доли сторон, если повтор перескочил размер перебора.
    pub side_split_recording_frame: Option<Rect>,
    pub debug_log: Vec<String>,
    /// Высота основного экрана — база переворота Cocoa ↔ AX.
    pub primary_height: f64,
}

impl<W: CooperativeWindow> ApplicationPlan<W> {
    /// Хоть одно окно не там, где его ставит план (`needsApplication(focusedCurrentFrame:)`).
    pub fn needs_application(&self, focused_current_frame: Option<&Rect>) -> bool {
        if frame_needs_application(
            focused_current_frame,
            &self.focused_frame,
            &self.screen_frame,
            self.layout_tolerance,
        ) {
            return true;
        }

        self.adjustments.iter().any(|adjustment| {
            frame_needs_application(
                self.cocoa_frame(&adjustment.window).as_ref(),
                &adjustment.new_frame,
                &self.screen_frame,
                self.layout_tolerance,
            )
        })
    }

    /// Рамка окна сейчас, Cocoa.
    fn cocoa_frame(&self, window: &W) -> Option<Rect> {
        window
            .frame()
            .map(|frame| frame.screen_flipped(self.primary_height))
    }

    /// Параметры, с которыми план составлялся, — для повторного прохода.
    fn plan_params(&self) -> PlanParams<'_> {
        PlanParams {
            old_focused_frame: self.old_focused_frame,
            new_focused_frame: self.requested_focused_frame,
            screen_frame: self.screen_frame,
            candidates: &self.candidates,
            axis: self.axis,
            tolerance: self.detection_tolerance,
            minimum_size: self.minimum_size,
            focused_minimum_size: self.focused_minimum_size,
            gap_size: self.gap_size,
            capture_tolerance: Some(self.capture_tolerance),
            moved_edge_override: Some(self.moved_edge),
            candidate_discovery_frame: Some(self.candidate_discovery_frame),
            action_description: self.action_description,
        }
    }

    /// Геометрия плана без окон (`asGeometryPlan()`).
    fn as_geometry_plan(&self) -> Plan {
        Plan {
            focused_frame: self.focused_frame,
            adjustments: self
                .adjustments
                .iter()
                .map(|adjustment| Adjustment {
                    id: adjustment.id,
                    old_frame: adjustment.old_frame,
                    new_frame: adjustment.new_frame,
                    kind: adjustment.kind,
                })
                .collect(),
            debug_log: self.debug_log.clone(),
        }
    }

    /// Окна плана для новых рамок: окна, которых в плане нет, пропускаются.
    fn adjustments_for(&self, adjustments: &[Adjustment]) -> Vec<WindowAdjustment<W>> {
        adjustments
            .iter()
            .filter_map(|adjustment| {
                let window = self
                    .adjustments
                    .iter()
                    .find(|existing| existing.id == adjustment.id)?;
                Some(WindowAdjustment {
                    window: window.window.clone(),
                    id: adjustment.id,
                    old_frame: adjustment.old_frame,
                    new_frame: adjustment.new_frame,
                    kind: adjustment.kind,
                })
            })
            .collect()
    }

    /// Тот же план с новой геометрией (`replacingGeometry(with:)`); окон не осталось — `None`.
    fn replacing_geometry(&self, geometry_plan: &Plan) -> Option<ApplicationPlan<W>> {
        let adjustments = self.adjustments_for(&geometry_plan.adjustments);
        if adjustments.is_empty() {
            return None;
        }
        let mut plan = self.clone();
        plan.focused_frame = geometry_plan.focused_frame;
        plan.adjustments = adjustments;
        plan.debug_log = geometry_plan.debug_log.clone();
        Some(plan)
    }
}

/// Что конвейер знает о действии, когда решает, подстраивать ли соседей.
#[derive(Clone, Copy, Debug)]
pub struct PlanRequest<'a> {
    pub focused_window_id: Option<u32>,
    pub focused_window_is_fixed_size: bool,
    /// Минимальный размер окна в фокусе из AX.
    pub focused_window_minimum_size: Option<Size>,
    pub action: Action,
    pub source: ExecutionSource,
    /// Рамка окна до действия, Cocoa.
    pub old_focused_frame: Rect,
    /// Рамка из расчёта с гэпами, Cocoa.
    pub new_focused_frame: Rect,
    /// Рабочая область экрана назначения.
    pub screen_frame: Rect,
    pub destination_screen_is_current_screen: bool,
    pub last_action: Option<&'a LastAction>,
    pub config: &'a Config,
    pub primary_height: f64,
}

/// Составить согласованный план (`cooperativeCornerResizePlan(...)`): только с
/// включённым `cooperativeCornerResize`, из drag-to-snap, для половин и углов, у окна,
/// которое меняет размер и остаётся на своём экране. `None` — действовать как обычно.
pub fn cooperative_corner_resize_plan(request: &PlanRequest) -> Option<ApplicationPlan<AxElement>> {
    let (focused_window_id, axis, moved_edge) = plan_preconditions(request)?;
    let screen_frame_ax = request.screen_frame.screen_flipped(request.primary_height);
    let windows = neighbor_windows(
        Some(focused_window_id),
        &screen_frame_ax,
        request.primary_height,
    );
    plan_with_windows(request, axis, moved_edge, windows)
}

/// Условия плана: номер окна в фокусе, ось и двигающийся край действия.
fn plan_preconditions(request: &PlanRequest) -> Option<(u32, Axis, MovedEdge)> {
    let config = request.config;
    if !(config.cooperative_corner_resize && request.source.allows_cooperative_resize()) {
        return None;
    }
    let focused_window_id = request.focused_window_id?;
    if request.focused_window_is_fixed_size || !request.destination_screen_is_current_screen {
        return None;
    }
    let axis = resize_axis(request.action, config)?;
    let moved_edge = resize_moved_edge(request.action, config)?;
    Some((focused_window_id, axis, moved_edge))
}

/// План по уже найденным соседям.
fn plan_with_windows<W: CooperativeWindow>(
    request: &PlanRequest,
    axis: Axis,
    moved_edge: MovedEdge,
    windows: Vec<NeighborWindow<W>>,
) -> Option<ApplicationPlan<W>> {
    let config = request.config;
    let screen_frame = request.screen_frame;
    let gap_size = swift_max(0.0, config.gap_size as f64);
    let tolerance = detection_tolerance(&screen_frame, gap_size);
    let capture_tolerance = capture_tolerance(&screen_frame, axis);
    let is_repeated_cooperative_action = is_compatible_repeated_resize_action(
        request.action,
        request.last_action.map(|last| last.action),
        config,
    );
    let action_description = if is_repeated_cooperative_action {
        REPEATED_DESCRIPTION
    } else {
        INITIAL_DESCRIPTION
    };
    let candidates: Vec<Candidate> = windows.iter().map(NeighborWindow::candidate).collect();
    let minimum_size = configured_minimum_size(config);

    let mut side_split_recording_frame = None;
    let mut requested_focused_frame =
        if !is_repeated_cooperative_action && is_corner_action(request.action) {
            focused_frame_resolving_realized_corner_boundary(
                &request.new_focused_frame,
                &screen_frame,
                &candidates,
                axis,
                moved_edge,
                tolerance,
                gap_size,
            )
        } else {
            request.new_focused_frame
        };
    if is_repeated_cooperative_action {
        let context = CycleContext {
            action: request.action,
            screen_frame,
            axis,
            moved_edge,
            tolerance,
            gap_size,
            config,
        };
        if let Some(look_ahead_target) = cycle_look_ahead_target_for_minimum_restricted_adjacent(
            &context,
            &request.old_focused_frame,
            &requested_focused_frame,
            &candidates,
        ) {
            log!(
                "Согласованный ресайз: размер {} пропущен, сразу {} — соседнее окно {} уже в полосе минимального размера",
                look_ahead_target.skipped_cycle_size.title(),
                look_ahead_target.target_cycle_size.title(),
                look_ahead_target.restricted_adjacent_id
            );
            requested_focused_frame = look_ahead_target.gapped_frame;
            side_split_recording_frame = Some(look_ahead_target.raw_frame);
        }
    }
    let candidate_discovery_frame = if is_repeated_cooperative_action {
        request.old_focused_frame
    } else {
        requested_focused_frame
    };
    let geometry_plan = plan(&PlanParams {
        old_focused_frame: request.old_focused_frame,
        new_focused_frame: requested_focused_frame,
        screen_frame,
        candidates: &candidates,
        axis,
        tolerance,
        minimum_size,
        focused_minimum_size: request.focused_window_minimum_size,
        gap_size,
        capture_tolerance: Some(capture_tolerance),
        moved_edge_override: Some(moved_edge),
        candidate_discovery_frame: Some(candidate_discovery_frame),
        action_description,
    })?;

    log_lines(&geometry_plan.debug_log);

    let adjustments = window_adjustments(&geometry_plan.adjustments, &windows);
    if adjustments.is_empty() {
        return None;
    }

    Some(ApplicationPlan {
        old_focused_frame: request.old_focused_frame,
        requested_focused_frame,
        focused_frame: geometry_plan.focused_frame,
        screen_frame,
        candidates,
        axis,
        detection_tolerance: tolerance,
        capture_tolerance,
        layout_tolerance: LAYOUT_TOLERANCE,
        minimum_size,
        focused_minimum_size: request.focused_window_minimum_size,
        gap_size,
        moved_edge,
        action: request.action,
        candidate_discovery_frame,
        action_description,
        adjustments,
        side_split_recording_frame,
        debug_log: geometry_plan.debug_log,
        primary_height: request.primary_height,
    })
}

// ---------------------------------------------------------------- применение

/// Как история соседа меняется при установке (`CooperativeHistoryUpdate`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HistoryUpdate {
    /// Счётчик повторов +1 — первый проход.
    Advance,
    /// Счётчик прежний — повторные проходы и возврат соседей.
    Preserve,
}

/// Общее для установки соседей одного плана.
#[derive(Clone, Copy)]
struct ApplyContext {
    screen_frame: Rect,
    layout_tolerance: f64,
    source_action: Action,
    moved_edge: MovedEdge,
    primary_height: f64,
}

impl ApplyContext {
    fn of<W>(plan: &ApplicationPlan<W>) -> ApplyContext {
        ApplyContext {
            screen_frame: plan.screen_frame,
            layout_tolerance: plan.layout_tolerance,
            source_action: plan.action,
            moved_edge: plan.moved_edge,
            primary_height: plan.primary_height,
        }
    }
}

/// Поставить соседей одного вида (`apply(_:kind:...)`) и записать им историю.
fn apply<W: CooperativeWindow>(
    adjustments: &[WindowAdjustment<W>],
    kind: AdjustmentKind,
    context: ApplyContext,
    history_update: HistoryUpdate,
) {
    for adjustment in adjustments
        .iter()
        .filter(|adjustment| adjustment.kind == kind)
    {
        let current_frame = adjustment
            .window
            .frame()
            .map(|frame| frame.screen_flipped(context.primary_height));
        if frame_needs_application(
            current_frame.as_ref(),
            &adjustment.new_frame,
            &context.screen_frame,
            context.layout_tolerance,
        ) {
            adjustment
                .window
                .set_frame(&adjustment.new_frame.screen_flipped(context.primary_height));
        } else {
            log!(
                "Согласованный ресайз: окно {} уже на месте, не двигаем",
                adjustment.id
            );
        }
        record_cooperative_history(adjustment, context, history_update);
    }
}

/// История соседа (`recordCooperativeHistory`): рамка после установки (AX).
fn record_cooperative_history<W: CooperativeWindow>(
    adjustment: &WindowAdjustment<W>,
    context: ApplyContext,
    history_update: HistoryUpdate,
) {
    let Some(action) =
        cooperative_history_action(adjustment.kind, context.source_action, context.moved_edge)
    else {
        return;
    };
    let Some(resulting_rect) = adjustment.window.frame() else {
        return;
    };
    window_history::with(|history| {
        history.record_action(
            adjustment.id,
            resulting_rect,
            action,
            None,
            history_update == HistoryUpdate::Advance,
        )
    });
}

/// Растёт ли окно по оси; рамку не прочитать — как у `CGRect.null`, нулевой размер.
fn is_expanding(old_frame: Option<&Rect>, new_frame: &Rect, axis: Axis) -> bool {
    let empty = Rect::new(0.0, 0.0, 0.0, 0.0);
    focused_window_is_expanding(old_frame.unwrap_or(&empty), new_frame, axis)
}

/// Поставить окно в фокусе и соседей по плану (`applyCooperativeCornerResize(result:plan:)`).
/// `move_focused` ставит окно в фокусе цепочкой доводки (рамка — Cocoa). Возвращает рамку
/// окна в фокусе после всего (AX).
pub fn apply_cooperative_corner_resize<W: CooperativeWindow>(
    focused: &W,
    move_focused: &mut dyn FnMut(&Rect),
    plan: &ApplicationPlan<W>,
) -> Option<Rect> {
    let mut active_plan = plan.clone();
    let focused_frame_before = focused
        .frame()
        .map(|frame| frame.screen_flipped(plan.primary_height));
    let expanding = is_expanding(
        focused_frame_before.as_ref(),
        &active_plan.focused_frame,
        active_plan.axis,
    );

    if expanding {
        let context = ApplyContext::of(&active_plan);
        apply(
            &active_plan.adjustments,
            AdjustmentKind::Adjacent,
            context,
            HistoryUpdate::Advance,
        );
        apply(
            &active_plan.adjustments,
            AdjustmentKind::MatchingFocusedFrame,
            context,
            HistoryUpdate::Advance,
        );

        if let Some(corrected_plan) =
            correction_plan_after_applying_cooperating_windows(&active_plan)
        {
            active_plan = corrected_plan;
            let context = ApplyContext::of(&active_plan);
            apply(
                &active_plan.adjustments,
                AdjustmentKind::Adjacent,
                context,
                HistoryUpdate::Preserve,
            );
            apply(
                &active_plan.adjustments,
                AdjustmentKind::MatchingFocusedFrame,
                context,
                HistoryUpdate::Preserve,
            );
        }

        let mut resulting_rect = apply_focused_cooperative_frame_if_needed(
            focused,
            move_focused,
            &active_plan.focused_frame,
            &active_plan,
        );
        if let Some(settled_rect) =
            settle_cooperative_corner_resize_if_needed(&active_plan, focused, move_focused)
        {
            resulting_rect = settled_rect;
        }
        return resulting_rect;
    }

    let mut resulting_rect = apply_focused_cooperative_frame_if_needed(
        focused,
        move_focused,
        &active_plan.focused_frame,
        &active_plan,
    );
    let context = ApplyContext::of(&active_plan);
    apply(
        &active_plan.adjustments,
        AdjustmentKind::MatchingFocusedFrame,
        context,
        HistoryUpdate::Advance,
    );
    apply(
        &active_plan.adjustments,
        AdjustmentKind::Adjacent,
        context,
        HistoryUpdate::Advance,
    );

    if let Some(settled_rect) =
        settle_cooperative_corner_resize_if_needed(&active_plan, focused, move_focused)
    {
        resulting_rect = settled_rect;
    }
    resulting_rect
}

/// Соседи встали не туда — план по их фактическим рамкам, пока окно в фокусе ещё не
/// двигалось (`correctionPlanAfterApplyingCooperatingWindows`).
fn correction_plan_after_applying_cooperating_windows<W: CooperativeWindow>(
    plan: &ApplicationPlan<W>,
) -> Option<ApplicationPlan<W>> {
    let actual_candidate_frames: HashMap<u32, Option<Rect>> = plan
        .adjustments
        .iter()
        .map(|adjustment| (adjustment.id, plan.cocoa_frame(&adjustment.window)))
        .collect();
    let planned = plan.as_geometry_plan();
    let correction = correction_plan(&CorrectionParams {
        request: plan.plan_params(),
        planned: &planned,
        actual_focused_frame: Some(plan.focused_frame),
        actual_candidate_frames: &actual_candidate_frames,
        layout_tolerance: plan.layout_tolerance,
    })?;

    log_lines(&correction.debug_log);
    plan.replacing_geometry(&correction)
}

/// Проверка после установки всех окон и повторный проход
/// (`settleCooperativeCornerResizeIfNeeded`). `None` — всё встало по плану; иначе —
/// рамка окна в фокусе после прохода (AX; внутренний `None` — не читается).
fn settle_cooperative_corner_resize_if_needed<W: CooperativeWindow>(
    plan: &ApplicationPlan<W>,
    focused: &W,
    move_focused: &mut dyn FnMut(&Rect),
) -> Option<Option<Rect>> {
    let actual_focused_frame = plan.cocoa_frame(focused);
    let actual_candidate_frames: HashMap<u32, Option<Rect>> = plan
        .adjustments
        .iter()
        .map(|adjustment| (adjustment.id, plan.cocoa_frame(&adjustment.window)))
        .collect();
    let planned = plan.as_geometry_plan();
    let correction = correction_plan(&CorrectionParams {
        request: plan.plan_params(),
        planned: &planned,
        actual_focused_frame,
        actual_candidate_frames: &actual_candidate_frames,
        layout_tolerance: plan.layout_tolerance,
    })?;

    log_lines(&correction.debug_log);
    let correction_adjustments = plan.adjustments_for(&correction.adjustments);
    let context = ApplyContext::of(plan);

    if focused_window_is_expanding(
        &plan.old_focused_frame,
        &correction.focused_frame,
        plan.axis,
    ) {
        apply(
            &correction_adjustments,
            AdjustmentKind::Adjacent,
            context,
            HistoryUpdate::Preserve,
        );
        apply(
            &correction_adjustments,
            AdjustmentKind::MatchingFocusedFrame,
            context,
            HistoryUpdate::Preserve,
        );
        apply_focused_cooperative_frame_if_needed(
            focused,
            move_focused,
            &correction.focused_frame,
            plan,
        );
    } else {
        apply_focused_cooperative_frame_if_needed(
            focused,
            move_focused,
            &correction.focused_frame,
            plan,
        );
        apply(
            &correction_adjustments,
            AdjustmentKind::MatchingFocusedFrame,
            context,
            HistoryUpdate::Preserve,
        );
        apply(
            &correction_adjustments,
            AdjustmentKind::Adjacent,
            context,
            HistoryUpdate::Preserve,
        );
    }

    Some(focused.frame())
}

/// Поставить окно в фокусе, если оно не там (`applyFocusedCooperativeFrameIfNeeded`).
/// Возвращает его рамку (AX).
fn apply_focused_cooperative_frame_if_needed<W: CooperativeWindow>(
    focused: &W,
    move_focused: &mut dyn FnMut(&Rect),
    frame: &Rect,
    plan: &ApplicationPlan<W>,
) -> Option<Rect> {
    if frame_needs_application(
        plan.cocoa_frame(focused).as_ref(),
        frame,
        &plan.screen_frame,
        plan.layout_tolerance,
    ) {
        move_focused(frame);
    } else {
        log!("Согласованный ресайз: окно в фокусе уже на месте, не двигаем");
    }
    focused.frame()
}

// ---------------------------------------------------------------- возврат соседей

/// Что конвейер знает о действии, когда решает, возвращать ли соседей.
#[derive(Clone, Copy, Debug)]
pub struct CleanupRequest<'a> {
    pub focused_window_id: Option<u32>,
    pub source: ExecutionSource,
    /// Рамка окна до действия, Cocoa.
    pub old_focused_frame: Rect,
    /// Рамка окна после действия, Cocoa (`None` — не читается).
    pub new_focused_frame: Option<Rect>,
    /// Рабочая область экрана, с которого окно ушло.
    pub screen_frame: Rect,
    pub current_action: Action,
    pub last_action: Option<&'a LastAction>,
    pub config: &'a Config,
    pub primary_height: f64,
}

/// Окно в фокусе сменило действие — вернуть соседей, которых оно ужало прошлым
/// согласованным ресайзом, к размерам из настроек (`applyCooperativeCornerCleanupIfNeeded`).
pub fn apply_cooperative_corner_cleanup_if_needed(request: &CleanupRequest) {
    let Some(cleanup) = cleanup_preconditions(request) else {
        return;
    };
    let screen_frame_ax = request.screen_frame.screen_flipped(request.primary_height);
    let windows = neighbor_windows(None, &screen_frame_ax, request.primary_height);
    cleanup_with_windows(request, cleanup, windows);
}

/// Прошлое действие, чьих соседей возвращаем.
#[derive(Clone, Copy, Debug)]
struct CleanupAction {
    focused_window_id: u32,
    previous_action: Action,
    axis: Axis,
    moved_edge: MovedEdge,
    new_focused_frame: Rect,
}

/// Условия возврата: согласованный ресайз включён, источник позволяет, прошлое действие —
/// другое и с согласованным ресайзом, окно было на этом экране.
fn cleanup_preconditions(request: &CleanupRequest) -> Option<CleanupAction> {
    let config = request.config;
    if !(config.cooperative_corner_resize && request.source.allows_cooperative_resize()) {
        return None;
    }
    let previous_action = request.last_action?.action;
    if request.current_action == previous_action {
        return None;
    }
    let axis = resize_axis(previous_action, config)?;
    let moved_edge = resize_moved_edge(previous_action, config)?;
    let new_focused_frame = request.new_focused_frame?;
    if !intersects(&request.screen_frame, &request.old_focused_frame) {
        return None;
    }
    Some(CleanupAction {
        focused_window_id: request.focused_window_id?,
        previous_action,
        axis,
        moved_edge,
        new_focused_frame,
    })
}

/// Возврат соседей по уже найденным окнам (вместе с окном в фокусе).
fn cleanup_with_windows<W: CooperativeWindow>(
    request: &CleanupRequest,
    cleanup: CleanupAction,
    all_windows: Vec<NeighborWindow<W>>,
) {
    let config = request.config;
    let screen_frame = request.screen_frame;
    let axis = cleanup.axis;
    let moved_edge = cleanup.moved_edge;
    let gap_size = swift_max(0.0, config.gap_size as f64);
    let tolerance = detection_tolerance(&screen_frame, gap_size);
    let capture_tolerance = capture_tolerance(&screen_frame, axis);
    let layout_tolerance = LAYOUT_TOLERANCE;

    let focused_window = all_windows
        .iter()
        .find(|window| window.id == cleanup.focused_window_id)
        .cloned();
    let mut windows: Vec<NeighborWindow<W>> = all_windows
        .into_iter()
        .filter(|window| window.id != cleanup.focused_window_id)
        .collect();
    let candidates: Vec<Candidate> = windows.iter().map(NeighborWindow::candidate).collect();

    let cycle_context = CycleContext {
        action: cleanup.previous_action,
        screen_frame,
        axis,
        moved_edge,
        tolerance,
        gap_size,
        config,
    };
    let Some(observed_source_frame) = cleanup_source_frame(
        &cycle_context,
        &request.old_focused_frame,
        &candidates,
        capture_tolerance,
    ) else {
        return;
    };

    let mut target_candidates = candidates;
    if let Some(focused_window) = &focused_window {
        target_candidates.push(focused_window.candidate());
    }
    let include_cycle_targets = request.last_action.map_or(0, |last| last.count) > 1;
    let Some(target_frame) = cleanup_target_frame(
        &cycle_context,
        &observed_source_frame,
        include_cycle_targets,
        &target_candidates,
    ) else {
        return;
    };

    let focused_destination_frame = focused_window
        .as_ref()
        .map_or(cleanup.new_focused_frame, |window| window.frame);
    if !cleanup_destination_allows_source_resize(
        &cycle_context,
        &observed_source_frame,
        &target_frame,
        &focused_destination_frame,
    ) {
        return;
    }

    let minimum_size = configured_minimum_size(config);
    let synthetic_focused_minimum_size = Size::new(1.0, 1.0);
    if let Some(focused_window) = focused_window {
        windows.push(focused_window);
    }

    let cleanup_candidates: Vec<Candidate> =
        windows.iter().map(NeighborWindow::candidate).collect();
    let cleanup_request = PlanParams {
        old_focused_frame: observed_source_frame,
        new_focused_frame: target_frame,
        screen_frame,
        candidates: &cleanup_candidates,
        axis,
        tolerance,
        minimum_size,
        focused_minimum_size: Some(synthetic_focused_minimum_size),
        gap_size,
        capture_tolerance: Some(capture_tolerance),
        moved_edge_override: Some(moved_edge),
        candidate_discovery_frame: Some(observed_source_frame),
        action_description: CLEANUP_DESCRIPTION,
    };
    let Some(cleanup_plan) = plan(&cleanup_request) else {
        return;
    };

    log_lines(&cleanup_plan.debug_log);
    let cleanup_adjustments = window_adjustments(&cleanup_plan.adjustments, &windows);
    let cocoa_frame = |window: &W| {
        window
            .frame()
            .map(|frame| frame.screen_flipped(request.primary_height))
    };
    let needs_application = cleanup_adjustments.iter().any(|adjustment| {
        frame_needs_application(
            cocoa_frame(&adjustment.window).as_ref(),
            &adjustment.new_frame,
            &screen_frame,
            layout_tolerance,
        )
    });
    if !needs_application {
        log!("Согласованный ресайз, возврат соседей: все окна уже на местах");
        return;
    }

    let context = ApplyContext {
        screen_frame,
        layout_tolerance,
        source_action: cleanup.previous_action,
        moved_edge,
        primary_height: request.primary_height,
    };
    apply_cleanup_adjustments(
        &cleanup_adjustments,
        &observed_source_frame,
        &cleanup_plan.focused_frame,
        axis,
        context,
    );

    let actual_candidate_frames: HashMap<u32, Option<Rect>> = cleanup_adjustments
        .iter()
        .map(|adjustment| (adjustment.id, cocoa_frame(&adjustment.window)))
        .collect();
    let Some(correction) = correction_plan(&CorrectionParams {
        request: PlanParams {
            action_description: CLEANUP_SETTLING_DESCRIPTION,
            ..cleanup_request
        },
        planned: &cleanup_plan,
        actual_focused_frame: Some(cleanup_plan.focused_frame),
        actual_candidate_frames: &actual_candidate_frames,
        layout_tolerance,
    }) else {
        return;
    };

    log_lines(&correction.debug_log);
    apply_cleanup_adjustments(
        &window_adjustments(&correction.adjustments, &windows),
        &observed_source_frame,
        &correction.focused_frame,
        axis,
        context,
    );
}

/// Поставить соседей при возврате (`applyCleanupAdjustments`): история не продвигается.
fn apply_cleanup_adjustments<W: CooperativeWindow>(
    adjustments: &[WindowAdjustment<W>],
    old_focused_frame: &Rect,
    solved_focused_frame: &Rect,
    axis: Axis,
    context: ApplyContext,
) {
    let order = if focused_window_is_expanding(old_focused_frame, solved_focused_frame, axis) {
        [
            AdjustmentKind::Adjacent,
            AdjustmentKind::MatchingFocusedFrame,
        ]
    } else {
        [
            AdjustmentKind::MatchingFocusedFrame,
            AdjustmentKind::Adjacent,
        ]
    };
    for kind in order {
        apply(adjustments, kind, context, HistoryUpdate::Preserve);
    }
}
