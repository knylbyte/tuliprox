use std::rc::Rc;
use regex::Regex;
use serde::{Deserialize, Serialize};
use crate::model::{PlaylistItemType, SearchRequest, XtreamCluster};

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

fn filter_channels(groups: &Option<Vec<Rc<UiPlaylistGroup>>>, text: &str) -> Option<Vec<Rc<CommonPlaylistItem>>>{
    groups.as_ref().map(|gs| {
        gs.iter()
            .flat_map(|group| &group.channels)
            .filter(|&c| c.title.to_lowercase().contains(text)
                || c.name.to_lowercase().contains(text))
            .cloned()
            .collect::<Vec<_>>()
    })
}

fn filter_channels_re(groups: &Option<Vec<Rc<UiPlaylistGroup>>>, regex: &Regex) -> Option<Vec<Rc<CommonPlaylistItem>>>{
    groups.as_ref().map(|gs| {
        gs.iter()
            .flat_map(|group| &group.channels)
            .filter(|&c| regex.is_match(&c.title) || regex.is_match(&c.name))
            .cloned()
            .collect::<Vec<_>>()
    })
}

fn build_result(live: Option<Vec<Rc<CommonPlaylistItem>>>,
                video: Option<Vec<Rc<CommonPlaylistItem>>>,
                series: Option<Vec<Rc<CommonPlaylistItem>>>) -> Option<UiPlaylistCategories> {
    if live.is_none() && video.is_none() && series.is_none() {
        None
    } else {
        let build_group  = |xtream_cluster: XtreamCluster, channels: Vec<Rc<CommonPlaylistItem>>, id: u32| {
            if channels.is_empty() {
                None
            } else {
                Some(vec!(Rc::new(UiPlaylistGroup {
                    id,
                    title: format!("{} ({})", xtream_cluster.as_str().to_owned(), channels.len()),
                    channels,
                    xtream_cluster,
                })))
            }
        };

        let live = live.and_then(|g| build_group(XtreamCluster::Live, g, 1));
        let vod = video.and_then(|g| build_group(XtreamCluster::Video, g, 2));
        let series = series.and_then(|g| build_group(XtreamCluster::Series, g, 3));
        Some(UiPlaylistCategories {
            live,
            vod,
            series
        })
    }
}

impl UiPlaylistCategories {
    pub fn filter(&self, search_req: &SearchRequest) -> Option<Self> {
        match search_req {
            SearchRequest::Clear => None,
            SearchRequest::Text(text, _search_fields) => {
                let text_lc = text.to_lowercase();
                let live = filter_channels(&self.live, &text_lc);
                let video = filter_channels(&self.vod, &text_lc);
                let series = filter_channels(&self.series, &text_lc);
                build_result(live, video, series)
            }
            SearchRequest::Regexp(text, _search_fields) => {
                if let Ok(regex) = Regex::new(text) {
                    let live = filter_channels_re(&self.live, &regex);
                    let video = filter_channels_re(&self.vod, &regex);
                    let series = filter_channels_re(&self.series, &regex);
                    build_result(live, video, series)
                } else {
                    None
                }
            }
        }
    }
}