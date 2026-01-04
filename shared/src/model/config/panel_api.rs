use crate::utils::deserialize_as_option_string;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PanelApiQueryParamDto {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PanelApiQueryParametersDto {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub account_info: Vec<PanelApiQueryParamDto>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub client_info: Vec<PanelApiQueryParamDto>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub client_new: Vec<PanelApiQueryParamDto>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub client_renew: Vec<PanelApiQueryParamDto>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub client_adult_content: Vec<PanelApiQueryParamDto>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PanelApiAliasPoolAuto {
    Auto,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(untagged)]
pub enum PanelApiAliasPoolSizeValue {
    Auto(PanelApiAliasPoolAuto),
    Number(u16),
}

impl PanelApiAliasPoolSizeValue {
    pub fn as_number(&self) -> Option<u16> {
        match self {
            Self::Number(value) => Some(*value),
            Self::Auto(_) => None,
        }
    }

    pub fn is_auto(&self) -> bool {
        matches!(self, Self::Auto(_))
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PanelApiAliasPoolSizeDto {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<PanelApiAliasPoolSizeValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<PanelApiAliasPoolSizeValue>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PanelApiAliasPoolDto {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<PanelApiAliasPoolSizeDto>,
    #[serde(default)]
    pub remove_expired: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PanelApiConfigDto {
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    #[serde(default)]
    pub query_parameter: PanelApiQueryParametersDto,
    #[serde(
        default,
        deserialize_with = "deserialize_as_option_string",
        skip_serializing_if = "Option::is_none"
    )]
    pub credits: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alias_pool: Option<PanelApiAliasPoolDto>,
}
