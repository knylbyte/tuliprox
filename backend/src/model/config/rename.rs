use shared::model::{ConfigRenameDto, ItemField};
use crate::model::macros;

#[derive(Debug, Clone)]
pub struct ConfigRename {
    pub field: ItemField,
    pub new_name: String,
    pub pattern: regex::Regex,
}

macros::from_impl!(ConfigRename);
impl From<&ConfigRenameDto> for ConfigRename {
    fn from(dto: &ConfigRenameDto) -> Self {
        Self {
            field: dto.field,
            new_name: dto.new_name.clone(),
            pattern: regex::Regex::new(&dto.pattern).unwrap()
        }
    }
}

impl From<&ConfigRename> for ConfigRenameDto {
    fn from(instance: &ConfigRename) -> Self {
        Self {
            field: instance.field,
            new_name: instance.new_name.clone(),
            pattern: instance.pattern.to_string()
        }
    }
}