use crate::error::{TuliproxError, TuliproxErrorKind};
use crate::info_err;
use crate::utils::{default_grace_period_millis, default_grace_period_timeout_secs, parse_to_kbps};

const STREAM_QUEUE_SIZE: usize = 1024; // mpsc channel holding messages. with 8192byte chunks and 2Mbit/s approx 8MB

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct StreamBufferConfigDto {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub size: usize,
}


impl StreamBufferConfigDto {
    pub fn is_empty(&self) -> bool {
        !self.enabled && self.size == 0
    }
    fn prepare(&mut self) {
        if self.enabled && self.size == 0 {
            self.size = STREAM_QUEUE_SIZE;
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
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

impl Default for StreamConfigDto {
    fn default() -> Self {
        StreamConfigDto {
            retry: false,
            buffer: None,
            throttle: None,
            grace_period_millis: default_grace_period_millis(),
            grace_period_timeout_secs: default_grace_period_timeout_secs(),
            forced_retry_interval_secs: 0,
            throttle_kbps: 0,
        }
    }
}

impl StreamConfigDto {
    pub fn is_empty(&self) -> bool {
        let empty = Self::default();
        self.retry == empty.retry
        && (self.buffer.is_none() || self.buffer.as_ref().is_some_and(|b| b.is_empty()))
        && (self.throttle.is_none() || self.throttle.as_ref().is_some_and(|t| t.is_empty()))
        && self.grace_period_millis == empty.grace_period_millis
        && self.grace_period_timeout_secs == empty.grace_period_timeout_secs
        && self.forced_retry_interval_secs == empty.forced_retry_interval_secs
        && self.throttle_kbps == empty.throttle_kbps
    }


    pub(crate) fn prepare(&mut self) -> Result<(), TuliproxError> {
        if let Some(buffer) = self.buffer.as_mut() {
            buffer.prepare();
        }
        if let Some(throttle) = &self.throttle {
            parse_to_kbps(throttle).map_err(|err| TuliproxError::new(TuliproxErrorKind::Info, err))?;
        } else {
            self.throttle_kbps = 0;
        }

        if self.grace_period_millis > 0 {
            if self.grace_period_timeout_secs == 0 {
                let triple_ms = self.grace_period_millis.saturating_mul(3);
                self.grace_period_timeout_secs = std::cmp::max(1, triple_ms.div_ceil(1000));
            } else if self.grace_period_millis / 1000 > self.grace_period_timeout_secs {
                return Err(info_err!(format!("Grace time period timeout {} sec should be more than grace time period {} ms", self.grace_period_timeout_secs, self.grace_period_millis)));
            }
        }

        Ok(())
    }
}
