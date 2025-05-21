use enum_iterator::Sequence;
use log::{debug, error, trace};
use regex::Regex;
use std::collections::HashMap;
use std::fmt::Display;
use std::str::FromStr;
use std::sync::atomic::AtomicU32;
use std::sync::Arc;

use crate::foundation::filter::{apply_templates_to_pattern_single, get_filter, prepare_templates, Filter, PatternTemplate, RegexWithCaptures, ValueProcessor};
use crate::model::valid_property;
use crate::model::ItemField;
use crate::model::{FieldGetAccessor, FieldSetAccessor, PlaylistItem};
use crate::tuliprox_error::{create_tuliprox_error_result, info_err};
use crate::tuliprox_error::{TuliproxError, TuliproxErrorKind};
use crate::utils::Capitalize;
use crate::utils::CONSTANTS;

pub const COUNTER_FIELDS: &[&str] = &["name", "title", "caption", "chno"];

pub const MAPPER_FIELDS: &[&str] = &[
    "name", "title", "caption", "group", "id", "chno", "logo",
    "logo_small", "parent_code", "audio_track",
    "time_shift", "rec", "url", "epg_channel_id", "epg_id"
];


#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct MappingTag {
    pub name: String,
    pub captures: Vec<String>,
    #[serde(default)]
    pub concat: String,
    #[serde(default)]
    pub prefix: String,
    #[serde(default)]
    pub suffix: String,
}


#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Sequence, PartialEq, Eq)]
pub enum CounterModifier {
    #[serde(rename = "assign")]
    Assign,
    #[serde(rename = "suffix")]
    Suffix,
    #[serde(rename = "prefix")]
    Prefix,
}

impl Default for CounterModifier {
    fn default() -> Self {
        Self::Assign
    }
}

impl CounterModifier {
    const ASSIGN: &'static str = "assign";
    const SUFFIX: &'static str = "suffix";
    const PREFIX: &'static str = "prefix";
}

impl Display for CounterModifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", match self {
            Self::Assign => Self::ASSIGN,
            Self::Suffix => Self::SUFFIX,
            Self::Prefix => Self::PREFIX,
        })
    }
}

impl FromStr for CounterModifier {
    type Err = TuliproxError;

    fn from_str(s: &str) -> Result<Self, TuliproxError> {
        if s.eq("assign") {
            Ok(Self::Assign)
        } else if s.eq("suffix") {
            Ok(Self::Suffix)
        } else if s.eq("prefix") {
            Ok(Self::Prefix)
        } else {
            create_tuliprox_error_result!(TuliproxErrorKind::Info, "Unknown CounterModifier: {}", s)
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct MappingCounterDefinition {
    pub filter: String,
    pub field: String,
    #[serde(default)]
    pub concat: String,
    #[serde(default)]
    pub modifier: CounterModifier,
    #[serde(default)]
    pub value: u32,
    #[serde(default)]
    pub padding: u8,
}

#[derive(Debug, Clone)]
pub struct MappingCounter {
    pub filter: Filter,
    pub field: String,
    pub concat: String,
    pub modifier: CounterModifier,
    pub value: Arc<AtomicU32>,
    pub padding: u8,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "modifier", rename_all = "snake_case")]
pub enum MapperOperation {
    Lowercase { field: String },
    Uppercase { field: String },
    Capitalize { field: String },
    Suffix { field: String, value: String },
    Prefix { field: String, value: String },
    Set { field: String, value: String },
    Copy { field: String, source: String },
}

impl MapperOperation {
    pub fn prepare(&mut self, templates: Option<&Vec<PatternTemplate>>) -> Result<(), TuliproxError> {
        match self {
            MapperOperation::Lowercase { field }
            | MapperOperation::Uppercase { field }
            | MapperOperation::Capitalize { field } => {
                if !valid_property!(field.as_str(), MAPPER_FIELDS) {
                    return Err(info_err!(format!("Invalid mapper attribute field {field}")));
                }
            }

            MapperOperation::Copy { field, source } => {
                if !valid_property!(field.as_str(), MAPPER_FIELDS) {
                    return Err(info_err!(format!("Invalid mapper attribute field {field}")));
                }
                if !valid_property!(source.as_str(), MAPPER_FIELDS) {
                    return Err(info_err!(format!("Invalid mapper source field {source}")));
                }
            }

            MapperOperation::Suffix { field, value }
            | MapperOperation::Prefix { field, value }
            | MapperOperation::Set { field, value } => {
                if !valid_property!(field.as_str(), MAPPER_FIELDS) {
                    return Err(info_err!(format!("Invalid mapper attribute field {field}")));
                }

                if let Some(template_list) = templates {
                    *value = apply_templates_to_pattern_single(value, template_list)?;
                }
            }
        }

        Ok(())
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct Mapper {
    pub filter: String,
    pub pattern: Vec<String>,
    pub pipeline: Vec<MapperOperation>,
    #[serde(skip_serializing, skip_deserializing)]
    pub t_filter: Option<Filter>,
    #[serde(skip_serializing, skip_deserializing)]
    pub t_pattern: Option<Vec<Regex>>,
    #[serde(skip_serializing, skip_deserializing)]
    t_tags: Vec<MappingTag>,
}

impl Mapper {
    /// # Panics
    ///
    /// Will panic if default `RegEx` gets invalid
    pub fn prepare(&mut self, templates: Option<&Vec<PatternTemplate>>, tags: Option<&Vec<MappingTag>>) -> Result<(), TuliproxError> {
        for op in &mut self.pipeline {
            op.prepare(templates)?;
        }
        match get_filter(&self.filter, templates) {
            Ok(filter) => self.t_filter = Some(filter),
            Err(err) => return Err(err),
        }
        self.t_pattern = Some(self
            .pattern
            .iter()
            .map(|pattern| {
                let processed = match templates {
                    None => pattern.to_string(),
                    Some(tmpls) => apply_templates_to_pattern_single(pattern, tmpls)?,
                };
                Regex::new(&processed)
                    .map_err(|err| info_err!(format!("Invalid regex: {processed} {err}")))
            })
            .collect::<Result<Vec<_>, _>>()?);

        self.t_tags = tags.map_or_else(Vec::new, std::clone::Clone::clone);

        Ok(())
    }
}

enum TransformModifier {
    Lowercase,
    Uppercase,
    Capitalize,
}

pub struct MappingValueProcessor<'a> {
    pub pli: &'a mut PlaylistItem,
    pub mapper: &'a Mapper,
}

impl MappingValueProcessor<'_> {
    fn get_property(&self, key: &str) -> Option<String> {
        self.pli.header.get_field(key)
    }

    fn set_property(&mut self, key: &str, value: &str) {
        if !self.pli.header.set_field(key, value) {
            error!("Cant set unknown field {key} set to {value}");
        }
        trace!("Property {key} set to {value}");
    }

    fn apply_affix(&mut self, field: &str, value: &str, captured_names: &HashMap<&str, &str>, prefix: bool) {
        if let Some(affix) = self.apply_tags(value, captured_names) {
            if let Some(old_value) = self.get_property(field) {
                let mut new_value = String::with_capacity(old_value.len() + affix.len());
                if prefix {
                    new_value.push_str(&affix);
                    new_value.push_str(&old_value);
                } else {
                    new_value.push_str(&old_value);
                    new_value.push_str(&affix);
                }
                self.set_property(field, &new_value);
            }
        }
    }

    fn apply_assignment(&mut self, field: &str, source: &str) {
        if let Some(prop_value) = self.get_property(source) {
            self.set_property(field, &prop_value);
        }
    }

    fn apply_attributes(&mut self, field: &str, value: &str, captured_names: &HashMap<&str, &str>) {
        if value.contains('<') { // possible replacement
            let replaced = CONSTANTS.re_template_attribute.replace_all(value, |captures: &regex::Captures| {
                let capture_name = &captures[1];
                (*captured_names.get(&capture_name).unwrap_or(&&captures[0])).to_string()
            });
            self.set_property(field, &replaced);
        } else {
            self.set_property(field, value);
        }
    }

    fn apply_transform_modifier(modifier: &TransformModifier, value: &str) -> String {
        match modifier {
            TransformModifier::Uppercase => value.to_uppercase(),
            TransformModifier::Lowercase => value.to_lowercase(),
            TransformModifier::Capitalize => value.capitalize(),
        }
    }

    fn apply_transform(&mut self, modifier: TransformModifier, field: &str, captured_names: &HashMap<&str, &str>) {
        match modifier {
            TransformModifier::Lowercase => {}
            TransformModifier::Uppercase => {}
            TransformModifier::Capitalize => {}

            // None => {}
            // Some(transform_list) => {
            //     for transform in transform_list {
            //         if let Some(prop_value) = self.get_property(&transform.field) {
            //             let value = transform.t_pattern.as_ref().map_or_else(|| Cow::from(Self::apply_transform_modifier(&transform.modifier, prop_value.as_str())), |regex| regex.replace_all(&prop_value, |caps: &regex::Captures| {
            //                 Self::apply_transform_modifier(&transform.modifier, &caps[0])
            //             }));
            //             self.set_property(&transform.field, &value);
            //         }
            //     }
            // }
        }
    }

    fn apply_pipeline(&mut self, captured_names: &HashMap<&str, &str>) {
        let pipeline = &self.mapper.pipeline;
        for op in pipeline {
            match op {
                MapperOperation::Lowercase { field } => self.apply_transform(TransformModifier::Lowercase, field,  captured_names),
                MapperOperation::Uppercase { field } => self.apply_transform(TransformModifier::Uppercase, field,  captured_names),
                MapperOperation::Capitalize { field } => self.apply_transform(TransformModifier::Capitalize, field,  captured_names),
                MapperOperation::Suffix { field, value } => self.apply_affix(field, value, captured_names, false),
                MapperOperation::Prefix { field, value } => self.apply_affix(field, value, captured_names, true),
                MapperOperation::Set { field, value } => self.apply_attributes(field, value, captured_names),
                MapperOperation::Copy { field, source } => self.apply_assignment(field, source),
            }
        }
    }

    fn apply_tags(&self, value: &str, captures: &HashMap<&str, &str>) -> Option<String> {
        let mut new_value = String::from(value);
        let tag_captures = CONSTANTS.re_template_tag.captures_iter(value)
            .filter(|caps| caps.len() > 1)
            .filter_map(|caps| caps.get(1))
            .map(|caps| caps.as_str())
            .collect::<Vec<&str>>();

        let mut captured_tag_values: Vec<&str> = Vec::with_capacity(128);
        for tag_capture in tag_captures {
            for mapping_tag in &self.mapper.t_tags {
                if mapping_tag.name.eq(tag_capture) {
                    // we have the right tag, now get all captured values
                    captured_tag_values.clear();
                    for cap in &mapping_tag.captures {
                        if let Some(cap_value) = captures.get(cap.as_str()) {
                            captured_tag_values.push(cap_value);
                        } else {
                            debug!("Cant find any tag match for {tag_capture}");
                            return None;
                        }
                    }
                    if !captured_tag_values.is_empty() {
                        let captured_text = captured_tag_values.join(&mapping_tag.concat);
                        let replacement = if captured_text.trim().is_empty() {
                            // nothing found so replace tag with empty string
                            String::new()
                        } else {
                            // Now we have all our captured values, lets create the tag
                            format!("{}{captured_text}{}", &mapping_tag.prefix, &mapping_tag.suffix)
                        };
                        new_value = new_value.replace(format!("<tag:{}>", mapping_tag.name).as_str(), replacement.as_str());
                    }
                }
            }
        }
        Some(new_value)
    }
}

impl ValueProcessor for MappingValueProcessor<'_> {
    fn process<'a>(&mut self, _: &ItemField, value: &str, rewc: &RegexWithCaptures) -> bool {
        let mut captured_values = HashMap::new();
        if !rewc.captures.is_empty() {
            rewc.re.captures_iter(value)
                .filter(|caps| caps.len() > 1)
                .for_each(|captures|
                    for capture_name in &rewc.captures {
                        let match_opt = captures.name(capture_name.as_str());
                        let capture_value = if match_opt.is_some() {
                            match_opt.map_or("", |m| m.as_str())
                        } else {
                            ""
                        };
                        debug!("match {capture_name}: {capture_value}");
                        captured_values.insert(capture_name.as_str(), capture_value);
                    }
                );
        }

        MappingValueProcessor::<'_>::apply_pipeline(self, &captured_values);
        true
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct Mapping {
    pub id: String,
    #[serde(default)]
    pub match_as_ascii: bool,
    pub mapper: Option<Vec<Mapper>>,
    pub counter: Option<Vec<MappingCounterDefinition>>,
    #[serde(skip_serializing, skip_deserializing)]
    pub t_counter: Option<Vec<MappingCounter>>,
}

impl Mapping {
    pub fn prepare(&mut self, templates: Option<&Vec<PatternTemplate>>,
                   tags: Option<&Vec<MappingTag>>) -> Result<(), TuliproxError> {
        if let Some(mapper_list) = &mut self.mapper {
            for mapper in mapper_list {
                mapper.prepare(templates, tags)?;
            }
        }

        if let Some(counter_def_list) = &self.counter {
            let mut counters = vec![];
            for def in counter_def_list {
                if !valid_property!(def.field.as_str(), COUNTER_FIELDS) {
                    return Err(info_err!(format!("Invalid counter field {}", def.field)));
                }
                match get_filter(&def.filter, templates) {
                    Ok(flt) => {
                        counters.push(MappingCounter {
                            filter: flt,
                            field: def.field.clone(),
                            concat: def.concat.clone(),
                            modifier: def.modifier.clone(),
                            value: Arc::new(AtomicU32::new(def.value)),
                            padding: def.padding,
                        });
                    }
                    Err(e) => return Err(info_err!(e.to_string()))
                }
            }
            self.t_counter = Some(counters);
        }

        Ok(())
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MappingDefinition {
    pub templates: Option<Vec<PatternTemplate>>,
    pub tags: Option<Vec<MappingTag>>,
    pub mapping: Vec<Mapping>,
}

impl MappingDefinition {
    pub fn prepare(&mut self) -> Result<(), TuliproxError> {
        if let Some(templates) = &mut self.templates {
            match prepare_templates(templates) {
                Ok(tmplts) => {
                    self.templates = Some(tmplts);
                }
                Err(err) => return Err(err),
            }
        }
        for mapping in &mut self.mapping {
            let template_list = self.templates.as_ref();
            let tag_list = self.tags.as_ref();
            mapping.prepare(template_list, tag_list)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Mappings {
    pub mappings: MappingDefinition,
}

impl Mappings {
    pub fn prepare(&mut self) -> Result<(), TuliproxError> {
        self.mappings.prepare()
    }

    pub fn get_mapping(&self, mapping_id: &str) -> Option<Mapping> {
        for mapping in &self.mappings.mapping {
            if mapping.id.eq(mapping_id) {
                return Some(mapping.clone());
            }
        }
        None
    }
}
