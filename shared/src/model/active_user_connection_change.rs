use std::net::SocketAddr;
use crate::model::StreamInfo;

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(tag = "type", content = "payload", rename_all = "camelCase")]
pub enum ActiveUserConnectionChange {
    Connected(StreamInfo),
    Updated(StreamInfo),
    Disconnected(SocketAddr), // addr
    Connections(usize, usize) // user_count, connection_count
}
