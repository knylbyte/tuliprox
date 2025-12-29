use crate::error::{TuliproxError};
use crate::foundation::filter::{get_filter, Filter};
use crate::model::{PatternTemplate};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ConfigFavouritesDto {
    pub group: String,
    #[serde(default)]
    pub filter: String,
    #[serde(default)]
    pub match_as_ascii: bool,
    #[serde(skip)]
    pub t_filter: Option<Filter>,
}

impl ConfigFavouritesDto {
    pub fn prepare(&mut self, templates: Option<&Vec<PatternTemplate>>) -> Result<(), TuliproxError> {
        self.t_filter = Some(get_filter(&self.filter, templates)?);
        Ok(())
    }
}