use std::rc::Rc;
use shared::model::{ActiveUserConnectionChange, ConfigType, PlaylistUpdateState, StatusCheck};
use crate::model::BusyStatus;

#[derive(Debug, Clone, PartialEq)]
pub enum EventMessage {
    Unauthorized,
    ServerError(String),
    ServerStatus(Rc<StatusCheck>),
    ActiveUser(ActiveUserConnectionChange),
    ActiveProvider(String, usize),
    ConfigChange(ConfigType),
    Busy(BusyStatus),
    PlaylistUpdate(PlaylistUpdateState),
    PlaylistUpdateProgress(String, String),
    WebSocketStatus(bool),
}