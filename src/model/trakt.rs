use serde::{Deserialize, Serialize};
use crate::model::{PlaylistItem};
use crate::utils::trakt::normalize_title_for_matching;

const  TRAKT_API_KEY: &str = "0183a05ad97098d87287fe46da4ae286f434f32e8e951caad4cc147c947d79a3";
const  TRAKT_API_VERSION: &str = "2";
const  TRAKT_API_URL: &str = "https://api.trakt.tv";
fn default_fuzzy_threshold() -> u8 {
    80
}

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TraktApiConfig {
    #[serde(default)]
    pub(crate) key: String,
    #[serde(default)]
    pub(crate) version: String,
    #[serde(default)]
    pub(crate) url: String,
}

impl TraktApiConfig {
    pub fn prepare(&mut self) {
        let key  =  self.key.trim();
        self.key = String::from(if key.is_empty() { TRAKT_API_KEY } else { key });
        let version = self.version.trim();
        self.version = String::from(if version.is_empty() { TRAKT_API_VERSION } else { version });
        let url = self.url.trim();
        self.url = String::from(if url.is_empty() { TRAKT_API_URL } else { url });
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TraktListConfig {
    pub user: String,
    pub list_slug: String,
    pub category_name: String,
    pub content_type: TraktContentType,
    #[serde(default = "default_fuzzy_threshold")]
    pub fuzzy_match_threshold: u8, // Percentage (0-100)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TraktContentType {
    Vod,
    Series,
    Both,
}

impl Default for TraktContentType {
    fn default() -> Self {
        Self::Both
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TraktConfig {
    #[serde(default)]
    pub api: TraktApiConfig,
    pub lists: Vec<TraktListConfig>,
}

impl TraktConfig {
    pub fn prepare(&mut self) {
        self.api.prepare();
    }
}

// API Response structures
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraktListItem {
    pub id: u64,
    pub rank: Option<u32>,
    pub listed_at: String,
    pub notes: Option<String>,
    #[serde(rename = "type")]
    pub item_type: String,
    pub movie: Option<TraktMovie>,
    pub show: Option<TraktShow>,
    #[serde(skip)]
    pub content_type: TraktContentType,
}

impl TraktListItem {
    pub fn prepare(&mut self) {
        self.content_type = match self.item_type.as_str() {
            "movie" => TraktContentType::Vod,
            "show" => if self.show.is_some() { TraktContentType::Series } else { TraktContentType::Both },
            _ => TraktContentType::Both,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraktMovie {
    pub ids: TraktIds,
    pub title: String,
    pub year: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraktShow {
    pub ids: TraktIds,
    pub title: String,
    pub year: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraktIds {
    pub trakt: u32,
    pub slug: String,
    pub tvdb: Option<u32>,
    pub imdb: Option<String>,
    pub tmdb: Option<u32>,
    pub tvrage: Option<u32>,
}

// Internal matching structures
#[derive(Debug, Clone)]
pub struct TraktMatchItem<'a> {
    pub title: &'a str,
    pub normalized_title: String,
    pub year: Option<u32>,
    pub tmdb_id: Option<u32>,
    pub trakt_id: u32,
    pub content_type: TraktContentType,
    pub rank: Option<u32>,
}

impl<'a> TraktMatchItem<'a> {
    pub fn from_trakt_list_item(item: &'a TraktListItem) -> Option<Self> {
        match item.item_type.as_str() {
            "movie" => {
                item.movie.as_ref().map(|movie| TraktMatchItem {
                        title: movie.title.as_str(),
                        normalized_title: normalize_title_for_matching(movie.title.as_str()),
                        year: movie.year,
                        tmdb_id: movie.ids.tmdb,
                        trakt_id: movie.ids.trakt,
                        content_type: TraktContentType::Vod,
                        rank: item.rank,
                    })
            }
            "show" => {
                item.show.as_ref().map(|show| TraktMatchItem {
                        title: show.title.as_str(),
                        normalized_title: normalize_title_for_matching(show.title.as_str()),
                        year: show.year,
                        tmdb_id: show.ids.tmdb,
                        trakt_id: show.ids.trakt,
                        content_type: TraktContentType::Series,
                        rank: item.rank,
                    })
            }
            _ => None,
        }
    }
}


// Matching results
#[derive(Debug, Clone)]
pub struct TraktMatchResult<'a> {
    pub playlist_item: &'a PlaylistItem,
    pub trakt_item: &'a TraktMatchItem<'a>,
    pub match_score: f64,
    pub match_type: MatchType,
}

#[derive(Debug, Clone, PartialEq)]
pub enum MatchType {
    TmdbExact,
    FuzzyTitle,
    FuzzyTitleYear,
} 