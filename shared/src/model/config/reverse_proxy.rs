use crate::error::TuliproxError;
use crate::model::{CacheConfigDto, RateLimitConfigDto, StreamConfigDto};
use crate::utils::default_as_true;
use log::warn;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ReverseProxyConfigDto {
    #[serde(default)]
    pub resource_rewrite_disabled: bool,
    #[serde(default)]
    pub disable_referer_header: bool,
    #[serde(default = "default_as_true")]
    pub remove_x_header: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream: Option<StreamConfigDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache: Option<CacheConfigDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate_limit: Option<RateLimitConfigDto>,
}

impl Default for ReverseProxyConfigDto {
    fn default() -> Self {
        Self {
            resource_rewrite_disabled: false,
            disable_referer_header: false,
            remove_x_header: default_as_true(),
            stream: None,
            cache: None,
            rate_limit: None,
        }
    }
}

impl ReverseProxyConfigDto {
    pub fn is_empty(&self) -> bool {
        !self.resource_rewrite_disabled
            && !self.disable_referer_header
            && self.remove_x_header
            && (self.stream.is_none() || self.stream.as_ref().is_some_and(|s| s.is_empty()))
            && (self.cache.is_none() || self.cache.as_ref().is_some_and(|c| c.is_empty()))
            && (self.rate_limit.is_none() || self.rate_limit.as_ref().is_some_and(|r| r.is_empty()))
    }

    pub fn clean(&mut self) {
        if self.stream.as_ref().is_some_and(|s| s.is_empty()) {
            self.stream = None;
        }
        if self.cache.as_ref().is_some_and(|s| s.is_empty()) {
            self.cache = None;
        }
        if self.rate_limit.as_ref().is_some_and(|s| s.is_empty()) {
            self.rate_limit = None;
        }
    }

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
