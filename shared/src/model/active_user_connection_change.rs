use crate::model::StreamInfo;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub enum ActiveUserConnectionChange {
    Connected(StreamInfo),
    Disconnected(String), // addr
    Connections(usize, usize) // user_count, connection_count
}
