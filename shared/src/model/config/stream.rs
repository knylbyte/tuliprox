use crate::utils::{default_grace_period_millis, default_grace_period_timeout_secs};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct StreamBufferConfigDto {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub size: usize,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct StreamConfigDto {
    #[serde(default)]
    pub retry: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub buffer: Option<StreamBufferConfigDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub throttle: Option<String>,
    #[serde(default = "default_grace_period_millis")]
    pub grace_period_millis: u64,
    #[serde(default = "default_grace_period_timeout_secs")]
    pub grace_period_timeout_secs: u64,
    #[serde(default)]
    pub forced_retry_interval_secs: u32,
    #[serde(default, skip)]
    pub throttle_kbps: u64,
}
