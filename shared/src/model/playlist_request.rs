use std::rc::Rc;
use serde::{Deserialize, Serialize};
use crate::model::{PlaylistItemType, XtreamCluster};

#[derive(Debug, Copy, Clone, Eq, Hash, PartialEq, Serialize, Deserialize, Default)]
pub enum PlaylistRequestType {
    #[default]
    Input,
    Target,
    Xtream,
    M3U
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct PlaylistRequest {
    pub rtype: PlaylistRequestType,
    pub username: Option<String>,
    pub password: Option<String>,
    pub url: Option<String>,
    pub source_id: Option<u16>,
    pub source_name: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct CommonPlaylistItem {
    pub virtual_id: u32,
    pub provider_id: String,
    pub name: String,
    pub chno: String,
    pub logo: String,
    pub logo_small: String,
    pub group: String,
    pub title: String,
    pub parent_code: String,
    pub audio_track: String,
    pub time_shift: String,
    pub rec: String,
    pub url: String,
    pub input_name: String,
    pub item_type: PlaylistItemType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub epg_channel_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub xtream_cluster: Option<XtreamCluster>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub additional_properties: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category_id: Option<u32>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct PlaylistResponseGroup {
    pub id: u32,
    pub title: String,
    pub channels: Vec<CommonPlaylistItem>,
    pub xtream_cluster: XtreamCluster,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct UiPlaylistGroup {
    pub id: u32,
    pub title: String,
    pub channels: Vec<Rc<CommonPlaylistItem>>,
    pub xtream_cluster: XtreamCluster,
}

impl From<PlaylistResponseGroup> for UiPlaylistGroup {
    fn from(response: PlaylistResponseGroup) -> Self {
        Self {
            id: response.id,
            title: response.title,
            channels: response.channels.into_iter().map(Rc::new).collect(),
            xtream_cluster: response.xtream_cluster,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct PlaylistCategoriesResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub live: Option<Vec<PlaylistResponseGroup>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vod: Option<Vec<PlaylistResponseGroup>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub series: Option<Vec<PlaylistResponseGroup>>,
}


#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct UiPlaylistCategories {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub live: Option<Vec<Rc<UiPlaylistGroup>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vod: Option<Vec<Rc<UiPlaylistGroup>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub series: Option<Vec<Rc<UiPlaylistGroup>>>,
}

impl From<PlaylistCategoriesResponse> for UiPlaylistCategories {
    fn from(response: PlaylistCategoriesResponse) -> Self {
        Self {
            live: response.live.map(|groups| groups.into_iter().map(Into::into).map(Rc::new).collect()),
            vod: response.vod.map(|groups| groups.into_iter().map(Into::into).map(Rc::new).collect()),
            series: response.series.map(|groups| groups.into_iter().map(Into::into).map(Rc::new).collect()),
        }
    }
}