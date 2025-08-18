use shared::error::TuliproxError;
use shared::model::{ContentSecurityPoliciesConfigDto, WebUiConfigDto};
use crate::model::{macros, WebAuthConfig};

#[derive(Debug, Clone)]
pub struct ContentSecurityPoliciesConfig {
    pub enabled: bool,
    pub custom_attributes: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct WebUiConfig {
    pub enabled: bool,
    pub user_ui_enabled: bool,
    pub content_security_policies: Option<ContentSecurityPoliciesConfig>,
    pub path: Option<String>,
    pub auth: Option<WebAuthConfig>,
    pub player_server: Option<String>,
}

impl WebUiConfig {
    pub fn prepare(&mut self, config_path: &str) -> Result<(), TuliproxError> {
        if let Some(web_auth) = &mut self.auth {
            if web_auth.enabled {
                web_auth.prepare(config_path)?;
            } else {
                self.auth = None;
            }
        }
        Ok(())
    }
}

macros::from_impl!(ContentSecurityPoliciesConfig);
impl From<&ContentSecurityPoliciesConfigDto> for ContentSecurityPoliciesConfig {
    fn from(dto: &ContentSecurityPoliciesConfigDto) -> Self {
        Self { enabled: dto.enabled, custom_attributes: dto.custom_attributes.clone() }
    }
}
impl From<&ContentSecurityPoliciesConfig> for ContentSecurityPoliciesConfigDto {
    fn from(instance: &ContentSecurityPoliciesConfig) -> Self {
        Self { enabled: instance.enabled, custom_attributes: instance.custom_attributes.clone() }
    }
}

macros::from_impl!(WebUiConfig);
impl From<&WebUiConfigDto> for WebUiConfig {
    fn from(dto: &WebUiConfigDto) -> Self {
        Self {
            enabled: dto.enabled,
            user_ui_enabled: dto.user_ui_enabled,
            content_security_policies: dto.content_security_policies.as_ref().map(Into::into),
            path: dto.path.clone(),
            auth: dto.auth.as_ref().map(Into::into),
            player_server: dto.player_server.clone(),
        }
    }
}
impl From<&WebUiConfig> for WebUiConfigDto {
    fn from(instance: &WebUiConfig) -> Self {
        Self {
            enabled: instance.enabled,
            user_ui_enabled: instance.user_ui_enabled,
            content_security_policies: instance.content_security_policies.as_ref().map(Into::into),
            path: instance.path.clone(),
            auth: instance.auth.as_ref().map(Into::into),
            player_server: instance.player_server.clone(),
        }
    }
}

