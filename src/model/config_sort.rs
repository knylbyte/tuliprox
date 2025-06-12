use regex::Regex;
use crate::foundation::filter::{apply_templates_to_pattern, apply_templates_to_pattern_single, PatternTemplate, TemplateValue};
use crate::tuliprox_error::{TuliproxError, TuliproxErrorKind, create_tuliprox_error, handle_tuliprox_error_result_list};
use crate::model::{ItemField};

#[derive(Debug, Copy, Clone, serde::Serialize, serde::Deserialize)]
pub enum SortOrder {
    #[serde(rename = "asc")]
    Asc,
    #[serde(rename = "desc")]
    Desc,
}

fn compile_regex_vec(patterns: Option<&Vec<String>>) -> Result<Option<Vec<Regex>>, TuliproxError> {
    patterns.as_ref()
        .map(|seq| {
            seq.iter()
                .map(|s| Regex::new(s).map_err(|err| {
                    create_tuliprox_error!(TuliproxErrorKind::Info, "cant parse regex: {s} {err}")
                }))
                .collect::<Result<Vec<_>, _>>()
        })
        .transpose() // convert Option<Result<...>> to Result<Option<...>>
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigSortGroup {
    pub order: SortOrder,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sequence: Option<Vec<String>>,
    #[serde(default, skip)]
    pub t_re_sequence: Option<Vec<Regex>>,
}


impl ConfigSortGroup {

    pub fn prepare(&mut self, templates: Option<&Vec<PatternTemplate>>) -> Result<(), TuliproxError> {
        let processed_sequence = match (&self.sequence, templates) {
            (Some(seqs), Some(tmpls)) => {
                let mut result = Vec::new();
                for s in seqs {
                    match apply_templates_to_pattern(s, Some(tmpls), true)? {
                        TemplateValue::Single(val) => result.push(val),
                        TemplateValue::Multi(vals) => result.extend(vals),
                    }
                }
                Some(result)
            },
            (Some(seqs), None) => Some(seqs.clone()),
            (None, _) => None,
        };

        self.t_re_sequence = compile_regex_vec(processed_sequence.as_ref())?;
        Ok(())
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigSortChannel {
    // channel field
    pub field: ItemField,
    // match against group title
    pub group_pattern: String,
    pub order: SortOrder,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sequence: Option<Vec<String>>,
    #[serde(default, skip)]
    pub t_re_sequence: Option<Vec<Regex>>,
    #[serde(skip)]
    pub t_re_group_pattern: Option<Regex>,
}

impl ConfigSortChannel {
    pub fn prepare(&mut self, templates: Option<&Vec<PatternTemplate>>) -> Result<(), TuliproxError> {
        self.group_pattern = apply_templates_to_pattern_single(&self.group_pattern, templates)?;
        // Compile group_pattern
        self.t_re_group_pattern = Some(
            Regex::new(&self.group_pattern).map_err(|err| {
                create_tuliprox_error!(TuliproxErrorKind::Info, "cant parse regex: {} {err}", &self.group_pattern)
            })?
        );

        // Transform sequence with templates if provided, otherwise use raw sequence
        let processed_sequence = match (&self.sequence, templates) {
            (Some(seqs), Some(tmpls)) => {
                let mut result = Vec::new();
                for s in seqs {
                    match apply_templates_to_pattern(s, Some(tmpls), true)? {
                        TemplateValue::Single(val) => result.push(val),
                        TemplateValue::Multi(vals) => result.extend(vals),
                    }
                }
                Some(result)
            },
            (Some(seqs), None) => Some(seqs.clone()),
            (None, _) => None,
        };

        // Compile regex patterns
        self.t_re_sequence = compile_regex_vec(processed_sequence.as_ref())?;
        Ok(())
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct ConfigSort {
    #[serde(default)]
    pub match_as_ascii: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub groups: Option<ConfigSortGroup>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channels: Option<Vec<ConfigSortChannel>>,
}

impl ConfigSort {
    pub fn prepare(&mut self, templates: Option<&Vec<PatternTemplate>>) -> Result<(), TuliproxError> {
        if let Some(group) = self.groups.as_mut() {
            group.prepare(templates)?;
        }
        if let Some(channels) = self.channels.as_mut() {
            handle_tuliprox_error_result_list!(TuliproxErrorKind::Info, channels.iter_mut().map(|csc| csc.prepare(templates)));
        }
        Ok(())
    }
}