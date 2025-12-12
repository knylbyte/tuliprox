use crate::error::to_io_error;
use chrono::{NaiveDateTime, ParseError, TimeZone, Utc};
use serde::de::DeserializeOwned;
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
    T: DeserializeOwned + std::str::FromStr,
{
    let raw: Value = Value::deserialize(deserializer)?;

    match raw {
        // Null → None
        Value::Null => Ok(None),

        // its a number
        Value::Number(n) => {
            let s = n.to_string();
            match s.parse::<T>() {
                Ok(v) => Ok(Some(v)),
                Err(_) => Ok(None), // Fehler ignorieren, None zurückgeben
            }
        }

        // String → extract first number
        Value::String(s) => {
            let s = s.trim();
            if s.is_empty() {
                return Ok(None);
            }

            // find the number
            let digits = s.chars()
                .skip_while(|c| !c.is_ascii_digit())
                .take_while(|c| c.is_ascii_digit())
                .collect::<String>();

            if digits.is_empty() {
                return Ok(None);
            }

            match digits.parse::<T>() {
                Ok(v) => Ok(Some(v)),
                Err(_) => Ok(None),
            }
        }

        // invalid -> return None
        _ => Ok(None),
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