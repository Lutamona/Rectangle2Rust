//! Журнал — аналог `Logger` оригинала (`Logging/LogViewer.swift`).
//!
//! Флаг «вести журнал» (`Logger.logging`) включает окно журнала, пока оно
//! открыто. Выключенный журнал стоит одну атомарную проверку: `log!` даже не
//! форматирует строку. Включённый копит строки
//! `«<время ISO 8601>: <текст>»` в кольцевом буфере: окно журнала при
//! открытии берёт `lines()`, а новые строки получает через `set_observer`.
//!
//! Для отладки: `R2_LOG=1` в окружении включает журнал с запуска и дублирует
//! строки в stderr.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::ffi::{c_char, c_int, c_long};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use objc2::MainThreadMarker;

/// Сколько последних строк хранит журнал.
pub const CAPACITY: usize = 5000;

/// Записать строку в журнал, если он включён. Аргументы — как у `format!`.
#[macro_export]
macro_rules! log {
    ($($arg:tt)+) => {
        if $crate::logging::is_enabled() {
            $crate::logging::write(&::std::format!($($arg)+));
        }
    };
}

/// Кольцевой буфер строк: сверх ёмкости уходят самые старые.
#[derive(Debug)]
pub struct LogBuffer {
    lines: VecDeque<String>,
    capacity: usize,
}

impl LogBuffer {
    pub const fn new(capacity: usize) -> LogBuffer {
        LogBuffer {
            lines: VecDeque::new(),
            capacity,
        }
    }

    pub fn push(&mut self, line: String) {
        if self.capacity == 0 {
            return;
        }
        while self.lines.len() >= self.capacity {
            self.lines.pop_front();
        }
        self.lines.push_back(line);
    }

    /// Строки от старых к новым.
    pub fn lines(&self) -> Vec<String> {
        self.lines.iter().cloned().collect()
    }

    pub fn len(&self) -> usize {
        self.lines.len()
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    pub fn clear(&mut self) {
        self.lines.clear();
    }
}

static ENABLED: AtomicBool = AtomicBool::new(false);
static ECHO_TO_STDERR: AtomicBool = AtomicBool::new(false);
/// Наблюдатель есть — строкам с других потоков есть смысл идти в главную очередь.
static HAS_OBSERVER: AtomicBool = AtomicBool::new(false);
static BUFFER: Mutex<LogBuffer> = Mutex::new(LogBuffer::new(CAPACITY));

/// Наблюдатель новых строк журнала (окно журнала).
pub type LogObserver = Box<dyn Fn(&str)>;

thread_local! {
    /// Кому показывать новые строки (окно журнала); живёт на главном потоке.
    static OBSERVER: RefCell<Option<LogObserver>> = const { RefCell::new(None) };
}

fn buffer() -> MutexGuard<'static, LogBuffer> {
    BUFFER
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Журнал включён (`Logger.logging`).
pub fn is_enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Включить или выключить журнал. Накопленные строки не трогает: очищает `clear`.
pub fn set_enabled(enabled: bool) {
    ENABLED.store(enabled, Ordering::Relaxed);
}

/// `R2_LOG=1` (или любое непустое значение, кроме `0`): журнал с запуска и копия в stderr.
pub fn init_from_env() {
    let requested = std::env::var("R2_LOG").is_ok_and(|value| !value.is_empty() && value != "0");
    if requested {
        ECHO_TO_STDERR.store(true, Ordering::Relaxed);
        set_enabled(true);
    }
}

/// Записать строку со временем, если журнал включён. Обычно зовут через `log!`.
pub fn write(text: &str) {
    if !is_enabled() {
        return;
    }
    let line = format!("{}: {}", timestamp(), text);
    if ECHO_TO_STDERR.load(Ordering::Relaxed) {
        eprintln!("rectangle2rust: {line}");
    }
    if !HAS_OBSERVER.load(Ordering::Relaxed) {
        buffer().push(line);
        return;
    }
    buffer().push(line.clone());
    if MainThreadMarker::new().is_some() {
        notify(&line);
    } else {
        crate::events::run_on_main(move || notify(&line));
    }
}

fn notify(line: &str) {
    OBSERVER.with(|observer| {
        if let Some(observer) = observer.borrow().as_ref() {
            observer(line);
        }
    });
}

/// Все накопленные строки, от старых к новым.
pub fn lines() -> Vec<String> {
    buffer().lines()
}

/// Очистить журнал (кнопка «Clear» окна журнала).
pub fn clear() {
    buffer().clear();
}

/// Получать новые строки по мере записи (`None` — перестать). Вызывать с
/// главного потока: наблюдатель вызывается там же, строки с других потоков
/// доходят до него через главную очередь.
pub fn set_observer(observer: Option<LogObserver>) {
    HAS_OBSERVER.store(observer.is_some(), Ordering::Relaxed);
    OBSERVER.with(|slot| *slot.borrow_mut() = observer);
}

// ---------------------------------------------------------------- время

/// `struct tm` из `<time.h>` macOS.
#[repr(C)]
struct Tm {
    tm_sec: c_int,
    tm_min: c_int,
    tm_hour: c_int,
    tm_mday: c_int,
    tm_mon: c_int,
    tm_year: c_int,
    tm_wday: c_int,
    tm_yday: c_int,
    tm_isdst: c_int,
    tm_gmtoff: c_long,
    tm_zone: *mut c_char,
}

extern "C" {
    fn localtime_r(clock: *const i64, result: *mut Tm) -> *mut Tm;
}

/// Местное время, разобранное по полям (`localtime_r`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LocalTime {
    pub year: i32,
    /// 1…12.
    pub month: i32,
    pub day: i32,
    pub hour: i32,
    pub minute: i32,
    pub second: i32,
    /// Доля секунды в наносекундах.
    pub nanos: u32,
    /// Смещение от UTC, секунд.
    pub offset_seconds: i64,
}

/// Момент `time` в местном часовом поясе; `None` — время до 1970 года или
/// `localtime_r` не справилась.
pub fn local_time(time: SystemTime) -> Option<LocalTime> {
    let elapsed = time.duration_since(UNIX_EPOCH).ok()?;
    let seconds = elapsed.as_secs() as i64;
    let mut tm = Tm {
        tm_sec: 0,
        tm_min: 0,
        tm_hour: 0,
        tm_mday: 0,
        tm_mon: 0,
        tm_year: 0,
        tm_wday: 0,
        tm_yday: 0,
        tm_isdst: 0,
        tm_gmtoff: 0,
        tm_zone: std::ptr::null_mut(),
    };
    // SAFETY: оба указателя валидны на время вызова; localtime_r потокобезопасна.
    if unsafe { localtime_r(&seconds, &mut tm) }.is_null() {
        return None;
    }
    Some(LocalTime {
        year: tm.tm_year + 1900,
        month: tm.tm_mon + 1,
        day: tm.tm_mday,
        hour: tm.tm_hour,
        minute: tm.tm_min,
        second: tm.tm_sec,
        nanos: elapsed.subsec_nanos(),
        offset_seconds: tm.tm_gmtoff as i64,
    })
}

/// Местное время в формате `ISO8601DateFormatter` с `.withInternetDateTime`,
/// как у журнала оригинала: `2026-09-27T01:02:03+03:00`.
fn timestamp() -> String {
    let now = SystemTime::now();
    let Some(time) = local_time(now) else {
        let seconds = now
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_secs());
        return seconds.to_string();
    };
    format_timestamp(
        time.year,
        time.month,
        time.day,
        time.hour,
        time.minute,
        time.second,
        time.offset_seconds,
    )
}

fn format_timestamp(
    year: i32,
    month: i32,
    day: i32,
    hour: i32,
    minute: i32,
    second: i32,
    offset_seconds: i64,
) -> String {
    let zone = if offset_seconds == 0 {
        "Z".to_string()
    } else {
        let sign = if offset_seconds < 0 { '-' } else { '+' };
        let offset = offset_seconds.abs();
        format!("{sign}{:02}:{:02}", offset / 3600, offset % 3600 / 60)
    };
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}{zone}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buffer_keeps_last_lines() {
        let mut buffer = LogBuffer::new(3);
        assert!(buffer.is_empty());
        for number in 1..=5 {
            buffer.push(format!("строка {number}"));
        }
        assert_eq!(buffer.len(), 3);
        assert_eq!(buffer.lines(), vec!["строка 3", "строка 4", "строка 5"]);
        buffer.clear();
        assert!(buffer.is_empty());
        buffer.push("снова".to_string());
        assert_eq!(buffer.lines(), vec!["снова"]);
    }

    #[test]
    fn zero_capacity_buffer_stays_empty() {
        let mut buffer = LogBuffer::new(0);
        buffer.push("строка".to_string());
        assert!(buffer.is_empty());
    }

    #[test]
    fn timestamp_format_matches_internet_date_time() {
        assert_eq!(
            format_timestamp(2026, 9, 27, 1, 2, 3, 3 * 3600),
            "2026-09-27T01:02:03+03:00"
        );
        assert_eq!(
            format_timestamp(2026, 1, 5, 23, 59, 0, 0),
            "2026-01-05T23:59:00Z"
        );
        assert_eq!(
            format_timestamp(2026, 12, 31, 7, 8, 9, -(9 * 3600 + 30 * 60)),
            "2026-12-31T07:08:09-09:30"
        );
        // Настоящее местное время: дата, «T», время и пояс «Z» или «±ЧЧ:ММ».
        let now = timestamp();
        let bytes = now.as_bytes();
        assert!(now.len() == 20 || now.len() == 25, "{now}");
        for (index, expected) in [(4, b'-'), (7, b'-'), (10, b'T'), (13, b':'), (16, b':')] {
            assert_eq!(bytes[index], expected, "{now}");
        }
        assert!(matches!(bytes[19], b'Z' | b'+' | b'-'), "{now}");
    }

    #[test]
    fn log_writes_only_when_enabled() {
        // Единственный тест, который трогает общий журнал.
        set_enabled(false);
        clear();
        crate::log!("не должно попасть {}", 1);
        assert!(lines().is_empty());

        set_enabled(true);
        crate::log!("действие {}", "left-half");
        set_enabled(false);
        crate::log!("после выключения");

        let written = lines();
        assert_eq!(written.len(), 1);
        assert!(
            written[0].ends_with(": действие left-half"),
            "{:?}",
            written
        );
        clear();
        assert!(lines().is_empty());
    }
}
