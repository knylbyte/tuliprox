use serde::{Deserialize, Serialize};

fn default_fuzzy_threshold() -> u8 {
    80
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TraktContentType {
    Vod,
    Series,
    Both,
}

impl Default for TraktContentType {
    fn default() -> Self {
        Self::Both
    }
}

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TraktApiConfigDto {
    #[serde(default)]
    pub(crate) key: String,
    #[serde(default)]
    pub(crate) version: String,
    #[serde(default)]
    pub(crate) url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TraktListConfigDto {
    pub user: String,
    pub list_slug: String,
    pub category_name: String,
    pub content_type: TraktContentType,
    #[serde(default = "default_fuzzy_threshold")]
    pub fuzzy_match_threshold: u8, // Percentage (0-100)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TraktConfigDto {
    #[serde(default)]
    pub api: TraktApiConfigDto,
    pub lists: Vec<TraktListConfigDto>,
}
