use crate::utils::serialize_option_string_as_null_if_empty;
use crate::utils::deserialize_as_option_string;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use serde_json::Value;
use crate::concat_string;
use crate::model::{PlaylistItemType, SeriesStreamDetailEpisodeProperties, SeriesStreamDetailSeasonProperties, SeriesStreamProperties, StreamProperties, VideoStreamProperties, VirtualId, XtreamCluster, XtreamMappingOptions};
use crate::model::info_doc_utils::InfoDocUtils;

#[inline]
fn build_season_episode_field(season: u32, episode: u32, field: &str) -> String {
    concat_string!("nfo_ep_", &season.to_string(), "_", &episode.to_string(), "_", field)
}

#[inline]
fn build_season_field(season: u32, field: &str) -> String {
    concat_string!("nfo_s_", &season.to_string(), "_", field)
}

#[allow(clippy::large_enum_variant)]
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(untagged)]
pub enum XtreamInfoDocument {
    Video(XtreamVideoInfoDoc),
    Series(XtreamSeriesInfoDoc),
    Empty(XtreamEmptyDoc),
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub struct XtreamEmptyDoc {}

impl Default for XtreamInfoDocument {
    fn default() -> Self {
        Self::Empty(XtreamEmptyDoc {})
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct XtreamVideoInfoDoc {
    pub info: XtreamVideoInfoData,
    pub movie_data: XtreamVideoMovieData,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct XtreamVideoInfoData {
    pub kinopoisk_url: String,
    pub tmdb_id: String,
    pub name: String,
    pub o_name: String,
    pub cover_big: String,
    pub movie_image: String,
    #[serde(rename = "releasedate")]
    pub release_date: String,
    pub episode_run_time: u32,
    pub youtube_trailer: String,
    pub director: String,
    pub actors: String,
    pub cast: String,
    pub description: String,
    pub plot: String,
    pub age: String,
    pub mpaa_rating: String,
    pub rating_count_kinopoisk: u32,
    pub country: String,
    pub genre: String,
    pub backdrop_path: Vec<String>,
    pub duration_secs: String,
    pub duration: String,
    pub video: Value,
    pub audio: Value,
    pub bitrate: u32,
    pub rating: String,
    pub runtime: String,
    pub status: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct XtreamVideoMovieData {
    pub stream_id: u32,
    pub name: String,
    pub added: String,
    pub category_id: String,
    pub category_ids: Vec<u32>,
    pub container_extension: String,
    #[serde(default, deserialize_with = "deserialize_as_option_string", serialize_with = "serialize_option_string_as_null_if_empty")]
    pub custom_sid: Option<String>,
    pub direct_source: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct XtreamSeriesInfoDoc {
    #[serde(default)]
    pub seasons: Vec<XtreamSeriesSeasonDoc>,
    pub info: XtreamSeriesInfoData,
    pub episodes: HashMap<String, Vec<XtreamSeriesEpisodeInfoDoc>>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct XtreamSeriesSeasonDoc {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub season_number: u32,
    #[serde(default)]
    pub episode_count: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overview: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub air_date: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cover: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cover_tmdb: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cover_big: Option<String>,
    #[serde(default, rename = "releaseDate", skip_serializing_if = "Option::is_none")]
    pub release_date: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration: Option<String>,
}


#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct XtreamSeriesInfoData {
    pub name: String,
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
    pub tmdb: String,
    pub youtube_trailer: String,
    pub episode_run_time: String,
    pub category_id: String,
    pub category_ids: Vec<u32>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct XtreamSeriesEpisodeInfoDoc {
    pub id: String,
    pub episode_num: u32,
    pub title: String,
    pub container_extension: String,
    pub info: XtreamSeriesEpisodeInfoData,
    #[serde(default, deserialize_with = "deserialize_as_option_string", serialize_with = "serialize_option_string_as_null_if_empty")]
    pub custom_sid: Option<String>,
    pub added: String,
    pub season: u32,
    pub direct_source: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct XtreamSeriesEpisodeInfoData {
    #[serde(rename = "id")]
    pub tmdb_id: u32,
    pub air_date: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crew: Option<String>,
    pub rating: f64,
    pub movie_image: String,
    pub duration: String,
    pub duration_secs: u32,
    pub video: Value,
    pub audio: Value,
    pub bitrate: u32,
}

impl StreamProperties {
    pub fn to_info_document(&self, options: &XtreamMappingOptions, item_type: PlaylistItemType,
                            virtual_id: VirtualId, category_id: u32) -> XtreamInfoDocument {
        match self {
            StreamProperties::Live(_live) => {
                // Live streams don't expose info documents through the Xtream API.
                XtreamInfoDocument::Empty(XtreamEmptyDoc {})
            }
            StreamProperties::Video(video) => {
                XtreamInfoDocument::Video(self.video_to_info_document(options, video, item_type, virtual_id, category_id))
            }
            StreamProperties::Series(series) => {
                XtreamInfoDocument::Series(self.series_to_info_document(options, series, item_type, virtual_id, category_id))
            }
            StreamProperties::Episode(_episode) => {
                // Episode streams don't expose info documents through the Xtream API.
                XtreamInfoDocument::Empty(XtreamEmptyDoc {})
            }
        }
    }

    fn series_to_info_document(&self, options: &XtreamMappingOptions, series: &SeriesStreamProperties,
                               item_type: PlaylistItemType, virtual_id: VirtualId, category_id: u32) -> XtreamSeriesInfoDoc {
        let resource_url = options.get_resource_url(XtreamCluster::Series, item_type, virtual_id);
        XtreamSeriesInfoDoc {
            seasons: if let Some(seasons) = series.details.as_ref().and_then(|d| d.seasons.as_ref()) {
                self.series_seasons_to_info_document(resource_url.as_deref(), seasons)
            } else {
                Vec::new()
            },
            info: XtreamSeriesInfoData {
                name: series.name.clone(),
                cover: InfoDocUtils::make_resource_url(resource_url.as_deref(), series.cover.as_ref(), "cover"),
                plot: series.plot.as_ref().map_or_else(String::new, Clone::clone),
                cast: series.cast.clone(),
                director: series.director.clone(),
                genre: series.genre.as_ref().map_or_else(String::new, Clone::clone),
                release_date: series.release_date.as_ref().map_or_else(String::new, Clone::clone),
                release_date_alternate: series.release_date.as_ref().map_or_else(String::new, Clone::clone),
                last_modified: series.last_modified.as_ref().map_or_else(String::new, Clone::clone),
                rating: InfoDocUtils::limited(series.rating),
                rating_5based: InfoDocUtils::limited(series.rating_5based),
                backdrop_path: series.backdrop_path.as_ref().map_or_else(Vec::new, |b| b.iter().enumerate().map(|(idx, p)|
                    InfoDocUtils::make_bdpath_resource_url(resource_url.as_deref(), p, idx, "")
                ).collect()),
                tmdb: series.tmdb.unwrap_or_default().to_string(),
                youtube_trailer: series.youtube_trailer.clone(),
                episode_run_time: series.episode_run_time.as_ref().map_or_else(String::new, Clone::clone),
                category_id: category_id.to_string(),
                category_ids: vec![category_id],
            },
            episodes: if let Some(episodes) = series.details.as_ref().and_then(|d| d.episodes.as_ref()) {
                self.series_episodes_to_info_document(options, resource_url.as_deref(), episodes)
            } else {
                HashMap::new()
            }
        }
    }

    fn video_to_info_document(&self, options: &XtreamMappingOptions, video: &VideoStreamProperties,
                              item_type: PlaylistItemType, virtual_id: VirtualId, category_id: u32) -> XtreamVideoInfoDoc {
        let resource_url = options.get_resource_url(XtreamCluster::Video, item_type, virtual_id);
        let stream_icon = InfoDocUtils::make_resource_url(resource_url.as_deref(), &self.get_stream_icon(), "logo");

        let info = if let Some(details) = video.details.as_ref() {
            XtreamVideoInfoData {
                kinopoisk_url: details.kinopoisk_url.as_ref().map_or_else(String::new, Clone::clone),
                tmdb_id: video.tmdb.unwrap_or_default().to_string(),
                name: video.name.clone(),
                o_name: details.o_name.as_ref().map_or_else(String::new, Clone::clone),
                cover_big: InfoDocUtils::make_resource_url(resource_url.as_deref(), &details.cover_big.as_ref().map_or_else(String::new, Clone::clone), "nfo_cover_big"),
                movie_image: InfoDocUtils::make_resource_url(resource_url.as_deref(), &details.cover_big.as_ref().map_or_else(String::new, Clone::clone), "nfo_movie_image"),
                release_date: details.release_date.as_ref().map_or_else(String::new, Clone::clone),
                episode_run_time: details.episode_run_time.unwrap_or_default(),
                youtube_trailer: details.youtube_trailer.as_ref().map_or_else(String::new, Clone::clone),
                director: details.director.as_ref().map_or_else(String::new, Clone::clone),
                actors: details.actors.as_ref().map_or_else(String::new, Clone::clone),
                cast: details.cast.as_ref().map_or_else(String::new, Clone::clone),
                description: details.description.as_ref().map_or_else(String::new, Clone::clone),
                plot: details.plot.as_ref().map_or_else(String::new, Clone::clone),
                age: details.age.as_ref().map_or_else(String::new, Clone::clone),
                mpaa_rating: details.mpaa_rating.as_ref().map_or_else(String::new, Clone::clone),
                rating_count_kinopoisk: details.rating_count_kinopoisk,
                country: details.country.as_ref().map_or_else(String::new, Clone::clone),
                genre: details.genre.as_ref().map_or_else(String::new, Clone::clone),
                backdrop_path: details.backdrop_path.as_deref().map_or_else(Vec::new, |b| b.iter().enumerate().map(|(idx, p)|
                    InfoDocUtils::make_bdpath_resource_url(resource_url.as_deref(), p, idx, "nfo_")
                ).collect()),
                duration_secs: details.duration_secs.as_ref().map_or_else(String::new, Clone::clone),
                duration: details.duration.as_ref().map_or_else(String::new, Clone::clone),
                video: InfoDocUtils::build_value(details.video.as_deref()),
                audio: InfoDocUtils::build_value(details.audio.as_deref()),
                bitrate: details.bitrate,
                rating: InfoDocUtils::limited(video.rating.unwrap_or_default()),
                runtime: details.runtime.as_ref().map_or_else(String::new, Clone::clone),
                status: details.status.as_ref().map_or_else(String::new, Clone::clone),
            }
        } else {
            XtreamVideoInfoData {
                kinopoisk_url: String::new(),
                tmdb_id: video.tmdb.unwrap_or_default().to_string(),
                name: video.name.clone(),
                o_name: video.name.clone(),
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
                duration_secs: "0".to_string(),
                duration: "0".to_string(),
                video: Value::Array(Vec::new()),
                audio: Value::Array(Vec::new()),
                bitrate: 0,
                rating: InfoDocUtils::limited(video.rating.unwrap_or_default()),
                runtime: "0".to_string(),
                status: "Released".to_string(),
            }
        };

        XtreamVideoInfoDoc {
            info,
            movie_data: XtreamVideoMovieData {
                stream_id: virtual_id,
                name: video.name.clone(),
                added: video.added.clone(),
                category_id: category_id.to_string(),
                category_ids: vec![category_id],
                container_extension: video.container_extension.clone(),
                custom_sid: video.custom_sid.clone(),
                direct_source: if options.skip_video_direct_source { String::new() } else { video.direct_source.clone() },
            }
        }
    }

    fn series_seasons_to_info_document(&self,
                                        resource_url: Option<&str>,
                                        seasons: &[SeriesStreamDetailSeasonProperties]) -> Vec<XtreamSeriesSeasonDoc> {
        seasons.iter().map(|season|
            XtreamSeriesSeasonDoc {
                name: season.name.clone(),
                season_number: season.season_number,
                episode_count: season.episode_count.to_string(),
                overview: season.overview.as_ref().map(|v| if v.starts_with("http") {
                    InfoDocUtils::make_resource_url(resource_url, v, &build_season_field(season.season_number, "overview"))
                } else {
                    v.clone()
                }),
                air_date: season.air_date.clone(),
                cover: season.cover.as_ref().map(|v| InfoDocUtils::make_resource_url(resource_url, v, &build_season_field(season.season_number, "cover"))),
                cover_tmdb: season.cover_tmdb.as_ref().map(|v| InfoDocUtils::make_resource_url(resource_url, v, &build_season_field(season.season_number, "cover_tmdb"))),
                cover_big: season.cover_big.as_ref().map(|v| InfoDocUtils::make_resource_url(resource_url, v, &build_season_field(season.season_number, "cover_big"))),
                release_date: season.air_date.clone(),
                duration: season.duration.clone(),
            }
            ).collect()
    }

    fn series_episodes_to_info_document(&self, options: &XtreamMappingOptions,
                                        resource_url: Option<&str>,
                                        episodes: &[SeriesStreamDetailEpisodeProperties]) -> HashMap<String, Vec<XtreamSeriesEpisodeInfoDoc>> {
        let mut map: HashMap<u32, Vec<XtreamSeriesEpisodeInfoDoc>> = HashMap::new();
        for ep in episodes {
            let doc = XtreamSeriesEpisodeInfoDoc {
                id: ep.id.to_string(),
                episode_num: ep.episode_num,
                title: ep.title.clone(),
                container_extension: ep.container_extension.clone(),
                info: XtreamSeriesEpisodeInfoData {
                    tmdb_id: ep.tmdb.unwrap_or_default(),
                    air_date: ep.release_date.clone(),
                    crew: ep.crew.clone(),
                    rating: ep.rating.unwrap_or_default(),
                    movie_image: InfoDocUtils::make_resource_url(resource_url, &ep.movie_image, &build_season_episode_field(ep.season, ep.episode_num, "movie_image")),
                    duration: ep.duration.clone(),
                    duration_secs: ep.duration_secs,
                    video: InfoDocUtils::build_value(ep.video.as_deref()),
                    audio: InfoDocUtils::build_value(ep.audio.as_deref()),
                    bitrate: ep.bitrate,
                },
                custom_sid: ep.custom_sid.clone(),
                added: ep.added.clone(),
                season: ep.season,
                direct_source: if options.skip_series_direct_source { String::new() } else { ep.direct_source.clone() },
            };
            map.entry(ep.season).or_default().push(doc);
        }

        map.into_iter()
            .map(|(season, episodes)| (season.to_string(), episodes))
            .collect()
    }
}


impl Default for XtreamVideoInfoDoc {
    fn default() -> Self {
        Self {
            info: XtreamVideoInfoData {
                kinopoisk_url: String::new(),
                tmdb_id: String::new(),
                name: String::new(),
                o_name: String::new(),
                cover_big: String::new(),
                movie_image: String::new(),
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
                backdrop_path: vec![],
                duration_secs: String::new(),
                duration: String::new(),
                video: Value::Array(Vec::new()),
                audio: Value::Array(Vec::new()),
                bitrate: 0,
                rating: String::new(),
                runtime: String::new(),
                status: String::new(),
            },
            movie_data: XtreamVideoMovieData {
                stream_id: 0,
                name: String::new(),
                added: String::new(),
                category_id: String::new(),
                category_ids: vec![],
                container_extension: String::new(),
                custom_sid: None,
                direct_source: String::new(),
            },
        }
    }
}