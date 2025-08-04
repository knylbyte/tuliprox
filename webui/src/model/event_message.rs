use std::rc::Rc;
use shared::model::{ConfigType, PlaylistUpdateState, StatusCheck};
use crate::model::BusyStatus;

#[derive(Debug, Clone, PartialEq)]
pub enum EventMessage {
    Unauthorized,
    ServerStatus(Rc<StatusCheck>),
    ActiveUser(usize, usize),
    ActiveProvider(String, usize),
    ConfigChange(ConfigType),
    Busy(BusyStatus),
    PlaylistUpdate(PlaylistUpdateState),
    PlaylistUpdateProgress(String, String),
}