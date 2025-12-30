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
    pub client_info: Vec<PanelApiQueryParamDto>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub client_new: Vec<PanelApiQueryParamDto>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub client_renew: Vec<PanelApiQueryParamDto>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub account_info: Vec<PanelApiQueryParamDto>,
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
}
