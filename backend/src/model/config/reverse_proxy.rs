use log::warn;
use shared::error::TuliproxError;
use crate::model::config::cache::CacheConfig;
use crate::model::{RateLimitConfig, StreamConfig};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct ReverseProxyConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream: Option<StreamConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache: Option<CacheConfig>,
    #[serde(default)]
    pub resource_rewrite_disabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate_limit: Option<RateLimitConfig>,
    #[serde(default)]
    pub disable_referer_header: bool,
}


impl ReverseProxyConfig {
    pub(crate) fn prepare(&mut self, working_dir: &str) -> Result<(), TuliproxError> {
        if let Some(stream) = self.stream.as_mut() {
            stream.prepare()?;
        }
        if let Some(cache) = self.cache.as_mut() {
            if cache.enabled && self.resource_rewrite_disabled {
                warn!("The cache is disabled because resource rewrite is disabled");
                cache.enabled = false;
            }
            cache.prepare(working_dir)?;
        }

        if let Some(rate_limit) = self.rate_limit.as_mut() {
            if rate_limit.enabled {
                rate_limit.prepare()?;
            }
        }
        Ok(())
    }
}
