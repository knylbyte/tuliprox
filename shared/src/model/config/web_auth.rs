use crate::utils::{default_as_true};
/// 30 minutes by default; `0` still means “no expiration.”
fn default_token_ttl_mins() -> u32 {
    30
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WebAuthConfigDto {
    #[serde(default = "default_as_true")]
    pub enabled: bool,
    pub issuer: String,
    pub secret: String,
    #[serde(default = "default_token_ttl_mins")]
    pub token_ttl_mins: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub userfile: Option<String>,
}

impl Default for WebAuthConfigDto {
    fn default() -> Self {
        Self {
            enabled: default_as_true(),
            issuer: String::new(),
            secret: String::new(),
            token_ttl_mins: default_token_ttl_mins(),
            userfile: None,
        }
    }
}