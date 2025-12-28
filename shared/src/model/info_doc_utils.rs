use crate::model::xtream_const;
use serde_json::Value;

pub struct InfoDocUtils {}

impl InfoDocUtils {
    pub fn extract_year_from_release_date(release_date: &str) -> Option<u32> {
        if release_date.len() >= 4 {
            release_date[..4].parse::<u32>().ok()
        } else {
            None
        }
    }

    pub fn make_bdpath_resource_url(resource_url: Option<&str>, bd_path: &str, index: usize, field_prefix: &str) -> String {
        if let Some(url) = resource_url {
            if bd_path.starts_with("http") {
                return format!("{url}/{field_prefix}{}_{index}", xtream_const::XC_PROP_BACKDROP_PATH);
            }
        }
        bd_path.to_string()
    }

    pub fn limited(n: f64) -> String {
        if n < 0.01 {
            "0".to_string()
        } else {
            let s = format!("{:.2}", n);
            s.strip_suffix(".00").unwrap_or(&s).to_string()
        }
    }

    pub fn build_string(value: Option<&str>) -> Value {
        Value::String(value.map_or_else(String::new, String::from))
    }

    pub fn empty_string() -> Value {
        Value::String(String::new())
    }

    pub fn build_value(value: Option<&str>) -> Value {
        if let Some(text) = value {
            if let Ok(result) = serde_json::from_str(text) {
                return result;
            }
        }
        Value::Array(Vec::new())
    }

    pub fn build_u32(value: u32) -> Value {
        Value::Number(serde_json::Number::from(value))
    }

    pub fn make_resource_url(resource_url: Option<&str>, value: &str, field: &str) -> String {
        if let Some(url) = resource_url {
            if value.starts_with("http") {
                return format!("{url}/{field}");
            }
        }
        value.to_string()
    }
}