use shared::model::ItemField;
use crate::foundation::filter::{apply_templates_to_pattern_single, PatternTemplate};
use shared::error::{TuliproxError, TuliproxErrorKind, create_tuliprox_error_result};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigRename {
    pub field: ItemField,
    pub pattern: String,
    pub new_name: String,
    #[serde(skip_serializing, skip_deserializing)]
    pub re: Option<regex::Regex>,
}

impl ConfigRename {
    pub fn prepare(&mut self, templates: Option<&Vec<PatternTemplate>>) -> Result<(), TuliproxError> {
       self.pattern = apply_templates_to_pattern_single(&self.pattern, templates)?;
        match regex::Regex::new(&self.pattern) {
            Ok(pattern) => {
                self.re = Some(pattern);
                Ok(())
            }
            Err(err) => create_tuliprox_error_result!(TuliproxErrorKind::Info, "cant parse regex: {} {err}", &self.pattern),
        }
    }
}