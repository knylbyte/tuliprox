use shared::utils::is_blank_optional_string;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Healthcheck {
    pub status: String,
    pub version: String,
    #[serde(default, skip_serializing_if = "is_blank_optional_string")]
    pub build_time: Option<String>,
    pub server_time: String,
}
