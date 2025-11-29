use crate::model::config::cache::CacheConfig;
use crate::model::{macros, GeoIpConfig, RateLimitConfig, StreamConfig};
use shared::model::{ResourceRetryConfigDto, ReverseProxyConfigDto, ReverseProxyDisabledHeaderConfigDto};
use shared::utils::{default_resource_retry_attempts, default_resource_retry_backoff_ms, default_resource_retry_backoff_multiplier, hex_to_u8_16, u8_16_to_hex};
use std::cmp::max;

#[derive(Debug, Clone)]
pub struct ReverseProxyDisabledHeaderConfig {
    pub referer_header: bool,
    pub x_header: bool,
    pub cloudfare_header: bool,
    pub custom_header: Vec<String>,
}

impl ReverseProxyDisabledHeaderConfig {
    pub fn should_remove(&self, header: &str) -> bool {
        let header_lc = header.to_ascii_lowercase();
        if self.referer_header && header_lc == "referer" {
            return true;
        }
        if self.x_header && header_lc.starts_with("x-") {
            return true;
        }
        if self.cloudfare_header && header_lc.starts_with("cf-") {
            return true;
        }
        self.custom_header
            .iter()
            .any(|h| h.trim().eq_ignore_ascii_case(&header_lc))
    }
}

#[derive(Debug, Clone)]
pub struct ResourceRetryConfig {
    pub max_attempts: u32,
    pub backoff_millis: u64,
    pub backoff_multiplier: f64,
}

impl Default for ResourceRetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: default_resource_retry_attempts(),
            backoff_millis: default_resource_retry_backoff_ms(),
            backoff_multiplier: default_resource_retry_backoff_multiplier(),
        }
    }
}

impl ResourceRetryConfig {
    pub fn get_retry_values(&self) -> (u32, u64, f64) {
        (
            max(1, self.max_attempts),
            self.backoff_millis.max(1),
            if self.backoff_multiplier.is_finite() {
                self.backoff_multiplier.max(1.0)
            } else {
                1.0
            },
        )
    }

    pub fn get_default_retry_values() -> (u32, u64, f64) {
        (
            default_resource_retry_attempts(),
            default_resource_retry_backoff_ms(),
            default_resource_retry_backoff_multiplier(),
        )
    }
}

macros::from_impl!(ResourceRetryConfig);

impl From<&ResourceRetryConfigDto> for ResourceRetryConfig {
    fn from(dto: &ResourceRetryConfigDto) -> Self {
        let multiplier = if dto.backoff_multiplier.is_finite() {
            dto.backoff_multiplier.max(1.0)
        } else {
            1.0
        };
        Self {
            max_attempts: dto.max_attempts,
            backoff_millis: dto.backoff_millis,
            backoff_multiplier: multiplier,
        }
    }
}

impl From<&ResourceRetryConfig> for ResourceRetryConfigDto {
    fn from(cfg: &ResourceRetryConfig) -> Self {
        Self {
            max_attempts: cfg.max_attempts,
            backoff_millis: cfg.backoff_millis,
            backoff_multiplier: cfg.backoff_multiplier,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ReverseProxyConfig {
    pub resource_rewrite_disabled: bool,
    pub rewrite_secret: [u8; 16],
    pub resource_retry: ResourceRetryConfig,
    pub disabled_header: Option<ReverseProxyDisabledHeaderConfig>,
    pub stream: Option<StreamConfig>,
    pub cache: Option<CacheConfig>,
    pub rate_limit: Option<RateLimitConfig>,
    pub geoip: Option<GeoIpConfig>,
}

macros::from_impl!(ReverseProxyConfig);

impl From<&ReverseProxyConfigDto> for ReverseProxyConfig {
    fn from(dto: &ReverseProxyConfigDto) -> Self {
        Self {
            resource_rewrite_disabled: dto.resource_rewrite_disabled,
            rewrite_secret: hex_to_u8_16(&dto.rewrite_secret).unwrap_or_default(),
            resource_retry: dto
                .resource_retry
                .as_ref()
                .map_or_else(ResourceRetryConfig::default, Into::into),
            disabled_header: dto.disabled_header.as_ref().map(|d| ReverseProxyDisabledHeaderConfig {
                referer_header: d.referer_header,
                x_header: d.x_header,
                cloudfare_header: d.cloudfare_header,
                custom_header: d.custom_header.clone(),
            }),
            stream: dto.stream.as_ref().map(Into::into),
            cache: dto.cache.as_ref().map(Into::into),
            rate_limit: dto.rate_limit.as_ref().map(Into::into),
            geoip: dto.geoip.as_ref().map(Into::into),
        }
    }
}

impl From<&ReverseProxyConfig> for ReverseProxyConfigDto {
    fn from(instance: &ReverseProxyConfig) -> Self {
        Self {
            resource_rewrite_disabled: instance.resource_rewrite_disabled,
            rewrite_secret: u8_16_to_hex(&instance.rewrite_secret),
            resource_retry: Some(ResourceRetryConfigDto::from(&instance.resource_retry)),
            disabled_header: instance.disabled_header.as_ref().map(|d| ReverseProxyDisabledHeaderConfigDto {
                referer_header: d.referer_header,
                x_header: d.x_header,
                cloudfare_header: d.cloudfare_header,
                custom_header: d.custom_header.clone(),
            }),
            stream: instance.stream.as_ref().map(Into::into),
            cache: instance.cache.as_ref().map(Into::into),
            rate_limit: instance.rate_limit.as_ref().map(Into::into),
            geoip: instance.geoip.as_ref().map(Into::into),
        }
    }
}
