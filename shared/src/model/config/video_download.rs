use std::collections::HashMap;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct VideoDownloadConfigDto {
    #[serde(default)]
    pub headers: HashMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub directory: Option<String>,
    #[serde(default)]
    pub organize_into_directories: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub episode_pattern: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct VideoConfigDto {
    #[serde(default)]
    pub extensions: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub download: Option<VideoDownloadConfigDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub web_search: Option<String>,
}
