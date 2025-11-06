use crate::error::{TuliproxError};
use crate::foundation::filter::{get_filter, Filter};
use crate::model::{PatternTemplate};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ConfigFavouritesDto {
    pub group: String,
    #[serde(default)]
    pub filter: String,
    #[serde(skip)]
    pub t_filter: Option<Filter>,
}

impl ConfigFavouritesDto {
    pub fn prepare(&mut self, templates: Option<&Vec<PatternTemplate>>) -> Result<(), TuliproxError> {
        match get_filter(&self.filter, templates) {
            Ok(filter) => self.t_filter = Some(filter),
            Err(err) => return Err(err),
        }
        Ok(())
    }
}