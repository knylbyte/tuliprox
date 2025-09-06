use crate::utils::default_as_true;
use crate::error::{TuliproxError, TuliproxErrorKind};
use crate::model::{ProxyType, ProxyUserStatus};

#[derive(Debug, Copy, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub enum UserConnectionPermission {
    Exhausted,
    Allowed,
    GracePeriod,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Default)]
#[serde(deny_unknown_fields)]
pub struct ProxyUserCredentialsDto {
    pub username: String,
    pub password: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    #[serde(default = "ProxyType::default")]
    pub proxy: ProxyType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub epg_timeshift: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exp_date: Option<i64>,
    #[serde(default)]
    pub max_connections: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<ProxyUserStatus>,
    #[serde(default = "default_as_true")]
    pub ui_enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
}

impl ProxyUserCredentialsDto {
    pub fn prepare(&mut self) {
        self.trim();
    }

    fn trim(&mut self) {
        self.username = self.username.trim().to_string();
        self.password = self.password.trim().to_string();
        match &self.token {
            None => {}
            Some(tkn) => {
                self.token = Some(tkn.trim().to_string());
            }
        }
    }

    pub fn validate(&self) -> Result<(), TuliproxError> {
        if self.username.is_empty() {
            return Err(TuliproxError::new(TuliproxErrorKind::Info, "Username required".to_string()));
        }
        if self.password.is_empty() {
            return Err(TuliproxError::new(TuliproxErrorKind::Info, "Password required".to_string()));
        }
        Ok(())
    }

    pub fn is_active(&self) -> bool {
        if let Some(status) = &self.status {
            if matches!(status, ProxyUserStatus::Expired
            | ProxyUserStatus::Banned
            | ProxyUserStatus::Disabled
            | ProxyUserStatus::Pending) {
                return false;
            }
        }
        if let Some(exp_date) = self.exp_date {
            let now =  chrono::Local::now();
            if (exp_date - now.timestamp()) < 0 {
                return false;
            }
        }
        true
    }
}


