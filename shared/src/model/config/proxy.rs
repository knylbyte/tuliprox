use crate::error::{TuliproxError, TuliproxErrorKind};
use crate::utils::is_blank_optional_string;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ProxyConfigDto {
    pub url: String,
    pub username: Option<String>,
    pub password: Option<String>,
}

impl ProxyConfigDto {
    pub fn is_empty(&self) -> bool {
        is_blank_optional_string(&self.username)
        && is_blank_optional_string(&self.password)
        && self.url.trim().is_empty()
    }

    pub fn clean(&mut self) {
        if is_blank_optional_string(&self.username) {
            self.username = None;
        }
        if is_blank_optional_string(&self.password) {
            self.password = None;
        }
    }

    pub fn prepare(&mut self) -> Result<(), TuliproxError> {
        if self.username.is_some() || self.password.is_some() {
            if let (Some(username), Some(password)) = (self.username.as_ref(), self.password.as_ref()) {
                let uname = username.trim();
                let pwd = password.trim();
                if uname.is_empty() || pwd.is_empty() {
                    return Err(TuliproxError::new(TuliproxErrorKind::Info, "Proxy credentials missing".to_string()));
                }
                self.username = Some(uname.to_string());
                self.password = Some(pwd.to_string());
            } else {
                return Err(TuliproxError::new(TuliproxErrorKind::Info, "Proxy credentials missing".to_string()));
            }
        }

        self.url = self.url.trim().to_string();
        if self.url.is_empty() {
            return Err(TuliproxError::new(TuliproxErrorKind::Info, "Proxy url missing".to_string()));
        }
        Ok(())
    }
}