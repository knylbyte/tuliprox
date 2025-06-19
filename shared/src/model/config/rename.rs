use crate::model::ItemField;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigRenameDto {
    pub field: ItemField,
    pub pattern: String,
    pub new_name: String,
}