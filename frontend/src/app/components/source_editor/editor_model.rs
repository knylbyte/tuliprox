use std::fmt;
use std::rc::Rc;
use serde::{Deserialize, Serialize};
use yew::{Callback, UseStateHandle};
use shared::model::{ConfigInputDto, ConfigTargetDto, InputType, TargetOutputDto};

// ----------------- Data Models -----------------
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub enum BlockType {
    InputXtream,
    InputM3u,
    Target,
    OutputM3u,
    OutputXtream,
    OutputHdHomeRun,
    OutputStrm,
}

// Define string constants
impl BlockType {
    pub const INPUT_XTREAM: &'static str = "InputXtream";
    pub const INPUT_M3U: &'static str = "InputM3u";
    pub const TARGET: &'static str = "Target";
    pub const OUTPUT_M3U: &'static str = "OutputM3u";
    pub const OUTPUT_XTREAM: &'static str = "OutputXtream";
    pub const OUTPUT_HDHOMERUN: &'static str = "OutputHdHomeRun";
    pub const OUTPUT_STRM: &'static str = "OutputStrm";

    pub fn is_input(&self) -> bool {
        matches!(self, Self::InputXtream | Self::InputM3u)
    }

    pub fn is_target(&self) -> bool {
        matches!(self, Self::Target)
    }

    // pub fn is_output(&self) -> bool {
    //     matches!(self, Self::OutputXtream | Self::OutputM3u | Self::OutputHdHomeRun | Self::OutputStrm)
    // }
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
            BlockType::OUTPUT_HDHOMERUN => BlockType::OutputHdHomeRun,
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

impl From<InputType> for BlockType {
    fn from(s: InputType) -> Self {
        match s {
            InputType::M3uBatch
            | InputType::M3u => BlockType::InputM3u,
            InputType::XtreamBatch
            | InputType::Xtream => BlockType::InputXtream,
        }
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
            BlockType::OutputHdHomeRun => Self::OUTPUT_HDHOMERUN,
            BlockType::OutputStrm => Self::OUTPUT_STRM,
        };
        write!(f, "{}", s)
    }
}
pub(crate) type BlockId = u16;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Block {
    pub id: BlockId,
    pub block_type: BlockType,
    pub position: (f32, f32),
    pub instance: BlockInstance,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Connection {
    pub from: BlockId,
    pub to: BlockId,
}


#[derive(Clone, Copy, PartialEq)]
pub enum PortStatus {
    Valid,
    Invalid,
    Inactive
}



#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum BlockInstance {
    Input(Rc<ConfigInputDto>),
    Target(Rc<ConfigTargetDto>),
    Output(Rc<TargetOutputDto>)
}

#[derive(Clone, PartialEq)]
pub enum EditMode {
    Inactive,
    Active(Block),
}

#[derive(Clone, PartialEq)]
pub struct SourceEditorContext {
    pub on_form_change: Callback<(BlockId, BlockInstance)>,
    pub edit_mode: UseStateHandle<EditMode>
}
