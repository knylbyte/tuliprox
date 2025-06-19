use crate::model::{ConfigInputDto, ItemField};
use crate::model::config::target::ConfigTargetDto;


#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum TemplateValue {
    Single(String),
    Multi(Vec<String>),
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PatternTemplateDto {
    pub name: String,
    pub value: TemplateValue,
    #[serde(skip)]
    pub placeholder: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigRenameDto {
    pub field: ItemField,
    pub pattern: String,
    pub new_name: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigSourceDto {
    pub inputs: Vec<ConfigInputDto>,
    pub targets: Vec<ConfigTargetDto>,
}

#[derive(Default, Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourcesConfigDto {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub templates: Option<Vec<PatternTemplateDto>>,
    pub sources: Vec<ConfigSourceDto>,
}