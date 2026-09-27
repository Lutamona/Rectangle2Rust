//! Доступ к окнам через Accessibility API (ApplicationServices) — порт
//! `AccessibilityElement.swift`, `AXExtension.swift` и `WindowUtil.swift`.
//!
//! Всё — через сырой FFI (`extern "C"`), без промежуточных крейтов.
//!
//! Правила, которые соблюдены здесь:
//! - Чтения не паникуют: ошибка AX, отсутствующий атрибут или значение не того
//!   типа — это `None`. Тип CF проверяется до распаковки (`CFGetTypeID`), как в
//!   Swift (`AXExtension.swift:47`, `AccessibilityElement.swift:27,32`).
//! - Сеттеры возвращают, приняла ли система значение (`AXError == success`).
//!   Как и в Swift, менеджер окон на это не опирается: итог он проверяет
//!   повторным чтением рамки.
//! - Память CF: `AXUIElementCopyAttributeValue` возвращает +1 объект —
//!   каждый такой указатель либо освобождается через `CFRelease`, либо
//!   заворачивается во владеющую обёртку. Элементы массивов (+0) получают
//!   `CFRetain` раньше, чем освобождается сам массив.
//! - Координаты: `frame()`/`set_frame()` работают в AX-координатах
//!   (origin сверху слева) и НЕ переворачивают их — переворот в Cocoa
//!   делает вызывающий код через `Rect::screen_flipped`.
//! - Без разрешений Accessibility `frame()`/`front_window()` вернут `None`,
//!   `is_process_trusted(false)` — false; это нормально, падений нет.
//!   `window_list()` разрешений не требует.
//! - Список окон, как `WindowUtil.getWindowList`, живёт 100 мс: повторный запрос
//!   в эти 100 мс отдаёт тот же снимок, а не идёт в WindowServer.

use std::collections::HashSet;
use std::ffi::c_void;
use std::hash::{Hash, Hasher};
use std::ptr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use core_foundation::base::TCFType;
use core_foundation::boolean::{CFBoolean, CFBooleanRef};
use core_foundation::dictionary::CFDictionary;
use core_foundation::number::{CFNumber, CFNumberRef};
use core_foundation::string::{CFString, CFStringRef};
use core_foundation_sys::array::{
    CFArrayCreate, CFArrayGetCount, CFArrayGetTypeID, CFArrayGetValueAtIndex, CFArrayRef,
};
use core_foundation_sys::base::{
    kCFAllocatorDefault, CFEqual, CFGetTypeID, CFHash, CFRelease, CFRetain, CFTypeID,
};
use core_foundation_sys::dictionary::{CFDictionaryGetValue, CFDictionaryRef};
use core_foundation_sys::number::{CFBooleanGetTypeID, CFNumberGetTypeID};
use core_foundation_sys::string::CFStringGetTypeID;
use objc2_app_kit::{NSApplicationActivationOptions, NSRunningApplication, NSWorkspace};

use crate::config::{self, EnhancedUI};
use crate::geometry::Rect;
use crate::{screens, stage};

// ---------------------------------------------------------------------------
// FFI: ApplicationServices (AXUIElement) + CoreFoundation (AXValue)
// ---------------------------------------------------------------------------

/// `AXError`, успех.
const K_AX_ERROR_SUCCESS: i32 = 0;
/// `AXValueType::kAXValueCGPointType`.
const K_AX_VALUE_CGPOINT: u32 = 1;
/// `AXValueType::kAXValueCGSizeType`.
const K_AX_VALUE_CGSIZE: u32 = 2;

/// Сколько ждать ответа приложения про `AXEnhancedUserInterface` перед
/// установкой рамки, с (`AxElement::set_frame_ordered`).
const ENHANCED_UI_TIMEOUT: f32 = 1.0;

/// Точка в layout `CGPoint` (`CGFloat` = `f64` на 64-битной macOS).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
struct RawPoint {
    x: f64,
    y: f64,
}

/// Размер в layout `CGSize`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
struct RawSize {
    width: f64,
    height: f64,
}

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXUIElementCreateApplication(pid: i32) -> *const c_void;
    fn AXUIElementCreateSystemWide() -> *const c_void;
    fn AXUIElementCopyAttributeValue(
        element: *const c_void,
        attr: CFStringRef,
        value: *mut *const c_void,
    ) -> i32;
    fn AXUIElementSetAttributeValue(
        element: *const c_void,
        attr: CFStringRef,
        value: *const c_void,
    ) -> i32;
    fn AXUIElementIsAttributeSettable(
        element: *const c_void,
        attr: CFStringRef,
        settable: *mut u8,
    ) -> i32;
    fn AXUIElementGetPid(element: *const c_void, pid: *mut i32) -> i32;
    fn AXUIElementSetMessagingTimeout(element: *const c_void, timeout: f32) -> i32;
    fn AXUIElementCopyElementAtPosition(
        element: *const c_void,
        x: f32,
        y: f32,
        out: *mut *const c_void,
    ) -> i32;
    fn AXIsProcessTrustedWithOptions(options: CFDictionaryRef) -> u8;
    fn AXUIElementGetTypeID() -> CFTypeID;
    fn AXValueGetTypeID() -> CFTypeID;
    fn AXValueCreate(value_type: u32, value_ptr: *const c_void) -> *const c_void;
    fn AXValueGetType(value: *const c_void) -> u32;
    fn AXValueGetValue(value: *const c_void, value_type: u32, value_ptr: *mut c_void) -> u8;
    fn _AXUIElementGetWindow(element: *const c_void, window_id: *mut u32) -> i32;
}

// ---------------------------------------------------------------------------
// AxElement
// ---------------------------------------------------------------------------

/// Владеющая обёртка над `AXUIElementRef` (CFTypeRef, +1 retain) —
/// `AccessibilityElement` оригинала.
///
/// AX-функции потокобезопасны (IPC под капотом), поэтому `Send` + `Sync`.
/// Равенство и хеш — как у `AXUIElement` в Swift (`CFEqual`/`CFHash`).
pub struct AxElement {
    raw: *const c_void,
    /// Окно, вытащенное из полосы Stage Manager (`StageWindowAccessibilityElement`):
    /// номер окна известен заранее, а положение берётся из списка окон.
    stage_window_id: Option<u32>,
}

unsafe impl Send for AxElement {}
unsafe impl Sync for AxElement {}

impl Clone for AxElement {
    fn clone(&self) -> Self {
        unsafe {
            CFRetain(self.raw);
        }
        AxElement {
            raw: self.raw,
            stage_window_id: self.stage_window_id,
        }
    }
}

impl Drop for AxElement {
    fn drop(&mut self) {
        unsafe {
            CFRelease(self.raw);
        }
    }
}

impl PartialEq for AxElement {
    fn eq(&self, other: &Self) -> bool {
        unsafe { CFEqual(self.raw, other.raw) != 0 }
    }
}

impl Eq for AxElement {}

impl Hash for AxElement {
    fn hash<H: Hasher>(&self, state: &mut H) {
        unsafe { CFHash(self.raw) }.hash(state);
    }
}

impl std::fmt::Debug for AxElement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AxElement")
            .field("pid", &self.pid())
            .finish()
    }
}

/// Производный номер окна (#640): старший бит уводит его из пространства
/// настоящих номеров, которые WindowServer раздаёт по порядку.
/// `AccessibilityElement.deriveWindowId(fromElementHash:)`.
pub fn derive_window_id(element_hash: usize) -> u32 {
    0x8000_0000 | ((element_hash as u64 as u32) & 0x7FFF_FFFF)
}

/// Номер получен из хеша элемента (`derive_window_id`), а не от WindowServer:
/// в списке окон такого нет.
pub fn is_derived_window_id(window_id: u32) -> bool {
    window_id & 0x8000_0000 != 0
}

impl AxElement {
    /// Забрать +1 объект, если это действительно `AXUIElement`; иначе освободить.
    unsafe fn from_owned(value: *const c_void) -> Option<AxElement> {
        if value.is_null() {
            return None;
        }
        if CFGetTypeID(value) != AXUIElementGetTypeID() {
            CFRelease(value);
            return None;
        }
        Some(AxElement {
            raw: value,
            stage_window_id: None,
        })
    }

    /// `AXUIElementCreateApplication` — элемент приложения по pid.
    pub fn application(pid: i32) -> AxElement {
        AxElement {
            raw: unsafe { AXUIElementCreateApplication(pid) },
            stage_window_id: None,
        }
    }

    /// Элемент запущенного приложения по bundle id (`init?(_ bundleIdentifier:)`).
    pub fn for_bundle_id(bundle_id: &str) -> Option<AxElement> {
        pid_for_bundle(bundle_id).map(AxElement::application)
    }

    /// Элемент в точке экрана, координаты AX (`init?(_ position:)`).
    pub fn at_position(x: f64, y: f64) -> Option<AxElement> {
        element_at_position(x, y)
    }

    /// `AXUIElementGetPid`.
    pub fn pid(&self) -> Option<i32> {
        let mut pid: i32 = 0;
        let err = unsafe { AXUIElementGetPid(self.raw, &mut pid) };
        if err == K_AX_ERROR_SUCCESS {
            Some(pid)
        } else {
            None
        }
    }

    /// Сырое чтение атрибута: возвращает +1 объект, владелец — вызывающий.
    fn copy_attribute(&self, name: &str) -> Option<*const c_void> {
        let attr = CFString::new(name);
        let mut value: *const c_void = ptr::null();
        let err = unsafe {
            AXUIElementCopyAttributeValue(self.raw, attr.as_concrete_TypeRef(), &mut value)
        };
        if err == K_AX_ERROR_SUCCESS && !value.is_null() {
            Some(value)
        } else {
            None
        }
    }

    /// Строковый атрибут (проверяем, что значение правда `CFString`).
    fn string_attribute(&self, name: &str) -> Option<String> {
        let value = self.copy_attribute(name)?;
        unsafe {
            if CFGetTypeID(value) != CFStringGetTypeID() {
                CFRelease(value);
                return None;
            }
            let s: CFString = TCFType::wrap_under_create_rule(value as CFStringRef);
            Some(s.to_string())
        }
    }

    /// Булев атрибут (`as? Bool`: `CFBoolean` или число 0/1).
    fn bool_attribute(&self, name: &str) -> Option<bool> {
        let value = self.copy_attribute(name)?;
        unsafe {
            let result = cf_to_bool(value);
            CFRelease(value);
            result
        }
    }

    /// Атрибут-ссылка на другой AX-элемент (проверяем тип `AXUIElement`).
    /// Возвращаемый `AxElement` забирает +1 владение.
    fn element_attribute(&self, name: &str) -> Option<AxElement> {
        let value = self.copy_attribute(name)?;
        unsafe { AxElement::from_owned(value) }
    }

    /// Атрибут-массив элементов (`value as? [AXUIElement]`: если хоть один элемент
    /// не `AXUIElement`, весь массив не принимается — как в Swift).
    fn elements_attribute(&self, name: &str) -> Option<Vec<AxElement>> {
        let value = self.copy_attribute(name)?;
        unsafe {
            if CFGetTypeID(value) != CFArrayGetTypeID() {
                CFRelease(value);
                return None;
            }
            let array = value as CFArrayRef;
            let count = CFArrayGetCount(array);
            let mut result = Vec::with_capacity(count.max(0) as usize);
            for index in 0..count {
                let item = CFArrayGetValueAtIndex(array, index);
                if item.is_null() || CFGetTypeID(item) != AXUIElementGetTypeID() {
                    CFRelease(value);
                    return None;
                }
                // Элемент массива — borrowed (+0): забираем retain до освобождения массива.
                CFRetain(item);
                result.push(AxElement {
                    raw: item,
                    stage_window_id: None,
                });
            }
            CFRelease(value);
            Some(result)
        }
    }

    /// Распаковать `AXValue`-атрибут заданного типа.
    fn value_attribute<T: Default>(&self, name: &str, value_type: u32) -> Option<T> {
        let value = self.copy_attribute(name)?;
        let result = unsafe { ax_value::<T>(value, value_type) };
        unsafe {
            CFRelease(value);
        }
        result
    }

    fn set_value_attribute<T>(&self, name: &str, value_type: u32, value: &T) -> bool {
        unsafe {
            let ax_value = AXValueCreate(value_type, ptr::from_ref(value) as *const c_void);
            if ax_value.is_null() {
                return false;
            }
            let attr = CFString::new(name);
            let err = AXUIElementSetAttributeValue(self.raw, attr.as_concrete_TypeRef(), ax_value);
            CFRelease(ax_value);
            err == K_AX_ERROR_SUCCESS
        }
    }

    fn set_bool_attribute(&self, name: &str, value: bool) -> bool {
        let attr = CFString::new(name);
        let flag = CFBoolean::from(value);
        // `flag` — синглтон (`get rule`), setter значение не забирает.
        let err = unsafe {
            AXUIElementSetAttributeValue(self.raw, attr.as_concrete_TypeRef(), flag.as_CFTypeRef())
        };
        err == K_AX_ERROR_SUCCESS
    }

    // ------------------------------------------------------------ роли

    /// `AXRole`.
    pub fn role(&self) -> Option<String> {
        self.string_attribute("AXRole")
    }

    /// `AXSubrole`.
    pub fn subrole(&self) -> Option<String> {
        self.string_attribute("AXSubrole")
    }

    fn has_role(&self, role: &str) -> bool {
        self.role().as_deref() == Some(role)
    }

    fn has_subrole(&self, subrole: &str) -> bool {
        self.subrole().as_deref() == Some(subrole)
    }

    /// `AXRole == AXApplication`.
    pub fn is_application(&self) -> bool {
        self.has_role("AXApplication")
    }

    /// `AXRole == AXWindow`.
    pub fn is_window(&self) -> bool {
        self.has_role("AXWindow")
    }

    /// `AXRole == AXSheet` — лист, прикреплённый к окну. `AXSheet` — значение
    /// роли, а не подроли (`NSAccessibility.Role.sheet`).
    pub fn is_sheet(&self) -> bool {
        self.has_role("AXSheet")
    }

    /// `AXRole == AXToolbar`.
    pub fn is_toolbar(&self) -> bool {
        self.has_role("AXToolbar")
    }

    /// `AXRole == AXGroup`.
    pub fn is_group(&self) -> bool {
        self.has_role("AXGroup")
    }

    /// `AXRole == AXTabGroup`.
    pub fn is_tab_group(&self) -> bool {
        self.has_role("AXTabGroup")
    }

    /// `AXRole == AXStaticText`.
    pub fn is_static_text(&self) -> bool {
        self.has_role("AXStaticText")
    }

    /// `AXSubrole == AXSystemDialog`.
    pub fn is_system_dialog(&self) -> bool {
        self.has_subrole("AXSystemDialog")
    }

    /// `AXSubrole == AXFullScreenButton`.
    pub fn is_full_screen_button(&self) -> bool {
        self.has_subrole("AXFullScreenButton")
    }

    // ------------------------------------------------------------ рамка

    /// `AXPosition` (координаты AX).
    pub fn position(&self) -> Option<(f64, f64)> {
        self.value_attribute::<RawPoint>("AXPosition", K_AX_VALUE_CGPOINT)
            .map(|point| (point.x, point.y))
    }

    /// `AXSize`.
    pub fn size(&self) -> Option<(f64, f64)> {
        self.value_attribute::<RawSize>("AXSize", K_AX_VALUE_CGSIZE)
            .map(|size| (size.width, size.height))
    }

    /// Поставить `AXPosition` (координаты AX). Без обхода AXEnhancedUserInterface —
    /// как сеттер `position` в Swift.
    pub fn set_position(&self, x: f64, y: f64) -> bool {
        self.set_value_attribute("AXPosition", K_AX_VALUE_CGPOINT, &RawPoint { x, y })
    }

    /// Поставить `AXSize`. Без обхода AXEnhancedUserInterface — как сеттер `size`
    /// в Swift (им пользуется возврат размера при отрыве окна в drag-to-snap).
    pub fn set_size(&self, w: f64, h: f64) -> bool {
        self.set_value_attribute(
            "AXSize",
            K_AX_VALUE_CGSIZE,
            &RawSize {
                width: w,
                height: h,
            },
        )
    }

    /// `AXPosition` + `AXSize` → `Rect` (координаты AX: origin сверху слева).
    /// У окна из полосы Stage Manager положение — из списка окон.
    pub fn frame(&self) -> Option<Rect> {
        let (x, y) = self.position()?;
        let (w, h) = self.size()?;
        let frame = Rect::new(x, y, w, h);
        if let Some(window_id) = self.stage_window_id {
            if let Some(info) = window_list_for(&[window_id]).first() {
                return Some(Rect::new(info.frame.x, info.frame.y, w, h));
            }
        }
        Some(frame)
    }

    /// Установка рамки: size → position → size.
    ///
    /// AX меняет размер и положение только по отдельности. При переезде на другой
    /// экран macOS подгоняет размер под текущий экран, поэтому сначала размер,
    /// потом положение, потом размер ещё раз. Координаты AX, без переворота.
    pub fn set_frame(&self, rect: &Rect) {
        self.set_frame_ordered(rect, true);
    }

    /// То же, но с выбором порядка: некоторым действиям (углы при переборе по
    /// оси) сначала нужно переместить окно, и только потом менять размер.
    ///
    /// Обход AXEnhancedUserInterface — внутри каждой установки рамки, как в
    /// `AccessibilityElement.setFrame`: пока у приложения включён этот флаг
    /// (его ставят VoiceOver и ряд утилит), оно двигает окна анимацией и может
    /// поставить их неточно. Флаг живёт у элемента приложения. Вернуть его
    /// обратно — только в режиме `enhancedUI = disableEnable`. Флаг не
    /// прочитался (приложение не ответило за `ENHANCED_UI_TIMEOUT`) — его не
    /// трогаем.
    pub fn set_frame_ordered(&self, rect: &Rect, adjust_size_first: bool) {
        let app_element = self.enhanced_ui_application();
        let mut enhanced_ui = None;
        if let Some(app_element) = &app_element {
            enhanced_ui = app_element.bool_attribute("AXEnhancedUserInterface");
            if enhanced_ui == Some(true) {
                app_element.set_bool_attribute("AXEnhancedUserInterface", false);
            }
        }

        if adjust_size_first {
            self.set_size(rect.w, rect.h);
        }
        self.set_position(rect.x, rect.y);
        self.set_size(rect.w, rect.h);

        if enhanced_ui == Some(true)
            && config::with(|config| config.enhanced_ui) == EnhancedUI::DisableEnable
        {
            if let Some(app_element) = &app_element {
                app_element.set_bool_attribute("AXEnhancedUserInterface", true);
            }
        }
    }

    /// Элемент приложения этого окна для `AXEnhancedUserInterface` при установке
    /// рамки — всегда новый и с коротким таймаутом: у нового элемента таймаут
    /// системный (~6 с), и зависшее приложение держало бы главный поток на
    /// каждой установке рамки («столбики» ставят её до семи раз на окно). Новый
    /// объект, а не `self`, чтобы таймаут не достался элементу вызывающего.
    fn enhanced_ui_application(&self) -> Option<AxElement> {
        let app_element = AxElement::application(self.pid()?);
        app_element.set_messaging_timeout(ENHANCED_UI_TIMEOUT);
        Some(app_element)
    }

    /// `AXUIElementIsAttributeSettable(AXSize)`; при ошибке — true
    /// (не блокируем действие, если система не ответила).
    pub fn is_resizable(&self) -> bool {
        let attr = CFString::new("AXSize");
        let mut settable: u8 = 0;
        let err = unsafe {
            AXUIElementIsAttributeSettable(self.raw, attr.as_concrete_TypeRef(), &mut settable)
        };
        if err != K_AX_ERROR_SUCCESS {
            true
        } else {
            settable != 0
        }
    }

    /// Минимальный размер, который требует приложение (`AXMinSize`, иначе `AXMinimumSize`).
    pub fn minimum_size(&self) -> Option<(f64, f64)> {
        ["AXMinSize", "AXMinimumSize"].iter().find_map(|attribute| {
            self.value_attribute::<RawSize>(attribute, K_AX_VALUE_CGSIZE)
                .map(|size| (size.width, size.height))
        })
    }

    // ------------------------------------------------------------ дети

    /// Дочерние элементы (`AXChildren`); пусто, если их нет или атрибут не читается.
    pub fn children(&self) -> Vec<AxElement> {
        self.elements_attribute("AXChildren").unwrap_or_default()
    }

    /// Первый дочерний элемент с ролью `role` (`getChildElement(_ role:)`).
    pub fn child_with_role(&self, role: &str) -> Option<AxElement> {
        self.children()
            .into_iter()
            .find(|child| child.has_role(role))
    }

    /// Все дочерние элементы с ролью `role`; `None`, если таких нет
    /// (`getChildElements(_ role:)`).
    pub fn children_with_role(&self, role: &str) -> Option<Vec<AxElement>> {
        let children: Vec<AxElement> = self
            .children()
            .into_iter()
            .filter(|child| child.has_role(role))
            .collect();
        if children.is_empty() {
            None
        } else {
            Some(children)
        }
    }

    /// Первый дочерний элемент с подролью `subrole`.
    pub fn child_with_subrole(&self, subrole: &str) -> Option<AxElement> {
        self.children()
            .into_iter()
            .find(|child| child.has_subrole(subrole))
    }

    /// Все дочерние элементы с подролью `subrole`; `None`, если таких нет.
    pub fn children_with_subrole(&self, subrole: &str) -> Option<Vec<AxElement>> {
        let children: Vec<AxElement> = self
            .children()
            .into_iter()
            .filter(|child| child.has_subrole(subrole))
            .collect();
        if children.is_empty() {
            None
        } else {
            Some(children)
        }
    }

    /// Самый глубокий элемент под точкой (координаты AX): на каждом шаге —
    /// наименьший по площади ребёнок, чья рамка содержит точку
    /// (`getSelfOrChildElementRecursively`, для двойного клика по заголовку).
    pub fn self_or_child_at(&self, x: f64, y: f64) -> AxElement {
        let mut element = self.clone();
        let mut visited: HashSet<AxElement> = HashSet::new();
        loop {
            let child = element
                .children()
                .into_iter()
                .filter_map(|child| child.frame().map(|frame| (child, frame)))
                .filter(|(_, frame)| rect_contains_point(frame, x, y))
                .min_by(|(_, a), (_, b)| {
                    (a.w * a.h)
                        .partial_cmp(&(b.w * b.h))
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .map(|(child, _)| child);
            match child {
                Some(child) if visited.insert(child.clone()) => element = child,
                _ => return element,
            }
        }
    }

    // ------------------------------------------------------------ окно

    /// Номер окна: `_AXUIElementGetWindow` (приватная, но стабильная функция).
    /// У окна из полосы Stage Manager — номер, с которым его создали.
    pub fn window_id(&self) -> Option<u32> {
        if let Some(window_id) = self.stage_window_id {
            return Some(window_id);
        }
        let mut id: u32 = 0;
        let err = unsafe { _AXUIElementGetWindow(self.raw, &mut id) };
        if err == K_AX_ERROR_SUCCESS {
            Some(id)
        } else {
            None
        }
    }

    /// Номер окна с запасными путями (`getWindowId()`): после смены сессии macOS
    /// может перестать отдавать номера окон (#640). Тогда — окно того же процесса
    /// с той же рамкой из списка окон, а в крайнем случае — производный номер из
    /// хеша элемента (постоянен для окна, не зависит от его положения).
    pub fn get_window_id(&self) -> Option<u32> {
        if let Some(window_id) = self.window_id() {
            return Some(window_id);
        }
        let frame = self.frame();
        if let (Some(pid), Some(frame)) = (self.pid(), frame) {
            // Берём первое совпадение: какое окно на самом деле — не узнать.
            if let Some(info) = cached_window_list(None)
                .iter()
                .find(|info| info.pid == pid && info.frame == frame)
            {
                return Some(info.id);
            }
        }
        if frame.is_some() {
            return Some(derive_window_id(unsafe { CFHash(self.raw) } as usize));
        }
        None
    }

    /// Само окно, если это окно, иначе его `AXWindow` (`windowElement`).
    pub fn window_element(&self) -> Option<AxElement> {
        if self.is_window() {
            return Some(self.clone());
        }
        self.element_attribute("AXWindow")
    }

    /// `AXMain` окна.
    pub fn is_main_window(&self) -> Option<bool> {
        self.window_element()?.bool_attribute("AXMain")
    }

    /// Сделать окно главным (`AXMain = value`).
    pub fn set_main_window(&self, value: bool) {
        if let Some(window) = self.window_element() {
            window.set_bool_attribute("AXMain", value);
        }
    }

    /// `AXMinimized` окна (нет ответа — считаем false).
    pub fn is_minimized(&self) -> bool {
        self.window_element()
            .and_then(|window| window.bool_attribute("AXMinimized"))
            .unwrap_or(false)
    }

    /// `AXTitle` — заголовок окна.
    pub fn title(&self) -> Option<String> {
        self.string_attribute("AXTitle")
    }

    /// `AXUIElementSetMessagingTimeout`: сколько ждать ответа зависшего
    /// приложения (по умолчанию в системе — несколько секунд). Менеджер окон,
    /// как и Swift, окнам его не ставит — только своему элементу приложения для
    /// `AXEnhancedUserInterface` при установке рамки; нужен значку стопки,
    /// «столбикам» и наблюдателю AX.
    pub fn set_messaging_timeout(&self, seconds: f32) {
        unsafe {
            AXUIElementSetMessagingTimeout(self.raw, seconds);
        }
    }

    /// Окно во весь экран: подроль кнопки `AXFullScreenButton` — `AXZoomButton`
    /// (`isFullScreen`); `None`, если кнопки нет.
    pub fn is_full_screen(&self) -> Option<bool> {
        let button = self
            .window_element()?
            .element_attribute("AXFullScreenButton")?;
        Some(button.subrole().as_deref() == Some("AXZoomButton"))
    }

    /// Рамка заголовка окна по кнопке закрытия (`titleBarFrame`), координаты AX.
    pub fn title_bar_frame(&self) -> Option<Rect> {
        let window = self.window_element()?;
        let window_frame = window.frame()?;
        let close_button = window.child_with_subrole("AXCloseButton")?.frame()?;
        let gap = close_button.min_y() - window_frame.min_y();
        let height = 2.0 * gap + close_button.h;
        Some(Rect::new(
            window_frame.x,
            window_frame.y,
            window_frame.w,
            height,
        ))
    }

    /// Элемент приложения, которому принадлежит этот элемент.
    pub fn application_element(&self) -> Option<AxElement> {
        if self.is_application() {
            return Some(self.clone());
        }
        self.pid().map(AxElement::application)
    }

    /// `AXFocusedWindow` приложения.
    pub fn focused_window_element(&self) -> Option<AxElement> {
        self.application_element()?
            .element_attribute("AXFocusedWindow")
    }

    /// `AXWindows` приложения.
    pub fn window_elements(&self) -> Option<Vec<AxElement>> {
        self.application_element()?.elements_attribute("AXWindows")
    }

    /// `AXHidden` приложения.
    pub fn is_hidden(&self) -> Option<bool> {
        self.application_element()?.bool_attribute("AXHidden")
    }

    /// `AXEnhancedUserInterface` — атрибут приложения, а не окна: читается у
    /// элемента приложения, которому принадлежит этот элемент.
    pub fn enhanced_ui(&self) -> Option<bool> {
        self.application_element()?
            .bool_attribute("AXEnhancedUserInterface")
    }

    /// Установить `AXEnhancedUserInterface` у приложения этого элемента.
    pub fn set_enhanced_ui(&self, value: bool) {
        if let Some(app_element) = self.application_element() {
            app_element.set_bool_attribute("AXEnhancedUserInterface", value);
        }
    }

    /// `AXWindowsIDs` — номера окон группы в полосе Stage Manager.
    pub fn window_ids(&self) -> Option<Vec<u32>> {
        let value = self.copy_attribute("AXWindowsIDs")?;
        unsafe {
            let result = cf_array_of_u32(value);
            CFRelease(value);
            result
        }
    }

    /// Вывести окно вперёд (`bringToFront(force:)`): сделать его главным, если оно
    /// ещё не главное, и активировать приложение — только его главное и ключевое
    /// окно, а не все окна приложения. Без `force` уже активное приложение не
    /// трогаем.
    pub fn bring_to_front(&self, force: bool) {
        if self.is_main_window() != Some(true) {
            self.set_main_window(true);
        }
        let Some(pid) = self.pid() else {
            return;
        };
        if let Some(app) = NSRunningApplication::runningApplicationWithProcessIdentifier(pid) {
            if !app.isActive() || force {
                // С macOS 14 флаг игнорируется, но и вреда от него нет — как в Swift.
                #[allow(deprecated)]
                app.activateWithOptions(NSApplicationActivationOptions::ActivateIgnoringOtherApps);
            }
        }
    }

    /// Элемент по имени атрибута (публичная обёртка для диагностики).
    pub fn element_attribute_public(&self, attribute: &str) -> Option<AxElement> {
        self.element_attribute(attribute)
    }

    /// Логический атрибут по имени (публичная обёртка для диагностики).
    pub fn bool_attribute_public(&self, attribute: &str) -> Option<bool> {
        self.bool_attribute(attribute)
    }

    // app_columns: окна приложения и сырой AXUIElementRef для AXObserver.

    /// `AXWindows` у элемента приложения — все его окна, включая свёрнутые.
    /// Нет атрибута или доступа — пусто.
    pub fn windows(&self) -> Vec<AxElement> {
        let Some(value) = self.copy_attribute("AXWindows") else {
            return Vec::new();
        };
        let mut result = Vec::new();
        unsafe {
            if CFGetTypeID(value) == core_foundation_sys::array::CFArrayGetTypeID() {
                let count = CFArrayGetCount(value as CFArrayRef);
                for index in 0..count {
                    let window = CFArrayGetValueAtIndex(value as CFArrayRef, index);
                    if !window.is_null() && CFGetTypeID(window) == AXUIElementGetTypeID() {
                        // Элементы массива — borrowed (+0), забираем себе retain.
                        CFRetain(window);
                        result.push(AxElement {
                            raw: window,
                            stage_window_id: None,
                        });
                    }
                }
            }
            CFRelease(value);
        }
        result
    }

    /// Сырой `AXUIElementRef` без передачи владения — для
    /// `AXObserverAddNotification` и `CFEqual`. Живёт, пока жив `self`.
    pub fn as_raw(&self) -> *const c_void {
        self.raw
    }

    /// Обернуть чужой (+0) `AXUIElementRef` — например, элемент из колбэка
    /// AXObserver, — забрав себе retain. `None` — null или объект не того типа.
    ///
    /// # Safety
    /// `raw` — null или живой CF-объект.
    pub unsafe fn retain_raw(raw: *const c_void) -> Option<AxElement> {
        if raw.is_null() || CFGetTypeID(raw) != AXUIElementGetTypeID() {
            return None;
        }
        CFRetain(raw);
        Some(AxElement {
            raw,
            stage_window_id: None,
        })
    }
}

/// Распаковать `AXValue` нужного типа. Сначала проверяем, что объект — вообще
/// `AXValue` (`CFGetTypeID`), потом — его тип.
unsafe fn ax_value<T: Default>(value: *const c_void, value_type: u32) -> Option<T> {
    if CFGetTypeID(value) != AXValueGetTypeID() || AXValueGetType(value) != value_type {
        return None;
    }
    let mut result = T::default();
    if AXValueGetValue(value, value_type, ptr::from_mut(&mut result) as *mut c_void) == 0 {
        return None;
    }
    Some(result)
}

/// `as? Bool`: `CFBoolean` или число, равное 0 или 1.
unsafe fn cf_to_bool(value: *const c_void) -> Option<bool> {
    let type_id = CFGetTypeID(value);
    if type_id == CFBooleanGetTypeID() {
        let b: CFBoolean = TCFType::wrap_under_get_rule(value as CFBooleanRef);
        return Some(bool::from(b));
    }
    if type_id == CFNumberGetTypeID() {
        let number: CFNumber = TCFType::wrap_under_get_rule(value as CFNumberRef);
        return match number.to_f64() {
            Some(0.0) => Some(false),
            Some(1.0) => Some(true),
            _ => None,
        };
    }
    None
}

/// `as? [CGWindowID]`: массив чисел, каждое помещается в `u32`.
unsafe fn cf_array_of_u32(value: *const c_void) -> Option<Vec<u32>> {
    if CFGetTypeID(value) != CFArrayGetTypeID() {
        return None;
    }
    let array = value as CFArrayRef;
    let count = CFArrayGetCount(array);
    let mut result = Vec::with_capacity(count.max(0) as usize);
    for index in 0..count {
        let item = CFArrayGetValueAtIndex(array, index);
        if item.is_null() || CFGetTypeID(item) != CFNumberGetTypeID() {
            return None;
        }
        let number: CFNumber = TCFType::wrap_under_get_rule(item as CFNumberRef);
        result.push(u32::try_from(number.to_i64()?).ok()?);
    }
    Some(result)
}

/// `CGRect.contains(CGPoint)`: левая и нижняя границы включены, правая и верхняя — нет.
pub(crate) fn rect_contains_point(rect: &Rect, x: f64, y: f64) -> bool {
    x >= rect.min_x() && x < rect.max_x() && y >= rect.min_y() && y < rect.max_y()
}

// ---------------------------------------------------------------------------
// Свободные функции
// ---------------------------------------------------------------------------

/// Элемент приложения, которое сейчас в фокусе (`getFrontApplicationElement`).
pub fn front_application_element() -> Option<AxElement> {
    let workspace = NSWorkspace::sharedWorkspace();
    let app = workspace.frontmostApplication()?;
    Some(AxElement::application(app.processIdentifier()))
}

/// Переднее окно (`getFrontWindowElement`): у приложения в фокусе —
/// `AXFocusedWindow`, если его нет — первое из `AXWindows`.
pub fn front_window() -> Option<AxElement> {
    let app_element = front_application_element()?;
    if let Some(window) = app_element.focused_window_element() {
        return Some(window);
    }
    app_element.window_elements()?.into_iter().next()
}

/// Окно под курсором (`getWindowElementUnderCursor`) — для drag-to-snap.
///
/// Порядок как в оригинале: системный элемент под курсором (если так настроено
/// для приложения в фокусе), окно из списка окон (с учётом полосы Stage Manager),
/// системный элемент (если не пробовали первым), и последним — окна приложения
/// в фокусе, когда WindowServer не отдаёт список окон (#640).
pub fn window_element_under_cursor() -> Option<AxElement> {
    let (cursor_x, cursor_y) = screens::cursor_position()?;
    let primary_height = screens::primary_screen_height();
    let (x, y) = (cursor_x, primary_height - cursor_y);
    let config = config::current();

    let mut system_wide_first = config.system_wide_mouse_down == Some(true);
    if config.system_wide_mouse_down.is_none() {
        if let Some(front_app_id) = frontmost_bundle_id() {
            system_wide_first = config.system_wide_mouse_down_apps.contains(&front_app_id);
        }
    }

    let from_system_wide =
        || element_at_position(x, y).and_then(|element| element.window_element());

    if system_wide_first {
        if let Some(window) = from_system_wide() {
            return Some(window);
        }
    }

    if let Some(info) = window_info_at(x, y) {
        if config.drag_from_stage != Some(false) && stage::stage_capable() && stage::stage_enabled()
        {
            let main_screen = screens::screens().into_iter().find(|screen| screen.is_main);
            if let Some(group) = main_screen
                .as_ref()
                .and_then(|screen| stage::stage_strip_window_group(info.id, screen))
            {
                if let Some(&window_id) = group.first() {
                    if window_id != info.id {
                        if let Some(element) = stage_window_element(window_id) {
                            return Some(element);
                        }
                    }
                }
            }
        }
        if let Some(windows) = AxElement::application(info.pid).window_elements() {
            if let Some(index) = windows
                .iter()
                .position(|window| window.window_id() == Some(info.id))
            {
                return windows.into_iter().nth(index);
            }
            if let Some(index) = windows
                .iter()
                .position(|window| window.frame() == Some(info.frame))
            {
                return windows.into_iter().nth(index);
            }
        }
    }

    if !system_wide_first {
        if let Some(window) = from_system_wide() {
            return Some(window);
        }
    }

    front_application_element()?
        .window_elements()?
        .into_iter()
        .filter_map(|window| window.frame().map(|frame| (window, frame)))
        .filter(|(_, frame)| rect_contains_point(frame, x, y))
        .min_by(|(_, a), (_, b)| {
            (a.w * a.h)
                .partial_cmp(&(b.w * b.h))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|(window, _)| window)
}

/// Окно под точкой (AX) из списка окон: ниже уровня Центра уведомлений (21),
/// не Dock и не WindowManager (`getWindowInfo(_ location:)`).
fn window_info_at(x: f64, y: f64) -> Option<WindowInfo> {
    cached_window_list(None)
        .iter()
        .find(|info| {
            info.level < 21
                && !matches!(
                    info.process_name.as_deref(),
                    Some("Dock") | Some("WindowManager")
                )
                && rect_contains_point(&info.frame, x, y)
        })
        .cloned()
}

/// Окно по номеру (`getWindowElement(_ windowId:)`).
pub fn window_element(window_id: u32) -> Option<AxElement> {
    let pid = window_list_for(&[window_id]).first()?.pid;
    AxElement::application(pid)
        .window_elements()?
        .into_iter()
        .find(|window| window.window_id() == Some(window_id))
}

/// Окно из полосы Stage Manager (`StageWindowAccessibilityElement`): свой номер
/// окна, положение — из списка окон.
pub fn stage_window_element(window_id: u32) -> Option<AxElement> {
    let mut element = window_element(window_id)?;
    element.stage_window_id = Some(window_id);
    Some(element)
}

/// Все окна всех приложений, у которых есть окна на экране, кроме Dock,
/// WindowManager и Центра уведомлений (`getAllWindowElements`).
pub fn all_window_elements() -> Vec<AxElement> {
    const EXCLUDED: [&str; 3] = ["Dock", "WindowManager", "Notification Center"];
    let mut seen = HashSet::new();
    let mut result = Vec::new();
    for info in cached_window_list(None).iter() {
        if EXCLUDED.contains(&info.process_name.as_deref().unwrap_or("")) {
            continue;
        }
        if !seen.insert(info.pid) {
            continue;
        }
        if let Some(windows) = AxElement::application(info.pid).window_elements() {
            result.extend(windows);
        }
    }
    result
}

/// Имя приложения, которое сейчас в фокусе.
pub fn frontmost_app_name() -> String {
    let workspace = NSWorkspace::sharedWorkspace();
    workspace
        .frontmostApplication()
        .and_then(|app| app.localizedName())
        .map(|name| name.to_string())
        .unwrap_or_default()
}

/// pid запущенного приложения по bundle id.
pub fn pid_for_bundle(bundle_id: &str) -> Option<i32> {
    let workspace = NSWorkspace::sharedWorkspace();
    for application in workspace.runningApplications().iter() {
        if application
            .bundleIdentifier()
            .map(|value| value.to_string() == bundle_id)
            .unwrap_or(false)
        {
            return Some(application.processIdentifier());
        }
    }
    None
}

/// Bundle identifier приложения, которое сейчас в фокусе.
pub fn frontmost_bundle_id() -> Option<String> {
    let workspace = NSWorkspace::sharedWorkspace();
    workspace
        .frontmostApplication()
        .and_then(|app| app.bundleIdentifier())
        .map(|value| value.to_string())
}

/// `AXIsProcessTrustedWithOptions` с `kAXTrustedCheckOptionPrompt`.
/// `prompt = true` показывает системный диалог с просьбой дать доступ.
pub fn is_process_trusted(prompt: bool) -> bool {
    let key = CFString::new("AXTrustedCheckOptionPrompt");
    let value = CFBoolean::from(prompt);
    let options: CFDictionary<CFString, CFBoolean> =
        CFDictionary::from_CFType_pairs(&[(key, value)]);
    unsafe { AXIsProcessTrustedWithOptions(options.as_concrete_TypeRef()) != 0 }
}

/// `AXUIElementCopyElementAtPosition` на system-wide элементе.
/// Координаты — AX (origin сверху слева).
pub fn element_at_position(x: f64, y: f64) -> Option<AxElement> {
    unsafe {
        let system_wide = AXUIElementCreateSystemWide();
        if system_wide.is_null() {
            return None;
        }
        let mut out: *const c_void = ptr::null();
        let err = AXUIElementCopyElementAtPosition(system_wide, x as f32, y as f32, &mut out);
        CFRelease(system_wide);
        if err == K_AX_ERROR_SUCCESS {
            AxElement::from_owned(out)
        } else {
            None
        }
    }
}

// ---------------------------------------------------------------------------
// Список окон (CGWindowList, разрешений не требует)
// ---------------------------------------------------------------------------

/// Одно окно из `CGWindowListCopyWindowInfo`.
#[derive(Clone, Debug, PartialEq)]
pub struct WindowInfo {
    /// `kCGWindowNumber`.
    pub id: u32,
    /// `kCGWindowOwnerPID`.
    pub pid: i32,
    /// `kCGWindowLayer`.
    pub level: i32,
    /// `kCGWindowBounds` — координаты Quartz с началом в левом верхнем углу
    /// основного экрана, то есть та же система, что у AX-рамок: переворот не нужен.
    /// Проверено на живом окне (`examples/window_probe.rs`, команда `info`): рамка
    /// окна из `CGWindowListCopyWindowInfo` совпадает с его AX-рамкой.
    pub frame: Rect,
    /// `kCGWindowOwnerName`.
    pub process_name: Option<String>,
}

/// Сколько живёт снимок списка окон — `WindowUtil.windowListCache`
/// (`TimeoutCache(timeout: 100)`): в циклах по окнам (номер окна по рамке,
/// соседи согласованного ресайза, сдвиг лесенкой, Stage Manager) одно действие
/// спрашивает WindowServer один раз, а не на каждое окно.
const WINDOW_LIST_TTL: Duration = Duration::from_millis(100);

/// Снимки списка окон: ключ `None` — все окна на экране (`window_list`), иначе —
/// номера (`window_list_for`).
type WindowListCache = TimeoutCache<Option<Vec<u32>>, Arc<Vec<WindowInfo>>>;

/// Список окон можно звать с любого потока: AX и CGWindowList потокобезопасны,
/// `AxElement` — `Send`, и его рамку читает, например, поток перехвата событий.
/// Поэтому кэш под мьютексом.
static WINDOW_LIST_CACHE: Mutex<WindowListCache> = Mutex::new(TimeoutCache::new(WINDOW_LIST_TTL));

/// Кэш с временем жизни записей — `TimeoutCache.swift`: запись живёт `timeout`
/// с момента, когда её положили (на самой границе ещё жива), просроченные
/// выбрасываются при следующей вставке. Записей единицы, поиск — перебором.
struct TimeoutCache<K, V> {
    timeout: Duration,
    /// Ключ, до какого момента жива, значение.
    entries: Vec<(K, Instant, V)>,
}

impl<K: PartialEq, V: Clone> TimeoutCache<K, V> {
    const fn new(timeout: Duration) -> Self {
        TimeoutCache {
            timeout,
            entries: Vec::new(),
        }
    }

    fn get(&self, key: &K, now: Instant) -> Option<V> {
        self.entries
            .iter()
            .find(|(entry_key, expires, _)| entry_key == key && now <= *expires)
            .map(|(_, _, value)| value.clone())
    }

    fn insert(&mut self, key: K, value: V, now: Instant) {
        self.entries
            .retain(|(entry_key, expires, _)| *entry_key != key && now <= *expires);
        self.entries.push((key, now + self.timeout, value));
    }

    /// Живое значение из кэша, а нет его — `fetch()` и запомнить. `fetch` идёт
    /// вне замка: запрос к WindowServer не держит другие потоки.
    fn get_or_fetch(cache: &Mutex<Self>, key: K, fetch: impl FnOnce() -> V) -> V {
        let lock = || {
            cache
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
        };
        if let Some(value) = lock().get(&key, Instant::now()) {
            return value;
        }
        let value = fetch();
        lock().insert(key, value.clone(), Instant::now());
        value
    }
}

/// Список окон через кэш (`WindowUtil.getWindowList(ids:)`).
fn cached_window_list(ids: Option<&[u32]>) -> Arc<Vec<WindowInfo>> {
    TimeoutCache::get_or_fetch(&WINDOW_LIST_CACHE, ids.map(<[u32]>::to_vec), || {
        Arc::new(match ids {
            None => copy_on_screen_window_list(),
            Some(ids) => describe_windows(ids),
        })
    })
}

/// `CGWindowListCopyWindowInfo([OnScreenOnly, ExcludeDesktopElements], kCGNullWindowID)`,
/// снимок не старше 100 мс.
pub fn window_list() -> Vec<WindowInfo> {
    cached_window_list(None).to_vec()
}

/// Описание окон с заданными номерами (`CGWindowListCreateDescriptionFromArray`),
/// снимок не старше 100 мс.
pub fn window_list_for(ids: &[u32]) -> Vec<WindowInfo> {
    if ids.is_empty() {
        return Vec::new();
    }
    cached_window_list(Some(ids)).to_vec()
}

fn copy_on_screen_window_list() -> Vec<WindowInfo> {
    use core_graphics::window::{
        kCGNullWindowID, kCGWindowListExcludeDesktopElements, kCGWindowListOptionOnScreenOnly,
        CGWindowListCopyWindowInfo,
    };
    unsafe {
        let array = CGWindowListCopyWindowInfo(
            kCGWindowListOptionOnScreenOnly | kCGWindowListExcludeDesktopElements,
            kCGNullWindowID,
        );
        take_window_infos(array)
    }
}

fn describe_windows(ids: &[u32]) -> Vec<WindowInfo> {
    use core_graphics::window::CGWindowListCreateDescriptionFromArray;

    // Массив номеров, а не объектов: значения — сами числа, без callbacks (как в Swift).
    let values: Vec<*const c_void> = ids.iter().map(|&id| id as usize as *const c_void).collect();
    unsafe {
        let array = CFArrayCreate(
            kCFAllocatorDefault,
            values.as_ptr(),
            values.len() as isize,
            ptr::null(),
        );
        if array.is_null() {
            return Vec::new();
        }
        let infos = CGWindowListCreateDescriptionFromArray(array);
        CFRelease(array as *const c_void);
        take_window_infos(infos)
    }
}

/// Номера всех окон сеанса, в том числе свёрнутых, спрятанных и на других
/// рабочих столах (`CGWindowListCreate(.optionAll)`: только номера, без
/// описаний) — чтобы история окон могла забыть закрытые. `None` — WindowServer
/// списка не дал; пустой список тоже `None`: так бывает после смены сессии
/// (#640), и забывать по нему нельзя.
pub fn existing_window_ids() -> Option<HashSet<u32>> {
    use core_graphics::window::{kCGNullWindowID, kCGWindowListOptionAll, CGWindowListCreate};

    unsafe {
        let array = CGWindowListCreate(kCGWindowListOptionAll, kCGNullWindowID);
        if array.is_null() {
            return None;
        }
        let mut ids = HashSet::new();
        if CFGetTypeID(array as *const c_void) == CFArrayGetTypeID() {
            // Значения массива — сами номера окон, а не объекты (как у
            // `CGWindowListCreateDescriptionFromArray` на входе).
            for index in 0..CFArrayGetCount(array) {
                ids.insert(CFArrayGetValueAtIndex(array, index) as usize as u32);
            }
        }
        CFRelease(array as *const c_void);
        (!ids.is_empty()).then_some(ids)
    }
}

/// Разобрать массив словарей окон (+1, освобождается здесь). Записи без номера,
/// процесса или рамки пропускаются, как в `WindowUtil`.
unsafe fn take_window_infos(array: CFArrayRef) -> Vec<WindowInfo> {
    use core_graphics::window::{
        kCGWindowBounds, kCGWindowLayer, kCGWindowNumber, kCGWindowOwnerName, kCGWindowOwnerPID,
    };

    if array.is_null() {
        return Vec::new();
    }
    if CFGetTypeID(array as *const c_void) != CFArrayGetTypeID() {
        CFRelease(array as *const c_void);
        return Vec::new();
    }

    // Ключи словаря границ создаём один раз на вызов, а не на каждое окно.
    let key_x = CFString::new("X");
    let key_y = CFString::new("Y");
    let key_w = CFString::new("Width");
    let key_h = CFString::new("Height");

    let count = CFArrayGetCount(array);
    let mut out = Vec::with_capacity(count.max(0) as usize);
    for index in 0..count {
        let raw = CFArrayGetValueAtIndex(array, index);
        if raw.is_null() {
            continue;
        }
        let dict = raw as CFDictionaryRef;
        let Some(id) = dict_u32(dict, kCGWindowNumber) else {
            continue;
        };
        let Some(pid) = dict_i32(dict, kCGWindowOwnerPID) else {
            continue;
        };
        let Some(frame) = dict_rect(dict, kCGWindowBounds, &key_x, &key_y, &key_w, &key_h) else {
            continue;
        };
        out.push(WindowInfo {
            id,
            pid,
            level: dict_i32(dict, kCGWindowLayer).unwrap_or(0),
            frame,
            process_name: dict_string(dict, kCGWindowOwnerName),
        });
    }
    CFRelease(array as *const c_void);
    out
}

/// Сырое значение из словаря по ключу (borrowed, освобождать НЕ нужно).
unsafe fn dict_get(dict: CFDictionaryRef, key: CFStringRef) -> Option<*const c_void> {
    let value = CFDictionaryGetValue(dict, key as *const c_void);
    if value.is_null() {
        None
    } else {
        Some(value)
    }
}

/// Число из словаря как `CFNumber` (borrowed → читаем без retain).
unsafe fn dict_number(dict: CFDictionaryRef, key: CFStringRef) -> Option<CFNumber> {
    let value = dict_get(dict, key)?;
    if CFGetTypeID(value) != CFNumberGetTypeID() {
        return None;
    }
    // `wrap_under_get_rule` делает retain; `drop` в конце — release.
    // Баланс нулевой, заимствованный указатель не трогаем.
    Some(TCFType::wrap_under_get_rule(value as CFNumberRef))
}

unsafe fn dict_i32(dict: CFDictionaryRef, key: CFStringRef) -> Option<i32> {
    dict_number(dict, key)?.to_i32()
}

unsafe fn dict_u32(dict: CFDictionaryRef, key: CFStringRef) -> Option<u32> {
    dict_number(dict, key)?
        .to_i64()
        .and_then(|v| u32::try_from(v).ok())
}

unsafe fn dict_f64(dict: CFDictionaryRef, key: CFStringRef) -> Option<f64> {
    dict_number(dict, key)?.to_f64()
}

unsafe fn dict_string(dict: CFDictionaryRef, key: CFStringRef) -> Option<String> {
    let value = dict_get(dict, key)?;
    if CFGetTypeID(value) != CFStringGetTypeID() {
        return None;
    }
    let s: CFString = TCFType::wrap_under_get_rule(value as CFStringRef);
    Some(s.to_string())
}

/// Словарь границ `kCGWindowBounds` → `Rect`.
unsafe fn dict_rect(
    dict: CFDictionaryRef,
    key: CFStringRef,
    key_x: &CFString,
    key_y: &CFString,
    key_w: &CFString,
    key_h: &CFString,
) -> Option<Rect> {
    let bounds = dict_get(dict, key)?;
    if CFGetTypeID(bounds) != core_foundation_sys::dictionary::CFDictionaryGetTypeID() {
        return None;
    }
    let bounds = bounds as CFDictionaryRef;
    let x = dict_f64(bounds, key_x.as_concrete_TypeRef())?;
    let y = dict_f64(bounds, key_y.as_concrete_TypeRef())?;
    let w = dict_f64(bounds, key_w.as_concrete_TypeRef())?;
    let h = dict_f64(bounds, key_h.as_concrete_TypeRef())?;
    Some(Rect::new(x, y, w, h))
}

#[cfg(test)]
mod tests {
    use super::*;

    extern "C" {
        fn CGSessionCopyCurrentDictionary() -> CFDictionaryRef;
    }

    /// Есть ли GUI-сеанс (WindowServer). Без него, например по ssh, списка окон
    /// нет, и тесты, которые читают живой список, пропускаются, а не падают.
    fn gui_session() -> bool {
        // SAFETY: функция Create-правила; словарь освобождаем сразу.
        let session = unsafe { CGSessionCopyCurrentDictionary() };
        if session.is_null() {
            eprintln!("нет GUI-сеанса — тест с живым списком окон пропущен");
            return false;
        }
        unsafe { CFRelease(session.cast()) };
        true
    }

    #[test]
    fn window_list_is_not_empty() {
        if !gui_session() {
            return;
        }
        let list = window_list();
        assert!(!list.is_empty(), "window_list() пуст в GUI-сеансе");
    }

    #[test]
    fn window_list_for_ids_describes_the_same_windows() {
        let list = window_list();
        let Some(first) = list.first() else {
            return;
        };
        let described = window_list_for(&[first.id]);
        assert_eq!(described.len(), 1);
        assert_eq!(described[0].id, first.id);
        assert_eq!(described[0].pid, first.pid);
        assert!(window_list_for(&[]).is_empty());
    }

    #[test]
    fn trust_check_does_not_crash() {
        // Без разрешений вернёт false — главное, что не падает.
        let _ = is_process_trusted(false);
    }

    #[test]
    fn front_window_does_not_crash() {
        // Без разрешений, скорее всего, None — не падаем в любом случае.
        let _ = front_window();
    }

    #[test]
    fn element_at_position_does_not_crash() {
        let _ = element_at_position(10.0, 10.0);
    }

    #[test]
    fn app_element_clone_keeps_retain_balance_and_identity() {
        // Создаём элемент своего процесса, клонируем, дропаем —
        // под ASan/течью это бы упало или потекло.
        let pid = std::process::id() as i32;
        let element = AxElement::application(pid);
        let clone = element.clone();
        // GetPid не требует доверенного доступа.
        assert_eq!(element.pid(), Some(pid));
        // Равенство — CFEqual: два элемента одного приложения равны, хеши совпадают.
        let again = AxElement::application(pid);
        assert_eq!(element, again);
        let mut set = HashSet::new();
        set.insert(element.clone());
        assert!(set.contains(&again));
        drop(clone);
        element.set_messaging_timeout(1.0);
    }

    #[test]
    fn enhanced_ui_is_toggled_through_a_fresh_application_element() {
        let pid = std::process::id() as i32;
        let element = AxElement::application(pid);
        let app_element = element.enhanced_ui_application().unwrap();
        assert_eq!(app_element.pid(), Some(pid));
        assert_eq!(app_element, element);
        // Другой объект: короткий таймаут не достаётся элементу вызывающего.
        assert_ne!(app_element.as_raw(), element.as_raw());
        assert_ne!(
            element.enhanced_ui_application().unwrap().as_raw(),
            app_element.as_raw()
        );
    }

    #[test]
    fn derived_window_ids_stay_out_of_real_id_space() {
        assert_eq!(derive_window_id(0), 0x8000_0000);
        assert_eq!(derive_window_id(0x1234), 0x8000_1234);
        // Старший бит хеша и всё выше 32 бит отбрасываются, как `truncatingIfNeeded`.
        assert_eq!(derive_window_id(0xFFFF_FFFF), 0xFFFF_FFFF);
        assert_eq!(derive_window_id(0x1_8000_0001), 0x8000_0001);
        assert!(is_derived_window_id(derive_window_id(0)));
        assert!(is_derived_window_id(derive_window_id(0x7FFF_FFFF)));
        assert!(!is_derived_window_id(1));
        assert!(!is_derived_window_id(0x7FFF_FFFF));
    }

    #[test]
    fn timeout_cache_keeps_entries_for_the_timeout() {
        let ms = Duration::from_millis;
        let start = Instant::now();
        let mut cache = TimeoutCache::new(ms(100));
        cache.insert(None, 1, start);
        cache.insert(Some(vec![7]), 2, start);
        assert_eq!(cache.get(&None, start + ms(50)), Some(1));
        // На границе запись ещё жива, как `now > expirationTimestamp` в Swift.
        assert_eq!(cache.get(&None, start + ms(100)), Some(1));
        assert_eq!(cache.get(&None, start + ms(101)), None);
        assert_eq!(cache.get(&Some(vec![7]), start + ms(50)), Some(2));
        assert_eq!(cache.get(&Some(vec![8]), start), None);
        // Вставка заменяет запись с тем же ключом и выбрасывает просроченные.
        cache.insert(None, 3, start + ms(150));
        assert_eq!(cache.entries.len(), 1);
        assert_eq!(cache.get(&None, start + ms(200)), Some(3));
    }

    #[test]
    fn window_list_asks_the_window_server_once_per_timeout() {
        let fetches = std::cell::Cell::new(0);
        let fetch = |value: u32| {
            fetches.set(fetches.get() + 1);
            value
        };
        let cache = Mutex::new(TimeoutCache::new(Duration::from_secs(60)));
        assert_eq!(TimeoutCache::get_or_fetch(&cache, None, || fetch(1)), 1);
        assert_eq!(TimeoutCache::get_or_fetch(&cache, None, || fetch(2)), 1);
        assert_eq!(fetches.get(), 1);
        // Другие номера — другой снимок.
        assert_eq!(
            TimeoutCache::get_or_fetch(&cache, Some(vec![5]), || fetch(3)),
            3
        );
        assert_eq!(fetches.get(), 2);

        // Просроченный снимок запрашивается заново.
        let short = Mutex::new(TimeoutCache::<Option<Vec<u32>>, u32>::new(
            Duration::from_millis(10),
        ));
        assert_eq!(TimeoutCache::get_or_fetch(&short, None, || fetch(4)), 4);
        std::thread::sleep(Duration::from_millis(30));
        assert_eq!(TimeoutCache::get_or_fetch(&short, None, || fetch(5)), 5);
        assert_eq!(fetches.get(), 4);
    }

    #[test]
    fn all_session_windows_include_on_screen_ones() {
        if !gui_session() {
            return;
        }
        let on_screen = window_list();
        let existing = existing_window_ids().expect("список окон сеанса — нужен GUI-сеанс");
        // Окна появляются и исчезают и между двумя запросами — достаточно, чтобы
        // хоть одно окно с экрана нашлось в полном списке.
        assert!(on_screen.is_empty() || on_screen.iter().any(|info| existing.contains(&info.id)));
        assert!(existing.iter().all(|&id| !is_derived_window_id(id)));
    }

    #[test]
    fn point_containment_is_half_open() {
        let rect = Rect::new(0.0, 0.0, 100.0, 50.0);
        assert!(rect_contains_point(&rect, 0.0, 0.0));
        assert!(rect_contains_point(&rect, 99.9, 49.9));
        assert!(!rect_contains_point(&rect, 100.0, 10.0));
        assert!(!rect_contains_point(&rect, 10.0, 50.0));
    }
}
