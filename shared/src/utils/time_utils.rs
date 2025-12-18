#[cfg(target_arch = "wasm32")]
pub fn current_time_secs() -> u64 {
    (js_sys::Date::now() / 1000.0) as u64
}

#[cfg(not(target_arch = "wasm32"))]
pub fn current_time_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub fn unix_ts_to_str(ts: i64) -> Option<String> {
    if ts > 0 {
        let normalized_ts = if ts > 4102444800 { ts / 1000 } else { ts };
        chrono::DateTime::from_timestamp(normalized_ts, 0)
            .map(|dt| dt.format("%d.%m.%Y").to_string())
    } else {
        None
    }
}
