//! Значок стопки окон — `StackBadgeManager.swift` оригинала.
//!
//! Когда курсор замирает у угла ячейки сетки, где друг на друге стоят
//! несколько окон, у верхнего края стопки появляются пилюля с числом окон и
//! список их названий; щелчок по названию выводит окно вперёд. Включается
//! флажком «Значок стопки окон при наведении» в поповере «Ещё» (`stackBadge`, по
//! умолчанию выключен) и включается/выключается на лету.
//!
//! Как в оригинале, ничего об окнах и экранах между остановками курсора не
//! хранится: каждая остановка заново берёт список окон у WindowServer (его
//! нельзя подвесить зависшим приложением; как `WindowUtil.getWindowList`,
//! снимок живёт до 100 мс — `ax::window_list`), так что окна, которые двигали,
//! закрывали или переносили мимо Rectangle, не оставят устаревшего значка. AX
//! трогается только ради заголовков нескольких найденных окон — в фоне, с
//! тайм-аутом 0,25 с, — и результат выбрасывается, если курсор успел уйти.
//!
//! Курсор опрашивается таймером (10 раз в секунду), а не глобальным монитором
//! мыши: монитор после сна перестаёт получать события, а чтение положения —
//! нет. Тик — сравнение координат, и таймер работает, только пока функция
//! включена. Игнорируемые приложения значок не пропускает — как в оригинале,
//! в стопку идут все обычные окна.

pub mod geometry;
mod windows;

use std::cell::{Cell, OnceCell, RefCell};
use std::collections::{HashMap, HashSet};

use objc2::rc::Retained;
use objc2::runtime::NSObject;
use objc2::{define_class, msg_send, sel, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::NSWindow;
use objc2_foundation::{NSComparisonResult, NSProcessInfo, NSString, NSTimer};

use self::geometry::{Point, StackRanges, HOVER_ZONE};
use self::windows::ListWindow;
use crate::ax::{self, AxElement, WindowInfo};
use crate::geometry::Rect;
use crate::{config, events, screens, window_manager};

/// Окно из стопки (`StackedWindow`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StackedWindow {
    pub window_id: u32,
    pub pid: i32,
    /// Название для списка (`displayTitle`).
    pub title: String,
}

/// Период опроса курсора, с, и допустимое опоздание тика.
const TICK_INTERVAL: f64 = 0.1;
const TICK_TOLERANCE: f64 = 0.05;
/// Сколько курсор должен простоять, чтобы искать стопку, с.
const DWELL_INTERVAL: f64 = 0.15;
/// Сдвиг меньше этого, pt, — не движение.
const MOVE_TOLERANCE: f64 = 2.0;
/// Сколько ждать ответа приложения по AX, с (зависшее приложение не держит
/// ни чтение заголовков, ни вывод окна вперёд).
const AX_TIMEOUT: f32 = 0.25;

// ---------------------------------------------------------------- курсор

/// Что показал очередной тик.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    /// Курсор сдвинулся.
    Moved,
    /// Стоит, но ещё мало или остановка уже обработана.
    Wait,
    /// Простоял достаточно — пора искать стопку (один раз на остановку).
    Dwell,
}

/// Слежение за курсором между тиками: где был, когда двигался, обработана ли
/// остановка. Время — `systemUptime`, как в оригинале: «движения» до запуска
/// считаются бывшими в момент 0.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Pointer {
    last_location: Point,
    last_move_time: f64,
    dwell_fired: bool,
}

impl Pointer {
    fn step(&mut self, location: Point, now: f64) -> Step {
        let dx = location.0 - self.last_location.0;
        let dy = location.1 - self.last_location.1;
        if dx.abs() > MOVE_TOLERANCE || dy.abs() > MOVE_TOLERANCE {
            self.last_location = location;
            self.last_move_time = now;
            self.dwell_fired = false;
            return Step::Moved;
        }
        if self.dwell_fired || now - self.last_move_time < DWELL_INTERVAL {
            return Step::Wait;
        }
        self.dwell_fired = true;
        Step::Dwell
    }
}

// ---------------------------------------------------------------- названия

/// Название окна для списка (`displayTitle`): в строке уже есть иконка
/// приложения, поэтому имя приложения в начале заголовка
/// («Terminal — voice-bridge» → «voice-bridge») лишнее. Нет заголовка — имя
/// приложения.
pub fn display_title(process_name: Option<&str>, ax_title: Option<&str>) -> String {
    let process_name = process_name.unwrap_or("");
    let mut title = ax_title.unwrap_or("").to_string();
    if !process_name.is_empty() {
        for separator in [" — ", " - ", ": "] {
            if let Some(rest) =
                strip_prefix_like_swift(&title, &format!("{process_name}{separator}"))
            {
                title = rest;
                break;
            }
        }
    }
    if title.is_empty() {
        process_name.to_string()
    } else {
        title
    }
}

/// `title.hasPrefix(prefix)` и `dropFirst(prefix.count)` строк Swift: строки
/// сравниваются посимвольно (по графемам) с канонической эквивалентностью —
/// «é» одним знаком равно «e» с надстрочным знаком, а начало не совпадает, если
/// к последнему его знаку в заголовке прилип надстрочный. Остаток — как был.
/// `None` — заголовок начинается не с `prefix`.
fn strip_prefix_like_swift(title: &str, prefix: &str) -> Option<String> {
    let title = NSString::from_str(title);
    let prefix = NSString::from_str(prefix);
    let (mut in_title, mut in_prefix) = (0, 0);
    while in_prefix < prefix.length() {
        if in_title >= title.length() {
            return None;
        }
        let title_char = title.rangeOfComposedCharacterSequenceAtIndex(in_title);
        let prefix_char = prefix.rangeOfComposedCharacterSequenceAtIndex(in_prefix);
        // `compare:` без флагов — каноническая эквивалентность, как у `Character`.
        let same = title
            .substringWithRange(title_char)
            .compare(&prefix.substringWithRange(prefix_char))
            == NSComparisonResult::Same;
        if !same {
            return None;
        }
        in_title = title_char.end();
        in_prefix = prefix_char.end();
    }
    Some(title.substringFromIndex(in_title).to_string())
}

/// Заголовки окон стопки по номерам (`titlesByWindowId`): одно перечисление
/// окон по AX на приложение, сколько бы его окон ни было в стопке. Вызывается
/// в фоне.
fn titles_by_window_id(stacked: &[WindowInfo]) -> HashMap<u32, String> {
    let mut titles = HashMap::new();
    let pids: HashSet<i32> = stacked.iter().map(|info| info.pid).collect();
    for pid in pids {
        let app_element = AxElement::application(pid);
        app_element.set_messaging_timeout(AX_TIMEOUT);
        let stacked_ids: HashSet<u32> = stacked
            .iter()
            .filter(|info| info.pid == pid)
            .map(|info| info.id)
            .collect();
        for element in app_element.window_elements().unwrap_or_default() {
            let Some(id) = element.window_id().filter(|id| stacked_ids.contains(id)) else {
                continue;
            };
            match element.title() {
                Some(title) => titles.insert(id, title),
                None => titles.remove(&id),
            };
        }
    }
    titles
}

// ---------------------------------------------------------------- состояние

/// Показанные пилюля и список.
struct Ui {
    badge: Retained<NSWindow>,
    list: ListWindow,
    /// Где курсор не закрывает значок (`visibleUIFrames`): пилюля, список и
    /// коридор между ними и стопкой; AppKit.
    frames: [Rect; 3],
    windows: Vec<StackedWindow>,
}

#[derive(Default)]
struct State {
    /// Таймер опроса курсора; есть — функция включена.
    timer: Option<Retained<NSTimer>>,
    pointer: Pointer,
    /// Номер остановки: растёт при каждом движении и закрытии значка, так что
    /// заголовки, прочитанные для прошлой остановки, выбрасываются.
    generation: u64,
    ui: Option<Ui>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
    /// Цель таймера (таймер её удерживает, но и она живёт всё время работы).
    static TICKER: OnceCell<Retained<Ticker>> = const { OnceCell::new() };
    static INSTALLED: Cell<bool> = const { Cell::new(false) };
    /// Положение курсора для проверок без мыши (`set_cursor_override`).
    static CURSOR_OVERRIDE: Cell<Option<Point>> = const { Cell::new(None) };
}

fn with_state<R>(f: impl FnOnce(&mut State) -> R) -> R {
    STATE.with(|state| f(&mut state.borrow_mut()))
}

define_class!(
    /// Цель таймера опроса курсора.
    #[unsafe(super(NSObject))]
    #[name = "R2StackBadgeTicker"]
    #[thread_kind = MainThreadOnly]
    #[ivars = ()]
    struct Ticker;

    impl Ticker {
        #[unsafe(method(tick:))]
        fn tick(&self, _timer: &NSTimer) {
            on_tick();
        }
    }
);

impl Ticker {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm);
        unsafe { msg_send![this, init] }
    }
}

// ---------------------------------------------------------------- запуск

/// Запустить (`StackBadgeManager.init`, из `subsystems::start_all`): смена
/// экранов закрывает значок, а опрос курсора включается, если функция
/// включена. Повторный вызов ничего не делает.
pub fn install(_mtm: MainThreadMarker) {
    if INSTALLED.with(|installed| installed.replace(true)) {
        return;
    }
    events::on_screens_changed(dismiss);
    toggle_listening();
}

/// Настройки изменились (`stackBadgeChanged`, `configImported`): включить или
/// выключить опрос курсора.
pub fn reload() {
    if INSTALLED.with(Cell::get) {
        toggle_listening();
    }
}

/// Опрос курсора идёт (функция включена и запущена).
pub fn is_listening() -> bool {
    with_state(|state| state.timer.is_some())
}

/// Включить таймер, если `stackBadge` включён явно, иначе выключить его и
/// закрыть значок.
fn toggle_listening() {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    if config::with(|config| config.stack_badge == Some(true)) {
        if is_listening() {
            return;
        }
        let ticker = TICKER.with(|ticker| ticker.get_or_init(|| Ticker::new(mtm)).clone());
        // SAFETY: у цели есть метод `tick:` с аргументом NSTimer.
        let timer = unsafe {
            NSTimer::scheduledTimerWithTimeInterval_target_selector_userInfo_repeats(
                TICK_INTERVAL,
                &ticker,
                sel!(tick:),
                None,
                true,
            )
        };
        timer.setTolerance(TICK_TOLERANCE);
        with_state(|state| state.timer = Some(timer));
    } else {
        if let Some(timer) = with_state(|state| state.timer.take()) {
            timer.invalidate();
        }
        dismiss();
    }
}

// ---------------------------------------------------------------- тик

/// Положение курсора (AppKit): подменённое для проверок или настоящее.
fn cursor_location() -> Point {
    CURSOR_OVERRIDE
        .with(Cell::get)
        .or_else(screens::cursor_position)
        .unwrap_or_default()
}

/// Тик таймера (`tick()`): движение закрывает значок, если курсор ушёл с
/// него; остановка у угла сетки ищет стопку.
fn on_tick() {
    let location = cursor_location();
    let now = NSProcessInfo::processInfo().systemUptime();
    match with_state(|state| state.pointer.step(location, now)) {
        Step::Moved => {
            let left_ui = with_state(|state| {
                state.generation = state.generation.wrapping_add(1);
                state
                    .ui
                    .as_ref()
                    .is_some_and(|ui| !geometry::inside_visible_ui(&ui.frames, location))
            });
            if left_ui {
                dismiss();
            }
        }
        Step::Wait => {}
        Step::Dwell => dwell(location),
    }
}

/// Курсор замер в `location` (AppKit). Зазоры отодвигают окна от
/// геометрического угла, поэтому зона наведения тянется ещё на зазор. Углы
/// считаются заново на каждую остановку, а не кэшируются: рабочая область
/// зависит от большего, чем смена экранов (Todo, Stage Manager, отступы от
/// краёв), и надёжного сигнала сбросить кэш нет — по той же причине не
/// кэшируются и окна.
fn dwell(location: Point) {
    let zone = HOVER_ZONE + f64::from(config::with(|config| config.gap_size));
    if with_state(|state| state.ui.is_some()) {
        return;
    }
    let all_screens = screens::screens();
    let Some(screen) = all_screens
        .iter()
        .find(|screen| geometry::contains_point(&screen.frame, location))
    else {
        return;
    };
    let screen_frame = geometry::standardized(&window_manager::adjusted_visible_frame(
        screen, false, false,
    ));
    let corners = geometry::corner_points(&screen_frame);
    let Some(corner) = geometry::corner_near(location, &corners, zone) else {
        return;
    };
    let primary_height = all_screens
        .first()
        .map_or(0.0, |primary| primary.frame.max_y());
    query(corner, screen_frame, primary_height);
}

/// Один свежий взгляд на окна на остановку (`query`): число — от WindowServer
/// (список окон не блокируется зависшим приложением, снимок не старше 100 мс),
/// заголовки — по AX в фоне и только у найденных окон. `corner` и `screen_frame`
/// — AppKit.
fn query(corner: Point, screen_frame: Rect, primary_height: f64) {
    let Some((stack, top_left)) = find(&ax::window_list(), corner, &screen_frame, primary_height)
    else {
        return;
    };
    let request_generation = with_state(|state| state.generation);
    let spawned = std::thread::Builder::new()
        .name("stack-badge-titles".to_string())
        .spawn(move || {
            let windows = stacked_windows(&stack);
            events::run_on_main(move || {
                let current = with_state(|state| {
                    state.generation == request_generation && state.timer.is_some()
                });
                if current {
                    show(windows, top_left, screen_frame);
                }
            });
        });
    if let Err(error) = spawned {
        crate::log!("Значок стопки: не запустить поток заголовков: {error}");
    }
}

/// Стопка у угла `corner` рабочей области `screen_frame` (обе — AppKit) среди
/// окон `windows` (список окон, AX) и её видимый верхний левый край (AppKit):
/// значок встаёт там, где начинаются заголовки, а не у угла, от которого окна
/// отодвинул зазор.
fn find(
    windows: &[WindowInfo],
    corner: Point,
    screen_frame: &Rect,
    primary_height: f64,
) -> Option<(Vec<WindowInfo>, Point)> {
    let corner_ax = (corner.0, primary_height - corner.1);
    let screen_frame_ax = screen_frame.screen_flipped(primary_height);
    let ranges = config::with(|config| {
        StackRanges::new(
            config.gap_size,
            config.cycling_overlap_offset_size,
            config.cycling_overlap_max_cascade,
        )
    });
    let stack = geometry::find_stack(windows, corner_ax, &screen_frame_ax, ranges)?;
    let top_left = (stack.top_left.0, primary_height - stack.top_left.1);
    Some((stack.windows, top_left))
}

/// Строки списка для окон стопки: заголовки по AX, порядок списка окон —
/// спереди назад. Может ждать AX, поэтому зовётся в фоне.
fn stacked_windows(stack: &[WindowInfo]) -> Vec<StackedWindow> {
    let titles = titles_by_window_id(stack);
    stack
        .iter()
        .map(|info| StackedWindow {
            window_id: info.id,
            pid: info.pid,
            title: display_title(
                info.process_name.as_deref(),
                titles.get(&info.id).map(String::as_str),
            ),
        })
        .collect()
}

// ---------------------------------------------------------------- показ

/// Показать пилюлю и список для стопки с верхним левым краем `top_left`
/// (AppKit) в рабочей области `screen_frame` (`show`).
fn show(windows: Vec<StackedWindow>, top_left: Point, screen_frame: Rect) {
    dismiss();
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };

    // Пилюля и список — ниже полосы заголовка, чтобы «светофор» переднего окна
    // оставался доступен; кнопки нижних окон достаются щелчком по названию.
    let anchor = geometry::badge_anchor(top_left);
    let badge = windows::make_badge_window(mtm, windows.len(), anchor);
    badge.orderFrontRegardless();
    let badge_frame = windows::rect(badge.frame());

    // Список — сразу под пилюлей, с отступом под «выглядывающие» окна.
    let list_top = geometry::list_top(anchor, &badge_frame);
    let list = windows::make_list_window(mtm, &windows, list_top, &screen_frame, focus);
    list.panel.orderFrontRegardless();
    let list_frame = windows::rect(list.panel.frame());

    // Коридор от верха списка до «пика», где курсор вызвал значок: иначе по
    // дороге вниз курсор пересёк бы пустоту и всё закрылось бы.
    let corridor = geometry::corridor(&badge_frame, &list_frame, top_left.1);
    with_state(|state| {
        state.ui = Some(Ui {
            badge,
            list,
            frames: [badge_frame, list_frame, corridor],
            windows,
        });
    });
}

/// Закрыть значок (`dismiss`). Номер остановки растёт, чтобы заголовки,
/// которые ещё читаются, не вернули значок обратно.
fn dismiss() {
    let ui = with_state(|state| {
        state.generation = state.generation.wrapping_add(1);
        state.ui.take()
    });
    if let Some(ui) = ui {
        ui.badge.orderOut(None);
        ui.list.panel.orderOut(None);
    }
}

/// Вывести окно вперёд (`focus`): окно ищется по pid напрямую, в фоне и с
/// тайм-аутами AX, чтобы зависшее приложение не держало щелчок.
///
/// Список при этом НЕ закрывается: можно щёлкнуть по нескольким окнам стопки
/// подряд, не вызывая его заново. Он закроется, когда курсор уйдёт с него.
/// Щелчки доходят, хотя впереди уже другое приложение: строки принимают
/// первый щелчок, а панель не делает приложение активным.
fn focus(window: &StackedWindow) {
    let (pid, window_id) = (window.pid, window.window_id);
    let spawned = std::thread::Builder::new()
        .name("stack-badge-focus".to_string())
        .spawn(move || {
            let app_element = AxElement::application(pid);
            app_element.set_messaging_timeout(AX_TIMEOUT);
            let Some(window_element) = app_element.window_elements().and_then(|elements| {
                elements
                    .into_iter()
                    .find(|element| element.window_id() == Some(window_id))
            }) else {
                return;
            };
            window_element.set_messaging_timeout(AX_TIMEOUT);
            window_element.bring_to_front(true);
        });
    if let Err(error) = spawned {
        crate::log!("Значок стопки: не запустить поток вывода окна: {error}");
    }
}

// ---------------------------------------------------------------- проверки

/// Проверки без мыши (примеры): таймер будет читать это положение курсора
/// (AppKit) вместо настоящего; `None` — снова настоящее.
pub fn set_cursor_override(location: Option<Point>) {
    CURSOR_OVERRIDE.with(|cell| cell.set(location));
}

/// Что сейчас показано — для проверок.
#[derive(Clone, Debug, PartialEq)]
pub struct Snapshot {
    /// Номер окна пилюли (`windowNumber`, он же CGWindowID) и её рамка (AppKit).
    pub badge_window: isize,
    pub badge_frame: Rect,
    /// То же для списка.
    pub list_window: isize,
    pub list_frame: Rect,
    /// Коридор между стопкой и списком (AppKit).
    pub corridor: Rect,
    /// Окна в строках списка, сверху вниз.
    pub windows: Vec<StackedWindow>,
}

/// Показанный значок, если он есть.
pub fn snapshot() -> Option<Snapshot> {
    with_state(|state| {
        state.ui.as_ref().map(|ui| Snapshot {
            badge_window: ui.badge.windowNumber(),
            badge_frame: ui.frames[0],
            list_window: ui.list.panel.windowNumber(),
            list_frame: ui.frames[1],
            corridor: ui.frames[2],
            windows: ui.windows.clone(),
        })
    })
}

/// Показать значок, как после остановки курсора у угла `corner` рабочей
/// области `screen_frame` (обе — AppKit) при списке окон `windows` (AX), но
/// сразу: без таймера, задержки и фонового потока. `primary_height` — высота
/// основного экрана (переворот AppKit ↔ AX). `false` — стопки нет, показанное
/// не меняется.
pub fn show_stack_now(
    windows: &[WindowInfo],
    corner: Point,
    screen_frame: Rect,
    primary_height: f64,
) -> bool {
    let Some((stack, top_left)) = find(windows, corner, &screen_frame, primary_height) else {
        return false;
    };
    show(stacked_windows(&stack), top_left, screen_frame);
    true
}

/// Строка `index` показанного списка.
fn row(index: usize) -> Option<Retained<windows::RowView>> {
    with_state(|state| state.ui.as_ref()?.list.rows.get(index).cloned())
}

/// Выполнить действие строки `index`, как щелчок по ней (без событий мыши).
/// `false` — значка или такой строки нет.
pub fn press_row(index: usize) -> bool {
    let Some(row) = row(index) else {
        return false;
    };
    row.click();
    true
}

/// Подсветить строку `index`, как при наведении курсора. `false` — значка или
/// такой строки нет.
pub fn highlight_row(index: usize, selected: bool) -> bool {
    let Some(row) = row(index) else {
        return false;
    };
    row.set_selected(selected);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pointer_waits_for_the_dwell_and_fires_once() {
        let mut pointer = Pointer::default();
        // Первый тик: курсор «переехал» из (0, 0).
        assert_eq!(pointer.step((500.0, 400.0), 100.0), Step::Moved);
        assert_eq!(pointer.step((500.0, 400.0), 100.1), Step::Wait);
        assert_eq!(pointer.step((501.0, 399.0), 100.149), Step::Wait);
        assert_eq!(pointer.step((501.0, 399.0), 100.15), Step::Dwell);
        // Остановка обработана — до следующего движения ничего.
        assert_eq!(pointer.step((501.0, 399.0), 100.3), Step::Wait);
        assert_eq!(pointer.step((502.0, 402.0), 100.4), Step::Wait);
        assert_eq!(pointer.step((502.1, 400.0), 100.5), Step::Moved);
        assert_eq!(pointer.last_location, (502.1, 400.0));
        assert_eq!(pointer.step((502.1, 400.0), 100.6), Step::Wait);
        assert_eq!(pointer.step((502.1, 400.0), 100.65), Step::Dwell);
    }

    #[test]
    fn pointer_measures_moves_from_the_last_move() {
        let mut pointer = Pointer::default();
        pointer.step((100.0, 100.0), 10.0);
        // Медленный дрейф по 1,5 pt за тик — не движение, отсчёт от места
        // последнего движения, а не от прошлого тика.
        assert_eq!(pointer.step((101.5, 100.0), 10.1), Step::Wait);
        assert_eq!(pointer.step((103.0, 100.0), 10.2), Step::Moved);
        assert_eq!(pointer.step((103.0, 97.9), 10.3), Step::Moved);
    }

    #[test]
    fn cursor_at_start_near_zero_dwells_at_once() {
        // Как в оригинале: до первого движения «последнее движение» было в
        // момент 0 системного времени.
        let mut pointer = Pointer::default();
        assert_eq!(pointer.step((1.0, 1.0), 5000.0), Step::Dwell);
    }

    #[test]
    fn display_title_drops_the_app_name_prefix() {
        assert_eq!(
            display_title(Some("Terminal"), Some("Terminal — voice-bridge")),
            "voice-bridge"
        );
        assert_eq!(
            display_title(Some("Code"), Some("Code - main.rs")),
            "main.rs"
        );
        assert_eq!(
            display_title(Some("Notes"), Some("Notes: список")),
            "список"
        );
        // Только первое совпадение и только в начале.
        assert_eq!(
            display_title(Some("Terminal"), Some("Terminal — Terminal - x")),
            "Terminal - x"
        );
        assert_eq!(
            display_title(Some("Terminal"), Some("zsh — Terminal")),
            "zsh — Terminal"
        );
        // Без разделителя — как есть.
        assert_eq!(display_title(Some("Safari"), Some("Safari")), "Safari");
        assert_eq!(display_title(Some("Safari"), Some("SafariX")), "SafariX");
    }

    #[test]
    fn display_title_falls_back_to_the_app_name() {
        assert_eq!(display_title(Some("Finder"), None), "Finder");
        assert_eq!(display_title(Some("Finder"), Some("")), "Finder");
        // Заголовок — одно имя приложения с разделителем: остаток пуст.
        assert_eq!(display_title(Some("Finder"), Some("Finder — ")), "Finder");
        assert_eq!(display_title(None, Some("окно")), "окно");
        assert_eq!(display_title(None, None), "");
        // Без имени приложения префиксы не ищутся.
        assert_eq!(display_title(Some(""), Some(" — окно")), " — окно");
    }
}
