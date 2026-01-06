use crate::utils::{deserialize_as_option_string, deserialize_as_string_array,
                   deserialize_number_from_string, deserialize_number_from_string_or_zero,
                   deserialize_as_string, string_default_on_null, serialize_option_string_as_null_if_empty,
                   deserialize_json_as_opt_string, serialize_json_as_opt_string};
use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct XtreamVideoInfoMovieData {
    #[serde(default, deserialize_with = "string_default_on_null")]
    pub name: String,
    #[serde(default, deserialize_with = "deserialize_number_from_string_or_zero")]
    pub category_id: u32,
    #[serde(default, deserialize_with = "deserialize_number_from_string_or_zero")]
    pub stream_id: u32,
    #[serde(default, deserialize_with = "string_default_on_null")]
    pub direct_source: String,
    #[serde(default, deserialize_with = "deserialize_as_option_string", serialize_with = "serialize_option_string_as_null_if_empty")]
    pub custom_sid: Option<String>,
    #[serde(default, deserialize_with = "deserialize_as_string")]
    pub added: String,
    #[serde(default, deserialize_with = "string_default_on_null")]
    pub container_extension: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct XtreamVideoInfoInfo {
    #[serde(default)]
    pub kinopoisk_url: Option<String>,
    #[serde(default, deserialize_with = "deserialize_as_string")]
    pub tmdb_id: String, // is in get_vod_streams
    #[serde(default, deserialize_with = "string_default_on_null")]
    pub name: String, // is in get_vod_streams
    pub o_name: Option<String>,
    pub cover_big: Option<String>,
    pub movie_image: Option<String>,
    #[serde(default, deserialize_with = "deserialize_as_option_string")]
    pub releasedate: Option<String>,
    #[serde(default, deserialize_with = "deserialize_number_from_string")]
    pub episode_run_time: Option<u32>,
    pub youtube_trailer: Option<String>,
    pub director: Option<String>,
    pub actors: Option<String>,
    pub cast: Option<String>,
    pub description: Option<String>,
    pub plot: Option<String>,
    #[serde(default, deserialize_with = "deserialize_as_option_string")]
    pub age: Option<String>,
    #[serde(default, deserialize_with = "deserialize_as_option_string")]
    pub mpaa_rating: Option<String>,
    #[serde(default)]
    pub rating_count_kinopoisk: u32,
    pub country: Option<String>,
    pub genre: Option<String>,
    #[serde(default, deserialize_with = "deserialize_as_string_array")]
    pub backdrop_path: Option<Vec<String>>,
    #[serde(default, deserialize_with = "deserialize_as_option_string")]
    pub duration_secs: Option<String>,
    #[serde(default, deserialize_with = "deserialize_as_option_string")]
    pub duration: Option<String>,
    #[serde(default, serialize_with = "serialize_json_as_opt_string", deserialize_with = "deserialize_json_as_opt_string")]
    pub video: Option<String>,
    #[serde(default, serialize_with = "serialize_json_as_opt_string", deserialize_with = "deserialize_json_as_opt_string")]
    pub audio: Option<String>,
    #[serde(default)]
    pub bitrate: u32,
    #[serde(default, deserialize_with = "deserialize_as_option_string")]
    pub runtime: Option<String>,
    #[serde(default, deserialize_with = "deserialize_as_option_string")]
    pub status: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct XtreamVideoInfo {
    pub info: XtreamVideoInfoInfo,
    pub movie_data: XtreamVideoInfoMovieData,
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XtreamSeriesInfoSeason {
    #[serde(default, deserialize_with = "string_default_on_null")]
    pub air_date: String,
    #[serde(default, deserialize_with = "deserialize_number_from_string_or_zero")]
    pub episode_count: u32,
    #[serde(default, deserialize_with = "deserialize_number_from_string_or_zero")]
    pub id: u32,
    #[serde(default, deserialize_with = "string_default_on_null")]
    pub name: String,
    #[serde(default, deserialize_with = "string_default_on_null")]
    pub overview: String,
    #[serde(default, deserialize_with = "deserialize_number_from_string_or_zero")]
    pub season_number: u32,
    #[serde(default, deserialize_with = "deserialize_number_from_string_or_zero")]
    pub vote_average: f64,
    #[serde(default, deserialize_with = "string_default_on_null")]
    pub cover: String,
    #[serde(default, deserialize_with = "string_default_on_null")]
    pub cover_big: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[allow(non_snake_case)]
pub struct XtreamSeriesInfoInfo {
    #[serde(default, deserialize_with = "string_default_on_null")]
    pub(crate) name: String,
    #[serde(default, deserialize_with = "string_default_on_null")]
    pub cover: String,
    #[serde(default, deserialize_with = "string_default_on_null")]
    pub plot: String,
    #[serde(default, deserialize_with = "string_default_on_null")]
    pub cast: String,
    #[serde(default, deserialize_with = "string_default_on_null")]
    pub director: String,
    #[serde(default, deserialize_with = "string_default_on_null")]
    pub genre: String,
    #[serde(default, deserialize_with = "string_default_on_null")]
    pub release_date: String,
    #[serde(default, deserialize_with = "string_default_on_null")]
    pub last_modified: String,
    #[serde(default, deserialize_with = "deserialize_number_from_string_or_zero")]
    pub rating: f64,
    #[serde(default, deserialize_with = "deserialize_number_from_string_or_zero")]
    pub rating_5based: f64,
    #[serde(default, deserialize_with = "deserialize_as_string_array")]
    pub backdrop_path: Option<Vec<String>>,
    #[serde(default, deserialize_with = "deserialize_number_from_string")]
    pub tmdb: Option<u32>,
    #[serde(default, deserialize_with = "string_default_on_null")]
    pub youtube_trailer: String,
    #[serde(default, deserialize_with = "string_default_on_null")]
    pub episode_run_time: String,
    #[serde(default, deserialize_with = "deserialize_number_from_string_or_zero")]
    pub category_id: u32,
}

#[allow(non_snake_case)]
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct XtreamSeriesInfoEpisodeInfo {
    #[serde(default, deserialize_with = "string_default_on_null")]
    pub air_date: String,
    #[serde(default, deserialize_with = "string_default_on_null")]
    pub crew: String,
    #[serde(default, deserialize_with = "deserialize_number_from_string_or_zero")]
    pub rating: f64,
    #[serde(default, deserialize_with = "deserialize_number_from_string")]
    pub id: Option<u32>,
    #[serde(default, deserialize_with = "deserialize_number_from_string_or_zero")]
    pub duration_secs: u32,
    #[serde(default, deserialize_with = "string_default_on_null")]
    pub duration: String,
    #[serde(default, deserialize_with = "string_default_on_null")]
    pub movie_image: String,
    #[serde(default, deserialize_with = "deserialize_json_as_opt_string")]
    pub video: Option<String>,
    #[serde(default, deserialize_with = "deserialize_json_as_opt_string")]
    pub audio: Option<String>,
    #[serde(default, deserialize_with = "deserialize_number_from_string_or_zero")]
    pub bitrate: u32,
}

// Used for serde_json deserialization, cannot be used with bincode
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XtreamSeriesInfoEpisode {
    #[serde(default, deserialize_with = "deserialize_number_from_string_or_zero")]
    pub id: u32,
    #[serde(default, deserialize_with = "deserialize_number_from_string_or_zero")]
    pub episode_num: u32,
    #[serde(default, deserialize_with = "string_default_on_null")]
    pub title: String,
    #[serde(default, deserialize_with = "string_default_on_null")]
    pub container_extension: String,
    #[serde(default)]
    pub info: Option<XtreamSeriesInfoEpisodeInfo>,
    #[serde(default, deserialize_with = "deserialize_as_option_string", serialize_with = "serialize_option_string_as_null_if_empty")]
    pub custom_sid: Option<String>,
    #[serde(default, deserialize_with = "string_default_on_null")]
    pub added: String,
    #[serde(default, deserialize_with = "deserialize_number_from_string_or_zero")]
    pub season: u32,
    #[serde(default, deserialize_with = "string_default_on_null")]
    pub direct_source: String,
}

impl XtreamSeriesInfoEpisode {
    pub fn get_id(&self) -> u32 {
        self.id
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XtreamSeriesInfo {
    #[serde(default)]
    pub seasons: Option<Vec<XtreamSeriesInfoSeason>>,
    #[serde(default)]
    pub info: XtreamSeriesInfoInfo,
    #[serde(
        default,
        serialize_with = "serialize_episodes",
        deserialize_with = "deserialize_episodes"
    )]
    pub episodes: Option<Vec<XtreamSeriesInfoEpisode>>,
}


// sometimes episodes are a map with season as key, sometimes an array
fn deserialize_episodes<'de, D>(deserializer: D) -> Result<Option<Vec<XtreamSeriesInfoEpisode>>, D::Error>
where
    D: Deserializer<'de>,
{
    // read as generic value
    let value = Value::deserialize(deserializer)?;
    match value {
        Value::Null => Ok(None),
        Value::Array(array) => {
            if array.is_empty() {
                Ok(None)
            } else {
                let mut result = Vec::new();
                for inner in array {
                    if let Some(inner_array) = inner.as_array() {
                        // Nested array case: [[ep1, ep2], [ep3]]
                        for item in inner_array {
                            let ep: XtreamSeriesInfoEpisode = serde_json::from_value(item.clone())
                                .map_err(serde::de::Error::custom)?;
                            result.push(ep);
                        }
                    } else if inner.is_object() {
                        // Flat array case: [ep1, ep2, ep3]
                        let ep: XtreamSeriesInfoEpisode = serde_json::from_value(inner.clone())
                            .map_err(serde::de::Error::custom)?;
                        result.push(ep);
                    }
                }
                Ok(if result.is_empty() { None } else { Some(result) })
            }
        }
        Value::Object(object) => {
            if object.is_empty() {
                Ok(None)
            } else {
                let mut result = Vec::new();
                for (_key, val) in object {
                    if let Some(inner_array) = val.as_array() {
                        for item in inner_array {
                            let ep: XtreamSeriesInfoEpisode = serde_json::from_value(item.clone())
                                .map_err(serde::de::Error::custom)?;
                            result.push(ep);
                        }
                    }
                }
                Ok(Some(result))
            }
        }
        _ => Err(serde::de::Error::custom("Invalid format for episodes")),
    }
}

#[allow(clippy::ref_option)]
fn serialize_episodes<S>(
    episodes: &Option<Vec<XtreamSeriesInfoEpisode>>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    match episodes {
        None => {
            let map = serializer.serialize_map(Some(0))?;
            map.end()
        }
        Some(list) => {
            if list.is_empty() {
                let map = serializer.serialize_map(Some(0))?;
                return map.end();
            }
            let mut seasons: BTreeMap<String, Vec<&XtreamSeriesInfoEpisode>> = BTreeMap::new();
            for ep in list {
                seasons.entry(ep.season.to_string()).or_default().push(ep);
            }

            seasons.serialize(serializer)
        }
    }
}