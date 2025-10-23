use regex::Regex;
use shared::foundation::filter::{CompiledRegex, Filter};
use shared::model::{ConfigFavouritesDto, ItemField};
use crate::model::macros;

#[derive(Debug, Clone)]
pub struct ConfigFavourites {
    pub group: String,
    pub filter: Filter,
}

macros::from_impl!(ConfigFavourites);
impl From<&ConfigFavouritesDto> for ConfigFavourites {
    fn from(dto: &ConfigFavouritesDto) -> Self {
        Self {
            group: dto.group.clone(),
            filter: dto.t_filter.as_ref().map_or_else(|| Filter::FieldComparison(ItemField::Group, CompiledRegex { restr: String::new(), re: Regex::new(".*").unwrap() }), Clone::clone),
        }
    }
}

impl From<&ConfigFavourites> for ConfigFavouritesDto {
    fn from(instance: &ConfigFavourites) -> Self {
        Self {
            group: instance.group.clone(),
            filter: instance.filter.to_string(),
            t_filter: Some(instance.filter.clone()),
        }
    }
}