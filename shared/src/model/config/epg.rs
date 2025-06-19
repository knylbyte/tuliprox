
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EpgSourceDto {
    pub url: String,
    #[serde(default)]
    pub priority: i16,
    #[serde(default)]
    pub logo_override: bool,
}


#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum EpgNamePrefix {
    #[default]
    Ignore,
    Suffix(String),
    Prefix(String),
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EpgSmartMatchConfigDto {
    #[serde(default)]
    pub enabled: bool,
    pub normalize_regex: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strip: Option<Vec<String>>,
    #[serde(default)]
    pub name_prefix: EpgNamePrefix,
    #[serde(default)]
    pub name_prefix_separator: Option<Vec<char>>,
    #[serde(default)]
    pub fuzzy_matching: bool,
    #[serde(default)]
    pub match_threshold: u16,
    #[serde(default)]
    pub best_match_threshold: u16,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EpgConfigDto {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sources: Option<Vec<EpgSourceDto>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub smart_match: Option<EpgSmartMatchConfigDto>,
}
