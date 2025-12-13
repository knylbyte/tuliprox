pub const fn default_as_true() -> bool { true }

pub fn default_as_default() -> String { String::from("default") }

// Default delay values for resolving VOD or Series requests,
// used to prevent frequent requests that could trigger a provider ban.
pub const fn default_resolve_delay_secs() -> u16 { 2 }

// Default grace values to accommodate rapid channel changes and seek requests,
// helping avoid triggering hard max_connection enforcement.
pub const fn default_grace_period_millis() -> u64 { 2000 }
pub const fn default_grace_period_timeout_secs() -> u64 { 4 }
pub const fn default_connect_timeout_secs() -> u32 { 6 }
pub const fn default_resource_retry_attempts() -> u32 { 3 }
pub const fn default_resource_retry_backoff_ms() -> u64 { 250 }
pub const fn default_resource_retry_backoff_multiplier() -> f64 { 1.0 }
pub fn default_secret() -> String {
    let mut out = [0u8; 16];
    for x in &mut out {
        *x = fastrand::u8(..);
    }

    out.iter().map(|b| format!("{:02X}", b)).collect()
}

pub const fn default_kick_secs() -> u64 { 90 }