use std::fmt;

#[derive(Debug, Copy, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub enum MsgKind {
    #[serde(rename = "info")]
    Info,
    #[serde(rename = "stats")]
    Stats,
    #[serde(rename = "error")]
    Error,
    #[serde(rename = "watch")]
    Watch,
}
impl fmt::Display for MsgKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            MsgKind::Info => "Info",
            MsgKind::Stats => "Stats",
            MsgKind::Error => "Error",
            MsgKind::Watch => "Watch",
        };
        write!(f, "{s}")
    }
}