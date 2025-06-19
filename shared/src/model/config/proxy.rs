
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct ProxyConfigDto {
    pub url: String,
    pub username: Option<String>,
    pub password: Option<String>,
}
