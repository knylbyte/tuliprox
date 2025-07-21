use std::io;
use bytes::Bytes;
use crate::model::StatusCheck;
use crate::utils::{bincode_deserialize, bincode_serialize};
use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: u8 = 1;

pub enum ProtocolHandler {
    Version(u8),
    Default,
}

pub enum WsCloseCode {
    // Normal,
    // Away,
    Protocol,
    // Unsupported,
    // Abnormal,
    // Invalid,
    // Policy,
    // Size,
    // Extension,
    // Error,
    // Restart,
    // Again,
    // Tls,
}

impl WsCloseCode {
    pub fn code(&self) -> u16 {
        match self {
            // WsCloseCode::Normal => 1000,
            // WsCloseCode::Away => 1001,
            WsCloseCode::Protocol => 1002,
            // WsCloseCode::Unsupported => 1003,
            // WsCloseCode::Abnormal => 1006,
            // WsCloseCode::Invalid => 1007,
            // WsCloseCode::Policy => 1008,
            // WsCloseCode::Size => 1009,
            // WsCloseCode::Extension => 1010,
            // WsCloseCode::Error => 1011,
            // WsCloseCode::Restart => 1012,
            // WsCloseCode::Again => 1013,
            // WsCloseCode::Tls => 1015,
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub enum ProtocolMessage {
    Version(u8),
    StatusRequest(String),
    StatusResponse(StatusCheck),
    ActiveUserResponse(usize, usize), // user_count, connection count
    ActiveProviderResponse(String, usize)
}

impl ProtocolMessage {
    pub fn to_bytes(&self) -> io::Result<Bytes> {
        match self {
            ProtocolMessage::Version(version) => {
                Ok(Bytes::from(vec![*version]))
            }
            _ => {
                let encoded = bincode_serialize(self)?;
                Ok(Bytes::from(encoded))
            }
        }
    }

    pub fn from_bytes(bytes: Bytes) -> io::Result<Self> {
        if bytes.len() == 1 {
            Ok(ProtocolMessage::Version(bytes[0]))
        } else {
            bincode_deserialize::<ProtocolMessage>(bytes.as_ref())
        }
    }
}