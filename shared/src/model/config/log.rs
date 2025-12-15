use crate::utils::{default_as_true, is_blank_optional_string};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct LogConfigDto {
    #[serde(default = "default_as_true")]
    pub sanitize_sensitive_info: bool,
    #[serde(default)]
    pub log_active_user: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log_level: Option<String>,
}

impl Default for LogConfigDto {
    fn default() -> Self {
        LogConfigDto {
            sanitize_sensitive_info: default_as_true(),
            log_active_user: false,
            log_level: None,
        }
    }
}

impl LogConfigDto {
    pub fn is_empty(&self) -> bool {
        self.sanitize_sensitive_info && !self.log_active_user && is_blank_optional_string(&self.log_level)
    }

    pub fn clean(&mut self) {
        if is_blank_optional_string(&self.log_level) {
            self.log_level = None;
        }
    }
}