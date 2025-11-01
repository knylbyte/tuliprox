use std::fmt;
use std::rc::Rc;
use serde::{Deserialize, Serialize};
use yew::{Callback, UseStateHandle};
use shared::model::{AppConfigDto, ConfigInputDto, SourcesConfigDto};
use crate::app::components::source_editor::input_form::ConfigInputFormState;

// ----------------- Data Models -----------------
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum BlockType {
    InputXtream,
    InputM3u,
    Target,
    OutputM3u,
    OutputXtream,
    OutputHdhomerun,
    OutputStrm,
}

// Define string constants
impl BlockType {
    pub const INPUT_XTREAM: &'static str = "InputXtream";
    pub const INPUT_M3U: &'static str = "InputM3u";
    pub const TARGET: &'static str = "Target";
    pub const OUTPUT_M3U: &'static str = "OutputM3u";
    pub const OUTPUT_XTREAM: &'static str = "OutputXtream";
    pub const OUTPUT_HDHOMERUN: &'static str = "OutputHdhomerun";
    pub const OUTPUT_STRM: &'static str = "OutputStrm";
}

// Convert from String to BlockType
impl From<&str> for BlockType {
    fn from(s: &str) -> Self {
        match s {
            BlockType::INPUT_XTREAM => BlockType::InputXtream,
            BlockType::INPUT_M3U => BlockType::InputM3u,
            BlockType::TARGET => BlockType::Target,
            BlockType::OUTPUT_M3U => BlockType::OutputM3u,
            BlockType::OUTPUT_XTREAM => BlockType::OutputXtream,
            BlockType::OUTPUT_HDHOMERUN => BlockType::OutputHdhomerun,
            BlockType::OUTPUT_STRM => BlockType::OutputStrm,
            _ => BlockType::Target, // fallback
        }
    }
}

impl From<String> for BlockType {
    fn from(s: String) -> Self {
        BlockType::from(s.as_str())
    }
}


// Display trait using constants
impl fmt::Display for BlockType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            BlockType::InputXtream => Self::INPUT_XTREAM,
            BlockType::InputM3u => Self::INPUT_M3U,
            BlockType::Target => Self::TARGET,
            BlockType::OutputM3u => Self::OUTPUT_M3U,
            BlockType::OutputXtream => Self::OUTPUT_XTREAM,
            BlockType::OutputHdhomerun => Self::OUTPUT_HDHOMERUN,
            BlockType::OutputStrm => Self::OUTPUT_STRM,
        };
        write!(f, "{}", s)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Block {
    pub id: usize,
    pub block_type: BlockType,
    pub position: (f32, f32),
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Connection {
    pub from: usize,
    pub to: usize,
}

#[derive(Clone, PartialEq)]
pub struct SourceEditorContext {
    pub input: Option<Rc<ConfigInputDto>>,
    pub on_form_change: Callback<ConfigInputFormState>,
}


