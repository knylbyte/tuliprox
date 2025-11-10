use std::rc::Rc;
use shared::model::{ActiveUserConnectionChange, ConfigType, PlaylistUpdateState, StatusCheck};
use crate::model::BusyStatus;

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq)]
pub enum EventMessage {
    Unauthorized,
    ServerError(String),
    ServerStatus(Rc<StatusCheck>),
    ActiveUser(ActiveUserConnectionChange),
    ActiveProvider(Option<String>, usize), // single provider
    ActiveProviderCount(usize), // all provider
    ConfigChange(ConfigType),
    Busy(BusyStatus),
    PlaylistUpdate(PlaylistUpdateState),
    PlaylistUpdateProgress(String, String),
    WebSocketStatus(bool),
}