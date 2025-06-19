use crate::model::{CacheConfigDto, RateLimitConfigDto, StreamConfigDto};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct ReverseProxyConfigDto {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream: Option<StreamConfigDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache: Option<CacheConfigDto>,
    #[serde(default)]
    pub resource_rewrite_disabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate_limit: Option<RateLimitConfigDto>,
    #[serde(default)]
    pub disable_referer_header: bool,
}
