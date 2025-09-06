use crate::error::{TuliproxError, TuliproxErrorKind};
use crate::model::WebAuthConfigDto;
use crate::utils::{default_as_true, is_blank_optional_string};

const RESERVED_PATHS: &[&str] = &[
    "live",
    "movie",
    "series",
    "m3u-stream",
    "healthcheck",
    "status",
    "player_api.php",
    "panel_api.php",
    "xtream",
    "timeshift",
    "timeshift.php",
    "streaming",
    "get.php",
    "apiget",
    "m3u",
    "resource",
];

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default, PartialEq)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct ContentSecurityPolicyConfigDto {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_attributes: Option<Vec<String>>,
}

impl ContentSecurityPolicyConfigDto {
    pub fn is_empty(&self) -> bool {
        !self.enabled
            && (self.custom_attributes.is_none()
                || self
                    .custom_attributes
                    .as_ref()
                    .is_some_and(|v| v.is_empty()))
    }

    pub fn validate(&self) -> Result<(), TuliproxError> {
        if let Some(attrs) = self.custom_attributes.as_ref() {
            for (i, attr) in attrs.iter().enumerate() {
                // Prohibit CR/LF/NUL (header injection)
                if attr.contains('\r') || attr.contains('\n') || attr.contains('\0') {
                    return Err(TuliproxError::new(
                        TuliproxErrorKind::Info,
                        format!("custom-attributes[{i}] contains forbidden control characters"),
                    ));
                }
                //Optional: prohibit additional CTLs (except HTAB)
                if attr.chars().any(|c| {
                    let u = c as u32;
                    (u < 0x20 && c != '\t') || u == 0x7F
                }) {
                    return Err(TuliproxError::new(
                        TuliproxErrorKind::Info,
                        format!("custom-attributes[{i}] contains control characters"),
                    ));
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WebUiConfigDto {
    #[serde(default = "default_as_true")]
    pub enabled: bool,
    #[serde(default = "default_as_true")]
    pub user_ui_enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_security_policy: Option<ContentSecurityPolicyConfigDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth: Option<WebAuthConfigDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub player_server: Option<String>,
}

impl Default for WebUiConfigDto {
    fn default() -> Self {
        WebUiConfigDto {
            enabled: default_as_true(),
            user_ui_enabled: default_as_true(),
            content_security_policy: None,
            path: None,
            auth: None,
            player_server: None,
        }
    }
}

impl WebUiConfigDto {
    pub fn is_empty(&self) -> bool {
        let empty = WebUiConfigDto::default();
        self.enabled == empty.enabled
            && self.user_ui_enabled == empty.user_ui_enabled
            && is_blank_optional_string(&self.path)
            && is_blank_optional_string(&self.player_server)
            && (self.content_security_policy.is_none()
                || self
                    .content_security_policy
                    .as_ref()
                    .is_some_and(|c| c.is_empty()))
            && (self.auth.is_none() || self.auth.as_ref().is_some_and(|c| c.is_empty()))
    }

    pub fn clean(&mut self) {
        if self
            .content_security_policy
            .as_ref()
            .is_some_and(|c| c.is_empty())
        {
            self.content_security_policy = None;
        }
        if self.auth.as_ref().is_some_and(|c| c.is_empty()) {
            self.auth = None;
        }

        if is_blank_optional_string(&self.path) {
            self.path = None;
        }
        if is_blank_optional_string(&self.player_server) {
            self.player_server = None;
        }
    }

    pub fn prepare(&mut self) -> Result<(), TuliproxError> {
        if !self.enabled {
            self.auth = None;
        }

        if let Some(web_ui_path) = self.path.as_ref() {
            let web_path = web_ui_path.trim();
            if web_path.is_empty() {
                self.path = None;
            } else {
                let web_path = web_path
                    .trim()
                    .trim_start_matches('/')
                    .trim_end_matches('/')
                    .to_string();
                if RESERVED_PATHS.contains(&web_path.to_lowercase().as_str()) {
                    return Err(TuliproxError::new(
                        TuliproxErrorKind::Info,
                        format!("web ui path is a reserved path. Do not use {RESERVED_PATHS:?}"),
                    ));
                }
                self.path = Some(web_path);
            }
        }
        if let Some(csp) = &self.content_security_policy {
            csp.validate()?;
        }
        Ok(())
    }
}
