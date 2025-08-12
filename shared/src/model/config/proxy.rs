use crate::error::{TuliproxError, TuliproxErrorKind};

fn default_interval() -> u64 { 5 }
fn default_weight() -> u8 { 10 }

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ProxyServerConfigDto {
    pub url: String,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub password: Option<String>,
    #[serde(default = "default_weight")]
    pub weight: u8,
}

impl ProxyServerConfigDto {
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
        if !(1..=10).contains(&self.weight) {
            return Err(TuliproxError::new(TuliproxErrorKind::Info, "Proxy weight must be between 1 and 10".to_string()));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ProxyPoolConfigDto {
    #[serde(default = "default_interval")]
    pub interval_secs: u64,
    #[serde(default)]
    pub proxies: Vec<ProxyServerConfigDto>,
}

impl ProxyPoolConfigDto {
    pub fn prepare(&mut self) -> Result<(), TuliproxError> {
        if self.interval_secs == 0 {
            return Err(TuliproxError::new(TuliproxErrorKind::Info, "Proxy healthcheck interval must be > 0".to_string()));
        }
        if self.proxies.is_empty() {
            return Err(TuliproxError::new(TuliproxErrorKind::Info, "At least one proxy must be configured".to_string()));
        }
        for proxy in &mut self.proxies {
            proxy.prepare()?;
        }
        Ok(())
    }
}

