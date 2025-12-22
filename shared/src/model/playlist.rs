use crate::model::{xtream_const, ClusterFlags, CommonPlaylistItem, ConfigTargetOptions, EpisodeStreamProperties, SeriesStreamProperties, StreamProperties, VideoStreamProperties};
use crate::utils::{extract_extension_from_url, generate_playlist_uuid, get_provider_id};
use enum_iterator::Sequence;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::borrow::Cow;
use std::fmt::Write;
use std::fmt::{Display, Formatter};
use std::str::FromStr;
// https://de.wikipedia.org/wiki/M3U
// https://siptv.eu/howto/playlist.html

pub type UUIDType = [u8; 32];
pub type VirtualId = u32;

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

impl FromStr for XtreamCluster {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "live" => Ok(XtreamCluster::Live),
            "video" | "vod" | "movie" => Ok(XtreamCluster::Video),
            "series" => Ok(XtreamCluster::Series),
            _ => Err(format!("Invalid XtreamCluster: {s}")),
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
            PlaylistItemType::Catchup | PlaylistItemType::Video | PlaylistItemType::LocalVideo => Ok(Self::Video),
            PlaylistItemType::Series | PlaylistItemType::SeriesInfo | PlaylistItemType::LocalSeries | PlaylistItemType::LocalSeriesInfo => Ok(Self::Series),
        }
    }
}

#[derive(Debug, Copy, Clone, Eq, Hash, PartialEq, Serialize, Deserialize, Default, Sequence)]
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
    LocalVideo = 9,
    LocalSeries = 10,
    LocalSeriesInfo = 11,
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
            "LocalVideo" => Ok(PlaylistItemType::LocalVideo),
            "Series" => Ok(PlaylistItemType::Series),
            "SeriesInfo" => Ok(PlaylistItemType::SeriesInfo),
            "LocalSeries" => Ok(PlaylistItemType::LocalSeries),
            "LocalSeriesInfo" => Ok(PlaylistItemType::LocalSeriesInfo),
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

    pub fn is_local(&self) -> bool {
        matches!(self, PlaylistItemType::LocalVideo | PlaylistItemType::LocalSeries | PlaylistItemType::LocalSeriesInfo)
    }

    pub fn as_u8(self) -> u8 {
        self as u8
    }
}

impl Display for PlaylistItemType {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", match self {
            Self::Live | Self::LiveHls | Self::LiveDash | Self::LiveUnknown => Self::LIVE,
            Self::Video | Self::LocalVideo => Self::VIDEO,
            Self::Series | Self::LocalSeries => Self::SERIES,
            Self::SeriesInfo | Self::LocalSeriesInfo => Self::SERIES_INFO,
            Self::Catchup => Self::CATCHUP,
        })
    }
}

#[derive(Copy, Clone, Default, Debug)]
pub struct PlaylistItemTypeSet(u16);
impl PlaylistItemTypeSet {
    #[inline]
    pub fn empty() -> Self {
        Self(0)
    }

    #[inline]
    pub fn from_item(item: PlaylistItemType) -> Self {
        let bit = 1u16 << ((item as u8) - 1);
        Self(bit)
    }

    #[inline]
    pub fn insert(&mut self, item: PlaylistItemType) {
        self.0 |= 1u16 << ((item as u8) - 1);
    }

    #[inline]
    pub fn remove(&mut self, item: PlaylistItemType) {
        self.0 &= !(1u16 << ((item as u8) - 1));
    }

    #[inline]
    pub fn is_set(&self, item: PlaylistItemType) -> bool {
        (self.0 & (1u16 << ((item as u8) - 1))) != 0
    }

    #[inline]
    pub fn bits(self) -> u16 {
        self.0
    }
}


pub trait FieldGetAccessor {
    fn get_field(&self, field: &str) -> Option<Cow<'_, str>>;
}
pub trait FieldSetAccessor {
    fn set_field(&mut self, field: &str, value: &str) -> bool;
}

pub trait PlaylistEntry: Send + Sync {
    fn get_virtual_id(&self) -> VirtualId;
    fn get_provider_id(&self) -> Option<u32>;
    fn get_category_id(&self) -> Option<u32>;
    fn get_provider_url(&self) -> String;
    fn get_uuid(&self) -> UUIDType;
    fn get_item_type(&self) -> PlaylistItemType;
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PlaylistItemHeader {
    #[serde(skip)]
    pub uuid: UUIDType, // calculated
    pub id: String, // provider id
    pub virtual_id: VirtualId, // virtual id
    pub name: String,
    pub chno: u32,
    pub logo: String,
    pub logo_small: String,
    pub group: String,
    pub title: String,
    pub parent_code: String,
    pub audio_track: String,
    pub time_shift: String,
    pub rec: String,
    pub url: String,
    pub epg_channel_id: Option<String>,
    pub xtream_cluster: XtreamCluster,
    pub additional_properties: Option<StreamProperties>,
    #[serde(default)]
    pub item_type: PlaylistItemType,
    #[serde(default)]
    pub category_id: u32,
    pub input_name: String,
}

impl PlaylistItemHeader {
    pub fn gen_uuid(&mut self) {
        self.uuid = generate_playlist_uuid(&self.input_name, &self.id, self.item_type, &self.url);
    }
    pub const fn get_uuid(&self) -> &UUIDType {
        &self.uuid
    }

    pub fn get_provider_id(&mut self) -> Option<u32> {
        match get_provider_id(&self.id, &self.url) {
            None => None,
            Some(newid) => {
                self.id = newid.to_string();
                Some(newid)
            }
        }
    }

    pub fn get_container_extension(&self) -> Option<String> {
        self.additional_properties.as_ref().and_then(|a| a.get_container_extension())
    }
}

macro_rules! to_m3u_non_empty_fields {
    ($header:expr, $line:expr, $(($prop:ident, $field:expr)),*;) => {
        $(
            if !$header.$prop.is_empty() {
                let _ = write!($line," {}=\"{}\"", $field, $header.$prop );
            }
         )*
    };
}

macro_rules! to_m3u_resource_non_empty_fields {
    ($header:expr, $url:expr, $line:expr, $(($prop:ident, $field:expr)),*;) => {
        $(
           if !$header.$prop.is_empty() {
               let _ = write!($line, " {}=\"{}/{}\"", $field, $url, stringify!($prop));
            }
         )*
    };
}

macro_rules! generate_field_accessor_impl_for_playlist_item_header {
    ($($prop:ident),*;) => {
        impl crate::model::FieldGetAccessor for crate::model::PlaylistItemHeader {
            fn get_field(&self, field: &str) -> Option<Cow<'_, str>> {
                let field = field.to_lowercase();
                match field.as_str() {
                    $(
                        stringify!($prop) => Some(Cow::Borrowed(&self.$prop)),
                    )*
                    "chno" => Some(Cow::Owned(self.chno.to_string())),
                    "input" =>  Some(Cow::Borrowed(self.input_name.as_str())),
                    "type" => Some(Cow::Owned(self.item_type.to_string())),
                    "caption" =>  Some(if self.title.is_empty() { Cow::Borrowed(&self.name) } else { Cow::Borrowed(&self.title) }),
                    "epg_channel_id" | "epg_id" => self.epg_channel_id.as_ref().map(|s| Cow::Borrowed(s.as_str())),
                    _ => None,
                }
            }
         }
         impl crate::model::FieldSetAccessor for crate::model::PlaylistItemHeader {
            fn set_field(&mut self, field: &str, value: &str) -> bool {
                let field = field.to_lowercase();
                let val = String::from(value);
                match field.as_str() {
                    $(
                        stringify!($prop) => {
                            self.$prop = val;
                            true
                        }
                    )*
                    "chno" => {
                        if let Ok(val) = value.parse::<u32>() {
                            self.chno = val;
                            true
                        } else {
                            false
                        }
                    },
                    "caption" => {
                        self.title = val.clone();
                        self.name = val;
                        true
                    }
                    "epg_channel_id" | "epg_id" => {
                        self.epg_channel_id = Some(value.to_owned());
                        true
                    }
                    _ => false,
                }
            }
        }
    }
}

generate_field_accessor_impl_for_playlist_item_header!(id, /*virtual_id,*/ name, logo, logo_small, group, title, parent_code, audio_track, time_shift, rec, url;);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct M3uPlaylistItem {
    pub virtual_id: VirtualId,
    pub provider_id: String,
    pub name: String,
    pub chno: u32,
    pub logo: String,
    pub logo_small: String,
    pub group: String,
    pub title: String,
    pub parent_code: String,
    pub audio_track: String,
    pub time_shift: String,
    pub rec: String,
    pub url: String,
    pub epg_channel_id: Option<String>,
    pub input_name: String,
    pub item_type: PlaylistItemType,
    #[serde(skip)]
    pub t_stream_url: String,
    #[serde(skip)]
    pub t_resource_url: Option<String>,
}

impl M3uPlaylistItem {
    #[allow(clippy::missing_panics_doc)]
    pub fn to_m3u(&self, target_options: Option<&ConfigTargetOptions>, rewrite_urls: bool) -> String {
        let options = target_options.as_ref();
        let ignore_logo = options.is_some_and(|o| o.ignore_logo);
        let mut line = String::with_capacity(256);
        let _ = write!(&mut line, "#EXTINF:-1 tvg-id=\"{}\" tvg-name=\"{}\" group-title=\"{}\"",
                       self.epg_channel_id.as_ref().map_or("", |o| o.as_ref()),
                       self.name, self.group);

        if !ignore_logo {
            if let (true, Some(resource_url)) = (rewrite_urls, self.t_resource_url.as_ref()) {
                to_m3u_resource_non_empty_fields!(self, resource_url, line, (logo, "tvg-logo"), (logo_small, "tvg-logo-small"););
            } else {
                to_m3u_non_empty_fields!(self, line, (logo, "tvg-logo"), (logo_small, "tvg-logo-small"););
            }
        }

        if self.chno != 0 {
            let _ = write!(line, " tvg-chno=\"{}\"", self.chno);
        }
        to_m3u_non_empty_fields!(self, line,
            (parent_code, "parent-code"),
            (audio_track, "audio-track"),
            (time_shift, "timeshift"),
            (rec, "tvg-rec"););

        let url = if self.t_stream_url.is_empty() { &self.url } else { &self.t_stream_url };
        let _ = write!(&mut line, ",{}\n{}", self.title, url);
        line
    }

    pub fn to_common(&self) -> CommonPlaylistItem {
        CommonPlaylistItem {
            virtual_id: self.virtual_id,
            provider_id: self.provider_id.to_string(),
            name: self.name.clone(),
            chno: self.chno,
            logo: self.logo.clone(),
            logo_small: self.logo_small.clone(),
            group: self.group.clone(),
            title: self.title.clone(),
            parent_code: self.parent_code.clone(),
            audio_track: self.audio_track.to_string(),
            time_shift: self.time_shift.to_string(),
            rec: self.rec.clone(),
            url: self.url.clone(),
            input_name: self.input_name.clone(),
            item_type: self.item_type,
            epg_channel_id: self.epg_channel_id.clone(),
            xtream_cluster: XtreamCluster::try_from(self.item_type).ok(),
            additional_properties: None,
            category_id: None,
        }
    }
}

impl PlaylistEntry for M3uPlaylistItem {
    #[inline]
    fn get_virtual_id(&self) -> VirtualId {
        self.virtual_id
    }

    fn get_provider_id(&self) -> Option<u32> {
        get_provider_id(&self.provider_id, &self.url)
    }
    #[inline]
    fn get_category_id(&self) -> Option<u32> {
        None
    }
    #[inline]
    fn get_provider_url(&self) -> String {
        self.url.to_string()
    }

    fn get_uuid(&self) -> UUIDType {
        generate_playlist_uuid(&self.input_name, &self.provider_id, self.item_type, &self.url)
    }

    #[inline]
    fn get_item_type(&self) -> PlaylistItemType {
        self.item_type
    }
}

macro_rules! generate_field_accessor_impl_for_m3u_playlist_item {
    ($($prop:ident),*;) => {
        impl crate::model::FieldGetAccessor for M3uPlaylistItem {
            fn get_field(&self, field: &str) -> Option<Cow<'_, str>> {
                let field = field.to_lowercase();
                match field.as_str() {
                    $(
                        stringify!($prop) => Some(Cow::Borrowed(&self.$prop)),
                    )*
                    "chno" => Some(Cow::Owned(self.chno.to_string())),
                    "caption" =>  Some(if self.title.is_empty() { Cow::Borrowed(&self.name) } else { Cow::Borrowed(&self.title) }),
                    "epg_channel_id" | "epg_id" => self.epg_channel_id.as_ref().map(|s| Cow::Borrowed(s.as_str())),
                    _ => None,
                }
            }
        }
    }
}

generate_field_accessor_impl_for_m3u_playlist_item!(provider_id, name, logo, logo_small, group, title, parent_code, audio_track, time_shift, rec, url;);

impl From<M3uPlaylistItem> for CommonPlaylistItem {
    fn from(item: M3uPlaylistItem) -> Self {
        item.to_common()
    }
}

#[allow(clippy::struct_excessive_bools)]
pub struct XtreamMappingOptions {
    pub skip_live_direct_source: bool,
    pub skip_video_direct_source: bool,
    pub skip_series_direct_source: bool,
    pub rewrite_resource_url: bool,
    pub force_redirect: Option<ClusterFlags>,
    pub reverse_item_types: PlaylistItemTypeSet,
    pub username: String,
    pub password: String,
    pub base_url: Option<String>,
}

impl XtreamMappingOptions {
    pub fn is_reverse(&self, item_type: PlaylistItemType) -> bool {
        self.reverse_item_types.is_set(item_type)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XtreamPlaylistItem {
    pub virtual_id: VirtualId,
    pub provider_id: u32,
    pub name: String,
    pub logo: String,
    pub logo_small: String,
    pub group: String,
    pub title: String,
    pub parent_code: String,
    pub rec: String,
    pub url: String,
    pub epg_channel_id: Option<String>,
    pub xtream_cluster: XtreamCluster,
    pub additional_properties: Option<StreamProperties>,
    pub item_type: PlaylistItemType,
    pub category_id: u32,
    pub input_name: String,
    pub channel_no: u32,
}

fn make_bdpath_resource_url(resource_url: Option<&String>, bd_path: &str, index: usize, field_prefix: &str) -> String {
    if let Some(url) = resource_url {
        if bd_path.starts_with("http") {
            return format!("{url}/{field_prefix}{}_{index}", xtream_const::XC_PROP_BACKDROP_PATH);
        }
    }
    bd_path.to_string()
}

fn make_resource_url(resource_url: Option<&String>, value: &str, field: &str) -> String {
    if let Some(url) = resource_url {
        if value.starts_with("http") {
            return format!("{url}/{field}");
        }
    }
    value.to_string()
}


impl XtreamPlaylistItem {
    pub fn get_container_extension(&self) -> Option<String> {
        match self.additional_properties {
            None => None,
            Some(ref props) => {
                match props {
                    StreamProperties::Live(_) => Some("ts".to_string()),
                    StreamProperties::Video(video) => Some(video.container_extension.clone()),
                    StreamProperties::Series(_) => None,
                    StreamProperties::Episode(episode) => Some(episode.container_extension.clone()),
                }
            }
        }
    }

    pub fn to_common(&self) -> CommonPlaylistItem {
        CommonPlaylistItem {
            virtual_id: self.virtual_id,
            provider_id: self.provider_id.to_string(),
            name: self.name.clone(),
            chno: self.channel_no,
            logo: self.logo.clone(),
            logo_small: self.logo_small.clone(),
            group: self.group.clone(),
            title: self.title.clone(),
            parent_code: self.parent_code.clone(),
            audio_track: String::new(),
            time_shift: String::new(),
            rec: self.rec.clone(),
            url: self.url.clone(),
            input_name: self.input_name.clone(),
            item_type: self.item_type,
            epg_channel_id: self.epg_channel_id.clone(),
            xtream_cluster: Some(self.xtream_cluster),
            additional_properties: self.additional_properties.clone(),
            category_id: Some(self.category_id),
        }
    }

    pub fn to_document(&self, options: &XtreamMappingOptions) -> serde_json::Value {
        let is_reverse = options.is_reverse(self.item_type);
        let resource_url = if is_reverse && options.rewrite_resource_url && options.base_url.is_some() {
            let resource_url = format!("{}/resource/{}/{}/{}/{}", options.base_url.as_ref().map_or_else(String::new, |b| b.clone()),
                                       self.xtream_cluster.as_stream_type(), options.username, options.password, self.get_virtual_id());
            Some(resource_url)
        } else {
            None
        };

        if let Some(props) = self.additional_properties.as_ref() {
            match props {
                StreamProperties::Live(live) => {
                    let stream_icon = if !self.logo.is_empty() {
                        make_resource_url(resource_url.as_ref(), &self.logo, "logo")
                    } else if !self.logo_small.is_empty() {
                        make_resource_url(resource_url.as_ref(), &self.logo_small, "logo_small")
                    } else {
                        String::new()
                    };
                    Value::Object(serde_json::Map::from_iter([
                        ("num".to_string(), Value::Number(serde_json::Number::from(self.channel_no))),
                        ("name".to_string(), Value::String(self.title.clone())), // name or title ?
                        ("stream_id".to_string(), Value::Number(serde_json::Number::from(self.virtual_id))),
                        ("stream_icon".to_string(), Value::String(stream_icon)),
                        ("epg_channel_id".to_string(), Value::String(self.epg_channel_id.as_ref().map_or_else(String::new, Clone::clone))),
                        ("added".to_string(), Value::String(String::new())),
                        ("is_adult".to_string(), Value::Number(serde_json::Number::from(live.is_adult))),
                        ("category_id".to_string(), Value::String(self.category_id.to_string())),
                        ("category_ids".to_string(), Value::Array(Vec::from([Value::Number(serde_json::Number::from(self.category_id))]))),
                        ("custom_sid".to_string(), live.custom_sid.as_ref().map_or(Value::Null, |s| Value::String(s.clone()))),
                        ("direct_source".to_string(), if options.skip_live_direct_source { Value::String(String::new()) } else { Value::String(live.direct_source.clone()) }),
                        ("tv_archive".to_string(), Value::Number(serde_json::Number::from(live.tv_archive.unwrap_or_default()))),
                        ("tv_archive_duration".to_string(), Value::Number(serde_json::Number::from(live.tv_archive_duration.unwrap_or_default()))),
                    ]))
                }
                StreamProperties::Video(video) => {
                    let stream_icon = if !self.logo.is_empty() {
                        make_resource_url(resource_url.as_ref(), &self.logo, "logo")
                    } else if !self.logo_small.is_empty() {
                        make_resource_url(resource_url.as_ref(), &self.logo_small, "logo_small")
                    } else {
                        String::new()
                    };
                    Value::Object(serde_json::Map::from_iter([
                        ("num".to_string(), Value::Number(serde_json::Number::from(self.channel_no))),
                        ("name".to_string(), Value::String(self.title.clone())), // name or title ?
                        ("stream_type".to_string(), Value::String("movie".to_string())),
                        ("stream_id".to_string(), Value::Number(serde_json::Number::from(self.virtual_id))),
                        ("stream_icon".to_string(), Value::String(stream_icon)),
                        ("rating".to_string(), Value::String(video.rating.map_or_else(String::new, |r| format!("{r:.2}")))),
                        ("rating_5based".to_string(), Value::String(video.rating_5based.map_or_else(String::new, |r| format!("{r:.2}")))),
                        ("tmdb".to_string(), Value::String(video.tmdb.as_ref().map_or_else(String::new, ToString::to_string))),
                        ("trailer".to_string(), Value::String(video.trailer.as_ref().map_or_else(String::new, Clone::clone))),
                        ("added".to_string(), Value::String(video.added.clone())),
                        ("is_adult".to_string(), Value::Number(serde_json::Number::from(video.is_adult))),
                        ("category_id".to_string(), Value::String(self.category_id.to_string())),
                        ("category_ids".to_string(), Value::Array(Vec::from([Value::Number(serde_json::Number::from(self.category_id))]))),
                        ("container_extension".to_string(), Value::String(video.container_extension.clone())),
                        ("custom_sid".to_string(), video.custom_sid.as_ref().map_or(Value::Null, |s| Value::String(s.clone()))),
                        ("direct_source".to_string(), if options.skip_video_direct_source { Value::String(String::new()) } else { Value::String(video.direct_source.clone()) }),
                    ]))
                }
                StreamProperties::Series(series) => {
                    Value::Object(serde_json::Map::from_iter([
                        ("num".to_string(), Value::Number(serde_json::Number::from(self.channel_no))),
                        ("name".to_string(), Value::String(self.title.clone())), // name or title ?
                        ("series_id".to_string(), Value::Number(serde_json::Number::from(self.virtual_id))),
                        ("cover".to_string(), Value::String(make_resource_url(resource_url.as_ref(), &series.cover, xtream_const::XC_PROP_COVER))),
                        ("plot".to_string(), Value::String(series.plot.as_ref().map_or_else(String::new, Clone::clone))),
                        ("cast".to_string(), Value::String(series.cast.clone())),
                        ("director".to_string(), Value::String(series.director.clone())),
                        ("genre".to_string(), Value::String(series.genre.as_ref().map_or_else(String::new, Clone::clone))),
                        ("release_date".to_string(), Value::String(series.release_date.as_ref().map_or_else(String::new, Clone::clone))),
                        ("releaseDate".to_string(), Value::String(series.release_date.as_ref().map_or_else(String::new, Clone::clone))),
                        ("last_modified".to_string(), Value::String(series.last_modified.as_ref().map_or_else(String::new, Clone::clone))),
                        ("rating".to_string(), Value::String(format!("{:.2}", series.rating))),
                        ("rating_5based".to_string(), Value::String(format!("{:.2}", series.rating_5based))),
                        ("backdrop_path".to_string(), Value::Array(
                            series.backdrop_path.as_ref().map_or_else(Vec::new, |b| b.iter().enumerate().map(|(idx, p)|
                                Value::String(
                                    make_bdpath_resource_url(resource_url.as_ref(), p, idx, "")
                                )
                            ).collect())
                        )),
                        ("youtube_trailer".to_string(), Value::String(series.youtube_trailer.clone())),
                        ("tmdb".to_string(), Value::String(series.tmdb.as_ref().map_or_else(String::new, ToString::to_string))),
                        ("episode_runtime".to_string(), Value::String(series.episode_run_time.as_ref().map_or_else(String::new, Clone::clone))),
                        ("category_id".to_string(), Value::String(self.category_id.to_string())),
                        ("category_ids".to_string(), Value::Array(Vec::from([Value::Number(serde_json::Number::from(self.category_id))]))),
                    ]))
                }
                StreamProperties::Episode(_episode) => {
                    Value::Object(serde_json::Map::from_iter([]))

                    //
                    // impl XtreamSeriesInfoEpisode {
                    //     pub fn get_additional_properties(&self, series_info: &XtreamSeriesInfo) -> Option<String> {
                    //         let mut result = serde_json::Map::new();
                    //         let info = series_info.info.as_ref();
                    //         let bdpath = info.and_then(|i| i.backdrop_path.as_ref());
                    //         let bdpath_is_set = bdpath.as_ref().is_some_and(|bdpath| !bdpath.is_empty());
                    //         if bdpath_is_set {
                    //             result.insert(String::from("backdrop_path"), Value::Array(Vec::from([Value::String(String::from(bdpath?.first()?))])));
                    //         }
                    //         add_str_property_if_exists!(result, info.map_or("", |i| i.name.as_str()), "series_name");
                    //         add_str_property_if_exists!(result, info.map_or("", |i| get_non_empty_str(i.release_date.as_str(), i.releaseDate.as_str(), i.releasedate.as_str())), "series_release_date");
                    //         add_str_property_if_exists!(result, self.added.as_str(), "added");
                    //         add_str_property_if_exists!(result, info.map_or("", |i| i.cast.as_str()), "cast");
                    //         add_str_property_if_exists!(result, self.container_extension.as_str(), "container_extension");
                    //         add_str_property_if_exists!(result, self.info.as_ref().map_or("", |info| info.movie_image.as_str()), "cover");
                    //         add_str_property_if_exists!(result, info.map_or("", |i| i.director.as_str()), "director");
                    //         add_str_property_if_exists!(result, info.map_or("", |i| i.episode_run_time.as_str()), "episode_run_time");
                    //         add_str_property_if_exists!(result, info.map_or("", |i| i.last_modified.as_str()), "last_modified");
                    //         add_str_property_if_exists!(result, self.info.as_ref().map_or("", |info| info.plot.as_str()), "plot");
                    //         add_f64_property_if_exists!(result, info.map_or(0_f64, |i| i.rating), "rating");
                    //         add_f64_property_if_exists!(result, info.map_or(0_f64, |i| i.rating_5based), "rating_5based");
                    //         add_str_property_if_exists!(result, self.info.as_ref().map_or("", |info| get_non_empty_str(info.release_date.as_str(), info.releaseDate.as_str(), info.releasedate.as_str())), "release_date");
                    //         add_str_property_if_exists!(result, self.title, "title");
                    //         add_i64_property_if_exists!(result, self.season, "season");
                    //         add_i64_property_if_exists!(result, self.episode_num, "episode");
                    //         let series_tmdb_id = info.and_then(|i| i.tmdb_id.or(i.tmdb));
                    //         add_opt_i64_property_if_exists!(result, self.info.as_ref().and_then(|info| info.tmdb_id.or(info.tmdb.or(series_tmdb_id))), "tmdb_id");
                    //
                    //         // Add the "info" section to the playlist item additional properties.
                    //         if let Some(episode_info) = &self.info {
                    //             if let Ok(info_value) = serde_json::to_value(episode_info) {
                    //                 result.insert("info".to_string(), info_value);
                    //             }
                    //         }
                    //
                    //         if result.is_empty() {
                    //             None
                    //         } else {
                    //             serde_json::to_string(&Value::Object(result)).ok()
                    //         }
                    //     }
                }
            }
        } else {
            match self.xtream_cluster {
                XtreamCluster::Live => {
                    let stream_icon = if !self.logo.is_empty() {
                        make_resource_url(resource_url.as_ref(), &self.logo, "logo")
                    } else if !self.logo_small.is_empty() {
                        make_resource_url(resource_url.as_ref(), &self.logo_small, "logo_small")
                    } else {
                        String::new()
                    };
                    Value::Object(serde_json::Map::from_iter([
                        ("num".to_string(), Value::Number(serde_json::Number::from(self.channel_no))),
                        ("name".to_string(), Value::String(self.title.clone())), // name or title ?
                        ("stream_id".to_string(), Value::Number(serde_json::Number::from(self.virtual_id))),
                        ("stream_icon".to_string(), Value::String(stream_icon)),
                        ("epg_channel_id".to_string(), Value::String(self.epg_channel_id.as_ref().map_or_else(String::new, Clone::clone))),
                        ("added".to_string(), Value::String(String::new())),
                        ("is_adult".to_string(), Value::Number(serde_json::Number::from(0u32))),
                        ("category_id".to_string(), Value::String(self.category_id.to_string())),
                        ("category_ids".to_string(), Value::Array(Vec::from([Value::Number(serde_json::Number::from(self.category_id))]))),
                        ("custom_sid".to_string(), Value::Null),
                        ("direct_source".to_string(), Value::String(String::new())),
                        ("tv_archive".to_string(), Value::Number(serde_json::Number::from(0u32))),
                        ("tv_archive_duration".to_string(), Value::Number(serde_json::Number::from(0u32))),
                    ]))
                }
                XtreamCluster::Video => {
                    let stream_icon = if !self.logo.is_empty() {
                        make_resource_url(resource_url.as_ref(), &self.logo, "logo")
                    } else if !self.logo_small.is_empty() {
                        make_resource_url(resource_url.as_ref(), &self.logo_small, "logo_small")
                    } else {
                        String::new()
                    };
                    Value::Object(serde_json::Map::from_iter([
                        ("num".to_string(), Value::Number(serde_json::Number::from(self.channel_no))),
                        ("name".to_string(), Value::String(self.title.clone())), // name or title ?
                        ("stream_type".to_string(), Value::String("movie".to_string())),
                        ("stream_id".to_string(), Value::Number(serde_json::Number::from(self.virtual_id))),
                        ("stream_icon".to_string(), Value::String(stream_icon)),
                        ("rating".to_string(), Value::String(String::new())),
                        ("rating_5based".to_string(), Value::String(String::new())),
                        ("tmdb".to_string(), Value::String(String::new())),
                        ("trailer".to_string(), Value::String(String::new())),
                        ("added".to_string(), Value::String(String::new())),
                        ("is_adult".to_string(), Value::Number(serde_json::Number::from(0u32))),
                        ("category_id".to_string(), Value::String(self.category_id.to_string())),
                        ("category_ids".to_string(), Value::Array(Vec::from([Value::Number(serde_json::Number::from(self.category_id))]))),
                        ("container_extension".to_string(), Value::String(String::new())),
                        ("custom_sid".to_string(), Value::Null),
                        ("direct_source".to_string(), Value::String(String::new())),
                    ]))
                }
                XtreamCluster::Series => {
                    let stream_icon = if !self.logo.is_empty() {
                        make_resource_url(resource_url.as_ref(), &self.logo, "logo")
                    } else if !self.logo_small.is_empty() {
                        make_resource_url(resource_url.as_ref(), &self.logo_small, "logo_small")
                    } else {
                        String::new()
                    };
                    Value::Object(serde_json::Map::from_iter([
                        ("num".to_string(), Value::Number(serde_json::Number::from(self.channel_no))),
                        ("name".to_string(), Value::String(self.title.clone())), // name or title ?
                        ("series_id".to_string(), Value::Number(serde_json::Number::from(self.virtual_id))),
                        ("cover".to_string(), Value::String(stream_icon)),
                        ("plot".to_string(), Value::String(String::new())),
                        ("cast".to_string(), Value::String(String::new())),
                        ("director".to_string(), Value::String(String::new())),
                        ("genre".to_string(), Value::String(String::new())),
                        ("release_date".to_string(), Value::String(String::new())),
                        ("releaseDate".to_string(), Value::String(String::new())),
                        ("last_modified".to_string(), Value::String(String::new())),
                        ("rating".to_string(), Value::String(String::new())),
                        ("rating_5based".to_string(), Value::String(String::new())),
                        ("backdrop_path".to_string(), Value::Array(Vec::new())),
                        ("youtube_trailer".to_string(), Value::String(String::new())),
                        ("tmdb".to_string(), Value::String(String::new())),
                        ("episode_runtime".to_string(), Value::String(String::new())),
                        ("category_id".to_string(), Value::String(self.category_id.to_string())),
                        ("category_ids".to_string(), Value::Array(Vec::from([Value::Number(serde_json::Number::from(self.category_id))]))),
                    ]))
                }
            }
        }
    }
}

impl PlaylistEntry for XtreamPlaylistItem {
    #[inline]
    fn get_virtual_id(&self) -> VirtualId {
        self.virtual_id
    }
    #[inline]
    fn get_provider_id(&self) -> Option<u32> {
        Some(self.provider_id)
    }
    #[inline]
    fn get_category_id(&self) -> Option<u32> {
        None
    }
    #[inline]
    fn get_provider_url(&self) -> String {
        self.url.to_string()
    }

    #[inline]
    fn get_uuid(&self) -> UUIDType {
        generate_playlist_uuid(&self.input_name, &self.provider_id.to_string(), self.item_type, &self.url)
    }
    #[inline]
    fn get_item_type(&self) -> PlaylistItemType {
        self.item_type
    }
}

pub fn get_backdrop_path_value<'a>(field: &'a str, value: Option<&'a Value>) -> Option<Cow<'a, str>> {
    match value {
        Some(Value::String(url)) => Some(Cow::Borrowed(url)),
        Some(Value::Array(values)) => {
            match values.as_slice() {
                [Value::String(single)] => Some(Cow::Borrowed(single)),
                multiple if !multiple.is_empty() => {
                    if let Some(index) = field.rfind('_') {
                        if let Ok(bd_index) = field[index + 1..].parse::<usize>() {
                            if let Some(Value::String(selected)) = multiple.get(bd_index) {
                                return Some(Cow::Borrowed(selected));
                            }
                        }
                    }
                    if let Value::String(url) = &multiple[0] {
                        Some(Cow::Borrowed(url))
                    } else {
                        None
                    }
                }
                _ => None,
            }
        }
        _ => None,
    }
}

macro_rules! generate_field_accessor_impl_for_xtream_playlist_item {
    ($($prop:ident),*;) => {
        impl crate::model::FieldGetAccessor for crate::model::XtreamPlaylistItem {
            fn get_field(&self, field: &str) -> Option<Cow<'_, str>> {
                let field = field.to_lowercase();
                match field.as_str() {
                    $(
                        stringify!($prop) => Some(Cow::Borrowed(&self.$prop)),
                    )*
                    "caption" =>  Some(if self.title.is_empty() { Cow::Borrowed(&self.name) } else { Cow::Borrowed(&self.title) }),
                    "epg_channel_id" | "epg_id" => self.epg_channel_id.as_ref().map(|s| Cow::Borrowed(s.as_str())),
                    _ => {
                        if field.starts_with(xtream_const::XC_PROP_BACKDROP_PATH)
                            || field == xtream_const::XC_PROP_COVER
                        {
                            match self.additional_properties.as_ref() {
                                Some(additional_properties) => match additional_properties {
                                    StreamProperties::Live(_) => None,
                                    StreamProperties::Video(video) => {
                                        if field == xtream_const::XC_PROP_COVER {
                                            video.details.as_ref().and_then(|details| {
                                                details.cover_big.as_ref()
                                                    .or(details.movie_image.as_ref())
                                                    .or_else(|| details.backdrop_path.as_ref().and_then(|p| p.first()))
                                                    .map(|s| Cow::<str>::Borrowed(s))
                                            })
                                        } else {
                                            None
                                        }
                                    }
                                    StreamProperties::Series(series) => {
                                        if field == xtream_const::XC_PROP_COVER {
                                            series.backdrop_path.as_ref().and_then(|p| p.first()).map(|s| Cow::<str>::Borrowed(s))
                                        } else {
                                            None
                                        }
                                    }
                                    StreamProperties::Episode(episode) => {
                                        if field == xtream_const::XC_PROP_COVER {
                                            Some(Cow::<str>::Borrowed(&episode.movie_image))
                                        } else {
                                            None
                                        }
                                    }
                                },
                                None => None,
                            }
                        } else {
                            None
                        }
                    }

                }
            }
        }
    }
}

impl From<XtreamPlaylistItem> for CommonPlaylistItem {
    fn from(item: XtreamPlaylistItem) -> Self {
        item.to_common()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaylistItem {
    #[serde(flatten)]
    pub header: PlaylistItemHeader,
}

generate_field_accessor_impl_for_xtream_playlist_item!(name, logo, logo_small, group, title, parent_code, rec, url;);

impl PlaylistItem {
    pub fn to_m3u(&self) -> M3uPlaylistItem {
        let header = &self.header;
        M3uPlaylistItem {
            virtual_id: header.virtual_id,
            provider_id: header.id.to_string(),
            name: if header.item_type == PlaylistItemType::Series { &header.title } else { &header.name }.to_string(),
            chno: header.chno,
            logo: header.logo.to_string(),
            logo_small: header.logo_small.to_string(),
            group: header.group.to_string(),
            title: header.title.to_string(),
            parent_code: header.parent_code.to_string(),
            audio_track: header.audio_track.to_string(),
            time_shift: header.time_shift.to_string(),
            rec: header.rec.to_string(),
            url: header.url.to_string(),
            epg_channel_id: header.epg_channel_id.clone(),
            input_name: header.input_name.to_string(),
            item_type: header.item_type,
            t_stream_url: header.url.to_string(),
            t_resource_url: None,
        }
    }

    pub fn to_xtream(&self) -> XtreamPlaylistItem {
        let header = &self.header;
        let provider_id = header.id.parse::<u32>().unwrap_or_default();
        let additional_properties = Self::get_additional_properties(header);

        XtreamPlaylistItem {
            virtual_id: header.virtual_id,
            provider_id,
            name: if header.item_type == PlaylistItemType::Series { &header.title } else { &header.name }.to_string(),
            logo: header.logo.to_string(),
            logo_small: header.logo_small.to_string(),
            group: header.group.to_string(),
            title: header.title.to_string(),
            parent_code: header.parent_code.to_string(),
            rec: header.rec.to_string(),
            url: header.url.to_string(),
            epg_channel_id: header.epg_channel_id.clone(),
            xtream_cluster: header.xtream_cluster,
            additional_properties,
            item_type: header.item_type,
            category_id: header.category_id,
            input_name: header.input_name.to_string(),
            channel_no: header.chno,
        }
    }

    pub fn to_common(&self) -> CommonPlaylistItem {
        let header = &self.header;

        let additional_properties = Self::get_additional_properties(header);

        CommonPlaylistItem {
            virtual_id: header.virtual_id,
            provider_id: header.id.clone(),
            name: if header.item_type == PlaylistItemType::Series { &header.title } else { &header.name }.clone(),
            logo: header.logo.clone(),
            logo_small: header.logo_small.clone(),
            group: header.group.clone(),
            title: header.title.clone(),
            parent_code: header.parent_code.clone(),
            audio_track: header.audio_track.clone(),
            time_shift: header.time_shift.clone(),
            rec: header.rec.clone(),
            url: header.url.clone(),
            epg_channel_id: header.epg_channel_id.clone(),
            xtream_cluster: Some(header.xtream_cluster),
            additional_properties,
            item_type: header.item_type,
            category_id: Some(header.category_id),
            input_name: header.input_name.clone(),
            chno: header.chno,
        }
    }


    fn get_additional_properties(header: &PlaylistItemHeader) -> Option<StreamProperties> {
        match &header.additional_properties {
            Some(props) => Some(props.clone()),
            None => {
                match header.xtream_cluster {
                    XtreamCluster::Live => None,
                    XtreamCluster::Video => {
                        let container_extension = extract_extension_from_url(&header.url).map(|e| e.strip_prefix('.').unwrap_or(e).to_string()).unwrap_or_default();
                        Some(StreamProperties::Video(Box::new(VideoStreamProperties {
                            name: header.name.clone(),
                            category_id: header.category_id,
                            stream_id: header.virtual_id,
                            stream_icon: "".to_string(),
                            direct_source: "".to_string(),
                            custom_sid: None,
                            added: String::new(),
                            container_extension,
                            rating: None,
                            rating_5based: None,
                            stream_type: "movie".to_string(),
                            trailer: None,
                            tmdb: None,
                            is_adult: 0,
                            details: None,
                        })))
                    }
                    XtreamCluster::Series => {
                        if header.item_type == PlaylistItemType::Series {
                            let container_extension = extract_extension_from_url(&header.url).map(|e| e.strip_prefix('.').unwrap_or(e).to_string()).unwrap_or_default();
                            // TODO maybe from link ? like s01e02 or something like this
                            Some(StreamProperties::Episode(EpisodeStreamProperties {
                                episode_id: 0,
                                episode: 0,
                                season: 0,
                                added: None,
                                release_date: None,
                                tmdb: None,
                                movie_image: String::new(),
                                container_extension,
                                audio: None,
                                video: None,
                            }))
                        } else if header.item_type == PlaylistItemType::SeriesInfo {
                            Some(StreamProperties::Series(Box::new(SeriesStreamProperties {
                                name: header.name.clone(),
                                category_id: header.category_id,
                                tmdb: None,
                                series_id: 0,
                                backdrop_path: None,
                                cast: String::new(),
                                cover: String::new(),
                                director: String::new(),
                                episode_run_time: None,
                                genre: None,
                                last_modified: None,
                                plot: None,
                                rating: 0.0,
                                rating_5based: 0.0,
                                release_date: None,
                                youtube_trailer: String::new(),
                                details: None,
                            })))
                        } else {
                            None
                        }
                    }
                }
            }
        }
    }

    pub fn to_info_document(&self) -> serde_json::Value {
        Value::Null
    }
}

impl PlaylistEntry for PlaylistItem {
    #[inline]
    fn get_virtual_id(&self) -> VirtualId {
        self.header.virtual_id
    }

    fn get_provider_id(&self) -> Option<u32> {
        let header = &self.header;
        get_provider_id(&header.id, &header.url)
    }

    #[inline]
    fn get_category_id(&self) -> Option<u32> {
        None
    }

    #[inline]
    fn get_provider_url(&self) -> String {
        self.header.url.to_string()
    }
    #[inline]
    fn get_uuid(&self) -> UUIDType {
        let header = &self.header;
        generate_playlist_uuid(&header.input_name, &header.id, header.item_type, &header.url)
    }

    #[inline]
    fn get_item_type(&self) -> PlaylistItemType {
        self.header.item_type
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaylistGroup {
    pub id: u32,
    pub title: String,
    pub channels: Vec<PlaylistItem>,
    #[serde(skip)]
    pub xtream_cluster: XtreamCluster,
}

impl PlaylistGroup {
    #[inline]
    pub fn on_load(&mut self) {
        for pl in &mut self.channels {
            pl.header.gen_uuid();
        }
    }

    #[inline]
    pub fn filter_count<F>(&self, filter: F) -> usize
    where
        F: Fn(&PlaylistItem) -> bool,
    {
        self.channels.iter().filter(|&c| filter(c)).count()
    }
}
