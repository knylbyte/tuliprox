use std::net::SocketAddr;
use serde::{Deserialize, Serialize};
use crate::model::VirtualId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UserCommand {
    Kick(SocketAddr, VirtualId, u64),
}