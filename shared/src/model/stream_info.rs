use serde::{Deserialize, Serialize};
use crate::model::{M3uPlaylistItem, PlaylistEntry, PlaylistItemType, XtreamCluster, XtreamPlaylistItem};
use crate::utils::{current_time_secs, StringExt};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StreamChannel {
    pub virtual_id: u32,
    pub provider_id: u32,
    pub item_type: PlaylistItemType,
    pub cluster: XtreamCluster,
    pub group: String,
    pub title: String,
    pub url: String,
    pub shared: bool,
}
impl XtreamPlaylistItem {
    pub fn to_stream_channel(&self) -> StreamChannel {
        StreamChannel {
            virtual_id: self.virtual_id,
            provider_id: self.provider_id,
            item_type: self.item_type,
            cluster: self.xtream_cluster,
            group: self.group.clone(),
            title: String::longest(self.title.as_str(), self.name.as_str()).to_string(),
            url: self.url.clone(),
            shared: false,
        }
    }
}

impl M3uPlaylistItem {
    pub fn to_stream_channel(&self) -> StreamChannel {
        StreamChannel {
            virtual_id: self.virtual_id,
            provider_id: self.get_provider_id().unwrap_or_default(),
            item_type: self.item_type,
            cluster: XtreamCluster::try_from(self.item_type).unwrap_or(XtreamCluster::Live),
            group: self.group.clone(),
            title: String::longest(self.title.as_str(), self.name.as_str()).to_string(),
            url: self.url.clone(),
            shared: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StreamInfo {
    pub username: String,
    pub channel: StreamChannel,
    pub provider: String,
    pub addr: String,
    pub user_agent: String,
    pub ts: u64,
}

impl StreamInfo {
    pub fn new(username: &str, addr: &str, provider: &str, stream_channel: StreamChannel, user_agent: String) -> Self {
        Self {
            username: username.to_string(),
            channel: stream_channel,
            provider: provider.to_string(),
            addr: addr.to_string(),
            user_agent,
            ts: current_time_secs(),
        }
    }
}