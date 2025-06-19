use crate::model::{ProxyUserCredentialsDto};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TargetUserDto {
    pub target: String,
    pub credentials: Vec<ProxyUserCredentialsDto>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApiProxyServerInfoDto {
    pub name: String,
    pub protocol: String,
    pub host: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<String>,
    pub timezone: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApiProxyConfigDto {
    pub server: Vec<ApiProxyServerInfoDto>,
    pub user: Vec<TargetUserDto>,
    #[serde(default)]
    pub use_user_db: bool,
}
