use crate::error::{TuliproxError, TuliproxErrorKind};
use crate::model::EpgConfigDto;
use crate::utils::{default_as_true, get_credentials_from_url_str, get_trimmed_string, sanitize_sensitive_info, trim_last_slash};
use crate::{check_input_credentials, check_input_connections, create_tuliprox_error_result, handle_tuliprox_error_result_list, info_err};
use enum_iterator::Sequence;
use std::collections::{HashMap, HashSet};
use std::fmt::Display;
use std::str::FromStr;

#[macro_export]
macro_rules! apply_batch_aliases {
    ($source:expr, $batch_aliases:expr, $index:expr) => {{
        if $batch_aliases.is_empty() {
            $source.aliases = None;
            None
        } else {
            if let Some(aliases) = $source.aliases.as_mut() {
                let mut names = aliases.iter().map(|a| a.name.clone()).collect::<std::collections::HashSet<String>>();
                names.insert($source.name.clone());

                for alias in $batch_aliases.into_iter() {
                    if !names.contains(&alias.name) {
                        aliases.push(alias)
                    }
                }
            } else {
                $source.aliases = Some($batch_aliases);
            }
            if let Some(index) = $index {
                let mut idx = index;
                // set to the same id as the first alias, because the first alias is copied into this input
                $source.id = index + 1;
                if let Some(aliases) = $source.aliases.as_mut() {
                    for alias in aliases {
                        idx += 1;
                        alias.id = idx;
                    }
                }
                Some(idx)
            } else {
                None
            }
        }
    }};
}

#[derive(Debug, Copy, Clone, serde::Serialize, serde::Deserialize, Sequence,
    PartialEq, Eq, Default)]
pub enum InputType {
    #[serde(rename = "m3u")]
    #[default]
    M3u,
    #[serde(rename = "xtream")]
    Xtream,
    #[serde(rename = "m3u_batch")]
    M3uBatch,
    #[serde(rename = "xtream_batch")]
    XtreamBatch,
}


impl InputType {
    const M3U: &'static str = "m3u";
    const XTREAM: &'static str = "xtream";
    const M3U_BATCH: &'static str = "m3u_batch";
    const XTREAM_BATCH: &'static str = "xtream_batch";
}

impl Display for InputType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", match self {
            Self::M3u => Self::M3U,
            Self::Xtream => Self::XTREAM,
            Self::M3uBatch => Self::M3U_BATCH,
            Self::XtreamBatch => Self::XTREAM_BATCH,
        })
    }
}

impl FromStr for InputType {
    type Err = TuliproxError;

    fn from_str(s: &str) -> Result<Self, TuliproxError> {
        if s.eq(Self::M3U) {
            Ok(Self::M3u)
        } else if s.eq(Self::XTREAM) {
            Ok(Self::Xtream)
        } else if s.eq(Self::M3U_BATCH) {
            Ok(Self::M3uBatch)
        } else if s.eq(Self::XTREAM_BATCH) {
            Ok(Self::XtreamBatch)
        } else {
            create_tuliprox_error_result!(TuliproxErrorKind::Info, "Unknown InputType: {}", s)
        }
    }
}

#[derive(
    Debug,
    Copy,
    Clone,
    serde::Serialize,
    serde::Deserialize,
    Sequence,
    PartialEq,
    Eq,
    Default
)]
pub enum InputFetchMethod {
    #[default]
    GET,
    POST,
}

impl InputFetchMethod {
    const GET_METHOD: &'static str = "GET";
    const POST_METHOD: &'static str = "POST";
}

impl Display for InputFetchMethod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", match self {
            Self::GET => Self::GET_METHOD,
            Self::POST => Self::POST_METHOD,
        })
    }
}

impl FromStr for InputFetchMethod {
    type Err = TuliproxError;

    fn from_str(s: &str) -> Result<Self, TuliproxError> {
        if s.eq(Self::GET_METHOD) {
            Ok(Self::GET)
        } else if s.eq(Self::POST_METHOD) {
            Ok(Self::POST)
        } else {
            create_tuliprox_error_result!(TuliproxErrorKind::Info, "Unknown Fetch Method: {}", s)
        }
    }
}


#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ConfigInputOptionsDto {
    #[serde(default)]
    pub xtream_skip_live: bool,
    #[serde(default)]
    pub xtream_skip_vod: bool,
    #[serde(default)]
    pub xtream_skip_series: bool,
    #[serde(default = "default_as_true")]
    pub xtream_live_stream_use_prefix: bool,
    #[serde(default)]
    pub xtream_live_stream_without_extension: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct StagedInputDto {
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
    #[serde(default)]
    pub method: InputFetchMethod,
    #[serde(default, rename = "type")]
    pub input_type: InputType,
    #[serde(default)]
    pub headers: HashMap<String, String>,
}


#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ConfigInputAliasDto {
    #[serde(default)]
    pub id: u16,
    pub name: String,
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
    #[serde(default)]
    pub priority: i16,
    #[serde(default)]
    pub max_connections: u16,
}

impl ConfigInputAliasDto {
    pub fn prepare(&mut self, index: u16, input_type: &InputType) -> Result<u16, TuliproxError> {
        self.id = index + 1;
        self.name = self.name.trim().to_string();
        if self.name.is_empty() {
            return Err(info_err!("name for input is mandatory".to_string()));
        }
        self.url = self.url.trim().to_string();
        if self.url.is_empty() {
            return Err(info_err!("url for input is mandatory".to_string()));
        }
        check_input_credentials!(self, input_type, true);
        check_input_connections!(self, input_type);

        Ok(self.id)
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ConfigInputDto {
    #[serde(default)]
    pub id: u16,
    #[serde(default)]
    pub name: String,
    #[serde(default, rename = "type")]
    pub input_type: InputType,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub epg: Option<EpgConfigDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub persist: Option<String>,
    #[serde(default = "default_as_true")]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options: Option<ConfigInputOptionsDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aliases: Option<Vec<ConfigInputAliasDto>>,
    #[serde(default)]
    pub priority: i16,
    #[serde(default)]
    pub max_connections: u16,
    #[serde(default)]
    pub method: InputFetchMethod,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub staged: Option<StagedInputDto>,
}

impl ConfigInputDto {
    #[allow(clippy::cast_possible_truncation)]
    pub fn prepare(&mut self, index: u16, _include_computed: bool) -> Result<u16, TuliproxError> {

        let is_batch = matches!(self.input_type, InputType::M3uBatch | InputType::XtreamBatch);
        self.name = self.name.trim().to_owned();
        if self.name.is_empty()  && !is_batch {
            return Err(info_err!("name for input is mandatory".to_owned()));
        }

        check_input_credentials!(self, self.input_type, true);
        check_input_connections!(self, self.input_type);
        if let Some(staged_input) = self.staged.as_mut() {
            check_input_credentials!(staged_input, staged_input.input_type, true);
            if !matches!(staged_input.input_type, InputType::M3u | InputType::Xtream) {
               return Err(info_err!("Staged input can only be of type m3u or xtream".to_owned()));
            }
        }

        self.persist = get_trimmed_string(&self.persist);


        let mut current_index = index;
        if let Some(aliases) = self.aliases.as_mut() {
            let input_type = &self.input_type;
            self.id = current_index + 1; // The same id as the first alias
            handle_tuliprox_error_result_list!(TuliproxErrorKind::Info, aliases.iter_mut()
                .map(|i| match i.prepare(current_index, input_type) {
                    Ok(new_idx) => {
                        current_index = new_idx;
                        Ok(())
                    },
                    Err(err) => Err(err)
                }));
        } else if !matches!(self.input_type, InputType::M3uBatch | InputType::XtreamBatch) {
            current_index += 1;
            self.id = current_index;
        }
        Ok(current_index)
    }

    pub fn prepare_epg(&mut self, include_computed: bool) -> Result<(), TuliproxError> {
        if let Some(epg) = self.epg.as_mut() {
            let create_auto_url = || {
                let get_creds = || {
                    if self.username.is_some() && self.password.is_some() {
                        return (self.username.clone(), self.password.clone(), Some(self.url.clone()));
                    }

                    let (u, p, r) = self.aliases
                        .as_ref()
                        .and_then(|aliases| aliases.first())
                        .map(|alias|  (alias.username.clone(), alias.password.clone(), Some(alias.url.clone())))
                        .unwrap_or((None, None, None));

                    if u.is_some() && p.is_some() && r.is_some() {
                        return (u, p, r);
                    }

                    let (u, p) = get_credentials_from_url_str(&self.url);
                    if u.is_some() && p.is_some() {
                        return (u, p, Some(self.url.clone()));
                    }

                    self.aliases
                        .as_ref()
                        .and_then(|aliases| aliases.first())
                        .map(|alias| {
                            let (u, p) = get_credentials_from_url_str(alias.url.as_str());
                            (u,p, Some(alias.url.clone()))
                        })
                        .unwrap_or((None, None, None))
                };

                let (username, password, base_url) = get_creds();

                if username.is_none() || password.is_none() || base_url.is_none(){
                    Err(format!("auto_epg is enabled for input {}, but no credentials could be extracted", self.name))
                } else if base_url.is_some() {
                    let provider_epg_url = format!("{}/xmltv.php?username={}&password={}",
                                                   trim_last_slash(&base_url.unwrap_or_default()),
                                                   username.unwrap_or_default(),
                                                   password.unwrap_or_default());
                    Ok(provider_epg_url)
                } else {
                    Err(format!("auto_epg is enabled for input {}, but url could not be parsed {}", self.name, sanitize_sensitive_info(&self.url)))
                }
            };

            epg.prepare(create_auto_url, include_computed)?;
            epg.t_sources = {
                let mut seen_urls = HashSet::new();
                epg.t_sources
                    .drain(..)
                    .filter(|src| seen_urls.insert(src.url.clone()))
                    .collect()
            };
        }
        Ok(())
    }

    pub fn prepare_batch(&mut self, batch_aliases: Vec<ConfigInputAliasDto>, index: u16) -> Result<Option<u16>, TuliproxError> {
        let idx = apply_batch_aliases!(self, batch_aliases, Some(index));
        Ok(idx)
    }
}
