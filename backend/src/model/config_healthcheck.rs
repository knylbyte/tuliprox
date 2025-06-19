use crate::model::ConfigApi;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct HealthcheckConfig {
    pub api: ConfigApi,
}
