//! «Окна приложения столбиками» — функция сверх оригинала (просьба Алексея).
//!
//! Все стандартные окна приложения на экране его фокусного окна встают в N
//! столбиков во всю высоту рабочей области, N — число окон. Геометрия — та же, что
//! у действий «столбик» (`geometry::columns`, гэпы только на стыках), но для любого N.
//!
//! Два режима:
//! - разово — `tile_frontmost_app()` (пункт меню «Окна приложения столбиками»);
//! - «держать столбиками» по приложению — `set_keeping()` (настройка
//!   `r2AppColumnsBundleIds`): `ax_observer` сообщает о новых, закрытых, свёрнутых и
//!   развёрнутых окнах, и через ~200 мс после серии событий раскладка
//!   перестраивается. Перемещение и ресайз окон пользователем раскладку не трогают:
//!   на них не подписываемся, а перестройка идёт, только если поменялся набор окон.
//!
//! Порядок столбиков стабильный: для приложения запоминаются окна прошлой раскладки
//! (по window id); известные окна сохраняют порядок, новые встают справа в порядке
//! создания, закрытые выпадают; без памяти — слева направо по текущему положению.
//!
//! Всё, что трогает AX, подписки и память раскладок, — на главном потоке.

use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fmt;
use std::time::{Duration, Instant};

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject};
use objc2::{define_class, msg_send, sel, AllocAnyThread, MainThreadMarker};
use objc2_app_kit::{
    NSRunningApplication, NSWorkspace, NSWorkspaceDidLaunchApplicationNotification,
    NSWorkspaceDidTerminateApplicationNotification,
};
use objc2_foundation::NSNotification;

use crate::actions::Dimension;
use crate::ax::{self, AxElement};
use crate::ax_observer::{AppObserver, MainTimer, WindowEvent};
use crate::calc;
use crate::config::{self, Config, EnhancedUI};
use crate::geometry::{columns, Edge, Rect};
use crate::screens;

/// Подпись пункта меню разовой раскладки.
pub const TILE_MENU_TITLE: &str = "Окна приложения столбиками";

/// Подпись пункта меню режима «держать» (с галочкой) для приложения `app_name`.
pub fn keep_menu_title(app_name: &str) -> String {
    format!("Держать окна «{app_name}» столбиками")
}

/// Больше столбиков не бывает: номер столбика в `geometry::columns` — `u8`.
pub const MAX_COLUMNS: usize = u8::MAX as usize;

/// Пауза после события: серия событий подряд даёт одну перестройку.
const DEBOUNCE: Duration = Duration::from_millis(200);
/// Дольше этого перестройку не откладываем, даже если события идут без перерыва.
const MAX_DELAY: Duration = Duration::from_secs(1);
/// Повтор подписки на только что запущенное приложение, которое ещё не отвечает AX.
const ATTACH_RETRY_DELAY: Duration = Duration::from_millis(500);
const ATTACH_ATTEMPTS: u8 = 10;
/// Страховка от зависшего приложения: дольше ответа AX не ждём (секунды).
const AX_TIMEOUT: f32 = 0.5;
/// Допуск «окно уже стоит где надо», пиксели.
const TOLERANCE: f64 = 0.5;

#[link(name = "AppKit", kind = "framework")]
extern "C" {
    fn NSBeep();
}

// ---------------------------------------------------------------- результат

/// Почему раскладка не вышла.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TileError {
    /// Нет активного приложения.
    NoApp,
    /// Нет разрешения управлять компьютером (Accessibility).
    NoAccess,
    /// У приложения нет подходящих окон на экране.
    NoWindows,
    /// Система не вернула ни одного экрана.
    NoScreens,
}

impl fmt::Display for TileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            TileError::NoApp => "нет активного приложения",
            TileError::NoAccess => "нет доступа к управлению компьютером",
            TileError::NoWindows => "у приложения нет стандартных развёрнутых окон на экране",
            TileError::NoScreens => "система не вернула ни одного экрана",
        };
        f.write_str(text)
    }
}

/// Как встало одно окно.
#[derive(Clone, Copy, Debug)]
pub struct Placement {
    /// `CGWindowID` (или суррогат из хэша AX-элемента, если система id не дала).
    pub window_id: u32,
    /// Рамка столбика с гэпами, координаты AX (origin сверху слева).
    pub target: Rect,
    /// Фактическая рамка окна после раскладки (AX); `None` — приложение не ответило.
    pub actual: Option<Rect>,
    /// Окно двигали (оно стояло не в своём столбике).
    pub moved: bool,
}

/// Итог раскладки.
#[derive(Clone, Debug)]
pub struct TileReport {
    pub pid: i32,
    /// Рабочая область экрана, поделённая на столбики (AX).
    pub area: Rect,
    /// Окна слева направо.
    pub placements: Vec<Placement>,
    /// Сколько заняла расстановка (обращения к AX).
    pub elapsed: Duration,
}

// ---------------------------------------------------------------- чистая часть

/// Рамки `count` столбиков в рабочей области `visible`, слева направо.
///
/// Геометрия — как у действий «столбик»: границы округлением
/// (`geometry::columns::rect`), гэпы как у `calc::apply_gaps` — полный зазор у краёв
/// области и половинный с каждой стороны стыка, так что между соседями ровно
/// `gap_size`. Координаты Cocoa (y снизу): `skip_gap_top_edge` снимает зазор сверху.
/// `count` больше `MAX_COLUMNS` обрезается, 0 — пусто.
pub fn column_frames(
    visible: &Rect,
    count: usize,
    gap_size: f32,
    skip_gap_top_edge: bool,
) -> Vec<Rect> {
    if count == 0 {
        return Vec::new();
    }
    let count = count.min(MAX_COLUMNS) as u8;
    (1..=count)
        .map(|index| {
            let rect = columns::rect(visible, count, index);
            if gap_size <= 0.0 {
                return rect;
            }
            let mut shared = Edge::NONE;
            if index > 1 {
                shared = shared.with(Edge::LEFT);
            }
            if index < count {
                shared = shared.with(Edge::RIGHT);
            }
            calc::apply_gaps_raw(rect, Dimension::BOTH, shared, gap_size, skip_gap_top_edge)
        })
        .collect()
}

/// Порядок окон слева направо (window id).
///
/// `previous` — порядок прошлой раскладки этого приложения: окна из неё сохраняют
/// взаимный порядок, закрытые выпадают, новые встают справа в порядке создания
/// (window id растёт). Если из прошлой раскладки не осталось ни одного окна (или
/// её нет) — слева направо по левому краю, при равном — выше стоящее раньше.
/// `windows` — рамки в координатах AX (y сверху).
pub fn column_order(previous: Option<&[u32]>, windows: &[(u32, Rect)]) -> Vec<u32> {
    let mut seen = HashSet::new();
    let current: Vec<(u32, Rect)> = windows
        .iter()
        .copied()
        .filter(|(id, _)| seen.insert(*id))
        .collect();

    let mut order = Vec::with_capacity(current.len());
    let mut known = HashSet::new();
    for id in previous.unwrap_or_default() {
        if seen.contains(id) && known.insert(*id) {
            order.push(*id);
        }
    }

    if order.is_empty() {
        let mut sorted = current;
        sorted.sort_by(|(a_id, a), (b_id, b)| {
            a.min_x()
                .total_cmp(&b.min_x())
                .then(a.min_y().total_cmp(&b.min_y()))
                .then(a_id.cmp(b_id))
        });
        return sorted.into_iter().map(|(id, _)| id).collect();
    }

    let mut new: Vec<u32> = current
        .iter()
        .map(|(id, _)| *id)
        .filter(|id| !known.contains(id))
        .collect();
    new.sort_unstable();
    order.extend(new);
    order
}

/// Экран окна: наибольшая площадь пересечения; окно вне всех экранов — экран
/// с ближайшим центром. `None` — экранов нет. Координаты окна и экранов — одни.
pub fn screen_for(window: &Rect, screens: &[Rect]) -> Option<usize> {
    let mut best: Option<(usize, f64)> = None;
    for (index, screen) in screens.iter().enumerate() {
        let area = window
            .intersection(screen)
            .map(|inter| inter.area())
            .unwrap_or(0.0);
        if area > 0.0 && best.is_none_or(|(_, best_area)| area > best_area) {
            best = Some((index, area));
        }
    }
    if let Some((index, _)) = best {
        return Some(index);
    }
    let distance = |screen: &Rect| {
        let (x, y) = window.center();
        let (sx, sy) = screen.center();
        (x - sx).hypot(y - sy)
    };
    screens
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| distance(a).total_cmp(&distance(b)))
        .map(|(index, _)| index)
}

/// Когда перестраивать после события в `now`, если серия событий началась в
/// `first`: через `DEBOUNCE` после последнего, но не позже `MAX_DELAY` от первого.
fn next_due(first: Instant, now: Instant) -> Instant {
    (now + DEBOUNCE).min(first + MAX_DELAY)
}

/// Список приложений режима «держать» после включения/выключения `bundle_id`;
/// пустой список — «не задано».
fn with_bundle(
    list: Option<BTreeSet<String>>,
    bundle_id: &str,
    keep: bool,
) -> Option<BTreeSet<String>> {
    let mut list = list.unwrap_or_default();
    if keep {
        list.insert(bundle_id.to_string());
    } else {
        list.remove(bundle_id);
    }
    (!list.is_empty()).then_some(list)
}

// ---------------------------------------------------------------- состояние

/// Приложение в режиме «держать столбиками».
struct Kept {
    observer: AppObserver,
    /// Подключено по настройке `r2AppColumnsBundleIds` (иначе — через `keep_pid`).
    from_config: bool,
}

/// Отложенная перестройка.
struct Pending {
    first: Instant,
    due: Instant,
    /// Раскладывать, даже если набор окон не поменялся.
    force: bool,
}

#[derive(Default)]
struct State {
    /// Порядок окон прошлой раскладки по приложению (pid), слева направо.
    orders: HashMap<i32, Vec<u32>>,
    /// По window id: куда окно ставили в прошлый раз и где оно в итоге встало (AX).
    placed: HashMap<u32, (Rect, Rect)>,
    kept: HashMap<i32, Kept>,
    pending: HashMap<i32, Pending>,
    /// Приложения из настройки, к которым не удалось подключиться: попытка и когда повторить.
    retries: HashMap<i32, (u8, Instant)>,
    timer: Option<MainTimer>,
    workspace_observer: Option<Retained<WorkspaceObserver>>,
    installed: bool,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

fn on_main_thread() -> bool {
    MainThreadMarker::new().is_some()
}

// ---------------------------------------------------------------- раскладка

/// Окно-кандидат: стандартное, не свёрнутое, не во весь экран.
struct Candidate {
    id: u32,
    element: AxElement,
    /// Рамка AX.
    frame: Rect,
}

/// Всё, что нужно для раскладки.
struct Layout {
    app: AxElement,
    windows: Vec<Candidate>,
    /// Рабочая область, Cocoa.
    area: Rect,
    primary_height: f64,
    config: Config,
    /// Сколько окнам можно уйти за левый и правый край экрана (`spill_limits`).
    max_spill: (f64, f64),
}

/// Суррогатный id окна, если система не дала настоящий: хэш AX-элемента со
/// старшим битом, как `deriveWindowId` в оригинале (настоящие id так велики не бывают).
fn derived_window_id(element: &AxElement) -> u32 {
    let hash = unsafe { core_foundation_sys::base::CFHash(element.as_raw()) };
    0x8000_0000 | (hash as u32 & 0x7FFF_FFFF)
}

/// Собрать окна приложения на экране его фокусного окна.
fn collect(pid: i32) -> Result<Layout, TileError> {
    let all_screens = screens::screens();
    let Some(primary) = all_screens.first() else {
        return Err(TileError::NoScreens);
    };
    let primary_height = primary.frame.max_y();

    let app = AxElement::application(pid);
    app.set_messaging_timeout(AX_TIMEOUT);

    let mut windows = Vec::new();
    let mut seen = HashSet::new();
    for element in app.windows() {
        element.set_messaging_timeout(AX_TIMEOUT);
        if element.role().as_deref() != Some("AXWindow")
            || element.subrole().as_deref() != Some("AXStandardWindow")
            || element.is_minimized()
            || element.bool_attribute_public("AXFullScreen") == Some(true)
        {
            continue;
        }
        let Some(frame) = element.frame() else {
            continue;
        };
        if frame.is_empty() {
            continue;
        }
        let id = element
            .window_id()
            .unwrap_or_else(|| derived_window_id(&element));
        if seen.insert(id) {
            windows.push(Candidate { id, element, frame });
        }
    }
    if windows.is_empty() {
        // Без разрешения AX список окон всегда пуст — скажем об этом прямо.
        return Err(if ax::is_process_trusted(false) {
            TileError::NoWindows
        } else {
            TileError::NoAccess
        });
    }

    // Экран фокусного окна приложения (нет фокусного — главного). Если там нет
    // стандартных окон (в фокусе панель на другом экране) — экран переднего
    // стандартного окна: `AXWindows` идут спереди назад.
    let screen_frames: Vec<Rect> = all_screens.iter().map(|screen| screen.frame).collect();
    let screen_of = |frame: &Rect| {
        screen_for(&frame.screen_flipped(primary_height), &screen_frames).unwrap_or(0)
    };
    let screens_of_windows: Vec<usize> = windows
        .iter()
        .map(|candidate| screen_of(&candidate.frame))
        .collect();
    let focused = ["AXFocusedWindow", "AXMainWindow"]
        .into_iter()
        .filter_map(|attribute| app.element_attribute_public(attribute))
        .find_map(|window| {
            window.set_messaging_timeout(AX_TIMEOUT);
            window.frame()
        })
        .map(|frame| screen_of(&frame));
    let screen_index = focused
        .filter(|index| screens_of_windows.contains(index))
        .unwrap_or(screens_of_windows[0]);
    let windows: Vec<Candidate> = windows
        .into_iter()
        .zip(screens_of_windows)
        .filter(|(_, index)| *index == screen_index)
        .map(|(candidate, _)| candidate)
        .collect();

    let config = config::current();
    // Общая рабочая область (отступы от краёв, «чёлка», Stage Manager, todo), как у действий.
    let area =
        crate::window_manager::adjusted_visible_frame(&all_screens[screen_index], false, false);
    let others: Vec<Rect> = screen_frames
        .iter()
        .enumerate()
        .filter(|(index, _)| *index != screen_index)
        .map(|(_, frame)| *frame)
        .collect();
    let edge_gap = f64::from(config.gap_size.max(0.0));
    let max_spill = spill_limits(&screen_frames[screen_index], &others, edge_gap);
    Ok(Layout {
        app,
        windows,
        area,
        primary_height,
        config,
        max_spill,
    })
}

/// Окно в раскладке: прочитать рамку, передвинуть, поменять размер (AX; в тестах —
/// макет с сеткой символов и ограничениями AppKit).
trait ColumnWindow {
    /// Рамка, координаты AX; `None` — приложение не ответило.
    fn frame(&self) -> Option<Rect>;
    fn set_position(&self, x: f64, y: f64);
    fn set_size(&self, w: f64, h: f64);
}

impl ColumnWindow for AxElement {
    fn frame(&self) -> Option<Rect> {
        AxElement::frame(self)
    }
    fn set_position(&self, x: f64, y: f64) {
        AxElement::set_position(self, x, y);
    }
    fn set_size(&self, w: f64, h: f64) {
        AxElement::set_size(self, w, h);
    }
}

/// Рамка как `AccessibilityElement.setFrame`: размер (если `adjust_size_first`),
/// положение, размер. `AXEnhancedUserInterface` на время раскладки выключен у
/// всего приложения (`apply`).
fn set_frame<W: ColumnWindow>(window: &W, rect: &Rect, adjust_size_first: bool) {
    if adjust_size_first {
        window.set_size(rect.w, rect.h);
    }
    window.set_position(rect.x, rect.y);
    window.set_size(rect.w, rect.h);
}

/// Поставить окно в столбик `target` (AX). Приложение вправе поменять размер
/// (сетка символов, минимальная ширина): окно остаётся у левого верхнего угла
/// своего столбика и не вылезает из него, пока это возможно; окно, которое не
/// ужимается до столбика, ставится как получится (и не за правый край области).
/// Щели от сетки потом закрывает `pack`.
fn place<W: ColumnWindow>(
    window: &W,
    id: u32,
    current: Rect,
    target: Rect,
    area: &Rect,
    remembered: Option<(Rect, Rect)>,
) -> Placement {
    // Уже стоит: ровно в столбике или там же, где встало после прошлой раскладки
    // в тот же столбик (ужатое приложением окно не дёргаем заново).
    let unchanged = current.is_close(&target, TOLERANCE)
        || remembered.is_some_and(|(last_target, last_actual)| {
            last_target.is_close(&target, TOLERANCE) && current.is_close(&last_actual, TOLERANCE)
        });
    if unchanged {
        return Placement {
            window_id: id,
            target,
            actual: Some(current),
            moved: false,
        };
    }

    set_frame(window, &target, true);
    let mut actual = window.frame();
    if let Some(frame) = actual {
        let (frame, request) = fit_size(window, &target, frame);
        let mut x = target.x;
        if x + frame.w > area.max_x() {
            x = (area.max_x() - frame.w).max(area.min_x());
        }
        if (frame.x - x).abs() > TOLERANCE || (frame.y - target.y).abs() > TOLERANCE {
            set_frame(window, &Rect::new(x, target.y, request.0, request.1), false);
            actual = window.frame();
        } else {
            actual = Some(frame);
        }
        if frame.w > target.w + TOLERANCE || frame.h > target.h + TOLERANCE {
            eprintln!(
                "rectangle2rust: столбики: окно {id} не ужимается до столбика {:.0}×{:.0} (стоит {:.0}×{:.0})",
                target.w, target.h, frame.w, frame.h
            );
        }
    }
    Placement {
        window_id: id,
        target,
        actual,
        moved: true,
    }
}

/// Сколько раз просить размер поменьше (шаг растёт вдвое: 1, 2, 4, 8, 16 перелётов).
const FIT_ATTEMPTS: u32 = 5;
/// Шаг сетки у приложений меньше этого (пиксели): окно не ужалось, хотя просили
/// меньше столбика на столько, — значит, упёрлись в минимальный размер.
const MAX_GRID_STEP: f64 = 32.0;
/// Сколько раз просить окно шире в поисках шага сетки (+1, +2, +4 … +64).
const PROBE_STEPS: i32 = 7;

/// Приложение сделало окно больше столбика: округлило размер вверх до своей сетки
/// (у сетки символов — «к ближайшему») или не даёт ужать (минимальный размер).
/// Просим размер меньше с растущим шагом, пока окно не влезет: при округлении
/// первый же запрос ниже середины шага сетки даёт ближайший к столбику размер
/// внутри него. Окно не ужимается — оставляем как есть. Возвращает рамку окна и
/// размер, который для неё просили.
fn fit_size<W: ColumnWindow>(window: &W, target: &Rect, mut frame: Rect) -> (Rect, (f64, f64)) {
    let mut request = (target.w, target.h);
    let first_over = ((frame.w - target.w).max(1.0), (frame.h - target.h).max(1.0));
    let mut factor = 1.0;
    for _ in 0..FIT_ATTEMPTS {
        let too_wide = frame.w > target.w + TOLERANCE;
        let too_tall = frame.h > target.h + TOLERANCE;
        if !too_wide && !too_tall {
            break;
        }
        if too_wide {
            request.0 = (target.w - first_over.0 * factor).max(1.0);
        }
        if too_tall {
            request.1 = (target.h - first_over.1 * factor).max(1.0);
        }
        factor *= 2.0;
        let previous = frame;
        set_frame(
            window,
            &Rect::new(target.x, target.y, request.0, request.1),
            false,
        );
        match window.frame() {
            Some(next) => frame = next,
            None => break,
        }
        let stuck = |too_big: bool, before: f64, after: f64, target: f64, request: f64| {
            !too_big || (after >= before - TOLERANCE && target - request >= MAX_GRID_STEP)
        };
        if stuck(too_wide, previous.w, frame.w, target.w, request.0)
            && stuck(too_tall, previous.h, frame.h, target.h, request.1)
        {
            break;
        }
    }
    (frame, request)
}

/// `k` номеров из `count`, разбросанных поровну (окна, которым дать шаг сетки шире).
pub fn spread_indices(count: usize, k: usize) -> Vec<usize> {
    let k = k.min(count);
    (0..k).map(|j| (2 * j + 1) * count / (2 * k)).collect()
}

/// Где встать окнам шириной `widths` вплотную друг к другу (между соседями ровно
/// `gap`) в полосе `span` (левый и правый край, AX). Сумма шире полосы (окна на шаг
/// сетки шире столбика) — лишнее уходит за края поровну, но не дальше `max_spill`
/// с каждой стороны (у края с соседним монитором — только в краевой зазор), а что
/// не ушло — нахлёстом на стыках. Сумма уже полосы — вплотную от левого края.
pub fn pack_positions(
    widths: &[f64],
    span: (f64, f64),
    gap: f64,
    max_spill: (f64, f64),
) -> Vec<f64> {
    let count = widths.len();
    if count == 0 {
        return Vec::new();
    }
    let total = widths.iter().sum::<f64>() + gap * (count - 1) as f64;
    let over = total - (span.1 - span.0);
    let (left, overlap) = if over <= 0.0 {
        (0.0, 0.0)
    } else if count == 1 {
        ((over / 2.0).floor(), 0.0)
    } else {
        let mut left = (over / 2.0).floor().min(max_spill.0);
        let right = (over - left).min(max_spill.1);
        left = (over - right).min(max_spill.0);
        (left, over - left - right)
    };
    let boundaries = count.saturating_sub(1).max(1) as f64;
    let base = (overlap / boundaries).floor();
    let mut extra = overlap - base * boundaries;
    let mut x = span.0 - left;
    let mut xs = Vec::with_capacity(count);
    for width in widths {
        xs.push(x);
        let mut step_back = base;
        if extra > 0.0 {
            let one = extra.min(1.0);
            step_back += one;
            extra -= one;
        }
        x += width + gap - step_back;
    }
    xs
}

/// Шаг сетки окна по ширине: просим окно шире на 1, 2, 4 … 64 там, где растянуться
/// есть куда, пока оно не станет шире. `None` — шире не становится.
fn grid_step<W: ColumnWindow>(window: &W, current: &Rect, area: &Rect) -> Option<f64> {
    let room = 2f64.powi(PROBE_STEPS - 1);
    let x = current
        .x
        .min(area.max_x() - current.w - room)
        .max(area.min_x());
    window.set_position(x, current.y);
    let mut extra = 1.0;
    for _ in 0..PROBE_STEPS {
        window.set_size(current.w + extra, current.h);
        let got = window.frame()?;
        if got.w > current.w + TOLERANCE {
            return Some(got.w - current.w);
        }
        extra *= 2.0;
    }
    None
}

/// Поставить рамку, в том числе заходящую за край рабочей области: растянуть окно
/// за край AppKit не даёт (размер режется), а сдвинуть — даёт. Поэтому размер
/// задаётся там, где окно влезает, и только потом окно встаёт на место.
fn put<W: ColumnWindow>(window: &W, frame: &Rect, area: &Rect) -> Option<Rect> {
    let sizing_x = frame.x.min(area.max_x() - frame.w).max(area.min_x());
    window.set_position(sizing_x, frame.y);
    window.set_size(frame.w, frame.h);
    if (sizing_x - frame.x).abs() > TOLERANCE {
        window.set_position(frame.x, frame.y);
    }
    window.frame()
}

/// Вплотную, без щелей. Приложения с сеткой (Терминал — целые символы) берут
/// ширину чуть меньше столбика, и между окнами остаются щели. Окна встают вплотную
/// друг к другу; нехватку закрывают окна на шаг сетки шире (разбросанные поровну),
/// а лишнее — меньше шага — уходит за края экрана (где нет соседнего монитора) или
/// нахлёстом на стыках. Высоту не трогаем: окна прижаты к верху (решение Алексея).
/// Окна, вставшие ровно, и окна с минимальным размером больше столбика — как были.
fn pack<W: ColumnWindow>(
    windows: &[&W],
    mut placements: Vec<Placement>,
    area: &Rect,
    max_spill: (f64, f64),
) -> Vec<Placement> {
    let count = placements.len();
    if count == 0 || windows.len() != count {
        return placements;
    }
    let Some(actuals) = placements
        .iter()
        .map(|placement| placement.actual)
        .collect::<Option<Vec<Rect>>>()
    else {
        return placements;
    };
    let fits = |placement: &Placement, actual: &Rect| {
        (actual.x - placement.target.x).abs() <= TOLERANCE
            && (actual.w - placement.target.w).abs() <= TOLERANCE
    };
    if placements.iter().zip(&actuals).all(|(p, a)| fits(p, a)) {
        return placements;
    }
    let grid_like = placements
        .iter()
        .zip(&actuals)
        .all(|(p, a)| a.w <= p.target.w + MAX_GRID_STEP && p.target.w - a.w < MAX_GRID_STEP);
    if !grid_like {
        return placements;
    }

    let span = (
        placements[0].target.min_x(),
        placements[count - 1].target.max_x(),
    );
    let gap = if count > 1 {
        placements[1].target.min_x() - placements[0].target.max_x()
    } else {
        0.0
    };
    let mut widths: Vec<f64> = actuals.iter().map(|actual| actual.w).collect();
    let deficit = (span.1 - span.0) - (widths.iter().sum::<f64>() + gap * (count - 1) as f64);
    let mut probed = false;
    if deficit > TOLERANCE {
        probed = true;
        if let Some(step) = grid_step(windows[0], &actuals[0], area) {
            let enlarge = (deficit / step).ceil() as usize;
            for index in spread_indices(count, enlarge) {
                widths[index] += step;
            }
        }
    }

    let xs = pack_positions(&widths, span, gap, max_spill);
    for index in 0..count {
        let frame = Rect::new(xs[index], actuals[index].y, widths[index], actuals[index].h);
        if !(index == 0 && probed) && actuals[index].is_close(&frame, TOLERANCE) {
            continue;
        }
        placements[index].actual = put(windows[index], &frame, area);
        placements[index].moved = true;
    }

    // Приложение не приняло ширину — следующие встают вплотную к тому, что вышло.
    let got: Option<Vec<Rect>> = placements.iter().map(|p| p.actual).collect();
    if let Some(got) = got {
        if got
            .iter()
            .zip(&widths)
            .any(|(g, w)| (g.w - w).abs() > TOLERANCE)
        {
            let real: Vec<f64> = got.iter().map(|g| g.w).collect();
            let xs = pack_positions(&real, span, gap, max_spill);
            for index in 0..count {
                if (got[index].x - xs[index]).abs() > TOLERANCE {
                    windows[index].set_position(xs[index], got[index].y);
                    placements[index].actual = windows[index].frame();
                }
            }
        }
    }
    placements
}

/// Сколько окнам можно уйти за левый и правый край экрана `screen` (Cocoa): за
/// край, у которого стоит другой монитор, — только в краевой зазор `edge_gap`,
/// иначе — сколько угодно (за краем экрана ничего не видно).
pub fn spill_limits(screen: &Rect, others: &[Rect], edge_gap: f64) -> (f64, f64) {
    let overlaps_vertically =
        |other: &Rect| other.min_y() < screen.max_y() && other.max_y() > screen.min_y();
    let left = others
        .iter()
        .any(|other| overlaps_vertically(other) && (other.max_x() - screen.min_x()).abs() < 1.0);
    let right = others
        .iter()
        .any(|other| overlaps_vertically(other) && (other.min_x() - screen.max_x()).abs() < 1.0);
    let limit = |neighbour: bool| if neighbour { edge_gap } else { f64::INFINITY };
    (limit(left), limit(right))
}

/// Разложить собранные окна и запомнить порядок.
fn apply(pid: i32, layout: Layout) -> TileReport {
    let started = Instant::now();
    let previous = STATE.with(|state| state.borrow().orders.get(&pid).cloned());
    let frames: Vec<(u32, Rect)> = layout
        .windows
        .iter()
        .map(|candidate| (candidate.id, candidate.frame))
        .collect();
    let order = column_order(previous.as_deref(), &frames);

    let mut windows = layout.windows;
    windows.sort_by_key(|candidate| {
        order
            .iter()
            .position(|id| *id == candidate.id)
            .unwrap_or(usize::MAX)
    });
    if windows.len() > MAX_COLUMNS {
        eprintln!(
            "rectangle2rust: столбики: окон {} — больше {MAX_COLUMNS} столбиков не бывает, остальные не трогаю",
            windows.len()
        );
        windows.truncate(MAX_COLUMNS);
    }
    let config = &layout.config;
    let targets = column_frames(
        &layout.area,
        windows.len(),
        config.gap_size,
        config.skip_gap_top_edge,
    );
    let area = layout.area.screen_flipped(layout.primary_height);

    // AXEnhancedUserInterface мешает ресайзу — выключаем на время раскладки, как оригинал.
    let enhanced = layout.app.enhanced_ui() == Some(true);
    if enhanced {
        layout.app.set_enhanced_ui(false);
    }

    let remembered: Vec<Option<(Rect, Rect)>> = STATE.with(|state| {
        let state = state.borrow();
        windows
            .iter()
            .map(|candidate| state.placed.get(&candidate.id).copied())
            .collect()
    });
    let placements: Vec<Placement> = windows
        .iter()
        .zip(&targets)
        .zip(remembered)
        .map(|((candidate, target), remembered)| {
            place(
                &candidate.element,
                candidate.id,
                candidate.frame,
                target.screen_flipped(layout.primary_height),
                &area,
                remembered,
            )
        })
        .collect();
    let elements: Vec<&AxElement> = windows.iter().map(|candidate| &candidate.element).collect();
    let placements = pack(&elements, placements, &area, layout.max_spill);

    if enhanced && config.enhanced_ui == EnhancedUI::DisableEnable {
        layout.app.set_enhanced_ui(true);
    }

    STATE.with(|state| {
        let mut state = state.borrow_mut();
        let ids: Vec<u32> = placements
            .iter()
            .map(|placement| placement.window_id)
            .collect();
        if let Some(old) = state.orders.insert(pid, ids.clone()) {
            for id in old.into_iter().filter(|id| !ids.contains(id)) {
                state.placed.remove(&id);
            }
        }
        for placement in &placements {
            if let Some(actual) = placement.actual {
                state
                    .placed
                    .insert(placement.window_id, (placement.target, actual));
            }
        }
    });

    TileReport {
        pid,
        area,
        placements,
        elapsed: started.elapsed(),
    }
}

/// Разложить окна приложения `pid` столбиками (разово), на экране его фокусного
/// окна. Главный поток.
pub fn tile_app(pid: i32) -> Result<TileReport, TileError> {
    let layout = collect(pid)?;
    Ok(apply(pid, layout))
}

/// Пункт меню «Окна приложения столбиками»: окна активного приложения. Если не
/// вышло — системный сигнал, как у остальных действий меню.
pub fn tile_frontmost_app() -> Result<TileReport, TileError> {
    let pid = NSWorkspace::sharedWorkspace()
        .frontmostApplication()
        .map(|app| app.processIdentifier());
    let result = match pid {
        Some(pid) => tile_app(pid),
        None => Err(TileError::NoApp),
    };
    if let Err(error) = &result {
        eprintln!("rectangle2rust: столбики: {error}");
        unsafe { NSBeep() };
    }
    result
}

// ---------------------------------------------------------------- режим «держать»

/// Приложение с этим bundle id держим столбиками (настройка `r2AppColumnsBundleIds`).
pub fn is_keeping(bundle_id: &str) -> bool {
    config::with(|config| {
        config
            .app_columns_bundle_ids
            .as_ref()
            .is_some_and(|list| list.contains(bundle_id))
    })
}

/// Включить или выключить режим «держать столбиками» для приложения: сохраняет
/// настройку, подключает или снимает наблюдение за окнами всех его запущенных
/// копий; при включении сразу раскладывает. Главный поток.
pub fn set_keeping(bundle_id: &str, keep: bool) {
    if bundle_id.is_empty() {
        return;
    }
    config::update(|config| {
        config.app_columns_bundle_ids =
            with_bundle(config.app_columns_bundle_ids.take(), bundle_id, keep);
    });
    if !on_main_thread() {
        eprintln!("rectangle2rust: столбики: режим «держать» переключают с главного потока");
        return;
    }
    sync_with_config();
    if keep {
        for pid in pids_for_bundle(bundle_id) {
            cancel_pending(pid);
            if let Err(error) = tile_app(pid) {
                eprintln!("rectangle2rust: столбики: {bundle_id}: {error}");
            }
        }
    }
}

/// Пункт меню «Держать окна «…» столбиками»: переключить режим для активного
/// приложения. Новое состояние; `None` — у приложения нет bundle id.
pub fn toggle_keeping_frontmost() -> Option<bool> {
    let bundle_id = ax::frontmost_bundle_id()?;
    let keep = !is_keeping(&bundle_id);
    set_keeping(&bundle_id, keep);
    Some(keep)
}

/// Для меню: подпись пункта «Держать окна «<имя>» столбиками» для активного
/// приложения и стоит ли галочка. `None` — у приложения нет bundle id или имени
/// (пункт прячем).
pub fn keep_menu_state() -> Option<(String, bool)> {
    let bundle_id = ax::frontmost_bundle_id()?;
    let name = ax::frontmost_app_name();
    if name.is_empty() {
        return None;
    }
    Some((keep_menu_title(&name), is_keeping(&bundle_id)))
}

/// Держать окна процесса `pid` столбиками, не трогая настройку (проверки,
/// приложения без bundle id). При включении сразу раскладывает. Возвращает,
/// удалось ли подписаться на окна. Главный поток.
pub fn keep_pid(pid: i32, keep: bool) -> bool {
    if !keep {
        detach(pid);
        return true;
    }
    if !attach(pid, false) {
        return false;
    }
    cancel_pending(pid);
    if let Err(error) = tile_app(pid) {
        eprintln!("rectangle2rust: столбики: pid {pid}: {error}");
    }
    true
}

/// pid приложений, за окнами которых сейчас следим.
pub fn kept_pids() -> Vec<i32> {
    let mut pids: Vec<i32> = STATE.with(|state| state.borrow().kept.keys().copied().collect());
    pids.sort_unstable();
    pids
}

/// Запуск: подключить наблюдение к уже запущенным приложениям из настройки и
/// следить за запуском и завершением приложений (NSWorkspace) и за самой
/// настройкой. Вызывать один раз, на главном потоке, после загрузки настроек.
pub fn install() {
    if !on_main_thread() {
        eprintln!("rectangle2rust: столбики: install() — только с главного потока");
        return;
    }
    let first = STATE.with(|state| !std::mem::replace(&mut state.borrow_mut().installed, true));
    if !first {
        return;
    }

    let observer = WorkspaceObserver::new();
    let center = NSWorkspace::sharedWorkspace().notificationCenter();
    let target: &AnyObject = &observer;
    unsafe {
        center.addObserver_selector_name_object(
            target,
            sel!(applicationLaunched:),
            Some(NSWorkspaceDidLaunchApplicationNotification),
            None,
        );
        center.addObserver_selector_name_object(
            target,
            sel!(applicationTerminated:),
            Some(NSWorkspaceDidTerminateApplicationNotification),
            None,
        );
    }
    STATE.with(|state| state.borrow_mut().workspace_observer = Some(observer));

    config::subscribe(Box::new(|old, new| {
        if old.app_columns_bundle_ids != new.app_columns_bundle_ids {
            sync_with_config();
        }
    }));
    sync_with_config();
}

define_class!(
    /// Следит за запуском и завершением приложений (уведомления NSWorkspace).
    #[unsafe(super(NSObject))]
    #[name = "R2AppColumnsWorkspaceObserver"]
    #[ivars = ()]
    struct WorkspaceObserver;

    impl WorkspaceObserver {
        #[unsafe(method(applicationLaunched:))]
        fn application_launched(&self, _notification: &NSNotification) {
            sync_with_config();
        }

        #[unsafe(method(applicationTerminated:))]
        fn application_terminated(&self, _notification: &NSNotification) {
            prune_terminated();
        }
    }
);

impl WorkspaceObserver {
    fn new() -> Retained<Self> {
        let this = Self::alloc();
        unsafe { msg_send![this, init] }
    }
}

/// Запущенные приложения: pid и bundle id.
fn running_apps() -> Vec<(i32, Option<String>)> {
    NSWorkspace::sharedWorkspace()
        .runningApplications()
        .iter()
        .filter(|app| !app.isTerminated())
        .map(|app| {
            (
                app.processIdentifier(),
                app.bundleIdentifier().map(|id| id.to_string()),
            )
        })
        .collect()
}

fn pids_for_bundle(bundle_id: &str) -> Vec<i32> {
    running_apps()
        .into_iter()
        .filter(|(_, id)| id.as_deref() == Some(bundle_id))
        .map(|(pid, _)| pid)
        .collect()
}

fn is_running(pid: i32) -> bool {
    NSRunningApplication::runningApplicationWithProcessIdentifier(pid)
        .is_some_and(|app| !app.isTerminated())
}

/// Подписаться на окна приложения (если ещё не подписаны).
fn attach(pid: i32, from_config: bool) -> bool {
    let attached = STATE.with(|state| {
        let mut state = state.borrow_mut();
        match state.kept.get_mut(&pid) {
            Some(kept) => {
                kept.from_config |= from_config;
                true
            }
            None => false,
        }
    });
    if attached {
        return true;
    }
    match AppObserver::attach(pid, on_window_event) {
        Ok(observer) => {
            STATE.with(|state| {
                state.borrow_mut().kept.insert(
                    pid,
                    Kept {
                        observer,
                        from_config,
                    },
                )
            });
            true
        }
        Err(error) => {
            eprintln!("rectangle2rust: столбики: не подписаться на окна pid {pid}: {error}");
            false
        }
    }
}

/// Снять наблюдение за окнами приложения.
fn detach(pid: i32) {
    let kept = STATE.with(|state| {
        let mut state = state.borrow_mut();
        state.pending.remove(&pid);
        state.retries.remove(&pid);
        state.kept.remove(&pid)
    });
    // Подписки снимаются через AX — уже вне заёма состояния.
    drop(kept);
}

/// Забыть порядок окон завершившегося приложения.
fn forget(pid: i32) {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        if let Some(order) = state.orders.remove(&pid) {
            for id in order {
                state.placed.remove(&id);
            }
        }
    });
}

/// Привести наблюдение в соответствие с настройкой и запущенными приложениями.
fn sync_with_config() {
    if !on_main_thread() {
        return;
    }
    let wanted = config::with(|config| config.app_columns_bundle_ids.clone().unwrap_or_default());
    let desired: BTreeSet<i32> = running_apps()
        .into_iter()
        .filter(|(_, id)| id.as_ref().is_some_and(|id| wanted.contains(id)))
        .map(|(pid, _)| pid)
        .collect();

    let stale: Vec<i32> = STATE.with(|state| {
        let mut state = state.borrow_mut();
        state.retries.retain(|pid, _| desired.contains(pid));
        state
            .kept
            .iter()
            .filter(|(pid, kept)| kept.from_config && !desired.contains(pid))
            .map(|(pid, _)| *pid)
            .collect()
    });
    for pid in stale {
        detach(pid);
    }

    for pid in desired {
        let waiting = STATE.with(|state| state.borrow().retries.contains_key(&pid));
        let known = STATE.with(|state| state.borrow().kept.contains_key(&pid));
        if waiting {
            continue;
        }
        if attach(pid, true) {
            if !known {
                // Первая раскладка — после паузы: у только что запущенного
                // приложения окна ещё появляются.
                schedule(pid, true);
            }
        } else {
            STATE.with(|state| {
                state
                    .borrow_mut()
                    .retries
                    .insert(pid, (1, Instant::now() + ATTACH_RETRY_DELAY))
            });
        }
    }
    arm_timer();
}

/// Закрылось какое-то приложение: снять наблюдение за умершими процессами.
fn prune_terminated() {
    let pids: BTreeSet<i32> = STATE.with(|state| {
        let state = state.borrow();
        state
            .kept
            .keys()
            .chain(state.orders.keys())
            .chain(state.pending.keys())
            .chain(state.retries.keys())
            .copied()
            .collect()
    });
    for pid in pids {
        if !is_running(pid) {
            detach(pid);
            forget(pid);
        }
    }
    arm_timer();
}

/// Событие AX об окнах приложения: перестроить после паузы.
fn on_window_event(pid: i32, _event: WindowEvent) {
    schedule(pid, false);
}

fn schedule(pid: i32, force: bool) {
    let now = Instant::now();
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        let pending = state.pending.entry(pid).or_insert(Pending {
            first: now,
            due: now,
            force: false,
        });
        pending.force |= force;
        pending.due = next_due(pending.first, now);
    });
    arm_timer();
}

fn cancel_pending(pid: i32) {
    STATE.with(|state| state.borrow_mut().pending.remove(&pid));
    arm_timer();
}

/// Завести таймер на ближайшее дело (перестройку или повтор подписки).
fn arm_timer() {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        let next = state
            .pending
            .values()
            .map(|pending| pending.due)
            .chain(state.retries.values().map(|(_, due)| *due))
            .min();
        match next {
            Some(due) => {
                let timer = state.timer.get_or_insert_with(|| MainTimer::new(on_timer));
                timer.fire_in(due.saturating_duration_since(Instant::now()));
            }
            None => {
                if let Some(timer) = &state.timer {
                    timer.cancel();
                }
            }
        }
    });
}

/// Таймер: повторить подписки и перестроить всё, чему подошёл срок.
fn on_timer() {
    // Небольшой запас: таймер срабатывает с точностью до долей миллисекунды.
    let now = Instant::now() + Duration::from_millis(2);
    let (due, retries) = STATE.with(|state| {
        let mut state = state.borrow_mut();
        let due: Vec<(i32, bool)> = state
            .pending
            .iter()
            .filter(|(_, pending)| pending.due <= now)
            .map(|(pid, pending)| (*pid, pending.force))
            .collect();
        for (pid, _) in &due {
            state.pending.remove(pid);
        }
        let retries: Vec<(i32, u8)> = state
            .retries
            .iter()
            .filter(|(_, (_, when))| *when <= now)
            .map(|(pid, (attempt, _))| (*pid, *attempt))
            .collect();
        for (pid, _) in &retries {
            state.retries.remove(pid);
        }
        (due, retries)
    });

    for (pid, attempt) in retries {
        if !is_running(pid) {
            continue;
        }
        if attach(pid, true) {
            schedule(pid, true);
        } else if attempt < ATTACH_ATTEMPTS {
            STATE.with(|state| {
                state
                    .borrow_mut()
                    .retries
                    .insert(pid, (attempt + 1, Instant::now() + ATTACH_RETRY_DELAY))
            });
        } else {
            eprintln!(
                "rectangle2rust: столбики: pid {pid} так и не ответил, наблюдение не подключено"
            );
        }
    }
    for (pid, force) in due {
        retile(pid, force);
    }
    arm_timer();
}

/// Перестроить окна приложения из режима «держать», если поменялся их набор
/// (или раскладку просят принудительно).
fn retile(pid: i32, force: bool) {
    let observed = STATE.with(|state| {
        state
            .borrow()
            .kept
            .get(&pid)
            .map(|kept| kept.observer.refresh_windows())
    });
    if observed.is_none() {
        return;
    }
    let layout = match collect(pid) {
        Ok(layout) => layout,
        // Все окна закрыты или свёрнуты — раскладывать нечего.
        Err(TileError::NoWindows) => return,
        Err(error) => {
            eprintln!("rectangle2rust: столбики: pid {pid}: {error}");
            return;
        }
    };
    if !force {
        let unchanged = STATE.with(|state| {
            state.borrow().orders.get(&pid).is_some_and(|order| {
                order.len() == layout.windows.len()
                    && layout
                        .windows
                        .iter()
                        .all(|candidate| order.contains(&candidate.id))
            })
        });
        if unchanged {
            return;
        }
    }
    apply(pid, layout);
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::cell::Cell;

    /// Макет окна Терминала под AppKit: рамка = 13 + k·7 по ширине и 14 + k·14 по
    /// высоте (сетка вниз, как `windowWillResize:toSize:`), растянуть за правый край
    /// рабочей области и ниже неё минус точку нельзя, выше неё — не встать.
    struct FakeTerminal {
        frame: Cell<Rect>,
        area: Rect,
    }

    impl ColumnWindow for FakeTerminal {
        fn frame(&self) -> Option<Rect> {
            Some(self.frame.get())
        }
        fn set_position(&self, x: f64, y: f64) {
            let mut frame = self.frame.get();
            frame.x = x;
            frame.y = y.max(self.area.min_y());
            self.frame.set(frame);
        }
        fn set_size(&self, w: f64, h: f64) {
            let frame = self.frame.get();
            let w = w.min(self.area.max_x() - frame.x);
            let h = h.min(self.area.max_y() - 1.0 - frame.y);
            let snap = |value: f64, step: f64, base: f64| {
                base + ((value - base) / step).floor().max(1.0) * step
            };
            self.frame.set(Rect::new(
                frame.x,
                frame.y,
                snap(w, 7.0, 13.0),
                snap(h, 14.0, 14.0),
            ));
        }
    }

    /// Разложить `count` макетов, как `apply`: столбик, потом вплотную.
    fn tile_fakes(count: usize, gap: f32, max_spill: (f64, f64)) -> Vec<Rect> {
        // Экран 1728×1117, строка меню 33, Док 83: рабочая область AX 0,33 1728×1001.
        let cocoa = Rect::new(0.0, 83.0, 1728.0, 1001.0);
        let area = cocoa.screen_flipped(1117.0);
        let windows: Vec<FakeTerminal> = (0..count)
            .map(|index| FakeTerminal {
                frame: Cell::new(Rect::new(100.0 + 20.0 * index as f64, 200.0, 520.0, 392.0)),
                area,
            })
            .collect();
        let targets = column_frames(&cocoa, count, gap, false);
        let placements: Vec<Placement> = windows
            .iter()
            .zip(&targets)
            .enumerate()
            .map(|(index, (window, target))| {
                place(
                    window,
                    index as u32,
                    window.frame.get(),
                    target.screen_flipped(1117.0),
                    &area,
                    None,
                )
            })
            .collect();
        let refs: Vec<&FakeTerminal> = windows.iter().collect();
        pack(&refs, placements, &area, max_spill)
            .into_iter()
            .map(|placement| placement.actual.unwrap())
            .collect()
    }

    #[test]
    fn terminal_columns_have_no_gaps_for_any_count() {
        for count in 1..=12 {
            let frames = tile_fakes(count, 0.0, (f64::INFINITY, f64::INFINITY));
            for pair in frames.windows(2) {
                assert_eq!(
                    pair[0].max_x(),
                    pair[1].min_x(),
                    "{count} окон: щель или нахлёст"
                );
            }
            let left = frames[0].min_x();
            let right = frames[count - 1].max_x();
            assert!(
                left <= 0.0 && right >= 1728.0,
                "{count} окон: {left}…{right}"
            );
            // За краем — меньше символа, поровну.
            assert!(
                -left < 7.0 && right - 1728.0 < 7.0,
                "{count} окон: {left}…{right}"
            );
            for frame in &frames {
                assert_eq!((frame.y, frame.h), (33.0, 994.0), "прижаты к строке меню");
                assert_eq!((frame.w - 13.0) % 7.0, 0.0, "ширина по сетке");
            }
        }
    }

    #[test]
    fn neighbour_monitor_side_takes_no_spill() {
        for count in 2..=8 {
            let frames = tile_fakes(count, 0.0, (f64::INFINITY, 0.0));
            assert!(
                frames[count - 1].max_x() <= 1728.0,
                "{count}: за край к соседу"
            );
            for pair in frames.windows(2) {
                assert_eq!(pair[0].max_x(), pair[1].min_x());
            }
            let frames = tile_fakes(count, 0.0, (0.0, 0.0));
            assert_eq!(frames[0].min_x(), 0.0);
            assert!(frames[count - 1].max_x() >= 1727.0 && frames[count - 1].max_x() <= 1728.0);
            for pair in frames.windows(2) {
                // Щелей нет; нахлёст — меньше символа на все стыки, поровну.
                let limit = (6.0 / (count - 1) as f64).ceil();
                let overlap = pair[0].max_x() - pair[1].min_x();
                assert!(
                    (0.0..=limit).contains(&overlap),
                    "{count} окон: нахлёст {overlap}"
                );
            }
        }
    }

    #[test]
    fn gaps_between_terminals_are_exact() {
        for count in 2..=8 {
            let frames = tile_fakes(count, 10.0, (10.0, 10.0));
            for pair in frames.windows(2) {
                assert_eq!(pair[1].min_x() - pair[0].max_x(), 10.0, "{count} окон");
            }
            assert!(frames[0].min_x() >= 0.0 && frames[count - 1].max_x() <= 1728.0);
        }
    }

    #[test]
    fn pack_positions_spill_and_overlap() {
        let wide = [433.0; 4];
        let inf = f64::INFINITY;
        assert_eq!(
            pack_positions(&wide, (0.0, 1728.0), 0.0, (inf, inf)),
            vec![-2.0, 431.0, 864.0, 1297.0]
        );
        assert_eq!(
            pack_positions(&wide, (0.0, 1728.0), 0.0, (inf, 0.0)),
            vec![-4.0, 429.0, 862.0, 1295.0]
        );
        assert_eq!(
            pack_positions(&wide, (0.0, 1728.0), 0.0, (0.0, 0.0)),
            vec![0.0, 431.0, 863.0, 1295.0]
        );
        assert_eq!(
            pack_positions(&[426.0; 4], (0.0, 1728.0), 0.0, (inf, inf)),
            vec![0.0, 426.0, 852.0, 1278.0]
        );
        assert_eq!(spread_indices(7, 3), vec![1, 3, 5]);
        assert_eq!(spread_indices(4, 9), vec![0, 1, 2, 3]);
    }

    #[test]
    fn spill_is_limited_only_towards_neighbour_screens() {
        let screen = Rect::new(0.0, 0.0, 1728.0, 1117.0);
        let right = Rect::new(1728.0, 0.0, 1920.0, 1080.0);
        assert_eq!(
            spill_limits(&screen, &[], 0.0),
            (f64::INFINITY, f64::INFINITY)
        );
        assert_eq!(spill_limits(&screen, &[right], 5.0), (f64::INFINITY, 5.0));
        let above = Rect::new(1728.0, 1117.0, 1920.0, 1080.0);
        assert_eq!(spill_limits(&screen, &[above], 0.0).1, f64::INFINITY);
    }
    use crate::defaults_store::{export_json, MemoryStore, Store};

    const EPS: f64 = 1e-9;

    /// Рабочие области: MacBook (Dock снизу), маленький экран, внешний справа,
    /// экран слева с отрицательными координатами, нецелая ширина.
    fn areas() -> Vec<Rect> {
        vec![
            Rect::new(0.0, 83.0, 1728.0, 1001.0),
            Rect::new(0.0, 0.0, 1440.0, 875.0),
            Rect::new(1728.0, -200.0, 2560.0, 1415.0),
            Rect::new(-1920.0, 0.0, 1920.0, 1050.0),
            Rect::new(10.0, 20.0, 999.5, 700.0),
        ]
    }

    #[test]
    fn columns_fill_area_without_gaps_for_1_to_16() {
        for area in areas() {
            for count in 1..=16usize {
                let frames = column_frames(&area, count, 0.0, false);
                assert_eq!(frames.len(), count);
                let label = format!("область {area:?}, столбиков {count}");
                assert!((frames[0].x - area.x).abs() < EPS, "{label}: левый край");
                for pair in frames.windows(2) {
                    assert!(
                        (pair[1].x - pair[0].max_x()).abs() < EPS,
                        "{label}: щель или нахлёст между {:?} и {:?}",
                        pair[0],
                        pair[1]
                    );
                }
                let last = frames.last().unwrap();
                assert!(
                    (last.max_x() - area.max_x()).abs() < EPS,
                    "{label}: правый край"
                );
                let total: f64 = frames.iter().map(|frame| frame.w).sum();
                assert!((total - area.w).abs() < EPS, "{label}: сумма ширин {total}");
                let min = frames.iter().map(|frame| frame.w).fold(f64::MAX, f64::min);
                let max = frames.iter().map(|frame| frame.w).fold(f64::MIN, f64::max);
                assert!(max - min <= 1.0 + EPS, "{label}: ширины {min}…{max}");
                for frame in &frames {
                    assert_eq!(frame.y, area.y, "{label}: низ");
                    assert_eq!(frame.h, area.h, "{label}: во всю высоту");
                }
            }
        }
    }

    #[test]
    fn gaps_are_full_at_area_edges_and_half_on_shared_edges() {
        let gap_setting: f32 = 10.0;
        let gap = f64::from(gap_setting);
        for area in areas() {
            for count in 1..=16usize {
                let plain = column_frames(&area, count, 0.0, false);
                let frames = column_frames(&area, count, gap_setting, false);
                let label = format!("область {area:?}, столбиков {count}");
                assert!(
                    (frames[0].x - (area.x + gap)).abs() < EPS,
                    "{label}: левый зазор"
                );
                assert!(
                    (frames[count - 1].max_x() - (area.max_x() - gap)).abs() < EPS,
                    "{label}: правый зазор"
                );
                for index in 1..count {
                    let boundary = plain[index].x;
                    assert!(
                        (frames[index - 1].max_x() - (boundary - gap / 2.0)).abs() < EPS,
                        "{label}: слева от стыка {index} — половина зазора"
                    );
                    assert!(
                        (frames[index].x - (boundary + gap / 2.0)).abs() < EPS,
                        "{label}: справа от стыка {index} — половина зазора"
                    );
                }
                for frame in &frames {
                    assert!(
                        (frame.y - (area.y + gap)).abs() < EPS,
                        "{label}: нижний зазор"
                    );
                    assert!(
                        (frame.h - (area.h - 2.0 * gap)).abs() < EPS,
                        "{label}: высота"
                    );
                }

                // Без зазора сверху: верх столбиков — край области.
                for frame in column_frames(&area, count, gap_setting, true) {
                    assert!((frame.max_y() - area.max_y()).abs() < EPS, "{label}: верх");
                    assert!((frame.y - (area.y + gap)).abs() < EPS, "{label}: низ");
                }
            }
        }
    }

    #[test]
    fn columns_count_limits() {
        let area = Rect::new(0.0, 0.0, 1728.0, 1001.0);
        assert!(column_frames(&area, 0, 0.0, false).is_empty());
        assert_eq!(column_frames(&area, 1, 0.0, false), vec![area]);
        let many = column_frames(&area, 300, 0.0, false);
        assert_eq!(many.len(), MAX_COLUMNS);
        assert!((many.last().unwrap().max_x() - area.max_x()).abs() < EPS);
    }

    fn at(x: f64, y: f64) -> Rect {
        Rect::new(x, y, 500.0, 400.0)
    }

    #[test]
    fn order_without_memory_goes_left_to_right_then_top_down() {
        let windows = [
            (30, at(900.0, 100.0)),
            (10, at(0.0, 300.0)),
            (20, at(0.0, 50.0)),
            (40, at(450.0, 0.0)),
        ];
        assert_eq!(column_order(None, &windows), vec![20, 10, 40, 30]);
        assert_eq!(column_order(Some(&[]), &windows), vec![20, 10, 40, 30]);
    }

    #[test]
    fn order_keeps_known_windows_and_appends_new_ones() {
        // Известные окна стоят как в прошлый раз, даже если их подвинули.
        let windows = [(7, at(900.0, 0.0)), (5, at(0.0, 0.0)), (6, at(400.0, 0.0))];
        assert_eq!(column_order(Some(&[7, 5, 6]), &windows), vec![7, 5, 6]);

        // Новое окно — справа, закрытое выпадает.
        let windows = [(5, at(0.0, 0.0)), (7, at(900.0, 0.0)), (9, at(10.0, 10.0))];
        assert_eq!(column_order(Some(&[7, 5, 6]), &windows), vec![7, 5, 9]);

        // Несколько новых — в порядке создания (window id), а не по положению.
        let windows = [
            (12, at(0.0, 0.0)),
            (5, at(500.0, 0.0)),
            (11, at(900.0, 0.0)),
        ];
        assert_eq!(column_order(Some(&[5]), &windows), vec![5, 11, 12]);
    }

    #[test]
    fn order_with_stale_memory_falls_back_to_positions() {
        let windows = [(3, at(800.0, 0.0)), (4, at(100.0, 0.0))];
        assert_eq!(column_order(Some(&[1, 2]), &windows), vec![4, 3]);
    }

    #[test]
    fn order_ignores_duplicates() {
        let windows = [
            (3, at(800.0, 0.0)),
            (3, at(800.0, 0.0)),
            (4, at(100.0, 0.0)),
        ];
        assert_eq!(column_order(None, &windows), vec![4, 3]);
        assert_eq!(column_order(Some(&[3, 3, 4]), &windows), vec![3, 4]);
    }

    #[test]
    fn window_belongs_to_screen_with_largest_overlap() {
        let screens = [
            Rect::new(0.0, 0.0, 1728.0, 1117.0),
            Rect::new(1728.0, 0.0, 2560.0, 1440.0),
        ];
        assert_eq!(
            screen_for(&Rect::new(100.0, 100.0, 800.0, 600.0), &screens),
            Some(0)
        );
        // Больше половины — на втором экране.
        assert_eq!(
            screen_for(&Rect::new(1500.0, 100.0, 800.0, 600.0), &screens),
            Some(1)
        );
        // Вне всех экранов — ближайший.
        assert_eq!(
            screen_for(&Rect::new(5000.0, 0.0, 100.0, 100.0), &screens),
            Some(1)
        );
        assert_eq!(
            screen_for(&Rect::new(-900.0, 0.0, 100.0, 100.0), &screens),
            Some(0)
        );
        assert_eq!(screen_for(&Rect::new(0.0, 0.0, 10.0, 10.0), &[]), None);
    }

    #[test]
    fn events_in_a_row_give_one_retile_no_later_than_max_delay() {
        let first = Instant::now();
        // Одиночное событие — через паузу.
        assert_eq!(next_due(first, first), first + DEBOUNCE);
        // Следующее через 100 мс — пауза отсчитывается от него.
        let second = first + Duration::from_millis(100);
        assert_eq!(next_due(first, second), second + DEBOUNCE);
        // События без перерыва — не позже MAX_DELAY от первого.
        let late = first + Duration::from_millis(950);
        assert_eq!(next_due(first, late), first + MAX_DELAY);
    }

    #[test]
    fn keep_list_adds_and_removes_bundles() {
        let list = with_bundle(None, "com.apple.Terminal", true);
        assert_eq!(
            list,
            Some(BTreeSet::from(["com.apple.Terminal".to_string()]))
        );
        let list = with_bundle(list, "com.googlecode.iterm2", true);
        assert_eq!(list.as_ref().map(BTreeSet::len), Some(2));
        let list = with_bundle(list, "com.googlecode.iterm2", false);
        assert_eq!(
            list,
            Some(BTreeSet::from(["com.apple.Terminal".to_string()]))
        );
        // Пустой список — «не задано».
        assert_eq!(with_bundle(list, "com.apple.Terminal", false), None);
        assert_eq!(with_bundle(None, "x", false), None);
    }

    #[test]
    fn keep_list_setting_round_trips_and_stays_out_of_export() {
        let config = Config::default();
        assert_eq!(config.app_columns_bundle_ids, None);
        let mut changed = config.clone();
        changed.app_columns_bundle_ids = Some(BTreeSet::from([
            "com.apple.Terminal".to_string(),
            "com.googlecode.iterm2".to_string(),
        ]));
        let mut store = MemoryStore::new();
        Config::save_changes(&config, &changed, &mut store);
        assert_eq!(
            store.string("r2AppColumnsBundleIds").as_deref(),
            Some("[\"com.apple.Terminal\",\"com.googlecode.iterm2\"]")
        );
        assert_eq!(Config::load_from(&store), changed);
        // Настройка есть только в порте — в экспорт формата Rectangle не попадает.
        assert!(!export_json(&changed, "106").contains("r2AppColumnsBundleIds"));
    }

    #[test]
    fn menu_titles_are_russian() {
        assert_eq!(TILE_MENU_TITLE, "Окна приложения столбиками");
        assert_eq!(
            keep_menu_title("Терминал"),
            "Держать окна «Терминал» столбиками"
        );
    }
}
