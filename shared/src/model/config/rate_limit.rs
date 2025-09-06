use crate::error::{TuliproxError, TuliproxErrorKind};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RateLimitConfigDto {
    pub enabled: bool,
    pub period_millis: u64,
    pub burst_size: u32,
}

impl RateLimitConfigDto {
    pub fn is_empty(&self) -> bool {
        !self.enabled && self.period_millis == 0 && self.burst_size == 0
    }

    pub(crate) fn prepare(&self) -> Result<(), TuliproxError> {
        if self.period_millis == 0 {
            return Err(TuliproxError::new(TuliproxErrorKind::Info, "Rate limiter period can't be 0".to_string()));
        }
        if self.burst_size == 0 {
            return Err(TuliproxError::new(TuliproxErrorKind::Info, "Rate limiter burst can't be 0".to_string()));
        }
        Ok(())
    }
}