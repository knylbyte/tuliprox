use std::net::SocketAddr;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UserCommand {
    Kick(SocketAddr)
}