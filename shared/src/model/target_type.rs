use std::fmt::Display;
use enum_iterator::Sequence;

#[derive(Debug, Copy, Clone, serde::Serialize, serde::Deserialize, Sequence, PartialEq, Eq, Hash)]
pub enum TargetType {
    #[serde(rename = "m3u")]
    M3u,
    #[serde(rename = "xtream")]
    Xtream,
    #[serde(rename = "strm")]
    Strm,
    #[serde(rename = "hdhomerun")]
    HdHomeRun,
}

impl TargetType {
    const M3U: &'static str = "M3u";
    const XTREAM: &'static str = "Xtream";
    const STRM: &'static str = "Strm";
    const HDHOMERUN: &'static str = "HdHomeRun";
}

impl Display for TargetType {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", match *self {
            Self::M3u => Self::M3U,
            Self::Xtream => Self::XTREAM,
            Self::Strm => Self::STRM,
            Self::HdHomeRun => Self::HDHOMERUN,
        })
    }
}

