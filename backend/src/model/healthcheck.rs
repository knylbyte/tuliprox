use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Healthcheck {
    pub status: String,
    pub version: String,
    #[serde(default)]
    pub build_time: Option<String>,
    pub server_time: String,
}
