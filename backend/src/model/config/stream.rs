use shared::utils::default_grace_period_millis;
use shared::utils::default_grace_period_timeout_secs;
use shared::error::{TuliproxError, TuliproxErrorKind};
use shared::info_err;
use shared::utils::parse_to_kbps;
use crate::api::model::streams::transport_stream_buffer::TransportStreamBuffer;

const STREAM_QUEUE_SIZE: usize = 1024; // mpsc channel holding messages. with 8192byte chunks and 2Mbit/s approx 8MB

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct StreamBufferConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub size: usize,
}

impl StreamBufferConfig {
    fn prepare(&mut self) {
        if self.enabled && self.size == 0 {
            self.size = STREAM_QUEUE_SIZE;
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct StreamConfig {
    #[serde(default)]
    pub retry: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub buffer: Option<StreamBufferConfig>,
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

impl StreamConfig {
    pub(crate) fn prepare(&mut self) -> Result<(), TuliproxError> {
        if let Some(buffer) = self.buffer.as_mut() {
            buffer.prepare();
        }
        if let Some(throttle) = &self.throttle {
            self.throttle_kbps = parse_to_kbps(throttle).map_err(|err| TuliproxError::new(TuliproxErrorKind::Info, err))?;
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


#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct CustomStreamResponse {
    #[serde(default, skip)]
    pub channel_unavailable: Option<TransportStreamBuffer>,
    #[serde(default, skip)]
    pub user_connections_exhausted: Option<TransportStreamBuffer>, // user has no more connections
    #[serde(default, skip)]
    pub provider_connections_exhausted: Option<TransportStreamBuffer>, // provider limit reached, has no more connections
    #[serde(default, skip)]
    pub user_account_expired: Option<TransportStreamBuffer>,
}