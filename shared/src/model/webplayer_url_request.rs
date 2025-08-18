use serde::{Deserialize, Serialize};
use crate::model::XtreamCluster;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WebplayerUrlRequest {
    pub target_id: u16,
    pub virtual_id: u32,
    pub cluster: XtreamCluster,
}