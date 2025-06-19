#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct RateLimitConfigDto {
    pub enabled: bool,
    pub period_millis: u64,
    pub burst_size: u32,
}