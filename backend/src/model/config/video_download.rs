use shared::error::{create_tuliprox_error_result, TuliproxError, TuliproxErrorKind};
use regex::Regex;
use std::borrow::BorrowMut;
use std::collections::HashMap;
use crate::utils::request::DEFAULT_USER_AGENT;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct VideoDownloadConfig {
    #[serde(default)]
    pub headers: HashMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub directory: Option<String>,
    #[serde(default)]
    pub organize_into_directories: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub episode_pattern: Option<String>,
    #[serde(default, skip_serializing, skip_deserializing)]
    pub t_re_episode_pattern: Option<Regex>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct VideoConfig {
    #[serde(default)]
    pub extensions: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub download: Option<VideoDownloadConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub web_search: Option<String>,
}

impl VideoConfig {
    /// # Panics
    ///
    /// Will panic if default `RegEx` gets invalid
    pub fn prepare(&mut self) -> Result<(), TuliproxError> {
        if self.extensions.is_empty() {
            self.extensions = ["mkv", "avi", "mp4", "mpeg", "divx", "mov"]
                .iter()
                .map(|&arg| arg.to_string())
                .collect();
        }
        match &mut self.download {
            None => {}
            Some(downl) => {
                if downl.headers.is_empty() {
                    downl.headers.borrow_mut().insert("Accept".to_string(), "video/*".to_string());
                    downl.headers.borrow_mut().insert("User-Agent".to_string(), DEFAULT_USER_AGENT.to_string());
                }

                if let Some(episode_pattern) = &downl.episode_pattern {
                    if !episode_pattern.is_empty() {
                        match regex::Regex::new(episode_pattern) {
                            Ok(pattern) => {
                                downl.t_re_episode_pattern = Some(pattern);
                            }
                            Err(err) => {
                                return create_tuliprox_error_result!(TuliproxErrorKind::Info, "cant parse regex: {episode_pattern} {err}");
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }
}