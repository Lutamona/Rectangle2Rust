//! Минимальный JSON: разбор и запись в формате Swift `JSONEncoder`.
//!
//! Нужен для настроек, которые оригинал хранит JSON-строкой (`JSONDefault`), и
//! для экспорта/импорта конфига (`PrefsWindow/Config.swift`). Запись повторяет
//! `JSONEncoder` с `.sortedKeys` (и `.prettyPrinted` для экспорта) байт-в-байт —
//! сверено с JSONEncoder на этой машине: ключи по возрастанию, `" : "`, отступ
//! два пробела, пустые `{}`/`[]` в pretty-режиме с пустой строкой внутри,
//! `/` экранируется как `\/`, числа — `description` у Float/Double без хвоста `.0`.

/// JSON-значение. Число хранится текстом: при разборе — как в исходнике
/// (чтобы Float читался из текста напрямую, без округления через Double),
/// при записи — уже в формате Swift.
#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Number(String),
    String(String),
    Array(Vec<Json>),
    /// Пары в порядке появления; при записи сортируются по ключу.
    Object(Vec<(String, Json)>),
}

/// Глубина вложенности, как у JSONDecoder (защита от переполнения стека).
const MAX_DEPTH: usize = 512;

impl Json {
    /// Значение по ключу объекта (при повторах — последнее).
    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Object(pairs) => pairs
                .iter()
                .rev()
                .find(|(name, _)| name == key)
                .map(|(_, value)| value),
            _ => None,
        }
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Json::Null)
    }

    pub fn from_f32(value: f32) -> Json {
        Json::Number(json_number(swift_description_f32(value)))
    }

    pub fn from_f64(value: f64) -> Json {
        Json::Number(json_number(swift_description_f64(value)))
    }

    pub fn from_i64(value: i64) -> Json {
        Json::Number(value.to_string())
    }

    pub fn from_u64(value: u64) -> Json {
        Json::Number(value.to_string())
    }

    pub fn string(value: &str) -> Json {
        Json::String(value.to_string())
    }
}

// ---------------------------------------------------------------- чтение типов

fn type_name(value: &Json) -> &'static str {
    match value {
        Json::Null => "null",
        Json::Bool(_) => "bool",
        Json::Number(_) => "number",
        Json::String(_) => "string",
        Json::Array(_) => "array",
        Json::Object(_) => "object",
    }
}

fn mismatch(expected: &str, value: &Json) -> String {
    format!("ожидался {}, а пришёл {}", expected, type_name(value))
}

fn is_integer_literal(text: &str) -> bool {
    !text.contains(['.', 'e', 'E'])
}

pub fn decode_bool(value: &Json) -> Result<bool, String> {
    match value {
        Json::Bool(flag) => Ok(*flag),
        other => Err(mismatch("bool", other)),
    }
}

pub fn decode_string(value: &Json) -> Result<String, String> {
    match value {
        Json::String(text) => Ok(text.clone()),
        other => Err(mismatch("string", other)),
    }
}

/// Swift `Int`: целое без потерь; `1.0` и `1e2` JSONDecoder тоже принимает.
pub fn decode_i64(value: &Json) -> Result<i64, String> {
    let Json::Number(text) = value else {
        return Err(mismatch("number", value));
    };
    let not_representable = || format!("число {} не помещается в Int", text);
    if is_integer_literal(text) {
        return text.parse::<i64>().map_err(|_| not_representable());
    }
    let number: f64 = text.parse().map_err(|_| not_representable())?;
    // Границы — ±2^63; NaN и бесконечности отсекает fract().
    if number.fract() == 0.0
        && (-9.223_372_036_854_776e18..9.223_372_036_854_776e18).contains(&number)
    {
        Ok(number as i64)
    } else {
        Err(not_representable())
    }
}

/// Swift `UInt`: как `Int`, но без знака.
pub fn decode_u64(value: &Json) -> Result<u64, String> {
    let Json::Number(text) = value else {
        return Err(mismatch("number", value));
    };
    let not_representable = || format!("число {} не помещается в UInt", text);
    if is_integer_literal(text) {
        if let Some(rest) = text.strip_prefix('-') {
            return match rest.parse::<u64>() {
                Ok(0) => Ok(0),
                _ => Err(not_representable()),
            };
        }
        return text.parse::<u64>().map_err(|_| not_representable());
    }
    let number: f64 = text.parse().map_err(|_| not_representable())?;
    // Граница — 2^64; NaN и бесконечности отсекает fract().
    if number.fract() == 0.0 && (0.0..1.844_674_407_370_955_2e19).contains(&number) {
        Ok(number as u64)
    } else {
        Err(not_representable())
    }
}

/// Swift `Float`: текст числа разбирается сразу во Float; бесконечность — ошибка.
pub fn decode_f32(value: &Json) -> Result<f32, String> {
    let Json::Number(text) = value else {
        return Err(mismatch("number", value));
    };
    match text.parse::<f32>() {
        Ok(number) if number.is_finite() => Ok(number),
        _ => Err(format!("число {} не помещается в Float", text)),
    }
}

/// Swift `Double` (и `CGFloat`).
pub fn decode_f64(value: &Json) -> Result<f64, String> {
    let Json::Number(text) = value else {
        return Err(mismatch("number", value));
    };
    match text.parse::<f64>() {
        Ok(number) if number.is_finite() => Ok(number),
        _ => Err(format!("число {} не помещается в Double", text)),
    }
}

// ---------------------------------------------------------------- числа как в Swift

/// `Float.description` из Swift: кратчайшая запись, экспонента при |x| < 1e-4
/// или |x| > 2^24.
pub fn swift_description_f32(value: f32) -> String {
    if value.is_nan() {
        return "nan".to_string();
    }
    if value.is_infinite() {
        return if value < 0.0 { "-inf" } else { "inf" }.to_string();
    }
    let exponential_above = 16_777_216.0_f64; // 2^24
    swift_description(
        format!("{:e}", value.abs()),
        value.is_sign_negative(),
        value == 0.0,
        (value.abs() as f64) > exponential_above,
    )
}

/// `Double.description` из Swift: экспонента при |x| < 1e-4 или |x| > 2^53.
pub fn swift_description_f64(value: f64) -> String {
    if value.is_nan() {
        return "nan".to_string();
    }
    if value.is_infinite() {
        return if value < 0.0 { "-inf" } else { "inf" }.to_string();
    }
    let exponential_above = 9_007_199_254_740_992.0_f64; // 2^53
    swift_description(
        format!("{:e}", value.abs()),
        value.is_sign_negative(),
        value == 0.0,
        value.abs() > exponential_above,
    )
}

/// Общая часть: `scientific` — кратчайшая запись модуля числа вида `d.ddde±x`
/// (Rust печатает её тем же алгоритмом «кратчайшее, что читается обратно»).
fn swift_description(scientific: String, negative: bool, zero: bool, too_large: bool) -> String {
    let sign = if negative { "-" } else { "" };
    if zero {
        return format!("{}0.0", sign);
    }
    let (mantissa, exponent) = scientific.split_once('e').unwrap_or((&scientific, "0"));
    let exponent: i32 = exponent.parse().unwrap_or(0);
    let digits: String = mantissa.chars().filter(|ch| *ch != '.').collect();

    if exponent < -4 || too_large {
        let exponent_sign = if exponent < 0 { '-' } else { '+' };
        return format!(
            "{}{}e{}{:02}",
            sign,
            mantissa,
            exponent_sign,
            exponent.abs()
        );
    }

    if exponent < 0 {
        let zeros = "0".repeat((-exponent - 1) as usize);
        return format!("{}0.{}{}", sign, zeros, digits);
    }

    let integer_len = exponent as usize + 1;
    if digits.len() <= integer_len {
        let zeros = "0".repeat(integer_len - digits.len());
        format!("{}{}{}.0", sign, digits, zeros)
    } else {
        format!(
            "{}{}.{}",
            sign,
            &digits[..integer_len],
            &digits[integer_len..]
        )
    }
}

/// JSONEncoder пишет число как `description`, отрезая хвост `.0`.
fn json_number(description: String) -> String {
    match description.strip_suffix(".0") {
        Some(trimmed) => trimmed.to_string(),
        None => description,
    }
}

// ---------------------------------------------------------------- запись

/// Компактная запись с сортировкой ключей (`JSONEncoder` + `.sortedKeys`).
pub fn to_compact(value: &Json) -> String {
    let mut out = String::new();
    write_value(&mut out, value, None);
    out
}

/// Pretty-запись с сортировкой ключей (`.prettyPrinted, .sortedKeys`).
pub fn to_pretty(value: &Json) -> String {
    let mut out = String::new();
    write_value(&mut out, value, Some(0));
    out
}

fn indent(out: &mut String, depth: usize) {
    for _ in 0..depth {
        out.push_str("  ");
    }
}

/// `depth` — None для компактной записи, иначе текущий уровень отступа.
fn write_value(out: &mut String, value: &Json, depth: Option<usize>) {
    match value {
        Json::Null => out.push_str("null"),
        Json::Bool(flag) => out.push_str(if *flag { "true" } else { "false" }),
        Json::Number(text) => out.push_str(text),
        Json::String(text) => write_string(out, text),
        Json::Array(items) => {
            out.push('[');
            match depth {
                None => {
                    for (index, item) in items.iter().enumerate() {
                        if index > 0 {
                            out.push(',');
                        }
                        write_value(out, item, None);
                    }
                }
                Some(depth) => {
                    out.push('\n');
                    for (index, item) in items.iter().enumerate() {
                        if index > 0 {
                            out.push_str(",\n");
                        }
                        indent(out, depth + 1);
                        write_value(out, item, Some(depth + 1));
                    }
                    out.push('\n');
                    indent(out, depth);
                }
            }
            out.push(']');
        }
        Json::Object(pairs) => {
            let mut sorted: Vec<&(String, Json)> = pairs.iter().collect();
            sorted.sort_by(|a, b| a.0.cmp(&b.0));
            out.push('{');
            match depth {
                None => {
                    for (index, (key, item)) in
                        sorted.iter().map(|pair| (&pair.0, &pair.1)).enumerate()
                    {
                        if index > 0 {
                            out.push(',');
                        }
                        write_string(out, key);
                        out.push(':');
                        write_value(out, item, None);
                    }
                }
                Some(depth) => {
                    out.push('\n');
                    for (index, (key, item)) in
                        sorted.iter().map(|pair| (&pair.0, &pair.1)).enumerate()
                    {
                        if index > 0 {
                            out.push_str(",\n");
                        }
                        indent(out, depth + 1);
                        write_string(out, key);
                        out.push_str(" : ");
                        write_value(out, item, Some(depth + 1));
                    }
                    out.push('\n');
                    indent(out, depth);
                }
            }
            out.push('}');
        }
    }
}

/// Экранирование как у JSONEncoder: кавычка, обратная косая, `/`, управляющие
/// символы; всё остальное (включая кириллицу и эмодзи) — как есть.
fn write_string(out: &mut String, text: &str) {
    out.push('"');
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '/' => out.push_str("\\/"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            ch if (ch as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", ch as u32)),
            ch => out.push(ch),
        }
    }
    out.push('"');
}

// ---------------------------------------------------------------- разбор

pub fn parse(text: &str) -> Result<Json, String> {
    let mut parser = Parser {
        bytes: text.as_bytes(),
        position: 0,
    };
    parser.skip_whitespace();
    let value = parser.value(0)?;
    parser.skip_whitespace();
    if parser.position != parser.bytes.len() {
        return Err(parser.error("лишние символы после значения"));
    }
    Ok(value)
}

struct Parser<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl Parser<'_> {
    fn error(&self, message: &str) -> String {
        format!("JSON: {} (позиция {})", message, self.position)
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.position).copied()
    }

    fn skip_whitespace(&mut self) {
        while let Some(b' ' | b'\n' | b'\r' | b'\t') = self.peek() {
            self.position += 1;
        }
    }

    fn expect_literal(&mut self, literal: &str, value: Json) -> Result<Json, String> {
        if self.bytes[self.position..].starts_with(literal.as_bytes()) {
            self.position += literal.len();
            Ok(value)
        } else {
            Err(self.error("неизвестное слово"))
        }
    }

    fn value(&mut self, depth: usize) -> Result<Json, String> {
        if depth > MAX_DEPTH {
            return Err(self.error("слишком глубокая вложенность"));
        }
        match self.peek() {
            None => Err(self.error("неожиданный конец")),
            Some(b'n') => self.expect_literal("null", Json::Null),
            Some(b't') => self.expect_literal("true", Json::Bool(true)),
            Some(b'f') => self.expect_literal("false", Json::Bool(false)),
            Some(b'"') => self.string().map(Json::String),
            Some(b'[') => self.array(depth),
            Some(b'{') => self.object(depth),
            Some(b'-' | b'0'..=b'9') => self.number(),
            Some(_) => Err(self.error("неожиданный символ")),
        }
    }

    fn array(&mut self, depth: usize) -> Result<Json, String> {
        self.position += 1;
        let mut items = Vec::new();
        self.skip_whitespace();
        if self.peek() == Some(b']') {
            self.position += 1;
            return Ok(Json::Array(items));
        }
        loop {
            self.skip_whitespace();
            items.push(self.value(depth + 1)?);
            self.skip_whitespace();
            match self.peek() {
                Some(b',') => self.position += 1,
                Some(b']') => {
                    self.position += 1;
                    return Ok(Json::Array(items));
                }
                _ => return Err(self.error("ожидалась , или ]")),
            }
        }
    }

    fn object(&mut self, depth: usize) -> Result<Json, String> {
        self.position += 1;
        let mut pairs = Vec::new();
        self.skip_whitespace();
        if self.peek() == Some(b'}') {
            self.position += 1;
            return Ok(Json::Object(pairs));
        }
        loop {
            self.skip_whitespace();
            if self.peek() != Some(b'"') {
                return Err(self.error("ожидался ключ-строка"));
            }
            let key = self.string()?;
            self.skip_whitespace();
            if self.peek() != Some(b':') {
                return Err(self.error("ожидалось :"));
            }
            self.position += 1;
            self.skip_whitespace();
            let value = self.value(depth + 1)?;
            pairs.push((key, value));
            self.skip_whitespace();
            match self.peek() {
                Some(b',') => self.position += 1,
                Some(b'}') => {
                    self.position += 1;
                    return Ok(Json::Object(pairs));
                }
                _ => return Err(self.error("ожидалась , или }")),
            }
        }
    }

    fn digits(&mut self) -> usize {
        let start = self.position;
        while let Some(b'0'..=b'9') = self.peek() {
            self.position += 1;
        }
        self.position - start
    }

    fn number(&mut self) -> Result<Json, String> {
        let start = self.position;
        if self.peek() == Some(b'-') {
            self.position += 1;
        }
        match self.peek() {
            Some(b'0') => self.position += 1,
            Some(b'1'..=b'9') => {
                self.digits();
            }
            _ => return Err(self.error("неверное число")),
        }
        if self.peek() == Some(b'.') {
            self.position += 1;
            if self.digits() == 0 {
                return Err(self.error("неверное число"));
            }
        }
        if let Some(b'e' | b'E') = self.peek() {
            self.position += 1;
            if let Some(b'+' | b'-') = self.peek() {
                self.position += 1;
            }
            if self.digits() == 0 {
                return Err(self.error("неверное число"));
            }
        }
        let text = std::str::from_utf8(&self.bytes[start..self.position])
            .map_err(|_| self.error("неверное число"))?;
        Ok(Json::Number(text.to_string()))
    }

    fn hex4(&mut self) -> Result<u32, String> {
        let chunk = self
            .bytes
            .get(self.position..self.position + 4)
            .ok_or_else(|| self.error("обрыв \\u"))?;
        let text = std::str::from_utf8(chunk).map_err(|_| self.error("неверный \\u"))?;
        let code = u32::from_str_radix(text, 16).map_err(|_| self.error("неверный \\u"))?;
        self.position += 4;
        Ok(code)
    }

    fn string(&mut self) -> Result<String, String> {
        self.position += 1;
        let mut out = String::new();
        loop {
            let start = self.position;
            while let Some(byte) = self.peek() {
                if byte == b'"' || byte == b'\\' || byte < 0x20 {
                    break;
                }
                self.position += 1;
            }
            let chunk = std::str::from_utf8(&self.bytes[start..self.position])
                .map_err(|_| self.error("неверный UTF-8"))?;
            out.push_str(chunk);

            match self.peek() {
                None => return Err(self.error("незакрытая строка")),
                Some(b'"') => {
                    self.position += 1;
                    return Ok(out);
                }
                Some(b'\\') => {
                    self.position += 1;
                    let escape = self
                        .peek()
                        .ok_or_else(|| self.error("обрыв экранирования"))?;
                    self.position += 1;
                    match escape {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{8}'),
                        b'f' => out.push('\u{c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let first = self.hex4()?;
                            let code = if (0xD800..0xDC00).contains(&first) {
                                if !self.bytes[self.position..].starts_with(b"\\u") {
                                    return Err(self.error("одинокий суррогат"));
                                }
                                self.position += 2;
                                let second = self.hex4()?;
                                if !(0xDC00..0xE000).contains(&second) {
                                    return Err(self.error("одинокий суррогат"));
                                }
                                0x10000 + ((first - 0xD800) << 10) + (second - 0xDC00)
                            } else {
                                first
                            };
                            let ch = char::from_u32(code)
                                .ok_or_else(|| self.error("одинокий суррогат"))?;
                            out.push(ch);
                        }
                        _ => return Err(self.error("неизвестное экранирование")),
                    }
                }
                Some(_) => return Err(self.error("управляющий символ в строке")),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_are_written_like_swift() {
        // Сверено с JSONEncoder: Float(0.3) → 0.3, 1680 → 1680, 1e-5 → 1e-05 и т.д.
        let cases_f32: [(f32, &str); 10] = [
            (0.3, "0.3"),
            (1680.0, "1680"),
            (1e-5, "1e-05"),
            (1e-4, "0.0001"),
            (16_777_216.0, "16777216"),
            (16_777_218.0, "1.6777218e+07"),
            (1e15, "1e+15"),
            (2.0 / 3.0, "0.6666667"),
            (-0.0, "-0"),
            (-1.0, "-1"),
        ];
        for (value, expected) in cases_f32 {
            assert_eq!(to_compact(&Json::from_f32(value)), expected, "{}", value);
        }
        let cases_f64: [(f64, &str); 7] = [
            (0.25, "0.25"),
            (1e15, "1000000000000000"),
            (1e16, "1e+16"),
            (0.1 + 0.2, "0.30000000000000004"),
            (5e-324, "5e-324"),
            (1.7976931348623157e308, "1.7976931348623157e+308"),
            (123_456_789.123, "123456789.123"),
        ];
        for (value, expected) in cases_f64 {
            assert_eq!(to_compact(&Json::from_f64(value)), expected, "{}", value);
        }
        assert_eq!(swift_description_f32(9.9999e-5), "9.9999e-05");
        assert_eq!(swift_description_f32(1680.0), "1680.0");
    }

    #[test]
    fn pretty_output_matches_json_encoder() {
        let value = Json::Object(vec![
            ("b".to_string(), Json::Object(vec![])),
            (
                "a".to_string(),
                Json::Array(vec![Json::from_i64(1), Json::from_i64(2)]),
            ),
            ("c".to_string(), Json::Array(vec![])),
            ("d".to_string(), Json::string("a/b\"c\\d\n\t\u{1}é")),
        ]);
        let expected = "{\n  \"a\" : [\n    1,\n    2\n  ],\n  \"b\" : {\n\n  },\n  \"c\" : [\n\n  ],\n  \"d\" : \"a\\/b\\\"c\\\\d\\n\\t\\u0001é\"\n}";
        assert_eq!(to_pretty(&value), expected);
        assert_eq!(to_pretty(&Json::Object(vec![])), "{\n\n}");
        assert_eq!(
            to_compact(&value),
            "{\"a\":[1,2],\"b\":{},\"c\":[],\"d\":\"a\\/b\\\"c\\\\d\\n\\t\\u0001é\"}"
        );
    }

    #[test]
    fn parses_and_decodes_like_json_decoder() {
        let value =
            parse(" {\"a\": [1, 2.5e1, -0], \"s\": \"\\u00e9\\ud83d\\ude00\\/\", \"n\": null} ")
                .unwrap();
        let Some(Json::Array(items)) = value.get("a") else {
            panic!("нет массива");
        };
        assert_eq!(decode_i64(&items[0]), Ok(1));
        assert_eq!(decode_i64(&items[1]), Ok(25));
        assert_eq!(decode_u64(&items[2]), Ok(0));
        assert_eq!(
            decode_string(value.get("s").unwrap()),
            Ok("é😀/".to_string())
        );
        assert!(value.get("n").unwrap().is_null());

        assert!(decode_i64(&Json::Number("1.5".into())).is_err());
        assert!(decode_u64(&Json::Number("-1".into())).is_err());
        assert!(decode_f32(&Json::Number("1e39".into())).is_err());
        assert!(decode_bool(&Json::Number("1".into())).is_err());
        assert_eq!(
            decode_f32(&Json::Number("0.30000001192092896".into())),
            Ok(0.3)
        );

        assert!(parse("[1,]").is_err());
        assert!(parse("{\"a\":1} x").is_err());
        assert!(parse("\"\\ud83d\"").is_err());
        assert!(parse("01").is_err());
    }
}
