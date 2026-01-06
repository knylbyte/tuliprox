use crate::utils::deserialize_as_option_string;
use crate::utils::serialize_option_string_as_null_if_empty;
use crate::model::info_doc_utils::InfoDocUtils;
use crate::model::{
    LiveStreamProperties, SeriesStreamProperties, StreamProperties, VideoStreamProperties,
    XtreamCluster, XtreamEmptyDoc, XtreamInfoDocument, XtreamMappingOptions, XtreamPlaylistItem,
    XtreamSeriesInfoData, XtreamSeriesInfoDoc, XtreamVideoInfoData,
    XtreamVideoInfoDoc, XtreamVideoMovieData,
};
use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

fn default_as_live() -> String {
    "live".to_string()
}

fn default_as_movie() -> String {
    "movie".to_string()
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct XtreamLiveDoc {
    pub num: u32,
    pub name: String,
    pub stream_type: String,
    pub stream_id: u32,
    pub stream_icon: String,
    pub epg_channel_id: String,
    pub added: String,
    pub is_adult: i32,
    pub category_id: String,
    pub category_ids: Vec<u32>,
    #[serde(default, deserialize_with = "deserialize_as_option_string", serialize_with = "serialize_option_string_as_null_if_empty")]
    pub custom_sid: Option<String>,
    pub tv_archive: i32,
    pub direct_source: String,
    pub tv_archive_duration: i32,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct XtreamVideoDoc {
    pub num: u32,
    pub name: String,
    pub stream_type: String,
    pub stream_id: u32,
    pub stream_icon: String,
    pub rating: String,
    pub rating_5based: f64,
    pub tmdb: String,
    pub trailer: String,
    pub added: String,
    pub is_adult: i32,
    pub category_id: String,
    pub category_ids: Vec<u32>,
    pub container_extension: String,
    #[serde(default, deserialize_with = "deserialize_as_option_string", serialize_with = "serialize_option_string_as_null_if_empty")]
    pub custom_sid: Option<String>,
    pub direct_source: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct XtreamSeriesDoc {
    pub num: u32,
    pub name: String,
    pub series_id: u32,
    pub cover: String,
    pub plot: String,
    pub cast: String,
    pub director: String,
    pub genre: String,
    #[serde(rename = "releaseDate")]
    pub release_date_alternate: String,
    pub release_date: String,
    pub last_modified: String,
    pub rating: String,
    pub rating_5based: String,
    pub backdrop_path: Vec<String>,
    pub youtube_trailer: String,
    pub tmdb: String,
    pub episode_runtime: String,
    pub category_id: String,
    pub category_ids: Vec<u32>,
}

impl XtreamPlaylistItem {
    pub fn to_info_document(&self, options: &XtreamMappingOptions) -> XtreamInfoDocument {
        if self.has_details() {
            if let Some(doc) = self.additional_properties.as_ref() {
                return doc.to_info_document(options, self.item_type, self.virtual_id, self.category_id);
            }
        }
        let resource_url = options.get_resource_url(self.xtream_cluster, self.item_type, self.virtual_id);
        self.to_info_document_no_props(resource_url)
    }

    fn to_info_document_no_props(&self, resource_url: Option<String>) -> XtreamInfoDocument {
        let stream_icon = self.get_stream_icon(resource_url);
        match self.xtream_cluster {
            XtreamCluster::Live => XtreamInfoDocument::Empty(XtreamEmptyDoc {}),
            XtreamCluster::Video => {
                XtreamInfoDocument::Video(XtreamVideoInfoDoc {
                    info: XtreamVideoInfoData {
                        kinopoisk_url: String::new(),
                        tmdb_id: String::new(),
                        name: self.title.clone(),
                        o_name: self.name.clone(),
                        cover_big: stream_icon.clone(),
                        movie_image: stream_icon.clone(),
                        release_date: String::new(),
                        episode_run_time: 0,
                        youtube_trailer: String::new(),
                        director: String::new(),
                        actors: String::new(),
                        cast: String::new(),
                        description: String::new(),
                        plot: String::new(),
                        age: String::new(),
                        mpaa_rating: String::new(),
                        rating_count_kinopoisk: 0,
                        country: String::new(),
                        genre: String::new(),
                        backdrop_path: vec![stream_icon.clone()],
                        duration_secs: String::new(),
                        duration: String::new(),
                        video: Value::Array(Vec::new()),
                        audio: Value::Array(Vec::new()),
                        bitrate: 0,
                        rating: String::new(),
                        runtime: String::new(),
                        status: "Released".to_string(),
                    },
                    movie_data: XtreamVideoMovieData {
                        stream_id: self.virtual_id,
                        name: self.name.clone(),
                        added: String::new(),
                        category_id: self.category_id.to_string(),
                        category_ids: vec![self.category_id],
                        container_extension: String::new(),
                        custom_sid: None,
                        direct_source: String::new(),
                    },
                })
            }
            XtreamCluster::Series => {
                XtreamInfoDocument::Series(XtreamSeriesInfoDoc {
                    seasons: Vec::new(),
                    info: XtreamSeriesInfoData {
                        name: self.title.clone(),
                        cover: stream_icon.clone(),
                        plot: String::new(),
                        cast: String::new(),
                        director: String::new(),
                        genre: String::new(),
                        release_date_alternate: String::new(),
                        release_date: String::new(),
                        last_modified: String::new(),
                        rating: String::new(),
                        rating_5based: String::new(),
                        backdrop_path: if stream_icon.is_empty() {vec![] } else { vec![stream_icon] },
                        tmdb: String::new(),
                        youtube_trailer: String::new(),
                        episode_run_time: String::new(),
                        category_id: self.category_id.to_string(),
                        category_ids: vec![self.category_id],
                    },
                    episodes: HashMap::new(),
                })
            }
        }
    }

    pub fn to_document(&self, options: &XtreamMappingOptions) -> XtreamDocument {
        if let Some(props) = self.additional_properties.as_ref() {
            match props {
                StreamProperties::Live(live) => self.live_to_document(options, live),
                StreamProperties::Video(video) => self.video_to_document(options, video),
                StreamProperties::Series(series) => self.series_to_document(options, series),
                StreamProperties::Episode(_episode) => XtreamDocument::Episode(XtreamEmptyDoc::default()),
            }
        } else {
            let resource_url = options.get_resource_url(self.xtream_cluster, self.item_type, self.virtual_id);
            self.to_document_no_props(resource_url)
        }
    }

    fn series_to_document(&self, options: &XtreamMappingOptions, series: &SeriesStreamProperties) -> XtreamDocument {
        let resource_url = options.get_resource_url(self.xtream_cluster, self.item_type, self.virtual_id);

        XtreamDocument::Series(XtreamSeriesDoc {
            num: self.channel_no,
            name: self.title.clone(),
            series_id: self.virtual_id,
            cover: InfoDocUtils::make_resource_url(resource_url.as_deref(), &series.cover, "cover"),
            plot: series.plot.clone().unwrap_or_default(),
            cast: series.cast.clone(),
            director: series.director.clone(),
            genre: series.genre.clone().unwrap_or_default(),
            release_date: series.release_date.clone().unwrap_or_default(),
            release_date_alternate: series.release_date.clone().unwrap_or_default(),
            last_modified: series.last_modified.clone().unwrap_or_default(),
            rating: InfoDocUtils::limited(series.rating),
            rating_5based: InfoDocUtils::limited(series.rating_5based),
            backdrop_path: series.backdrop_path.as_ref().map_or_else(
                || {
                    let res_url = InfoDocUtils::make_resource_url(resource_url.as_deref(), &series.cover, "cover");
                    if res_url.is_empty() { vec![] } else { vec![res_url] }
                },
                |b| b.iter().enumerate().map(|(idx, p)|
                    InfoDocUtils::make_bdpath_resource_url(resource_url.as_deref(), p, idx, "")
                ).collect()),
            youtube_trailer: series.youtube_trailer.clone(),
            tmdb: series.tmdb.map(|v| v.to_string()).unwrap_or_default(),
            episode_runtime: series.episode_run_time.clone().unwrap_or_default(),
            category_id: self.category_id.to_string(),
            category_ids: vec![self.category_id],
        })
    }

    fn video_to_document(&self, options: &XtreamMappingOptions, video: &VideoStreamProperties) -> XtreamDocument {
        let resource_url = options.get_resource_url(self.xtream_cluster, self.item_type, self.virtual_id);
        let stream_icon = self.get_stream_icon(resource_url);
        XtreamDocument::Video(XtreamVideoDoc {
            num: self.channel_no,
            name: self.title.clone(),
            stream_type: video.stream_type.clone().unwrap_or_else(default_as_movie),
            stream_id: self.virtual_id,
            stream_icon,
            rating: video.rating.map(InfoDocUtils::limited).unwrap_or_default(),
            rating_5based: video.rating_5based.unwrap_or_default(),
            tmdb: video.tmdb.map(|v| v.to_string()).unwrap_or_default(),
            trailer: video.trailer.clone().unwrap_or_default(),
            added: video.added.clone(),
            is_adult: video.is_adult,
            category_id: self.category_id.to_string(),
            category_ids: vec![self.category_id],
            container_extension: video.container_extension.clone(),
            custom_sid: video.custom_sid.clone(),
            direct_source: if options.skip_video_direct_source { String::new() } else { video.direct_source.clone() },
        })
    }

    fn live_to_document(&self, options: &XtreamMappingOptions, live: &LiveStreamProperties) -> XtreamDocument {
        let resource_url = options.get_resource_url(self.xtream_cluster, self.item_type, self.virtual_id);
        let stream_icon = self.get_stream_icon(resource_url);
        XtreamDocument::Live(XtreamLiveDoc {
            num: self.channel_no,
            name: self.title.clone(),
            stream_type: live.stream_type.clone().unwrap_or_else(default_as_live),
            stream_id: self.virtual_id,
            stream_icon,
            epg_channel_id: self.epg_channel_id.clone().unwrap_or_default(),
            added: live.added.clone().unwrap_or_default(),
            is_adult: live.is_adult,
            category_id: self.category_id.to_string(),
            category_ids: vec![self.category_id],
            custom_sid: live.custom_sid.clone(),
            tv_archive: live.tv_archive.unwrap_or_default(),
            direct_source: if options.skip_live_direct_source { String::new() } else { live.direct_source.clone() },
            tv_archive_duration: live.tv_archive_duration.unwrap_or_default(),
        })
    }

    fn to_document_no_props(&self, resource_url: Option<String>) -> XtreamDocument {
        let stream_icon = self.get_stream_icon(resource_url);
        match self.xtream_cluster {
            XtreamCluster::Live => {
                XtreamDocument::Live(XtreamLiveDoc {
                    num: self.channel_no,
                    name: self.title.clone(),
                    stream_type: default_as_live(),
                    stream_id: self.virtual_id,
                    stream_icon,
                    epg_channel_id: self.epg_channel_id.clone().unwrap_or_default(),
                    added: String::new(),
                    is_adult: 0,
                    category_id: self.category_id.to_string(),
                    category_ids: vec![self.category_id],
                    custom_sid: None,
                    tv_archive: 0,
                    direct_source: String::new(),
                    tv_archive_duration: 0,
                })
            }
            XtreamCluster::Video => {
                XtreamDocument::Video(XtreamVideoDoc {
                    num: self.channel_no,
                    name: self.title.clone(),
                    stream_type: default_as_movie(),
                    stream_id: self.virtual_id,
                    stream_icon,
                    rating: "0".to_string(),
                    rating_5based: 0.0,
                    tmdb: String::new(),
                    trailer: String::new(),
                    added: String::new(),
                    is_adult: 0,
                    category_id: self.category_id.to_string(),
                    category_ids: vec![self.category_id],
                    container_extension: String::new(),
                    custom_sid: None,
                    direct_source: String::new(),
                })
            }
            XtreamCluster::Series => {
                XtreamDocument::Series(XtreamSeriesDoc {
                    num: self.channel_no,
                    name: self.title.clone(),
                    series_id: self.virtual_id,
                    cover: stream_icon.clone(),
                    plot: String::new(),
                    cast: String::new(),
                    director: String::new(),
                    genre: String::new(),
                    release_date: String::new(),
                    release_date_alternate: String::new(),
                    last_modified: String::new(),
                    rating: "0".to_string(),
                    rating_5based: "0".to_string(),
                    backdrop_path: if stream_icon.is_empty() { vec![] } else { vec![stream_icon] },
                    youtube_trailer: String::new(),
                    tmdb: String::new(),
                    episode_runtime: "0".to_string(),
                    category_id: self.category_id.to_string(),
                    category_ids: vec![self.category_id],
                })
            }
        }
    }

    fn get_stream_icon(&self, resource_url: Option<String>) -> String {
        if !self.logo.is_empty() {
            InfoDocUtils::make_resource_url(resource_url.as_deref(), &self.logo, "logo")
        } else if !self.logo_small.is_empty() {
            InfoDocUtils::make_resource_url(resource_url.as_deref(), &self.logo_small, "logo_small")
        } else {
            String::new()
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(untagged)]
pub enum XtreamDocument {
    Live(XtreamLiveDoc),
    Video(XtreamVideoDoc),
    Series(XtreamSeriesDoc),
    Episode(XtreamEmptyDoc),
}

