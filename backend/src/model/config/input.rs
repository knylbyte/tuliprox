use crate::model::{macros, EpgConfig};
use shared::error::{TuliproxError, TuliproxErrorKind};
use shared::{info_err, write_if_some};
use shared::model::{ConfigInputAliasDto, ConfigInputDto, ConfigInputOptionsDto, InputFetchMethod, InputType};
use shared::utils::get_credentials_from_url_str;
use shared::utils::{get_base_url_from_str, get_credentials_from_url};
use shared::{check_input_credentials};
use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;
use url::Url;
use crate::utils::{get_csv_file_path};

#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone)]
pub struct ConfigInputOptions {
    pub xtream_skip_live: bool,
    pub xtream_skip_vod: bool,
    pub xtream_skip_series: bool,
    pub xtream_live_stream_use_prefix: bool,
    pub xtream_live_stream_without_extension: bool,
}

macros::from_impl!(ConfigInputOptions);
impl From<&ConfigInputOptionsDto> for ConfigInputOptions {
    fn from(dto: &ConfigInputOptionsDto) -> Self {
        Self {
            xtream_skip_live: dto.xtream_skip_live,
            xtream_skip_vod: dto.xtream_skip_vod,
            xtream_skip_series: dto.xtream_skip_series,
            xtream_live_stream_use_prefix: dto.xtream_live_stream_use_prefix,
            xtream_live_stream_without_extension: dto.xtream_live_stream_without_extension,
        }
    }
}

pub struct InputUserInfo {
    pub base_url: String,
    pub username: String,
    pub password: String,
}

impl InputUserInfo {
    pub fn new(input_type: InputType, username: Option<&str>, password: Option<&str>, input_url: &str) -> Option<Self> {
        if input_type == InputType::Xtream {
            if let (Some(username), Some(password)) = (username, password) {
                return Some(Self {
                    base_url: input_url.to_string(),
                    username: username.to_owned(),
                    password: password.to_owned(),
                });
            }
        } else if let Ok(url) = Url::parse(input_url) {
            let base_url = url.origin().ascii_serialization();
            let (username, password) = get_credentials_from_url(&url);
            if username.is_some() || password.is_some() {
                if let (Some(username), Some(password)) = (username.as_ref(), password.as_ref()) {
                    return Some(Self {
                        base_url,
                        username: username.to_owned(),
                        password: password.to_owned(),
                    });
                }
            }
        }
        None
    }
}

#[derive(Debug, Clone)]
pub struct ConfigInputAlias {
    pub id: u16,
    pub name: String,
    pub url: String,
    pub username: Option<String>,
    pub password: Option<String>,
    pub priority: i16,
    pub max_connections: u16,
}

macros::from_impl!(ConfigInputAlias);
impl From<&ConfigInputAliasDto> for ConfigInputAlias {
    fn from(dto: &ConfigInputAliasDto) -> Self {
        Self {
            id: dto.id,
            name: dto.name.to_string(),
            url: get_base_url_from_str(&dto.url).map_or_else(|| dto.url.to_string(), |base_url| base_url),
            username: dto.username.clone(),
            password: dto.password.clone(),
            priority: dto.priority,
            max_connections: dto.max_connections,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ConfigInput {
    pub id: u16,
    pub name: String,
    pub input_type: InputType,
    pub headers: HashMap<String, String>,
    pub url: String,
    pub epg: Option<EpgConfig>,
    pub username: Option<String>,
    pub password: Option<String>,
    pub persist: Option<String>,
    pub enabled: bool,
    pub options: Option<ConfigInputOptions>,
    pub aliases: Option<Vec<ConfigInputAlias>>,
    pub priority: i16,
    pub max_connections: u16,
    pub method: InputFetchMethod,
    pub t_batch_url: Option<String>,
}

impl ConfigInput {
    pub fn prepare(&mut self) -> Result<Option<PathBuf>, TuliproxError> {
        let batch_file_path = self.prepare_batch();
        check_input_credentials!(self, self.input_type, false);
        Ok(batch_file_path)
    }

    pub fn get_user_info(&self) -> Option<InputUserInfo> {
        InputUserInfo::new(self.input_type, self.username.as_deref(), self.password.as_deref(), &self.url)
    }

    pub fn get_matched_config_by_url<'a>(&'a self, url: &str) -> Option<(&'a str, Option<&'a String>, Option<&'a String>)> {
        if url.starts_with(&self.url) {
            return Some((&self.url, self.username.as_ref(), self.password.as_ref()));
        }

        if let Some(aliases) = &self.aliases {
            for alias in aliases {
                if url.starts_with(&alias.url) {
                    return Some((&alias.url, alias.username.as_ref(), alias.password.as_ref()));
                }
            }
        }
        None
    }

    fn prepare_batch(&mut self) -> Option<PathBuf> {
        if matches!(self.input_type, InputType::M3uBatch | InputType::XtreamBatch) {
            let input_type = if self.input_type == InputType::M3uBatch {
                InputType::M3u
            } else {
                InputType::Xtream
            };

            self.t_batch_url= Some(self.url.clone());
            let file_path = get_csv_file_path(self.url.as_str()).ok();

            if let Some(aliases) = self.aliases.as_mut() {
                if !aliases.is_empty() {
                    let mut first = aliases.remove(0);
                    self.id = first.id;
                    self.username = first.username.take();
                    self.password = first.password.take();
                    self.url = first.url.trim().to_string();
                    self.max_connections = first.max_connections;
                    self.priority = first.priority;
                    if self.name.is_empty() {
                        self.name = first.name.to_string();
                    }
                }
            }

            self.input_type = input_type;
            file_path
        } else {
            None
        }
    }

    pub fn as_input(&self, alias: &ConfigInputAlias) -> ConfigInput {
        ConfigInput {
            id: alias.id,
            name: alias.name.clone(),
            input_type: self.input_type,
            headers: self.headers.clone(),
            url: alias.url.to_string(),
            epg: self.epg.clone(),
            username: alias.username.clone(),
            password: alias.password.clone(),
            persist: self.persist.clone(),
            enabled: self.enabled,
            options: self.options.clone(),
            aliases: None,
            priority: alias.priority,
            max_connections: alias.max_connections,
            method: self.method,
            t_batch_url: None,
        }
    }
}

macros::from_impl!(ConfigInput);
impl From<&ConfigInputDto> for ConfigInput {
    fn from(dto: &ConfigInputDto) -> Self {
        Self {
            id: dto.id,
            name: dto.name.to_string(),
            input_type: dto.input_type,
            headers: dto.headers.clone(),
            url: dto.url.clone(), //get_base_url_from_str(&dto.url).map_or_else(|| dto.url.to_string(), |base_url| base_url),
            epg: dto.epg.as_ref().map(std::convert::Into::into),
            username: dto.username.clone(),
            password: dto.password.clone(),
            persist: dto.persist.clone(),
            enabled: dto.enabled,
            options: dto.options.as_ref().map(std::convert::Into::into),
            aliases: dto.aliases.as_ref().map(|list| list.iter().map(std::convert::Into::into).collect()),
            priority: dto.priority,
            max_connections: dto.max_connections,
            method: dto.method,
            t_batch_url: None,
        }
    }
}

impl fmt::Display for ConfigInput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ConfigInput: {{")?;
        write!(f, "  id: {}", self.id)?;
        write!(f, ", name: {}", self.name)?;
        write!(f, ", input_type: {:?}", self.input_type)?;
        write!(f, ", url: {}", self.url)?;
        write!(f, ", enabled: {}", self.enabled)?;
        write!(f, ", priority: {}", self.priority)?;
        write!(f, ", max_connections: {}", self.max_connections)?;
        write!(f, ", method: {:?}", self.method)?;

        // headers, epg etc. wie gehabt…

        write_if_some!(f, self,
            ", username: " => username,
            ", password: " => password,
            ", persist: " => persist
        );
        write!(f, " }}")?;

        Ok(())
    }
}
