use crate::model::info_doc_utils::InfoDocUtils;
use crate::model::{PlaylistEntry, XtreamSeriesInfo, XtreamVideoInfo};
use crate::utils::{deserialize_as_option_string, deserialize_as_string,
                   deserialize_as_string_array, deserialize_json_as_opt_string, serialize_json_as_opt_string,
                   deserialize_number_from_string, deserialize_number_from_string_or_zero, string_default_on_null,
                   serialize_option_string_as_null_if_empty};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct LiveStreamProperties {
    #[serde(default, deserialize_with = "string_default_on_null")]
    pub name: String,
    #[serde(default, deserialize_with = "deserialize_number_from_string_or_zero")]
    pub category_id: u32,
    #[serde(default, deserialize_with = "deserialize_number_from_string_or_zero")]
    pub stream_id: u32,
    #[serde(default, deserialize_with = "string_default_on_null")]
    pub stream_icon: String,
    #[serde(default, deserialize_with = "string_default_on_null")]
    pub direct_source: String,
    #[serde(default, deserialize_with = "deserialize_as_option_string", serialize_with = "serialize_option_string_as_null_if_empty")]
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
    #[serde(default, serialize_with = "serialize_json_as_opt_string", deserialize_with = "deserialize_json_as_opt_string")]
    pub video: Option<String>,
    #[serde(default, serialize_with = "serialize_json_as_opt_string", deserialize_with = "deserialize_json_as_opt_string")]
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
    #[serde(default, deserialize_with = "deserialize_as_string")]
    pub stream_icon: String,
    #[serde(default, deserialize_with = "deserialize_as_string")]
    pub direct_source: String,
    #[serde(default, deserialize_with = "deserialize_as_option_string", serialize_with = "serialize_option_string_as_null_if_empty")]
    pub custom_sid: Option<String>,
    #[serde(default, deserialize_with = "deserialize_as_string")]
    pub added: String,
    #[serde(default, deserialize_with = "deserialize_as_string")]
    pub container_extension: String,
    #[serde(default, deserialize_with = "deserialize_number_from_string")]
    pub rating: Option<f64>,
    #[serde(default, deserialize_with = "deserialize_number_from_string")]
    pub rating_5based: Option<f64>,
    #[serde(default)]
    pub stream_type: Option<String>,
    #[serde(default)]
    pub trailer: Option<String>,
    #[serde(default, deserialize_with = "deserialize_number_from_string")]
    pub tmdb: Option<u32>,
    #[serde(default, deserialize_with = "deserialize_number_from_string_or_zero")]
    pub is_adult: i32,
    #[serde(default)]
    pub details: Option<VideoStreamDetailProperties>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct SeriesStreamDetailSeasonProperties {
    #[serde(default, deserialize_with = "string_default_on_null")]
    pub name: String,
    #[serde(default, deserialize_with = "deserialize_number_from_string_or_zero")]
    pub season_number: u32,
    #[serde(default, deserialize_with = "deserialize_number_from_string_or_zero")]
    pub episode_count: u32,
    #[serde(default, deserialize_with = "deserialize_as_option_string")]
    pub overview: Option<String>,
    #[serde(default, deserialize_with = "deserialize_as_option_string")]
    pub air_date: Option<String>,
    #[serde(default, deserialize_with = "deserialize_as_option_string")]
    pub cover: Option<String>,
    #[serde(default, deserialize_with = "deserialize_as_option_string")]
    pub cover_tmdb: Option<String>,
    #[serde(default, deserialize_with = "deserialize_as_option_string")]
    pub cover_big: Option<String>,
    #[serde(default, deserialize_with = "deserialize_as_option_string")]
    pub duration: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct SeriesStreamDetailEpisodeProperties {
    #[serde(default, deserialize_with = "deserialize_number_from_string_or_zero")]
    pub id: u32,
    #[serde(default, deserialize_with = "deserialize_number_from_string_or_zero")]
    pub episode_num: u32,
    #[serde(default, deserialize_with = "deserialize_number_from_string_or_zero")]
    pub season: u32,
    #[serde(default, deserialize_with = "string_default_on_null")]
    pub title: String,
    #[serde(default, deserialize_with = "string_default_on_null")]
    pub container_extension: String,
    #[serde(default, deserialize_with = "deserialize_as_option_string", serialize_with = "serialize_option_string_as_null_if_empty")]
    pub custom_sid: Option<String>,
    #[serde(default, deserialize_with = "deserialize_as_string")]
    pub added: String,
    #[serde(default, deserialize_with = "string_default_on_null")]
    pub direct_source: String,
    #[serde(default, deserialize_with = "deserialize_number_from_string")]
    pub tmdb: Option<u32>,
    #[serde(default, deserialize_with = "deserialize_as_string")]
    pub release_date: String,
    #[serde(default, deserialize_with = "deserialize_as_option_string")]
    pub plot: Option<String>,
    #[serde(default, deserialize_with = "deserialize_as_option_string")]
    pub crew: Option<String>,
    #[serde(default, deserialize_with = "deserialize_number_from_string_or_zero")]
    pub duration_secs: u32,
    #[serde(default, deserialize_with = "deserialize_as_string")]
    pub duration: String,
    #[serde(default, deserialize_with = "string_default_on_null")]
    pub movie_image: String,
    #[serde(default, deserialize_with = "deserialize_number_from_string_or_zero")]
    pub bitrate: u32,
    #[serde(default, deserialize_with = "deserialize_number_from_string")]
    pub rating: Option<f64>,
    #[serde(default, serialize_with = "serialize_json_as_opt_string", deserialize_with = "deserialize_json_as_opt_string")]
    pub video: Option<String>,
    #[serde(default, serialize_with = "serialize_json_as_opt_string", deserialize_with = "deserialize_json_as_opt_string")]
    pub audio: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct SeriesStreamDetailProperties {
    #[serde(default, deserialize_with = "deserialize_number_from_string")]
    pub year: Option<u32>,
    pub seasons: Option<Vec<SeriesStreamDetailSeasonProperties>>,
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
    #[serde(default, deserialize_with = "string_default_on_null")]
    pub cast: String,
    #[serde(default, deserialize_with = "string_default_on_null")]
    pub cover: String,
    #[serde(default, deserialize_with = "string_default_on_null")]
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
    #[serde(default, deserialize_with = "string_default_on_null")]
    pub youtube_trailer: String,
    #[serde(default, deserialize_with = "deserialize_number_from_string")]
    pub tmdb: Option<u32>,
    #[serde(default)]
    pub details: Option<SeriesStreamDetailProperties>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct EpisodeStreamProperties {
    #[serde(default, deserialize_with = "deserialize_number_from_string_or_zero")]
    pub episode_id: u32,
    #[serde(default, deserialize_with = "deserialize_number_from_string_or_zero")]
    pub episode: u32,
    #[serde(default, deserialize_with = "deserialize_number_from_string_or_zero")]
    pub season: u32,
    #[serde(default, deserialize_with = "deserialize_as_option_string")]
    pub added: Option<String>,
    #[serde(default, deserialize_with = "deserialize_as_option_string")]
    pub release_date: Option<String>,
    #[serde(default, deserialize_with = "deserialize_number_from_string")]
    pub tmdb: Option<u32>,
    #[serde(default, deserialize_with = "string_default_on_null")]
    pub movie_image: String,
    #[serde(default, deserialize_with = "string_default_on_null")]
    pub container_extension: String,
    #[serde(default, serialize_with = "serialize_json_as_opt_string", deserialize_with = "deserialize_json_as_opt_string")]
    pub video: Option<String>,
    #[serde(default, serialize_with = "serialize_json_as_opt_string", deserialize_with = "deserialize_json_as_opt_string")]
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
    pub fn has_details(&self) -> bool {
        match self {
            StreamProperties::Video(video) => video.details.is_some(),
            StreamProperties::Series(series) => series.details.is_some(),
            StreamProperties::Live(_)
            | StreamProperties::Episode(_) => false,
        }
    }

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
            StreamProperties::Episode(episode) => episode.tmdb,
        }
    }

    pub fn get_release_date(&self) -> Option<Cow<'_, str>> {
        match self {
            StreamProperties::Live(_) => None,
            StreamProperties::Video(video) => video.details.as_ref().and_then(|d| d.release_date.as_deref().map(Cow::Borrowed)),
            StreamProperties::Series(series) => series.release_date.as_deref().map(Cow::Borrowed),
            StreamProperties::Episode(episode) => episode.release_date.as_deref().map(Cow::Borrowed),
        }
    }

    pub fn get_added(&self) -> Option<Cow<'_, str>> {
        match self {
            StreamProperties::Live(_) => None,
            StreamProperties::Video(video) => non_empty_string(video.added.as_str()),
            StreamProperties::Series(series) => series.details.as_ref()
                .and_then(|d| d.episodes.as_ref())
                .and_then(|e| e.first().and_then(|e| non_empty_string(e.added.as_str()))),
            StreamProperties::Episode(episode) => episode.added.as_deref().map(Cow::Borrowed),
        }
    }

    pub fn get_container_extension(&self) -> Option<Cow<'_, str>> {
        match self {
            StreamProperties::Live(_) => None,
            StreamProperties::Video(video) => non_empty_string(&video.container_extension),
            StreamProperties::Series(series) => series.details.as_ref()
                .and_then(|d| d.episodes.as_ref())
                .and_then(|e| e.first().and_then(|e| non_empty_string(&e.container_extension))),
            StreamProperties::Episode(episode) => non_empty_string(&episode.container_extension),
        }
    }

    pub fn get_direct_source(&self) -> Option<Cow<'_, str>> {
        match self {
            StreamProperties::Live(_) => None,
            StreamProperties::Video(video) => non_empty_string(&video.direct_source),
            StreamProperties::Series(series) => series.details.as_ref()
                .and_then(|d| d.episodes.as_ref())
                .and_then(|e| e.first().and_then(|e| non_empty_string(&e.direct_source))),
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

    pub fn resolve_resource_url<'a>(&'a self, field: &str) -> Option<Cow<'a, str>> {
        if field.starts_with("backdrop_path") {
            if let StreamProperties::Series(series) = self {
                if let Some(backdrop) = series.backdrop_path.as_ref() {
                    if let Some(url) = backdrop.first() {
                        return Some(Cow::Borrowed(url));
                    }
                }
            }
            return None;
        } else if field.starts_with("nfo_backdrop_path") {
            if let StreamProperties::Video(video) = self {
                if let Some(details) = video.details.as_ref() {
                    if let Some(backdrop) = details.backdrop_path.as_ref() {
                        if let Some(url) = backdrop.first() {
                            return Some(Cow::Borrowed(url));
                        }
                    }
                }
            }
            return None;
        }

        if field == "cover" {
            if let StreamProperties::Series(series) = self {
                return Some(Cow::Borrowed(series.cover.as_str()));
            }
            return None;
        }
        if field == "logo" || field == "logo_small" {
            return match self {
                StreamProperties::Live(live) => {
                    Some(Cow::Borrowed(live.stream_icon.as_str()))
                }
                StreamProperties::Video(video) => {
                    Some(Cow::Borrowed(video.stream_icon.as_str()))
                }
                StreamProperties::Series(series) => {
                    Some(Cow::Borrowed(series.cover.as_str()))
                }
                StreamProperties::Episode(episode) => {
                    Some(Cow::Borrowed(episode.movie_image.as_str()))
                }
            };
        }
        if field == "movie_image" {
            if let StreamProperties::Episode(episode) = self {
                return Some(Cow::Borrowed(episode.movie_image.as_str()));
            }
            return None;
        }
        if field == "nfo_cover_big" {
            if let StreamProperties::Video(video) = self {
                if let Some(details) = video.details.as_ref() {
                    if let Some(cover_big) = details.cover_big.as_ref() {
                        return Some(Cow::Borrowed(cover_big.as_str()));
                    }
                }
            }
        }

        if field == "nfo_movie_image" {
            if let StreamProperties::Video(video) = self {
                if let Some(details) = video.details.as_ref() {
                    if let Some(movie_image) = details.movie_image.as_ref() {
                        return Some(Cow::Borrowed(movie_image.as_str()));
                    }
                }
            }
            return None;
        }

        if field.starts_with("nfo_s_") {
            if let Some((season_num, field)) = parse_season_field(field) {
                if let StreamProperties::Series(series) = self {
                    if let Some(details) = series.details.as_ref() {
                        if let Some(seasons) = details.seasons.as_ref() {
                            for season in seasons {
                                if season.season_number == season_num {
                                    if field == "cover" {
                                        return season.cover.as_ref().map(|c| Cow::Borrowed(c.as_str()));
                                    }
                                    if field == "cover_tmdb" {
                                        return season.cover_tmdb.as_ref().map(|c| Cow::Borrowed(c.as_str()));
                                    }
                                    if field == "cover_big" {
                                        return season.cover_big.as_ref().map(|c| Cow::Borrowed(c.as_str()));
                                    }
                                    if field == "overview" {
                                        return season.overview.as_ref().map(|c| Cow::Borrowed(c.as_str()));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        if field.starts_with("nfo_ep_") {
            if let Some((season, episode_num, field)) = parse_season_episode_field(field) {
                if let StreamProperties::Series(series) = self {
                    if let Some(details) = series.details.as_ref() {
                        if let Some(episodes) = details.episodes.as_ref() {
                            for episode in episodes {
                                if episode.season == season && episode_num == episode.episode_num && field == "movie_image" {
                                    return Some(Cow::Borrowed(episode.movie_image.as_str()));
                                }
                            }
                        }
                    }
                }
            }
        }

        None
    }
}

fn parse_season_field(s: &str) -> Option<(u32, String)> {
    let mut parts = s.split('_');

    let (prefix, suffix) = (parts.next()?, parts.next()?);
    if prefix != "nfo" || suffix != "s" {
        return None;
    }

    let season: u32 = parts.next()?.parse().ok()?;

    let kind = parts.collect::<Vec<_>>().join("_");
    if kind.is_empty() {
        return None;
    }

    Some((season, kind))
}

fn parse_season_episode_field(s: &str) -> Option<(u32, u32, String)> {
    let mut parts = s.split('_');

    let (prefix, ep) = (parts.next()?, parts.next()?);
    if prefix != "nfo" || ep != "ep" {
        return None;
    }

    let season: u32 = parts.next()?.parse().ok()?;
    let episode: u32 = parts.next()?.parse().ok()?;

    let kind = parts.collect::<Vec<_>>().join("_");
    if kind.is_empty() {
        return None;
    }

    Some((season, episode, kind))
}


impl VideoStreamProperties {
    pub fn from_info<P>(info: &XtreamVideoInfo, pli: &P) -> VideoStreamProperties
    where
        P: PlaylistEntry,
    {
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
            stream_type: None, // from PlaylistItem
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

        if let Some(StreamProperties::Video(video)) = pli.get_additional_properties() {
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
        } else {
            props.stream_type = Some("movie".to_string());
        }

        props
    }
}

impl SeriesStreamProperties {
    pub fn from_info<P>(info: &XtreamSeriesInfo, pli: &P) -> SeriesStreamProperties
    where
        P: PlaylistEntry,
    {
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
                year: InfoDocUtils::extract_year_from_release_date(&info.info.release_date),
                seasons: info.seasons.as_ref().map(|list| {
                    list.iter().map(|s| {
                        SeriesStreamDetailSeasonProperties {
                            name: s.name.clone(),
                            season_number: s.season_number,
                            episode_count: s.episode_count,
                            overview: Some(s.overview.clone()),
                            air_date: Some(s.air_date.clone()),
                            cover: Some(s.cover.clone()),
                            cover_tmdb: Some(s.cover_tmdb.clone()),
                            cover_big: Some(s.cover_big.clone()),
                            duration: Some(s.duration.clone()),
                        }
                    }).collect()
                }),
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
                            crew: e.info.as_ref().map(|i| i.crew.clone()),
                            duration_secs: e.info.as_ref().map(|i| i.duration_secs).unwrap_or_default(),
                            duration: e.info.as_ref().map(|i| i.duration.clone()).unwrap_or_default(),
                            movie_image: e.info.as_ref().map(|i| i.movie_image.clone()).unwrap_or_default(),
                            bitrate: e.info.as_ref().map(|i| i.bitrate).unwrap_or_default(),
                            rating: e.info.as_ref().map(|i| i.rating),
                            video: e.info.as_ref().map(|i| i.video.clone()).unwrap_or_default(),
                            audio: e.info.as_ref().map(|i| i.audio.clone()).unwrap_or_default(),
                        }
                    }).collect()),
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

fn non_empty_string(s: &str) -> Option<Cow<'_, str>> {
    if s.is_empty() { None } else { Some(Cow::Borrowed(s)) }
}