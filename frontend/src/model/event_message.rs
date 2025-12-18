use crate::model::BusyStatus;
use shared::model::{
    ActiveUserConnectionChange, ConfigType, PlaylistUpdateState, StatusCheck, SystemInfo,
};
use std::rc::Rc;

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq)]
pub enum EventMessage {
    Unauthorized,
    ServerError(String),
    ServerStatus(Rc<StatusCheck>),
    ActiveUser(ActiveUserConnectionChange),
    ActiveProvider(String, usize), // single provider
    ActiveProviderCount(usize),    // all provider
    ConfigChange(ConfigType),
    Busy(BusyStatus),
    PlaylistUpdate(PlaylistUpdateState),
    PlaylistUpdateProgress(String, String),
    WebSocketStatus(bool),
    SystemInfoUpdate(SystemInfo),
}
