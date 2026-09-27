//! Хранилище настроек: UserDefaults (как у оригинала) и память (для тестов),
//! правила чтения/записи Swift-обёрток из `Defaults.swift` и экспорт/импорт
//! JSON-конфига в формате `PrefsWindow/Config.swift`.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::marker::PhantomData;

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::AllocAnyThread;
use objc2_foundation::{NSString, NSUserDefaults};

use crate::config::{Config, CycleSizes, SubsequentExecutionMode};
use crate::json::{self, Json};

// ---------------------------------------------------------------- хранилище

/// Доступ к UserDefaults с той же семантикой, что у `UserDefaults.standard`:
/// чтение отсутствующего ключа даёт `false`/`0`/`nil`, строки и числа
/// взаимно приводятся.
pub trait Store {
    fn contains(&self, key: &str) -> bool;
    fn bool(&self, key: &str) -> bool;
    fn integer(&self, key: &str) -> i64;
    fn float(&self, key: &str) -> f32;
    fn double(&self, key: &str) -> f64;
    fn string(&self, key: &str) -> Option<String>;

    fn set_bool(&mut self, key: &str, value: bool);
    fn set_integer(&mut self, key: &str, value: i64);
    fn set_float(&mut self, key: &str, value: f32);
    fn set_double(&mut self, key: &str, value: f64);
    /// `None` удаляет ключ — как `UserDefaults.set(nil, forKey:)`.
    fn set_string(&mut self, key: &str, value: Option<&str>);
    fn remove(&mut self, key: &str);
}

/// Значение в памяти — то, что умеет хранить plist UserDefaults.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Bool(bool),
    Integer(i64),
    Float(f32),
    Double(f64),
    String(String),
}

/// Начало строки как целое — как `-[NSString integerValue]` (мусор дальше игнорируется).
fn leading_integer(text: &str) -> i64 {
    let text = text.trim_start();
    let (negative, digits) = match text.as_bytes().first() {
        Some(b'-') => (true, &text[1..]),
        Some(b'+') => (false, &text[1..]),
        _ => (false, text),
    };
    let end = digits
        .find(|ch: char| !ch.is_ascii_digit())
        .unwrap_or(digits.len());
    let value = digits[..end].parse::<i64>().unwrap_or(0);
    if negative {
        -value
    } else {
        value
    }
}

/// Начало строки как число с точкой — как `-[NSString doubleValue]`.
fn leading_double(text: &str) -> f64 {
    let text = text.trim_start();
    let bytes = text.as_bytes();
    let digits_from = |start: usize| {
        let mut end = start;
        while end < bytes.len() && bytes[end].is_ascii_digit() {
            end += 1;
        }
        end
    };
    let mut end = usize::from(matches!(bytes.first(), Some(b'+' | b'-')));
    let integer_end = digits_from(end);
    let mut mantissa_digits = integer_end - end;
    end = integer_end;
    if bytes.get(end) == Some(&b'.') {
        let fraction_end = digits_from(end + 1);
        mantissa_digits += fraction_end - end - 1;
        end = fraction_end;
    }
    if mantissa_digits == 0 {
        return 0.0;
    }
    if let Some(b'e' | b'E') = bytes.get(end) {
        let sign = usize::from(matches!(bytes.get(end + 1), Some(b'+' | b'-')));
        let exponent_end = digits_from(end + 1 + sign);
        if exponent_end > end + 1 + sign {
            end = exponent_end;
        }
    }
    text[..end].parse::<f64>().unwrap_or(0.0)
}

impl Value {
    fn as_integer(&self) -> i64 {
        match self {
            Value::Bool(flag) => *flag as i64,
            Value::Integer(value) => *value,
            Value::Float(value) => *value as i64,
            Value::Double(value) => *value as i64,
            Value::String(text) => leading_integer(text),
        }
    }

    fn as_double(&self) -> f64 {
        match self {
            Value::Bool(flag) => *flag as i64 as f64,
            Value::Integer(value) => *value as f64,
            Value::Float(value) => *value as f64,
            Value::Double(value) => *value,
            Value::String(text) => leading_double(text),
        }
    }

    fn as_bool(&self) -> bool {
        match self {
            Value::Bool(flag) => *flag,
            Value::String(text) => {
                let text = text.trim();
                text.eq_ignore_ascii_case("yes")
                    || text.eq_ignore_ascii_case("true")
                    || leading_integer(text) != 0
            }
            other => other.as_double() != 0.0,
        }
    }
}

/// Хранилище в памяти: для тестов и до инициализации настроек.
#[derive(Clone, Debug, Default)]
pub struct MemoryStore {
    values: HashMap<String, Value>,
}

impl MemoryStore {
    pub fn new() -> Self {
        MemoryStore::default()
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.values.get(key)
    }

    /// Положить значение как есть (например, строку вместо числа).
    pub fn insert(&mut self, key: &str, value: Value) {
        self.values.insert(key.to_string(), value);
    }

    pub fn keys(&self) -> Vec<String> {
        let mut keys: Vec<String> = self.values.keys().cloned().collect();
        keys.sort();
        keys
    }

    pub fn len(&self) -> usize {
        self.values.len()
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
}

impl Store for MemoryStore {
    fn contains(&self, key: &str) -> bool {
        self.values.contains_key(key)
    }

    fn bool(&self, key: &str) -> bool {
        self.values.get(key).map(Value::as_bool).unwrap_or(false)
    }

    fn integer(&self, key: &str) -> i64 {
        self.values.get(key).map(Value::as_integer).unwrap_or(0)
    }

    fn float(&self, key: &str) -> f32 {
        match self.values.get(key) {
            Some(Value::Float(value)) => *value,
            Some(other) => other.as_double() as f32,
            None => 0.0,
        }
    }

    fn double(&self, key: &str) -> f64 {
        self.values.get(key).map(Value::as_double).unwrap_or(0.0)
    }

    fn string(&self, key: &str) -> Option<String> {
        match self.values.get(key)? {
            Value::String(text) => Some(text.clone()),
            Value::Bool(flag) => Some(if *flag { "1" } else { "0" }.to_string()),
            Value::Integer(value) => Some(value.to_string()),
            Value::Float(value) => Some(json::to_compact(&Json::from_f32(*value))),
            Value::Double(value) => Some(json::to_compact(&Json::from_f64(*value))),
        }
    }

    fn set_bool(&mut self, key: &str, value: bool) {
        self.insert(key, Value::Bool(value));
    }

    fn set_integer(&mut self, key: &str, value: i64) {
        self.insert(key, Value::Integer(value));
    }

    fn set_float(&mut self, key: &str, value: f32) {
        self.insert(key, Value::Float(value));
    }

    fn set_double(&mut self, key: &str, value: f64) {
        self.insert(key, Value::Double(value));
    }

    fn set_string(&mut self, key: &str, value: Option<&str>) {
        match value {
            Some(text) => self.insert(key, Value::String(text.to_string())),
            None => self.remove(key),
        }
    }

    fn remove(&mut self, key: &str) {
        self.values.remove(key);
    }
}

/// `NSUserDefaults` — то же хранилище, что у оригинала.
pub struct UserDefaultsStore {
    defaults: Retained<NSUserDefaults>,
}

impl UserDefaultsStore {
    /// `UserDefaults.standard`: домен — bundle id приложения (`local.rectangle2rust`),
    /// а при запуске без бандла — имя исполняемого файла.
    pub fn standard() -> Self {
        UserDefaultsStore {
            defaults: NSUserDefaults::standardUserDefaults(),
        }
    }

    /// Чужой домен настроек, например приложения из бандла — для примеров
    /// и диагностики, которые запускаются без бандла.
    pub fn suite(name: &str) -> Option<Self> {
        let defaults = NSUserDefaults::initWithSuiteName(
            NSUserDefaults::alloc(),
            Some(&NSString::from_str(name)),
        )?;
        Some(UserDefaultsStore { defaults })
    }
}

impl Store for UserDefaultsStore {
    fn contains(&self, key: &str) -> bool {
        self.defaults
            .objectForKey(&NSString::from_str(key))
            .is_some()
    }

    fn bool(&self, key: &str) -> bool {
        self.defaults.boolForKey(&NSString::from_str(key))
    }

    fn integer(&self, key: &str) -> i64 {
        self.defaults.integerForKey(&NSString::from_str(key)) as i64
    }

    fn float(&self, key: &str) -> f32 {
        self.defaults.floatForKey(&NSString::from_str(key))
    }

    fn double(&self, key: &str) -> f64 {
        self.defaults.doubleForKey(&NSString::from_str(key))
    }

    fn string(&self, key: &str) -> Option<String> {
        self.defaults
            .stringForKey(&NSString::from_str(key))
            .map(|text| text.to_string())
    }

    fn set_bool(&mut self, key: &str, value: bool) {
        self.defaults
            .setBool_forKey(value, &NSString::from_str(key));
    }

    fn set_integer(&mut self, key: &str, value: i64) {
        self.defaults
            .setInteger_forKey(value as isize, &NSString::from_str(key));
    }

    fn set_float(&mut self, key: &str, value: f32) {
        self.defaults
            .setFloat_forKey(value, &NSString::from_str(key));
    }

    fn set_double(&mut self, key: &str, value: f64) {
        self.defaults
            .setDouble_forKey(value, &NSString::from_str(key));
    }

    fn set_string(&mut self, key: &str, value: Option<&str>) {
        let key = NSString::from_str(key);
        match value {
            Some(text) => {
                let text = NSString::from_str(text);
                let object: &AnyObject = &text;
                // SAFETY: NSString — допустимое значение plist.
                unsafe { self.defaults.setObject_forKey(Some(object), &key) };
            }
            None => self.defaults.removeObjectForKey(&key),
        }
    }

    fn remove(&mut self, key: &str) {
        self.defaults.removeObjectForKey(&NSString::from_str(key));
    }
}

// ---------------------------------------------------------------- обёртки Swift

/// Значение настройки в экспорте (`CodableDefault`): заполнено одно поле.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CodableDefault {
    pub bool: Option<bool>,
    pub int: Option<i64>,
    pub float: Option<f32>,
    pub double: Option<f64>,
    pub string: Option<String>,
}

impl CodableDefault {
    pub fn to_json(&self) -> Json {
        let mut pairs = Vec::new();
        if let Some(value) = self.bool {
            pairs.push(("bool".to_string(), Json::Bool(value)));
        }
        if let Some(value) = self.int {
            pairs.push(("int".to_string(), Json::from_i64(value)));
        }
        if let Some(value) = self.float {
            pairs.push(("float".to_string(), Json::from_f32(value)));
        }
        if let Some(value) = self.double {
            pairs.push(("double".to_string(), Json::from_f64(value)));
        }
        if let Some(value) = &self.string {
            pairs.push(("string".to_string(), Json::string(value)));
        }
        Json::Object(pairs)
    }

    /// Разбор как у JSONDecoder: поле неверного типа — ошибка всего импорта.
    pub fn from_json(value: &Json) -> Result<Self, String> {
        if !matches!(value, Json::Object(_)) {
            return Err("CodableDefault: ожидался объект".to_string());
        }
        fn field<T>(
            value: &Json,
            name: &str,
            decode: fn(&Json) -> Result<T, String>,
        ) -> Result<Option<T>, String> {
            match value.get(name) {
                None | Some(Json::Null) => Ok(None),
                Some(field) => decode(field)
                    .map(Some)
                    .map_err(|error| format!("{}: {}", name, error)),
            }
        }
        Ok(CodableDefault {
            bool: field(value, "bool", json::decode_bool)?,
            int: field(value, "int", json::decode_i64)?,
            float: field(value, "float", json::decode_f32)?,
            double: field(value, "double", json::decode_f64)?,
            string: field(value, "string", json::decode_string)?,
        })
    }
}

/// Перечисление с целочисленным rawValue (для `IntEnumDefault`).
pub trait IntEnum: Copy + PartialEq {
    fn from_raw(raw: i64) -> Option<Self>;
    fn raw(self) -> i64;
}

/// Значение, которое оригинал хранит JSON-строкой (`Codable`).
pub trait JsonCodable: Sized + Clone + PartialEq {
    fn to_json(&self) -> Json;
    fn from_json(value: &Json) -> Result<Self, String>;
}

impl JsonCodable for BTreeSet<String> {
    fn to_json(&self) -> Json {
        Json::Array(self.iter().map(|item| Json::string(item)).collect())
    }

    fn from_json(value: &Json) -> Result<Self, String> {
        Vec::<String>::from_json(value).map(|items| items.into_iter().collect())
    }
}

impl JsonCodable for Vec<String> {
    fn to_json(&self) -> Json {
        Json::Array(self.iter().map(|item| Json::string(item)).collect())
    }

    fn from_json(value: &Json) -> Result<Self, String> {
        let Json::Array(items) = value else {
            return Err("ожидался массив строк".to_string());
        };
        items.iter().map(json::decode_string).collect()
    }
}

fn encode_json<T: JsonCodable>(value: &T) -> String {
    json::to_compact(&value.to_json())
}

fn decode_json<T: JsonCodable>(text: &str) -> Option<T> {
    json::parse(text)
        .ok()
        .and_then(|value| T::from_json(&value).ok())
}

/// Правила одной Swift-обёртки из `Defaults.swift`: чтение при старте (`init`),
/// запись (`didSet`), экспорт (`toCodable`) и импорт (`load(from:)`).
pub trait Codec {
    type Value: Clone + PartialEq;
    /// Имя класса обёртки в Swift — для документации.
    const SWIFT_KIND: &'static str;

    fn load(store: &dyn Store, key: &str, default: &Self::Value) -> Self::Value;
    fn save(store: &mut dyn Store, key: &str, value: &Self::Value);
    fn to_codable(value: &Self::Value, default: &Self::Value) -> CodableDefault;
    fn from_codable(
        codable: &CodableDefault,
        current: &Self::Value,
        default: &Self::Value,
    ) -> Self::Value;
}

/// `BoolDefault`: `bool(forKey:)`, по умолчанию `false`.
pub struct BoolDefault;

impl Codec for BoolDefault {
    type Value = bool;
    const SWIFT_KIND: &'static str = "BoolDefault";

    fn load(store: &dyn Store, key: &str, _default: &bool) -> bool {
        store.bool(key)
    }

    fn save(store: &mut dyn Store, key: &str, value: &bool) {
        store.set_bool(key, *value);
    }

    fn to_codable(value: &bool, _default: &bool) -> CodableDefault {
        CodableDefault {
            bool: Some(*value),
            ..CodableDefault::default()
        }
    }

    fn from_codable(codable: &CodableDefault, current: &bool, _default: &bool) -> bool {
        codable.bool.unwrap_or(*current)
    }
}

/// `SUHasLaunchedBefore`: ключ Sparkle, оригинал его только читает.
pub struct ReadOnlyBool;

impl Codec for ReadOnlyBool {
    type Value = bool;
    const SWIFT_KIND: &'static str = "bool(forKey:), только чтение";

    fn load(store: &dyn Store, key: &str, _default: &bool) -> bool {
        store.bool(key)
    }

    fn save(_store: &mut dyn Store, _key: &str, _value: &bool) {}

    fn to_codable(value: &bool, _default: &bool) -> CodableDefault {
        CodableDefault {
            bool: Some(*value),
            ..CodableDefault::default()
        }
    }

    fn from_codable(_codable: &CodableDefault, current: &bool, _default: &bool) -> bool {
        *current
    }
}

/// `OptionalBoolDefault`: целое 0 — не задано, 1 — да, 2 — нет.
pub struct OptionalBoolDefault;

impl OptionalBoolDefault {
    fn raw(value: Option<bool>) -> i64 {
        match value {
            None => 0,
            Some(true) => 1,
            Some(false) => 2,
        }
    }
}

impl Codec for OptionalBoolDefault {
    type Value = Option<bool>;
    const SWIFT_KIND: &'static str = "OptionalBoolDefault";

    fn load(store: &dyn Store, key: &str, _default: &Option<bool>) -> Option<bool> {
        match store.integer(key) {
            1 => Some(true),
            2 => Some(false),
            _ => None,
        }
    }

    fn save(store: &mut dyn Store, key: &str, value: &Option<bool>) {
        store.set_integer(key, Self::raw(*value));
    }

    fn to_codable(value: &Option<bool>, _default: &Option<bool>) -> CodableDefault {
        CodableDefault {
            int: Some(Self::raw(*value)),
            ..CodableDefault::default()
        }
    }

    /// Неизвестное число оставляет текущее значение (`set(using:)` → `default: break`).
    fn from_codable(
        codable: &CodableDefault,
        current: &Option<bool>,
        _default: &Option<bool>,
    ) -> Option<bool> {
        match codable.int {
            Some(0) => None,
            Some(1) => Some(true),
            Some(2) => Some(false),
            _ => *current,
        }
    }
}

/// `FloatDefault`: сохранённый 0 заменяется значением по умолчанию, если оно не 0.
/// NaN и бесконечности (ручной `defaults write`, битый plist) — тоже значение по
/// умолчанию: в расчётах они дали бы NaN-геометрию, а в экспорте — невалидный JSON.
pub struct FloatDefault;

impl Codec for FloatDefault {
    type Value = f32;
    const SWIFT_KIND: &'static str = "FloatDefault";

    fn load(store: &dyn Store, key: &str, default: &f32) -> f32 {
        let value = store.float(key);
        if !value.is_finite() || (*default != 0.0 && value == 0.0) {
            *default
        } else {
            value
        }
    }

    fn save(store: &mut dyn Store, key: &str, value: &f32) {
        store.set_float(key, *value);
    }

    fn to_codable(value: &f32, default: &f32) -> CodableDefault {
        CodableDefault {
            float: Some(if value.is_finite() { *value } else { *default }),
            ..CodableDefault::default()
        }
    }

    /// При импорте 0 не подменяется (подмена случится только при следующем чтении).
    fn from_codable(codable: &CodableDefault, current: &f32, _default: &f32) -> f32 {
        codable.float.unwrap_or(*current)
    }
}

/// `DoubleDefault`: значение по умолчанию — только если ключа нет совсем (или в
/// нём NaN либо бесконечность, как у `FloatDefault`).
pub struct DoubleDefault;

impl Codec for DoubleDefault {
    type Value = f64;
    const SWIFT_KIND: &'static str = "DoubleDefault";

    fn load(store: &dyn Store, key: &str, default: &f64) -> f64 {
        if !store.contains(key) {
            return *default;
        }
        let value = store.double(key);
        if value.is_finite() {
            value
        } else {
            *default
        }
    }

    fn save(store: &mut dyn Store, key: &str, value: &f64) {
        store.set_double(key, *value);
    }

    fn to_codable(value: &f64, default: &f64) -> CodableDefault {
        CodableDefault {
            double: Some(if value.is_finite() { *value } else { *default }),
            ..CodableDefault::default()
        }
    }

    /// Старые экспорты хранили такие настройки во Float: 0 там значит «по умолчанию».
    fn from_codable(codable: &CodableDefault, current: &f64, default: &f64) -> f64 {
        if let Some(value) = codable.double {
            value
        } else if let Some(value) = codable.float {
            if value == 0.0 && *default != 0.0 {
                *default
            } else {
                value as f64
            }
        } else {
            *current
        }
    }
}

/// `IntDefault`: сохранённый 0 заменяется значением по умолчанию, если оно не 0.
pub struct IntDefault;

impl Codec for IntDefault {
    type Value = i64;
    const SWIFT_KIND: &'static str = "IntDefault";

    fn load(store: &dyn Store, key: &str, default: &i64) -> i64 {
        let value = store.integer(key);
        if *default != 0 && value == 0 {
            *default
        } else {
            value
        }
    }

    fn save(store: &mut dyn Store, key: &str, value: &i64) {
        store.set_integer(key, *value);
    }

    fn to_codable(value: &i64, _default: &i64) -> CodableDefault {
        CodableDefault {
            int: Some(*value),
            ..CodableDefault::default()
        }
    }

    fn from_codable(codable: &CodableDefault, current: &i64, _default: &i64) -> i64 {
        codable.int.unwrap_or(*current)
    }
}

/// `IntEnumDefault<E>`: неизвестный rawValue — значение по умолчанию.
pub struct IntEnumDefault<E>(PhantomData<E>);

impl<E: IntEnum> Codec for IntEnumDefault<E> {
    type Value = E;
    const SWIFT_KIND: &'static str = "IntEnumDefault";

    fn load(store: &dyn Store, key: &str, default: &E) -> E {
        E::from_raw(store.integer(key)).unwrap_or(*default)
    }

    fn save(store: &mut dyn Store, key: &str, value: &E) {
        store.set_integer(key, value.raw());
    }

    fn to_codable(value: &E, _default: &E) -> CodableDefault {
        CodableDefault {
            int: Some(value.raw()),
            ..CodableDefault::default()
        }
    }

    fn from_codable(codable: &CodableDefault, current: &E, default: &E) -> E {
        match codable.int {
            Some(raw) if raw != current.raw() => E::from_raw(raw).unwrap_or(*default),
            _ => *current,
        }
    }
}

/// `SubsequentExecutionDefault`: как IntEnum, но неизвестное число при импорте игнорируется.
pub struct SubsequentExecutionDefault;

impl Codec for SubsequentExecutionDefault {
    type Value = SubsequentExecutionMode;
    const SWIFT_KIND: &'static str = "SubsequentExecutionDefault";

    fn load(
        store: &dyn Store,
        key: &str,
        default: &SubsequentExecutionMode,
    ) -> SubsequentExecutionMode {
        SubsequentExecutionMode::from_raw(store.integer(key)).unwrap_or(*default)
    }

    fn save(store: &mut dyn Store, key: &str, value: &SubsequentExecutionMode) {
        store.set_integer(key, value.raw());
    }

    fn to_codable(
        value: &SubsequentExecutionMode,
        _default: &SubsequentExecutionMode,
    ) -> CodableDefault {
        CodableDefault {
            int: Some(value.raw()),
            ..CodableDefault::default()
        }
    }

    fn from_codable(
        codable: &CodableDefault,
        current: &SubsequentExecutionMode,
        _default: &SubsequentExecutionMode,
    ) -> SubsequentExecutionMode {
        codable
            .int
            .and_then(SubsequentExecutionMode::from_raw)
            .unwrap_or(*current)
    }
}

/// `CycleSizesDefault`: битовая маска размеров.
pub struct CycleSizesDefault;

impl Codec for CycleSizesDefault {
    type Value = CycleSizes;
    const SWIFT_KIND: &'static str = "CycleSizesDefault";

    fn load(store: &dyn Store, key: &str, _default: &CycleSizes) -> CycleSizes {
        CycleSizes::from_bits(store.integer(key))
    }

    fn save(store: &mut dyn Store, key: &str, value: &CycleSizes) {
        store.set_integer(key, value.bits());
    }

    fn to_codable(value: &CycleSizes, _default: &CycleSizes) -> CodableDefault {
        CodableDefault {
            int: Some(value.bits()),
            ..CodableDefault::default()
        }
    }

    fn from_codable(
        codable: &CodableDefault,
        current: &CycleSizes,
        _default: &CycleSizes,
    ) -> CycleSizes {
        codable.int.map(CycleSizes::from_bits).unwrap_or(*current)
    }
}

/// `StringDefault`: `nil` удаляет ключ; импорт переписывает значение всегда.
pub struct StringDefault;

impl Codec for StringDefault {
    type Value = Option<String>;
    const SWIFT_KIND: &'static str = "StringDefault";

    fn load(store: &dyn Store, key: &str, _default: &Option<String>) -> Option<String> {
        store.string(key)
    }

    fn save(store: &mut dyn Store, key: &str, value: &Option<String>) {
        store.set_string(key, value.as_deref());
    }

    fn to_codable(value: &Option<String>, _default: &Option<String>) -> CodableDefault {
        CodableDefault {
            string: value.clone(),
            ..CodableDefault::default()
        }
    }

    fn from_codable(
        codable: &CodableDefault,
        _current: &Option<String>,
        _default: &Option<String>,
    ) -> Option<String> {
        codable.string.clone()
    }
}

/// `JSONDefault<T>`: значение — JSON-строка (компактная, ключи по порядку);
/// строка, которую не разобрать, даёт `None`. `None` записывается как `"null"`
/// (так кодирует JSONEncoder пустой Optional).
pub struct JsonDefault<T>(PhantomData<T>);

impl<T: JsonCodable> Codec for JsonDefault<T> {
    type Value = Option<T>;
    const SWIFT_KIND: &'static str = "JSONDefault";

    fn load(store: &dyn Store, key: &str, _default: &Option<T>) -> Option<T> {
        store.string(key).and_then(|text| decode_json(&text))
    }

    fn save(store: &mut dyn Store, key: &str, value: &Option<T>) {
        let text = match value {
            Some(value) => encode_json(value),
            None => "null".to_string(),
        };
        store.set_string(key, Some(&text));
    }

    fn to_codable(value: &Option<T>, _default: &Option<T>) -> CodableDefault {
        CodableDefault {
            string: value.as_ref().map(encode_json),
            ..CodableDefault::default()
        }
    }

    fn from_codable(
        codable: &CodableDefault,
        _current: &Option<T>,
        _default: &Option<T>,
    ) -> Option<T> {
        codable.string.as_deref().and_then(decode_json)
    }
}

/// `JSONDefault<T>(key:defaultValue:)`: пустой или битый ключ — значение по умолчанию.
/// Сам оригинал такое значение не записывает; в экспорт оно попадает, только
/// если отличается от значения по умолчанию (иначе строка пустая, как у Swift).
pub struct JsonDefaultWithValue<T>(PhantomData<T>);

impl<T: JsonCodable> Codec for JsonDefaultWithValue<T> {
    type Value = T;
    const SWIFT_KIND: &'static str = "JSONDefault (defaultValue)";

    fn load(store: &dyn Store, key: &str, default: &T) -> T {
        store
            .string(key)
            .and_then(|text| decode_json(&text))
            .unwrap_or_else(|| default.clone())
    }

    fn save(store: &mut dyn Store, key: &str, value: &T) {
        store.set_string(key, Some(&encode_json(value)));
    }

    fn to_codable(value: &T, default: &T) -> CodableDefault {
        CodableDefault {
            string: (value != default).then(|| encode_json(value)),
            ..CodableDefault::default()
        }
    }

    fn from_codable(codable: &CodableDefault, _current: &T, default: &T) -> T {
        codable
            .string
            .as_deref()
            .and_then(decode_json)
            .unwrap_or_else(|| default.clone())
    }
}

// ---------------------------------------------------------------- экспорт и импорт

/// `bundleId` в экспорте — как у оригинального Rectangle.
pub const EXPORT_BUNDLE_ID: &str = "com.knollsoft.Rectangle";

/// Файлы больше 1 МБ оригинал не импортирует (защита от раздутого конфига).
pub const MAX_IMPORT_SIZE: usize = 1_048_576;

/// Экспорт настроек — `Defaults.encoded()`: pretty JSON с сортировкой ключей,
/// в `defaults` — все настройки из `Defaults.array`. Горячих клавиш в порте
/// нет, поэтому `shortcuts` — пустой объект (формат Rectangle остаётся валидным).
/// `version` — CFBundleVersion приложения, как у оригинала (при импорте его
/// никто не проверяет).
/// NaN и бесконечности (в оригинале из-за них экспорт не получается вовсе) в
/// JSON не попадают: при чтении из UserDefaults они уже заменены значением по
/// умолчанию, а если такое число всё же оказалось в настройках, в экспорт
/// идёт значение по умолчанию.
pub fn export_json(config: &Config, version: &str) -> String {
    let defaults: Vec<(String, Json)> = config
        .exported_defaults()
        .into_iter()
        .map(|(key, codable)| (key.to_string(), codable.to_json()))
        .collect();

    let document = Json::Object(vec![
        ("bundleId".to_string(), Json::string(EXPORT_BUNDLE_ID)),
        ("version".to_string(), Json::string(version)),
        ("shortcuts".to_string(), Json::Object(Vec::new())),
        ("defaults".to_string(), Json::Object(defaults)),
    ]);
    json::to_pretty(&document)
}

/// Импорт — `Defaults.load(fileUrl:)`. Возвращает новые настройки, ничего не
/// сохраняет (сохранить — `config::update`). Как в оригинале:
/// - файл больше 1 МБ или JSON не того вида — ошибка, настройки не меняются;
/// - неизвестные ключи игнорируются;
/// - настройки, которых нет в файле, остаются как были.
///
/// Секция `shortcuts` пропускается целиком (горячих клавиш в порте нет).
pub fn import_json(text: &str, current: &Config) -> Result<Config, String> {
    if text.len() > MAX_IMPORT_SIZE {
        return Err(format!(
            "файл конфига больше {} байт — оригинал такие не импортирует",
            MAX_IMPORT_SIZE
        ));
    }
    let document = json::parse(text)?;
    if !matches!(document, Json::Object(_)) {
        return Err("конфиг: ожидался объект".to_string());
    }
    let field = |name: &str| {
        document
            .get(name)
            .ok_or_else(|| format!("конфиг: нет ключа {}", name))
    };
    json::decode_string(field("bundleId")?)?;
    json::decode_string(field("version")?)?;

    let Json::Object(default_pairs) = field("defaults")? else {
        return Err("конфиг: defaults должен быть объектом".to_string());
    };
    let mut defaults = BTreeMap::new();
    for (key, value) in default_pairs {
        let codable = CodableDefault::from_json(value)
            .map_err(|error| format!("настройка {}: {}", key, error))?;
        defaults.insert(key.clone(), codable);
    }

    let mut config = current.clone();
    config.import_defaults(&defaults);
    Ok(config)
}
