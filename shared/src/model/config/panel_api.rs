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
    pub panel_info: Vec<PanelApiQueryParamDto>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub new_client: Vec<PanelApiQueryParamDto>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub renew_client: Vec<PanelApiQueryParamDto>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PanelApiConfigDto {
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    #[serde(default)]
    pub query_parameter: PanelApiQueryParametersDto,
}

