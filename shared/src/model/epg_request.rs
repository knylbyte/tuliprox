use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
pub enum PlaylistEpgRequest {
    Target(u16),
    Input(u16),
    Custom(String),
}
