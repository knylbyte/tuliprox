use std::collections::{HashMap, HashSet};
use crate::foundation::filter::{prepare_templates, PatternTemplate};
use crate::tuliprox_error::{TuliproxError, TuliproxErrorKind, handle_tuliprox_error_result_list, create_tuliprox_error_result};
use crate::model::{ConfigInput, ProcessTargets};
use crate::model::config_target::ConfigTarget;
use crate::utils::default_as_default;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigSource {
    pub inputs: Vec<ConfigInput>,
    pub targets: Vec<ConfigTarget>,
}

impl ConfigSource {
    #[allow(clippy::cast_possible_truncation)]
    pub fn prepare(&mut self, index: u16, include_computed: bool) -> Result<u16, TuliproxError> {
        handle_tuliprox_error_result_list!(TuliproxErrorKind::Info, self.inputs.iter_mut().enumerate().map(|(idx, i)| i.prepare(index+(idx as u16), include_computed)));
        Ok(index + (self.inputs.len() as u16))
    }

    pub fn get_inputs_for_target(&self, target_name: &str) -> Option<Vec<&ConfigInput>> {
        for target in &self.targets {
            if target.name.eq(target_name) {
                let inputs = self.inputs.iter().filter(|&i| i.enabled).collect::<Vec<&ConfigInput>>();
                if !inputs.is_empty() {
                    return Some(inputs);
                }
            }
        }
        None
    }
}

#[derive(Default, Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourcesConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub templates: Option<Vec<PatternTemplate>>,
    pub sources: Vec<ConfigSource>,
}

impl SourcesConfig {
    pub(crate) fn get_source_at(&self, idx: usize) -> Option<&ConfigSource> {
        self.sources.get(idx)
    }

    pub fn prepare(&mut self, include_computed: bool) -> Result<(), TuliproxError> {
        self.prepare_templates()?;
        self.prepare_sources(include_computed)?;
        Ok(())
    }

    fn prepare_sources(&mut self, include_computed: bool) -> Result<(), TuliproxError> {
        // prepare sources and set id's
        let mut source_index: u16 = 1;
        let mut target_index: u16 = 1;
        for source in &mut self.sources {
            source_index = source.prepare(source_index, include_computed)?;
            for target in &mut source.targets {
                // prepare target templates
                let prepare_result = match &self.templates {
                    Some(templ) => target.prepare(target_index, Some(templ)),
                    _ => target.prepare(target_index, None)
                };
                prepare_result?;
                target_index += 1;
            }
        }
        Ok(())
    }

    fn prepare_templates(&mut self) -> Result<(), TuliproxError> {
        if let Some(templates) = &mut self.templates {
            match prepare_templates(templates) {
                Ok(tmplts) => {
                    self.templates = Some(tmplts);
                }
                Err(err) => {
                    return Err(err);
                }
            }
        }
        Ok(())
    }

    pub fn check_unique_target_names(&mut self) -> Result<HashSet<String>, TuliproxError> {
        let mut seen_names = HashSet::new();
        let default_target_name = default_as_default();
        for source in &self.sources {
            for target in &source.targets {
                // check the target name is unique
                let target_name = target.name.trim().to_string();
                if target_name.is_empty() {
                    return create_tuliprox_error_result!(TuliproxErrorKind::Info, "target name required");
                }
                if !default_target_name.eq_ignore_ascii_case(target_name.as_str()) {
                    if seen_names.contains(target_name.as_str()) {
                        return create_tuliprox_error_result!(TuliproxErrorKind::Info, "target names should be unique: {}", target_name);
                    }
                    seen_names.insert(target_name);
                }
            }
        }
        Ok(seen_names)
    }
    pub fn get_target_by_id(&self, target_id: u16) -> Option<&ConfigTarget> {
        for source in &self.sources {
            for target in &source.targets {
                if target.id == target_id {
                    return Some(target);
                }
            }
        }
        None
    }

    /// Returns the targets that were specified as parameters.
    /// If invalid targets are found, the program will be terminated.
    /// The return value has `enabled` set to true, if selective targets should be processed, otherwise false.
    ///
    /// * `target_args` the program parameters given with `-target` parameter.
    /// * `sources` configured sources in config file
    ///
    pub fn validate_targets(&self, target_args: Option<&Vec<String>>) -> Result<ProcessTargets, TuliproxError> {
        let mut enabled = true;
        let mut inputs: Vec<u16> = vec![];
        let mut targets: Vec<u16> = vec![];
        if let Some(user_targets) = target_args {
            let mut check_targets: HashMap<String, u16> = user_targets.iter().map(|t| (t.to_lowercase(), 0)).collect();
            for source in &self.sources {
                let mut target_added = false;
                for target in &source.targets {
                    for user_target in user_targets {
                        let key = user_target.to_lowercase();
                        if target.name.eq_ignore_ascii_case(key.as_str()) {
                            targets.push(target.id);
                            target_added = true;
                            if let Some(value) = check_targets.get(key.as_str()) {
                                check_targets.insert(key, value + 1);
                            }
                        }
                    }
                }
                if target_added {
                    source.inputs.iter().map(|i| i.id).for_each(|id| inputs.push(id));
                }
            }

            let missing_targets: Vec<String> = check_targets.iter().filter(|&(_, v)| *v == 0).map(|(k, _)| k.to_string()).collect();
            if !missing_targets.is_empty() {
                return create_tuliprox_error_result!(TuliproxErrorKind::Info, "No target found for {}", missing_targets.join(", "));
            }
            // let processing_targets: Vec<String> = check_targets.iter().filter(|&(_, v)| *v != 0).map(|(k, _)| k.to_string()).collect();
            // info!("Processing targets {}", processing_targets.join(", "));
        } else {
            enabled = false;
        }

        Ok(ProcessTargets {
            enabled,
            inputs,
            targets,
        })
    }
}