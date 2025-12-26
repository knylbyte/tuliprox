use crate::error::to_io_error;
use chrono::{NaiveDateTime, ParseError, TimeZone, Utc};
use serde::{Deserialize, Deserializer};
use serde_json::Value;
use std::io;

fn value_to_string_array(value: &[Value]) -> Vec<String> {
    value.iter().filter_map(value_to_string).collect()
}

fn value_to_string(v: &Value) -> Option<String> {
    match v {
        Value::Bool(value) => Some(value.to_string()),
        Value::Number(value) => Some(value.to_string()),
        Value::String(value) => Some(value.to_string()),
        _ => None,
    }
}

pub fn deserialize_as_option_string<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value: Value = serde::Deserialize::deserialize(deserializer)?;

    match &value {
        Value::String(s) => Ok(Some(s.to_owned())),
        Value::Number(s) => Ok(Some(s.to_string())),
        _ => Ok(None),
    }
}

pub fn deserialize_as_string<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value: Value = serde::Deserialize::deserialize(deserializer)?;

    match &value {
        Value::String(s) => Ok(s.to_string()),
        Value::Number(s) => Ok(s.to_string()),
        Value::Null => Ok(String::new()),
        _ => Ok(value.to_string()),
    }
}

pub fn deserialize_as_string_array<'de, D>(deserializer: D) -> Result<Option<Vec<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Value::deserialize(deserializer).map(|v| match v {
        Value::String(value) => Some(vec![value]),
        Value::Array(value) => Some(value_to_string_array(&value)),
        _ => None,
    })
}

pub fn deserialize_number_from_string<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: std::str::FromStr,
{
    let raw: Value = Value::deserialize(deserializer)?;

    match raw {
        // Null → None
        Value::Null => Ok(None),

        // its a number
        Value::Number(n) => {
            let s = n.to_string();
            if let Ok(r) = s.parse::<T>() {
                Ok(Some(r))
            } else {
                Ok(None)
            }
        }

        // String → extract first number
        Value::String(s) => {
            let s = s.trim();
            if s.is_empty() {
                return Ok(None);
            }

            // Find first digit OR a '.' that is immediately followed by a digit;
            // include optional sign immediately before it (ignoring whitespace).
            let mut last_non_ws: Option<(usize, char)> = None;
            let mut num_pos: Option<usize> = None;
            for (i, c) in s.char_indices() {
                if c.is_ascii_digit() {
                    num_pos = Some(i);
                    break;
                }
                if c == '.' && s[i + 1..].chars().next().is_some_and(|n| n.is_ascii_digit()) {
                    num_pos = Some(i);
                    break;
                }
                if !c.is_whitespace() {
                    last_non_ws = Some((i, c));
                }
            }
            let Some(num_i) = num_pos else { return Ok(None); };

            let start = match last_non_ws {
                Some((i, '-')) | Some((i, '+')) => i,
                _ => num_i,
            };
            let mut it = s[start..].chars().peekable();

            // optional sign
            let mut out = String::new();
            if matches!(it.peek(), Some('-' | '+')) {
                out.push(it.next().unwrap());
                while matches!(it.peek(), Some(c) if c.is_whitespace()) {
                    it.next();
                }
            }

            // digits + optional single dot
            let mut saw_digit = false;
            let mut saw_dot = false;
            while let Some(&c) = it.peek() {
                if c.is_ascii_digit() {
                    saw_digit = true;
                    out.push(c);
                    it.next();
                    continue;
                }
                if c == '.' && !saw_dot {
                    saw_dot = true;
                    out.push(c);
                    it.next();
                    continue;
                }
                break;
            }
            if !saw_digit {
                return Ok(None);
            }

            // Try full parse; if it fails and we included '.', fall back to integer part.
            if let Ok(v) = out.parse::<T>() {
                return Ok(Some(v));
            }
            if saw_dot {
                let int_part = out.split('.').next().unwrap_or("");
                if !int_part.is_empty() && int_part != "-" && int_part != "+" {
                    if let Ok(v) = int_part.parse::<T>() {
                        return Ok(Some(v));
                    }
                }
            }
            Ok(None)
        }

        // invalid -> return None
        _ => Ok(None),
    }
}

pub fn deserialize_number_from_string_or_zero<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: std::str::FromStr + Default,
{
    match deserialize_number_from_string(deserializer) {
        Ok(Some(v)) => Ok(v),
        Ok(None) => Ok(T::default()),
        Err(e) => Err(e),
    }
}

#[inline]
pub fn bin_serialize<T>(value: &T) -> io::Result<Vec<u8>>
where
    T: serde::Serialize,
{
    let mut buf = Vec::new();
    ciborium::ser::into_writer(value, &mut buf).map_err(to_io_error)?;
    Ok(buf)
}

#[inline]
pub fn bin_deserialize<T>(value: &[u8]) -> io::Result<T>
where
    T: for<'a> serde::Deserialize<'a>,
{
    ciborium::de::from_reader(value).map_err(to_io_error)
}


pub fn u8_16_to_hex(bytes: &[u8; 16]) -> String {
    bytes.iter().map(|b| format!("{:02X}", b)).collect()
}

pub fn hex_to_u8_16(hex: &str) -> Result<[u8; 16], String> {
    if hex.len() != 32 {
        return Err("Hex string must be exactly 32 characters".into());
    }

    let mut out = [0u8; 16];

    for (i, chunk) in hex.as_bytes().chunks(2).enumerate() {
        let s = std::str::from_utf8(chunk).map_err(|_| "Invalid UTF-8")?;
        out[i] = u8::from_str_radix(s, 16).map_err(|_| "Invalid hex")?;
    }

    Ok(out)
}

pub fn hex_to_secret<'de, D>(deserializer: D) -> Result<[u8; 16], D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s: String = Deserialize::deserialize(deserializer)?;
    hex_to_u8_16(&s).map_err(serde::de::Error::custom)
}

pub fn secret_to_hex<S>(bytes: &[u8; 16], serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_str(&u8_16_to_hex(bytes))
}

/// Deserializes a timestamp from either a Unix timestamp (seconds) or a UTC datetime string
/// in the format "YYYY-MM-DD HH:MM:SS". Note: Datetime strings are interpreted as UTC.
pub fn deserialize_timestamp<'de, D>(deserializer: D) -> Result<Option<i64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    // - try to deserialize as seconds
    // - try to deserialize as date-time string of format like "2028-11-23 14:12:34"
    let val = Option::<Value>::deserialize(deserializer)?;
    match val {
        Some(Value::Number(n)) => n
            .as_i64()
            .ok_or_else(|| serde::de::Error::custom("invalid number"))
            .map(Some),
        Some(Value::String(s)) => parse_timestamp(&s).map_err(serde::de::Error::custom),
        Some(Value::Null) => Ok(None),
        Some(_) => Err(serde::de::Error::custom("expected number or string")),
        None => Ok(None),
    }
}

pub fn parse_timestamp(value: &str) -> Result<Option<i64>, ParseError> {
    let value = value.trim();
    if value.is_empty() {
        return Ok(None);
    }

    if let Ok(ts) = value.parse::<i64>() {
        return Ok(Some(ts));
    }

    //  "YYYY-MM-DD HH:MM:SS"
    let dt = NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S")?;
    let timestamp = Utc.from_utc_datetime(&dt).timestamp();
    Ok(Some(timestamp))
}

pub fn deserialize_json_as_opt_string<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let val: Value = Deserialize::deserialize(deserializer)?;
    Ok(Some(val.to_string()))
}


const RELEASE_DATES: [&str; 3] = [
    "release_date",
    "releaseDate",
    "releasedata",
];

pub fn deserialize_release_date<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Value::deserialize(deserializer)?;

    for key in RELEASE_DATES {
        if let Some(v) = value.get(key) {
            if let Some(s) = v.as_str() {
                if !s.trim().is_empty() {
                    return Ok(s.to_string());
                }
            }
        }
    }

    Ok(String::new())
}