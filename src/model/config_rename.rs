use crate::foundation::filter::{apply_templates_to_pattern, PatternTemplate};
use crate::tuliprox_error::{TuliProxError, TuliProxErrorKind, create_tuliprox_error_result};
use crate::model::ItemField;

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
    pub fn prepare(&mut self, templates: Option<&Vec<PatternTemplate>>) -> Result<(), TuliProxError> {
        if let Some(templ) = templates {
            self.pattern = apply_templates_to_pattern(&self.pattern, templ);
        }
        match regex::Regex::new(&self.pattern) {
            Ok(pattern) => {
                self.re = Some(pattern);
                Ok(())
            }
            Err(err) => create_tuliprox_error_result!(TuliProxErrorKind::Info, "cant parse regex: {} {err}", &self.pattern),
        }
    }
}