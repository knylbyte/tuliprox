use std::borrow::Cow;
use std::fmt::{Display, Formatter};
use std::str::FromStr;
use serde::{Deserialize, Serialize};

pub type UUIDType = [u8; 32];

#[derive(Debug, Copy, Clone, Eq, Hash, PartialEq, Serialize, Deserialize, Default)]
#[repr(u8)]
pub enum XtreamCluster {
    #[default]
    Live = 1,
    Video = 2,
    Series = 3,
}

impl XtreamCluster {
    pub const fn as_str(&self) -> &str {
        match self {
            Self::Live => "Live",
            Self::Video => "Video",
            Self::Series => "Series",
        }
    }
    pub const fn as_stream_type(&self) -> &str {
        match self {
            Self::Live => "live",
            Self::Video => "movie",
            Self::Series => "series",
        }
    }
}

impl Display for XtreamCluster {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl TryFrom<PlaylistItemType> for XtreamCluster {
    type Error = String;
    fn try_from(item_type: PlaylistItemType) -> Result<Self, Self::Error> {
        match item_type {
            PlaylistItemType::Live | PlaylistItemType::LiveHls | PlaylistItemType::LiveDash | PlaylistItemType::LiveUnknown => Ok(Self::Live),
            PlaylistItemType::Catchup | PlaylistItemType::Video => Ok(Self::Video),
            PlaylistItemType::Series | PlaylistItemType::SeriesInfo => Ok(Self::Series),
        }
    }
}

#[derive(Debug, Copy, Clone, Eq, Hash, PartialEq, Serialize, Deserialize, Default)]
#[repr(u8)]
pub enum PlaylistItemType {
    #[default]
    Live = 1,
    Video = 2,
    Series = 3, //  xtream series description
    SeriesInfo = 4, //  xtream series info fetched for series description
    Catchup = 5,
    LiveUnknown = 6, // No Provider id
    LiveHls = 7, // m3u8 entry
    LiveDash = 8, // mpd
}

impl From<XtreamCluster> for PlaylistItemType {
    fn from(xtream_cluster: XtreamCluster) -> Self {
        match xtream_cluster {
            XtreamCluster::Live => Self::Live,
            XtreamCluster::Video => Self::Video,
            XtreamCluster::Series => Self::SeriesInfo,
        }
    }
}


impl FromStr for PlaylistItemType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "Live" => Ok(PlaylistItemType::Live),
            "Video" => Ok(PlaylistItemType::Video),
            "Series" => Ok(PlaylistItemType::Series),
            "SeriesInfo" => Ok(PlaylistItemType::SeriesInfo),
            "Catchup" => Ok(PlaylistItemType::Catchup),
            "LiveUnknown" => Ok(PlaylistItemType::LiveUnknown),
            "LiveHls" => Ok(PlaylistItemType::LiveHls),
            "LiveDash" => Ok(PlaylistItemType::LiveDash),
            _ => Err(format!("Invalid PlaylistItemType: {s}")),
        }
    }
}

impl PlaylistItemType {
    const LIVE: &'static str = "live";
    const VIDEO: &'static str = "video";
    const SERIES: &'static str = "series";
    const SERIES_INFO: &'static str = "series-info";
    const CATCHUP: &'static str = "catchup";
}

impl Display for PlaylistItemType {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", match self {
            Self::Live | Self::LiveHls | Self::LiveDash | Self::LiveUnknown => Self::LIVE,
            Self::Video => Self::VIDEO,
            Self::Series => Self::SERIES,
            Self::SeriesInfo => Self::SERIES_INFO,
            Self::Catchup => Self::CATCHUP,
        })
    }
}

pub trait FieldGetAccessor {
    fn get_field(&self, field: &str) -> Option<Cow<str>>;
}
pub trait FieldSetAccessor {
    fn set_field(&mut self, field: &str, value: &str) -> bool;
}

pub trait PlaylistEntry: Send + Sync {
    fn get_virtual_id(&self) -> u32;
    fn get_provider_id(&self) -> Option<u32>;
    fn get_category_id(&self) -> Option<u32>;
    fn get_provider_url(&self) -> String;
    fn get_uuid(&self) -> UUIDType;
    fn get_item_type(&self) -> PlaylistItemType;
}