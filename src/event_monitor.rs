//! Глобальные мониторы мыши — порт `Utilities/EventMonitor.swift`.
//!
//! Два вида, как в оригинале:
//! - `PassiveEventMonitor` — `NSEvent.addGlobalMonitorForEvents` (события
//!   других приложений) + `addLocalMonitorForEvents` (события своих окон).
//!   Менять события он не может; обработчик зовётся на главном потоке сразу.
//! - `ActiveEventMonitor` — `CGEventTap` уровня сессии в своём потоке с циклом
//!   событий (`RunLoopThread` оригинала). Фильтр вызывается в этом потоке
//!   синхронно и может поправить событие (`TapEvent::set_location`) или
//!   поглотить его (`true`); обработчик — на главном потоке асинхронно
//!   (`DispatchQueue.main.async`). Остановку поток замечает сразу, даже если
//!   она пришла до входа в цикл; если источник тапа пропал, поток завершается,
//!   а не крутит пустой цикл, и монитор больше не считается запущенным.
//!
//! Обработчики получают `MouseEvent` — то, что им нужно от `NSEvent`. Маска и
//! тип события — общие для NSEvent и CGEvent: у мышиных событий номера совпадают.

use std::ffi::c_void;
use std::ops::BitOr;
use std::ptr::{self, NonNull};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
use std::sync::{mpsc, Arc};
use std::thread::JoinHandle;

use block2::{Block, RcBlock};
use core_foundation_sys::base::{kCFAllocatorDefault, CFRelease, CFRetain, CFTypeRef};
use core_foundation_sys::mach_port::{
    CFMachPortCreateRunLoopSource, CFMachPortInvalidate, CFMachPortRef,
};
use core_foundation_sys::runloop::{
    kCFRunLoopDefaultMode, kCFRunLoopRunFinished, CFRunLoopAddSource, CFRunLoopGetCurrent,
    CFRunLoopRef, CFRunLoopRunInMode, CFRunLoopSourceRef, CFRunLoopStop, CFRunLoopWakeUp,
};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::MainThreadMarker;
use objc2_app_kit::{NSEvent, NSEventMask};

use crate::log;

// ---------------------------------------------------------------- событие

/// Вид мышиного события (`NSEvent.EventType` / `CGEventType`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseEventKind {
    LeftMouseDown,
    LeftMouseUp,
    RightMouseDown,
    RightMouseUp,
    MouseMoved,
    LeftMouseDragged,
    RightMouseDragged,
    /// Любое другое — с его номером.
    Other(u32),
}

impl MouseEventKind {
    pub fn from_raw(raw: u32) -> MouseEventKind {
        match raw {
            1 => MouseEventKind::LeftMouseDown,
            2 => MouseEventKind::LeftMouseUp,
            3 => MouseEventKind::RightMouseDown,
            4 => MouseEventKind::RightMouseUp,
            5 => MouseEventKind::MouseMoved,
            6 => MouseEventKind::LeftMouseDragged,
            7 => MouseEventKind::RightMouseDragged,
            other => MouseEventKind::Other(other),
        }
    }

    pub fn raw(self) -> u32 {
        match self {
            MouseEventKind::LeftMouseDown => 1,
            MouseEventKind::LeftMouseUp => 2,
            MouseEventKind::RightMouseDown => 3,
            MouseEventKind::RightMouseUp => 4,
            MouseEventKind::MouseMoved => 5,
            MouseEventKind::LeftMouseDragged => 6,
            MouseEventKind::RightMouseDragged => 7,
            MouseEventKind::Other(raw) => raw,
        }
    }

    /// Нажатие, отпускание или перетаскивание кнопкой — у таких событий есть
    /// `clickCount`.
    fn is_button_event(self) -> bool {
        matches!(
            self,
            MouseEventKind::LeftMouseDown
                | MouseEventKind::LeftMouseUp
                | MouseEventKind::RightMouseDown
                | MouseEventKind::RightMouseUp
                | MouseEventKind::LeftMouseDragged
                | MouseEventKind::RightMouseDragged
        )
    }

    /// Движение мыши — у таких событий есть `deltaY`.
    fn has_delta(self) -> bool {
        matches!(
            self,
            MouseEventKind::MouseMoved
                | MouseEventKind::LeftMouseDragged
                | MouseEventKind::RightMouseDragged
        )
    }
}

/// Какие события слушать: бит `1 << тип`, как у `NSEvent.EventTypeMask` и
/// `CGEventMask`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EventMask(pub u64);

impl EventMask {
    pub const LEFT_MOUSE_DOWN: EventMask = EventMask(1 << 1);
    pub const LEFT_MOUSE_UP: EventMask = EventMask(1 << 2);
    pub const LEFT_MOUSE_DRAGGED: EventMask = EventMask(1 << 6);

    pub fn contains(self, kind: MouseEventKind) -> bool {
        let raw = kind.raw();
        raw < 64 && self.0 & (1 << raw) != 0
    }
}

impl BitOr for EventMask {
    type Output = EventMask;

    fn bitor(self, other: EventMask) -> EventMask {
        EventMask(self.0 | other.0)
    }
}

/// Мышиное событие — то, что обработчики оригинала берут у `NSEvent`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MouseEvent {
    pub kind: MouseEventKind,
    /// `timestamp`: секунды с загрузки системы (без сна).
    pub timestamp: f64,
    /// `modifierFlags` целиком (`NSEvent.ModifierFlags` = `CGEventFlags`).
    pub modifier_flags: u64,
    /// `cgEvent?.location` — глобальные координаты Quartz (y сверху, как у AX).
    pub location: Option<(f64, f64)>,
    /// `deltaY` (у движений; у остальных 0): вверх — отрицательный.
    pub delta_y: f64,
    /// `clickCount` (у нажатий, отпусканий и перетаскиваний; у остальных 0).
    pub click_count: i64,
}

impl MouseEvent {
    /// `NSEvent.ModifierFlags.deviceIndependentFlagsMask`.
    pub const DEVICE_INDEPENDENT_FLAGS_MASK: u64 = 0xffff_0000;

    /// `modifierFlags.intersection(.deviceIndependentFlagsMask)`.
    pub fn device_independent_flags(&self) -> u64 {
        self.modifier_flags & Self::DEVICE_INDEPENDENT_FLAGS_MASK
    }

    fn from_ns_event(event: &NSEvent) -> MouseEvent {
        let kind = MouseEventKind::from_raw(event.r#type().0 as u32);
        let location = event.CGEvent().map(|cg_event| {
            let point = unsafe { CGEventGetLocation(Retained::as_ptr(&cg_event) as *const c_void) };
            (point.x, point.y)
        });
        MouseEvent {
            kind,
            timestamp: event.timestamp(),
            modifier_flags: event.modifierFlags().bits() as u64,
            location,
            delta_y: if kind.has_delta() {
                event.deltaY()
            } else {
                0.0
            },
            click_count: if kind.is_button_event() {
                event.clickCount() as i64
            } else {
                0
            },
        }
    }
}

// ---------------------------------------------------------------- CoreGraphics

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
struct CGPoint {
    x: f64,
    y: f64,
}

type TapCallback = extern "C" fn(
    proxy: *mut c_void,
    event_type: u32,
    event: *mut c_void,
    user_info: *mut c_void,
) -> *mut c_void;

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventTapCreate(
        tap: u32,
        place: u32,
        options: u32,
        events_of_interest: u64,
        callback: TapCallback,
        user_info: *mut c_void,
    ) -> CFMachPortRef;
    fn CGEventTapEnable(tap: CFMachPortRef, enable: bool);
    fn CGEventGetLocation(event: *const c_void) -> CGPoint;
    fn CGEventSetLocation(event: *mut c_void, location: CGPoint);
    fn CGEventGetFlags(event: *const c_void) -> u64;
    fn CGEventGetIntegerValueField(event: *const c_void, field: u32) -> i64;
    fn CGEventGetDoubleValueField(event: *const c_void, field: u32) -> f64;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    /// Поставить блок в очередь цикла событий (в core-foundation-sys не объявлена).
    fn CFRunLoopPerformBlock(run_loop: CFRunLoopRef, mode: CFTypeRef, block: &Block<dyn Fn()>);
}

extern "C" {
    fn clock_gettime_nsec_np(clock_id: u32) -> u64;
    fn pthread_set_qos_class_self_np(qos_class: u32, relative_priority: i32) -> i32;
}

/// `kCGSessionEventTap`.
const SESSION_EVENT_TAP: u32 = 1;
/// `kCGHeadInsertEventTap`.
const HEAD_INSERT_EVENT_TAP: u32 = 0;
/// `kCGEventTapOptionDefault` — активный тап: может менять и глотать события.
const TAP_OPTION_DEFAULT: u32 = 0;
/// `kCGEventTapDisabledByTimeout` / `kCGEventTapDisabledByUserInput`.
const TAP_DISABLED_BY_TIMEOUT: u32 = 0xFFFF_FFFE;
const TAP_DISABLED_BY_USER_INPUT: u32 = 0xFFFF_FFFF;
/// `kCGMouseEventClickState`, `kCGMouseEventDeltaY`.
const MOUSE_EVENT_CLICK_STATE: u32 = 1;
const MOUSE_EVENT_DELTA_Y: u32 = 5;
/// `kCGMouseEventWindowUnderMousePointerThatCanHandleThisEvent`.
const MOUSE_EVENT_WINDOW_THAT_CAN_HANDLE: u32 = 92;
/// `_CLOCK_UPTIME_RAW`: время с загрузки без сна — те же часы, что у
/// `NSEvent.timestamp` и `DispatchTime.now()`.
const CLOCK_UPTIME_RAW: u32 = 8;
/// `QOS_CLASS_USER_INTERACTIVE`.
const QOS_CLASS_USER_INTERACTIVE: u32 = 0x21;
/// Сколько цикл потока тапа ждёт за один заход, секунды. Остановку приносит
/// блок (`stop_run_loop`), так что таймаут — только страховка: за него поток
/// замечает и пропавший источник тапа, если событий нет.
const TAP_LOOP_TIMEOUT: f64 = 1.0;

/// Секунды с загрузки системы (без сна).
pub fn uptime_seconds() -> f64 {
    uptime_nanoseconds() as f64 / 1e9
}

/// Миллисекунды с загрузки системы (`DispatchTime.now().uptimeMilliseconds`).
pub fn uptime_milliseconds() -> u64 {
    uptime_nanoseconds() / 1_000_000
}

fn uptime_nanoseconds() -> u64 {
    unsafe { clock_gettime_nsec_np(CLOCK_UPTIME_RAW) }
}

/// Событие внутри тапа: его можно прочитать и поправить, пока оно не ушло
/// дальше по системе. Живёт только во время вызова фильтра.
pub struct TapEvent {
    raw: *mut c_void,
    kind: MouseEventKind,
}

impl TapEvent {
    pub fn kind(&self) -> MouseEventKind {
        self.kind
    }

    /// `cgEvent.location` — координаты Quartz.
    pub fn location(&self) -> (f64, f64) {
        let point = unsafe { CGEventGetLocation(self.raw) };
        (point.x, point.y)
    }

    /// `cgEvent.location = …`: событие уйдёт дальше уже с этой точкой.
    pub fn set_location(&mut self, x: f64, y: f64) {
        unsafe { CGEventSetLocation(self.raw, CGPoint { x, y }) }
    }

    /// Окно, которое, по решению WindowServer, примет это событие (номер окна;
    /// окна, пропускающие щелчки насквозь, не в счёт). `None` — поле не
    /// заполнено.
    pub fn window_under_pointer(&self) -> Option<u32> {
        let window_id =
            unsafe { CGEventGetIntegerValueField(self.raw, MOUSE_EVENT_WINDOW_THAT_CAN_HANDLE) };
        u32::try_from(window_id)
            .ok()
            .filter(|&window_id| window_id != 0)
    }

    /// `deltaY` движения (у остальных событий 0).
    pub fn delta_y(&self) -> f64 {
        if self.kind.has_delta() {
            unsafe { CGEventGetDoubleValueField(self.raw, MOUSE_EVENT_DELTA_Y) }
        } else {
            0.0
        }
    }

    /// Снимок для обработчика.
    pub fn to_mouse_event(&self) -> MouseEvent {
        MouseEvent {
            kind: self.kind,
            timestamp: uptime_seconds(),
            modifier_flags: unsafe { CGEventGetFlags(self.raw) },
            location: Some(self.location()),
            delta_y: self.delta_y(),
            click_count: if self.kind.is_button_event() {
                unsafe { CGEventGetIntegerValueField(self.raw, MOUSE_EVENT_CLICK_STATE) }
            } else {
                0
            },
        }
    }
}

// ---------------------------------------------------------------- пассивный

/// Пассивный монитор (`PassiveEventMonitor`): только главный поток.
pub struct PassiveEventMonitor {
    mask: EventMask,
    handler: Rc<dyn Fn(&MouseEvent)>,
    local: Option<Retained<AnyObject>>,
    global: Option<Retained<AnyObject>>,
}

impl PassiveEventMonitor {
    pub fn new(mask: EventMask, handler: impl Fn(&MouseEvent) + 'static) -> PassiveEventMonitor {
        PassiveEventMonitor {
            mask,
            handler: Rc::new(handler),
            local: None,
            global: None,
        }
    }

    /// Оба монитора (свои окна и чужие) установлены.
    pub fn running(&self) -> bool {
        self.local.is_some() && self.global.is_some()
    }

    pub fn start(&mut self) {
        assert!(
            MainThreadMarker::new().is_some(),
            "PassiveEventMonitor запускают с главного потока"
        );
        if self.local.is_some() || self.global.is_some() {
            return;
        }
        let mask = NSEventMask(self.mask.0 as _);

        let handler = self.handler.clone();
        let local_block = RcBlock::new(move |event: NonNull<NSEvent>| -> *mut NSEvent {
            handler(&MouseEvent::from_ns_event(unsafe { event.as_ref() }));
            event.as_ptr()
        });
        // SAFETY: блок возвращает то же событие, что получил.
        self.local =
            unsafe { NSEvent::addLocalMonitorForEventsMatchingMask_handler(mask, &local_block) };

        let handler = self.handler.clone();
        let global_block = RcBlock::new(move |event: NonNull<NSEvent>| {
            handler(&MouseEvent::from_ns_event(unsafe { event.as_ref() }));
        });
        self.global = NSEvent::addGlobalMonitorForEventsMatchingMask_handler(mask, &global_block);
    }

    pub fn stop(&mut self) {
        for monitor in [self.local.take(), self.global.take()]
            .into_iter()
            .flatten()
        {
            // SAFETY: это объект, который вернул `addLocal/GlobalMonitor…`.
            unsafe { NSEvent::removeMonitor(&monitor) };
        }
    }
}

impl Drop for PassiveEventMonitor {
    fn drop(&mut self) {
        self.stop();
    }
}

// ---------------------------------------------------------------- активный

/// Фильтр активного монитора: зовётся в потоке тапа, `true` — поглотить событие.
pub type TapFilter = Arc<dyn Fn(&mut TapEvent) -> bool + Send + Sync>;

/// Обработчик активного монитора: зовётся на главном потоке.
pub type TapHandler = Arc<dyn Fn(MouseEvent) + Send + Sync>;

/// Всё, что нужно колбэку тапа. Живёт дольше потока тапа: освобождается в
/// `stop` после того, как поток завершился.
struct TapContext {
    mask: EventMask,
    filter: TapFilter,
    handler: TapHandler,
    port: AtomicPtr<c_void>,
}

/// Указатель, который можно передать в поток тапа и обратно.
struct SendPtr(*mut c_void);

unsafe impl Send for SendPtr {}

struct Tap {
    thread: JoinHandle<()>,
    /// Цикл потока тапа (+1: держим до конца `stop`, иначе он освобождается
    /// вместе с потоком, а `stop` ещё обращается к нему).
    run_loop: CFRunLoopRef,
    port: CFMachPortRef,
    source: CFRunLoopSourceRef,
    context: *mut TapContext,
    stop: Arc<AtomicBool>,
}

/// Активный монитор (`ActiveEventMonitor`): тап сессии в своём потоке.
pub struct ActiveEventMonitor {
    mask: EventMask,
    filter: TapFilter,
    handler: TapHandler,
    tap: Option<Tap>,
}

impl ActiveEventMonitor {
    pub fn new(mask: EventMask, filter: TapFilter, handler: TapHandler) -> ActiveEventMonitor {
        ActiveEventMonitor {
            mask,
            filter,
            handler,
            tap: None,
        }
    }

    /// Тап создан (без доступа к управлению компьютером система его не даёт)
    /// и его поток жив: если источник тапа пропал, поток завершается, и
    /// владелец перезапустит монитор при следующей сверке с настройками.
    pub fn running(&self) -> bool {
        self.tap
            .as_ref()
            .is_some_and(|tap| !tap.thread.is_finished())
    }

    pub fn start(&mut self) {
        if self.tap.is_some() {
            return;
        }
        let context = Box::into_raw(Box::new(TapContext {
            mask: self.mask,
            filter: self.filter.clone(),
            handler: self.handler.clone(),
            port: AtomicPtr::new(ptr::null_mut()),
        }));
        // SAFETY: колбэк получает `context`, который живёт до конца `stop`.
        let port = unsafe {
            CGEventTapCreate(
                SESSION_EVENT_TAP,
                HEAD_INSERT_EVENT_TAP,
                TAP_OPTION_DEFAULT,
                self.mask.0,
                tap_callback,
                context as *mut c_void,
            )
        };
        if port.is_null() {
            drop(unsafe { Box::from_raw(context) });
            log!("Монитор мыши: система не дала перехват событий (CGEventTap)");
            return;
        }
        unsafe {
            (*context)
                .port
                .store(port as *mut c_void, Ordering::Release)
        };
        let source = unsafe { CFMachPortCreateRunLoopSource(kCFAllocatorDefault, port, 0) };
        if source.is_null() {
            unsafe {
                CFMachPortInvalidate(port);
                CFRelease(port as *const c_void);
                drop(Box::from_raw(context));
            }
            return;
        }

        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = stop.clone();
        let thread_source = SendPtr(source as *mut c_void);
        let thread_port = SendPtr(port as *mut c_void);
        let (sender, receiver) = mpsc::channel::<SendPtr>();
        let spawned = std::thread::Builder::new()
            .name("r2-event-tap".to_string())
            .spawn(move || {
                let (source, port) = (thread_source, thread_port);
                unsafe {
                    pthread_set_qos_class_self_np(QOS_CLASS_USER_INTERACTIVE, 0);
                    let run_loop = CFRunLoopGetCurrent();
                    CFRunLoopAddSource(
                        run_loop,
                        source.0 as CFRunLoopSourceRef,
                        kCFRunLoopDefaultMode,
                    );
                    CFRetain(run_loop as *const c_void);
                    let _ = sender.send(SendPtr(run_loop as *mut c_void));
                }
                if !run_tap_loop(&thread_stop) {
                    // Событий принимать некому: пусть WindowServer их не ждёт.
                    unsafe { CGEventTapEnable(port.0 as CFMachPortRef, false) };
                    log!("Монитор мыши: источник перехвата событий пропал — поток перехвата остановлен");
                }
            });
        let (thread, run_loop) = match spawned.map(|thread| (thread, receiver.recv())) {
            Ok((thread, Ok(run_loop))) => (thread, run_loop.0 as CFRunLoopRef),
            _ => {
                unsafe {
                    CFMachPortInvalidate(port);
                    CFRelease(source as *const c_void);
                    CFRelease(port as *const c_void);
                    drop(Box::from_raw(context));
                }
                log!("Монитор мыши: не удалось запустить поток перехвата событий");
                return;
            }
        };
        self.tap = Some(Tap {
            thread,
            run_loop,
            port,
            source,
            context,
            stop,
        });
    }

    pub fn stop(&mut self) {
        let Some(tap) = self.tap.take() else {
            return;
        };
        tap.stop.store(true, Ordering::Release);
        unsafe {
            CGEventTapEnable(tap.port, false);
            stop_run_loop(tap.run_loop);
        }
        let _ = tap.thread.join();
        // Поток завершился — колбэк больше не вызовется. Без явного
        // `CFMachPortInvalidate` WindowServer держал бы выключенный тап до
        // выхода из программы (так в оригинале).
        unsafe {
            CFMachPortInvalidate(tap.port);
            CFRelease(tap.source as *const c_void);
            CFRelease(tap.port as *const c_void);
            CFRelease(tap.run_loop as *const c_void);
            drop(Box::from_raw(tap.context));
        }
    }
}

impl Drop for ActiveEventMonitor {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Цикл событий потока тапа: крутится, пока не попросили остановиться
/// (`true`). `false` — в режиме не осталось источников (источник тапа сняли
/// или его порт умер): ждать больше нечего, а `CFRunLoopRunInMode` без
/// источников возвращается сразу, и цикл занял бы ядро вхолостую.
fn run_tap_loop(stop: &AtomicBool) -> bool {
    while !stop.load(Ordering::Acquire) {
        let result = unsafe { CFRunLoopRunInMode(kCFRunLoopDefaultMode, TAP_LOOP_TIMEOUT, 0) };
        if result == kCFRunLoopRunFinished {
            return false;
        }
    }
    true
}

/// Разбудить цикл `run_loop` и остановить его (с любого потока). Одного
/// `CFRunLoopStop` мало: если поток ещё не вошёл в `CFRunLoopRunInMode`, стоп
/// пропадает, и поток спит до таймаута. Блок из очереди цикла выполнится при
/// ближайшем заходе — и тот сразу вернётся.
///
/// # Safety
/// `run_loop` — живой цикл (держим его ссылкой).
unsafe fn stop_run_loop(run_loop: CFRunLoopRef) {
    let block = RcBlock::new(|| unsafe { CFRunLoopStop(CFRunLoopGetCurrent()) });
    CFRunLoopPerformBlock(run_loop, kCFRunLoopDefaultMode as CFTypeRef, &block);
    CFRunLoopWakeUp(run_loop);
}

extern "C" fn tap_callback(
    _proxy: *mut c_void,
    event_type: u32,
    event: *mut c_void,
    user_info: *mut c_void,
) -> *mut c_void {
    if user_info.is_null() {
        return event;
    }
    // SAFETY: `user_info` — `TapContext` из `start`, живёт, пока жив поток тапа.
    let context = unsafe { &*(user_info as *const TapContext) };
    if event_type == TAP_DISABLED_BY_TIMEOUT || event_type == TAP_DISABLED_BY_USER_INPUT {
        // Система выключила тап (долгий колбэк): включаем обратно.
        let port = context.port.load(Ordering::Acquire);
        if !port.is_null() {
            unsafe { CGEventTapEnable(port as CFMachPortRef, true) };
        }
        return event;
    }
    let kind = MouseEventKind::from_raw(event_type);
    if event.is_null() || !context.mask.contains(kind) {
        return event;
    }
    let mut tap_event = TapEvent { raw: event, kind };
    let filtered = (context.filter)(&mut tap_event);
    let mouse_event = tap_event.to_mouse_event();
    let handler = context.handler.clone();
    crate::events::run_on_main(move || handler(mouse_event));
    if filtered {
        ptr::null_mut()
    } else {
        event
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core_foundation_sys::runloop::{
        kCFRunLoopRunStopped, CFRunLoopSourceContext, CFRunLoopSourceCreate,
        CFRunLoopSourceInvalidate,
    };
    use std::time::{Duration, Instant};

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGEventCreateMouseEvent(
            source: *const c_void,
            mouse_type: u32,
            cursor: CGPoint,
            button: u32,
        ) -> *mut c_void;
        fn CGEventSetIntegerValueField(event: *mut c_void, field: u32, value: i64);
    }

    #[test]
    fn window_under_pointer_comes_from_the_event() {
        // Событие только создаётся в памяти, в систему не посылается.
        let raw =
            unsafe { CGEventCreateMouseEvent(ptr::null(), 1, CGPoint { x: 10.0, y: 20.0 }, 0) };
        assert!(!raw.is_null());
        let event = TapEvent {
            raw,
            kind: MouseEventKind::LeftMouseDown,
        };
        assert_eq!(event.window_under_pointer(), None, "поле не заполнено");
        unsafe { CGEventSetIntegerValueField(raw, MOUSE_EVENT_WINDOW_THAT_CAN_HANDLE, 4321) };
        assert_eq!(event.window_under_pointer(), Some(4321));
        // Окно под курсором вообще (91) — не то: оно может пропускать щелчки
        // насквозь, как прозрачные окна Dock и Центра уведомлений.
        unsafe {
            CGEventSetIntegerValueField(raw, MOUSE_EVENT_WINDOW_THAT_CAN_HANDLE, 0);
            CGEventSetIntegerValueField(raw, 91, 99);
        }
        assert_eq!(event.window_under_pointer(), None);
        unsafe { CFRelease(raw as *const c_void) };
    }

    extern "C" fn never_fires(_info: *const c_void) {}

    /// Источник-пустышка вместо порта тапа (+1): сам никогда не срабатывает.
    fn dummy_source() -> CFRunLoopSourceRef {
        let mut context = CFRunLoopSourceContext {
            version: 0,
            info: ptr::null_mut(),
            retain: None,
            release: None,
            copyDescription: None,
            equal: None,
            hash: None,
            schedule: None,
            cancel: None,
            perform: never_fires,
        };
        unsafe { CFRunLoopSourceCreate(kCFAllocatorDefault, 0, &mut context) }
    }

    /// Как поток тапа: цикл потока (+1) с источником в режиме по умолчанию.
    unsafe fn run_loop_with_source(source: CFRunLoopSourceRef) -> CFRunLoopRef {
        let run_loop = CFRunLoopGetCurrent();
        CFRunLoopAddSource(run_loop, source, kCFRunLoopDefaultMode);
        CFRetain(run_loop as *const c_void);
        run_loop
    }

    #[test]
    fn stop_that_comes_before_the_loop_starts_is_not_lost() {
        let (sender, receiver) = mpsc::channel::<SendPtr>();
        let (go, wait_for_go) = mpsc::channel::<()>();
        let thread = std::thread::spawn(move || unsafe {
            let source = dummy_source();
            let run_loop = run_loop_with_source(source);
            sender.send(SendPtr(run_loop as *mut c_void)).unwrap();
            // Поток проверил флаг остановки и только теперь входит в цикл.
            wait_for_go.recv().unwrap();
            let started = Instant::now();
            let result = CFRunLoopRunInMode(kCFRunLoopDefaultMode, 5.0, 0);
            CFRelease(source as *const c_void);
            (result, started.elapsed())
        });
        let run_loop = receiver.recv().unwrap().0 as CFRunLoopRef;
        // Остановка — до входа в цикл: одного `CFRunLoopStop` здесь было бы
        // мало, поток проспал бы весь таймаут.
        unsafe { stop_run_loop(run_loop) };
        go.send(()).unwrap();
        let (result, waited) = thread.join().unwrap();
        unsafe { CFRelease(run_loop as *const c_void) };
        assert_eq!(result, kCFRunLoopRunStopped);
        assert!(waited < Duration::from_secs(1), "{waited:?}");
    }

    #[test]
    fn tap_loop_ends_by_itself_when_its_source_is_gone() {
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = stop.clone();
        let (sender, receiver) = mpsc::channel::<(SendPtr, SendPtr)>();
        let thread = std::thread::spawn(move || {
            let source = dummy_source();
            let run_loop = unsafe { run_loop_with_source(source) };
            let pointers = (
                SendPtr(run_loop as *mut c_void),
                SendPtr(source as *mut c_void),
            );
            sender.send(pointers).unwrap();
            run_tap_loop(&thread_stop)
        });
        let (run_loop, source) = receiver.recv().unwrap();
        let (run_loop, source) = (run_loop.0 as CFRunLoopRef, source.0 as CFRunLoopSourceRef);
        // Источник пропал, как у умершего порта тапа: цикл без источников
        // возвращается сразу, и поток должен выйти, а не крутиться вхолостую.
        unsafe { CFRunLoopSourceInvalidate(source) };
        let deadline = Instant::now() + Duration::from_secs(5);
        while !thread.is_finished() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        let ended_by_itself = thread.is_finished();
        // Если не вышел — остановить, чтобы тест не завис.
        stop.store(true, Ordering::Release);
        unsafe { stop_run_loop(run_loop) };
        let stopped_on_request = thread.join().unwrap();
        unsafe {
            CFRelease(source as *const c_void);
            CFRelease(run_loop as *const c_void);
        }
        assert!(ended_by_itself);
        assert!(!stopped_on_request, "поток заметил, что источника нет");
    }

    #[test]
    fn tap_loop_stops_on_request() {
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = stop.clone();
        let (sender, receiver) = mpsc::channel::<SendPtr>();
        let thread = std::thread::spawn(move || {
            let source = dummy_source();
            let run_loop = unsafe { run_loop_with_source(source) };
            sender.send(SendPtr(run_loop as *mut c_void)).unwrap();
            let stopped = run_tap_loop(&thread_stop);
            unsafe { CFRelease(source as *const c_void) };
            stopped
        });
        let run_loop = receiver.recv().unwrap().0 as CFRunLoopRef;
        std::thread::sleep(Duration::from_millis(50));
        let started = Instant::now();
        stop.store(true, Ordering::Release);
        unsafe { stop_run_loop(run_loop) };
        let stopped_on_request = thread.join().unwrap();
        let waited = started.elapsed();
        unsafe { CFRelease(run_loop as *const c_void) };
        assert!(stopped_on_request);
        assert!(waited < Duration::from_millis(500), "{waited:?}");
    }

    #[test]
    fn event_kinds_round_trip_and_masks_match_appkit() {
        for raw in 0..10 {
            assert_eq!(MouseEventKind::from_raw(raw).raw(), raw);
        }
        let mask =
            EventMask::LEFT_MOUSE_DOWN | EventMask::LEFT_MOUSE_UP | EventMask::LEFT_MOUSE_DRAGGED;
        // Те же биты, что у `NSEvent.EventTypeMask`: [.leftMouseDown, .leftMouseUp, .leftMouseDragged].
        assert_eq!(
            mask.0,
            (NSEventMask::LeftMouseDown | NSEventMask::LeftMouseUp | NSEventMask::LeftMouseDragged)
                .0
        );
        assert!(mask.contains(MouseEventKind::LeftMouseDragged));
        assert!(!mask.contains(MouseEventKind::RightMouseDown));
        assert!(!mask.contains(MouseEventKind::Other(200)));
    }

    #[test]
    fn device_independent_flags_drop_device_bits() {
        let event = MouseEvent {
            kind: MouseEventKind::LeftMouseDragged,
            timestamp: 0.0,
            // ⌥ (1 << 19) + биты конкретной клавиши и «не склеено» (0x100).
            modifier_flags: (1 << 19) | 0x20 | 0x100,
            location: None,
            delta_y: 0.0,
            click_count: 1,
        };
        assert_eq!(event.device_independent_flags(), 1 << 19);
    }

    #[test]
    fn uptime_clock_runs() {
        let first = uptime_milliseconds();
        let seconds = uptime_seconds();
        assert!(first > 0);
        assert!(seconds * 1000.0 + 1.0 >= first as f64);
    }
}
