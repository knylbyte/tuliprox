use crate::model::{PlaylistEntry, PlaylistItem, XtreamSeriesInfo, XtreamVideoInfo};
use crate::utils::{deserialize_as_option_string, deserialize_as_string, deserialize_as_string_array,
                   deserialize_json_as_string, deserialize_number_from_string,
                   deserialize_number_from_string_or_zero, string_default_on_null,
                   string_or_number_u32};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct LiveStreamProperties {
    #[serde(default)]
    pub name: String,
    #[serde(default, deserialize_with = "deserialize_number_from_string_or_zero")]
    pub category_id: u32,
    #[serde(default, deserialize_with = "deserialize_number_from_string_or_zero")]
    pub stream_id: u32,
    #[serde(default)]
    pub stream_icon: String,
    #[serde(default)]
    pub direct_source: String,
    #[serde(default, deserialize_with = "deserialize_as_option_string")]
    pub custom_sid: Option<String>,
    #[serde(default, deserialize_with = "deserialize_as_option_string")]
    pub added: Option<String>,
    #[serde(default, deserialize_with = "deserialize_as_option_string")]
    pub stream_type: Option<String>,
    #[serde(default, deserialize_with = "deserialize_as_option_string")]
    pub epg_channel_id: Option<String>,
    #[serde(default, deserialize_with = "deserialize_number_from_string")]
    pub tv_archive: Option<i32>,
    #[serde(default, deserialize_with = "deserialize_number_from_string")]
    pub tv_archive_duration: Option<i32>,
    #[serde(default, deserialize_with = "deserialize_number_from_string_or_zero")]
    pub is_adult: i32,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct VideoStreamDetailProperties {
    pub kinopoisk_url: Option<String>,
    pub o_name: Option<String>,
    pub cover_big: Option<String>,
    pub movie_image: Option<String>,
    pub release_date: Option<String>,
    #[serde(default, deserialize_with = "deserialize_number_from_string")]
    pub episode_run_time: Option<u32>,
    pub youtube_trailer: Option<String>,
    pub director: Option<String>,
    pub actors: Option<String>,
    pub cast: Option<String>,
    pub description: Option<String>,
    pub plot: Option<String>,
    pub age: Option<String>,
    pub mpaa_rating: Option<String>,
    #[serde(default)]
    pub rating_count_kinopoisk: u32,
    pub country: Option<String>,
    pub genre: Option<String>,
    #[serde(default, deserialize_with = "deserialize_as_string_array")]
    pub backdrop_path: Option<Vec<String>>,
    pub duration_secs: Option<String>,
    pub duration: Option<String>,
    #[serde(default, deserialize_with = "deserialize_json_as_string")]
    pub video: Option<String>,
    #[serde(default, deserialize_with = "deserialize_json_as_string")]
    pub audio: Option<String>,
    #[serde(default)]
    pub bitrate: u32,
    pub runtime: Option<String>,
    pub status: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct VideoStreamProperties {
    #[serde(default, deserialize_with = "deserialize_as_string")]
    pub name: String,
    #[serde(default, deserialize_with = "deserialize_number_from_string_or_zero")]
    pub category_id: u32,
    #[serde(default, deserialize_with = "deserialize_number_from_string_or_zero")]
    pub stream_id: u32,
    #[serde(default)]
    pub stream_icon: String,
    #[serde(default)]
    pub direct_source: String,
    #[serde(default, deserialize_with = "deserialize_as_option_string")]
    pub custom_sid: Option<String>,
    #[serde(default)]
    pub added: String,
    #[serde(default)]
    pub container_extension: String,
    #[serde(default, deserialize_with = "deserialize_number_from_string")]
    pub rating: Option<f64>,
    #[serde(default, deserialize_with = "deserialize_number_from_string")]
    pub rating_5based: Option<f64>,
    #[serde(default)]
    pub stream_type: String,
    #[serde(default)]
    pub trailer: Option<String>,
    #[serde(default, deserialize_with = "deserialize_number_from_string")]
    pub tmdb: Option<u32>,
    #[serde(default, deserialize_with = "deserialize_number_from_string_or_zero")]
    pub is_adult: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<VideoStreamDetailProperties>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct SeriesStreamDetailEpisodeProperties {
    #[serde(default, deserialize_with = "string_or_number_u32")]
    pub id: u32,
    #[serde(default, deserialize_with = "string_or_number_u32")]
    pub episode_num: u32,
    #[serde(default, deserialize_with = "string_or_number_u32")]
    pub season: u32,
    #[serde(default, deserialize_with = "string_default_on_null")]
    pub title: String,
    #[serde(default, deserialize_with = "string_default_on_null")]
    pub container_extension: String,
    #[serde(default, deserialize_with = "string_default_on_null")]
    pub custom_sid: String,
    #[serde(default, deserialize_with = "string_default_on_null")]
    pub added: String,
    #[serde(default, deserialize_with = "string_default_on_null")]
    pub direct_source: String,
    #[serde(default, deserialize_with = "deserialize_number_from_string")]
    pub tmdb: Option<u32>,
    #[serde(default, deserialize_with = "string_default_on_null")]
    pub release_date: String,
    #[serde(default, deserialize_with = "deserialize_as_option_string")]
    pub plot: Option<String>,
    #[serde(default, deserialize_with = "string_or_number_u32")]
    pub duration_secs: u32,
    #[serde(default, deserialize_with = "string_default_on_null")]
    pub duration: String,
    #[serde(default, deserialize_with = "string_default_on_null")]
    pub movie_image: String,
    #[serde(default, deserialize_with = "string_or_number_u32")]
    pub bitrate: u32,
    #[serde(default, deserialize_with = "deserialize_number_from_string")]
    pub rating: Option<f64>,
    #[serde(default, deserialize_with = "deserialize_json_as_string")]
    pub video: Option<String>,
    #[serde(default, deserialize_with = "deserialize_json_as_string")]
    pub audio: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct SeriesStreamDetailProperties {
    #[serde(default, deserialize_with = "deserialize_number_from_string")]
    pub year: Option<u32>,
    pub episodes: Option<Vec<SeriesStreamDetailEpisodeProperties>>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct SeriesStreamProperties {
    #[serde(default, deserialize_with = "deserialize_as_string")]
    pub name: String,
    #[serde(default, deserialize_with = "deserialize_number_from_string_or_zero")]
    pub category_id: u32,
    #[serde(default, deserialize_with = "deserialize_number_from_string_or_zero")]
    pub series_id: u32,
    #[serde(default, deserialize_with = "deserialize_as_string_array")]
    pub backdrop_path: Option<Vec<String>>,
    #[serde(default)]
    pub cast: String,
    #[serde(default)]
    pub cover: String,
    #[serde(default)]
    pub director: String,
    #[serde(default, deserialize_with = "deserialize_as_option_string")]
    pub episode_run_time: Option<String>,
    #[serde(default, deserialize_with = "deserialize_as_option_string")]
    pub genre: Option<String>,
    #[serde(default, deserialize_with = "deserialize_as_option_string")]
    pub last_modified: Option<String>,
    #[serde(default, deserialize_with = "deserialize_as_option_string")]
    pub plot: Option<String>,
    #[serde(default, deserialize_with = "deserialize_number_from_string_or_zero")]
    pub rating: f64,
    #[serde(default, deserialize_with = "deserialize_number_from_string_or_zero")]
    pub rating_5based: f64,
    pub release_date: Option<String>,
    #[serde(default)]
    pub youtube_trailer: String,
    #[serde(default, deserialize_with = "deserialize_number_from_string")]
    pub tmdb: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<SeriesStreamDetailProperties>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct EpisodeStreamProperties {
    pub episode_id: u32,
    pub episode: u32,
    pub season: u32,
    #[serde(default, deserialize_with = "deserialize_as_option_string")]
    pub added: Option<String>,
    #[serde(default, deserialize_with = "deserialize_as_option_string")]
    pub release_date: Option<String>,
    #[serde(default, deserialize_with = "deserialize_number_from_string")]
    pub tmdb: Option<u32>,
    #[serde(default)]
    pub movie_image: String,
    #[serde(default, deserialize_with = "string_default_on_null")]
    pub container_extension: String,
    #[serde(default, deserialize_with = "deserialize_json_as_string")]
    pub video: Option<String>,
    #[serde(default, deserialize_with = "deserialize_json_as_string")]
    pub audio: Option<String>,
}


#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum StreamProperties {
    Live(LiveStreamProperties),
    Video(Box<VideoStreamProperties>),
    Series(Box<SeriesStreamProperties>),
    Episode(EpisodeStreamProperties),
}

impl StreamProperties {
    pub fn prepare(&mut self) {
        match self {
            StreamProperties::Live(live) => {
                live.epg_channel_id = live.epg_channel_id.as_ref()
                    .filter(|epg_id| !epg_id.trim().is_empty())
                    .map(|epg_id| epg_id.to_lowercase())
                    .or(None);
            }
            StreamProperties::Video(_) => {}
            StreamProperties::Series(_) => {}
            StreamProperties::Episode(_) => {}
        }
    }

    pub fn get_category_id(&self) -> u32 {
        match self {
            StreamProperties::Live(live) => live.category_id,
            StreamProperties::Video(video) => video.category_id,
            StreamProperties::Series(series) => series.category_id,
            StreamProperties::Episode(_episode) => 0,
        }
    }

    pub fn get_stream_id(&self) -> u32 {
        match self {
            StreamProperties::Live(live) => live.stream_id,
            StreamProperties::Video(video) => video.stream_id,
            StreamProperties::Series(series) => series.series_id,
            StreamProperties::Episode(episode) => episode.episode_id,
        }
    }

    pub fn get_stream_icon(&self) -> Cow<'_, str> {
        match self {
            StreamProperties::Live(live) => Cow::Borrowed(&live.stream_icon),
            StreamProperties::Video(video) => Cow::Borrowed(&video.stream_icon),
            StreamProperties::Series(series) => Cow::Borrowed(&series.cover),
            StreamProperties::Episode(episode) => Cow::Borrowed(&episode.movie_image),
        }
    }

    pub fn get_name(&self) -> Cow<'_, str> {
        match self {
            StreamProperties::Live(live) => Cow::Borrowed(&live.name),
            StreamProperties::Video(video) => Cow::Borrowed(&video.name),
            StreamProperties::Series(series) => Cow::Borrowed(&series.name),
            StreamProperties::Episode(_episode) => Cow::Borrowed(""),
        }
    }

    pub fn get_epg_channel_id(&self) -> Option<String> {
        match self {
            StreamProperties::Live(live) => live.epg_channel_id.clone(),
            StreamProperties::Video(_) => None,
            StreamProperties::Series(_) => None,
            StreamProperties::Episode(_) => None,
        }
    }

    pub fn get_tmdb_id(&self) -> Option<u32> {
        match self {
            StreamProperties::Live(_) => None,
            StreamProperties::Video(video) => video.tmdb,
            StreamProperties::Series(series) => series.tmdb,
            StreamProperties::Episode(epsiode) => epsiode.tmdb,
        }
    }

    pub fn get_release_date(&self) -> Option<String> {
        match self {
            StreamProperties::Live(_) => None,
            StreamProperties::Video(video) => video.details.as_ref().and_then(|d| d.release_date.clone()),
            StreamProperties::Series(series) => series.release_date.clone(),
            StreamProperties::Episode(episode) => episode.release_date.clone(),
        }
    }

    pub fn get_added(&self) -> Option<String> {
        match self {
            StreamProperties::Live(_) => None,
            StreamProperties::Video(video) => Some(video.added.clone()),
            StreamProperties::Series(series) => series.details.as_ref()
                .and_then(|d| d.episodes.as_ref())
                .and_then(|e| e.first().map(|e| e.added.clone())),
            StreamProperties::Episode(episode) => episode.added.clone(),
        }
    }

    pub fn get_container_extension(&self) -> Option<String> {
        match self {
            StreamProperties::Live(_) => None,
            StreamProperties::Video(video) => if video.container_extension.is_empty() { None } else { Some(video.container_extension.clone()) },
            StreamProperties::Series(series) => series.details.as_ref()
                .and_then(|d| d.episodes.as_ref())
                .and_then(|e| e.first().and_then(|e| {
                    if e.container_extension.is_empty() { None } else { Some(e.container_extension.clone()) }
                })),
            StreamProperties::Episode(episode) => if episode.container_extension.is_empty() { None } else { Some(episode.container_extension.clone()) },
        }
    }

    pub fn get_direct_source(&self) -> Option<String> {
        match self {
            StreamProperties::Live(_) => None,
            StreamProperties::Video(video) => if video.direct_source.is_empty() { None } else { Some(video.direct_source.clone()) },
            StreamProperties::Series(series) => series.details.as_ref()
                .and_then(|d| d.episodes.as_ref())
                .and_then(|e| e.first().and_then(|e| {
                    if e.direct_source.is_empty() { None } else { Some(e.direct_source.clone()) }
                })),
            StreamProperties::Episode(_episode) => None,
        }
    }

    pub fn get_season(&self) -> Option<u32> {
        match self {
            StreamProperties::Live(_)
            | StreamProperties::Video(_)
            | StreamProperties::Series(_) => None,
            StreamProperties::Episode(episode) => Some(episode.season),
        }
    }

    pub fn get_episode(&self) -> Option<u32> {
        match self {
            StreamProperties::Live(_)
            | StreamProperties::Video(_)
            | StreamProperties::Series(_) => None,
            StreamProperties::Episode(episode) => Some(episode.episode),
        }
    }

    pub fn get_last_modified(&self) -> Option<u64> {
        match self {
            StreamProperties::Live(_) => None,
            StreamProperties::Video(video) => video.added.parse::<u64>().ok(),
            StreamProperties::Series(series) => series.last_modified.as_ref().and_then(|v| v.parse::<u64>().ok()),
            StreamProperties::Episode(episode) => episode.added.as_ref().and_then(|v| v.parse::<u64>().ok()),
        }
    }
}

impl VideoStreamProperties {
    pub fn from_info(info: &XtreamVideoInfo, pli: &PlaylistItem) -> VideoStreamProperties {
        let mut props = VideoStreamProperties {
            name: info.info.name.clone(),
            category_id: info.movie_data.category_id,
            stream_id: info.movie_data.stream_id,
            stream_icon: info.info.movie_image.as_ref().map_or_else(String::new, Clone::clone),
            direct_source: info.movie_data.direct_source.clone(),
            custom_sid: info.movie_data.custom_sid.clone(),
            added: info.movie_data.added.clone(),
            container_extension: info.movie_data.container_extension.clone(),
            rating: None, // from PlaylistItem
            rating_5based: None, // from PlaylistItem
            stream_type: "movie".to_string(),
            trailer: info.info.youtube_trailer.clone(),
            tmdb: info.info.tmdb_id.parse::<u32>().ok(),
            is_adult: 0,  // from PlaylistItem
            details: Some(VideoStreamDetailProperties {
                kinopoisk_url: info.info.kinopoisk_url.clone(),
                o_name: info.info.o_name.clone(),
                cover_big: info.info.cover_big.clone(),
                movie_image: info.info.movie_image.clone(),
                release_date: info.info.releasedate.clone(),
                episode_run_time: info.info.episode_run_time,
                youtube_trailer: info.info.youtube_trailer.clone(),
                director: info.info.director.clone(),
                actors: info.info.actors.clone(),
                cast: info.info.cast.clone(),
                description: info.info.description.clone(),
                plot: info.info.plot.clone(),
                age: info.info.age.clone(),
                mpaa_rating: info.info.mpaa_rating.clone(),
                rating_count_kinopoisk: info.info.rating_count_kinopoisk,
                country: info.info.country.clone(),
                genre: info.info.genre.clone(),
                backdrop_path: info.info.backdrop_path.clone(),
                duration_secs: info.info.duration_secs.clone(),
                duration: info.info.duration.clone(),
                video: info.info.video.clone(),
                audio: info.info.audio.clone(),
                bitrate: info.info.bitrate,
                runtime: info.info.runtime.clone(),
                status: info.info.status.clone(),
            }),
        };

        if let Some(StreamProperties::Video(video)) = pli.header.additional_properties.as_ref() {
            props.rating = video.rating;
            props.rating_5based = video.rating_5based;
            props.stream_type = video.stream_type.clone();
            if props.tmdb.is_none() {
                props.tmdb = video.tmdb;
            }
            if props.trailer.is_none() {
                props.trailer = video.trailer.clone();
            }
            props.is_adult = video.is_adult;
        }

        props
    }
}


impl SeriesStreamProperties {
    pub fn from_info(info: &XtreamSeriesInfo, pli: &PlaylistItem) -> SeriesStreamProperties {
        SeriesStreamProperties {
            name: info.info.name.clone(),
            category_id: info.info.category_id,
            series_id: pli.get_virtual_id(),
            backdrop_path: info.info.backdrop_path.clone(),
            cast: info.info.cast.clone(),
            cover: info.info.cover.clone(),
            director: info.info.director.clone(),
            episode_run_time: Some(info.info.episode_run_time.clone()),
            genre: Some(info.info.genre.clone()),
            last_modified: Some(info.info.last_modified.clone()),
            plot: Some(info.info.plot.clone()),
            rating: info.info.rating,
            rating_5based: info.info.rating_5based,
            release_date: Some(info.info.release_date.clone()),
            youtube_trailer: info.info.youtube_trailer.clone(),
            tmdb: info.info.tmdb,
            details: Some(SeriesStreamDetailProperties {
                year: extract_year_from_release_date(&info.info.release_date),
                episodes: info.episodes.as_ref().map(|list|
                    list.iter().map(|e| {
                        SeriesStreamDetailEpisodeProperties {
                            id: e.id,
                            episode_num: e.episode_num,
                            season: e.season,
                            title: e.title.clone(),
                            container_extension: e.container_extension.clone(),
                            custom_sid: e.custom_sid.clone(),
                            added: e.added.clone(),
                            direct_source: e.direct_source.clone(),
                            tmdb: info.info.tmdb,
                            release_date: e.info.as_ref().map(|i| i.air_date.clone()).unwrap_or_default(),
                            plot: None,
                            duration_secs: e.info.as_ref().map(|i| i.duration_secs).unwrap_or_default(),
                            duration: e.info.as_ref().map(|i| i.duration.clone()).unwrap_or_default(),
                            movie_image: e.info.as_ref().map(|i| i.movie_image.clone()).unwrap_or_default(),
                            bitrate: e.info.as_ref().map(|i| i.bitrate).unwrap_or_default(),
                            rating: e.info.as_ref().map(|i| i.rating),
                            video: e.info.as_ref().map(|i| i.video.clone()).unwrap_or_default(),
                            audio: e.info.as_ref().map(|i| i.audio.clone()).unwrap_or_default(),
                        }
                    }).collect())
            }),
        }
    }
}

impl EpisodeStreamProperties {

    pub fn from_series(series: &SeriesStreamProperties, episode: &SeriesStreamDetailEpisodeProperties) -> EpisodeStreamProperties {
        EpisodeStreamProperties {
            episode_id: episode.id,
            episode: episode.episode_num,
            season: episode.season,
            added: Some(episode.added.clone()),
            release_date: Some(episode.release_date.clone()),
            tmdb: episode.tmdb.or(series.tmdb),
            movie_image: episode.movie_image.clone(),
            container_extension: episode.container_extension.clone(),
            video: episode.video.clone(),
            audio: episode.audio.clone(),
        }
    }
}

fn extract_year_from_release_date(release_date: &str) -> Option<u32> {
    if release_date.len() >= 4 {
        release_date[..4].parse::<u32>().ok()
    } else {
        None
    }
}

