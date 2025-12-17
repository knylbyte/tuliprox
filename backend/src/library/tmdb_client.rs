use crate::library::metadata::{MediaMetadata};
use crate::library::{MetadataStorage};
use log::{debug, error, warn};
use std::collections::{HashSet};
use tokio::time::{sleep, Duration};
use url::Url;
use crate::library::tmdb::{TmdbMovieDetails, TmdbSearchResponse, TmdbSeriesInfoDetails, TmdbSeriesInfoSeasonDetails, TmdbTvSearchResponse};

pub const TMDB_API_KEY: &str = "4219e299c89411838049ab0dab19ebd5";

// TODO make this configurable in Library tmdb config
const TMDB_API_BASE_URL: &str = "https://api.themoviedb.org/3";

/// TMDB API client with rate limiting
pub struct TmdbClient {
    api_key: String,
    client: reqwest::Client,
    rate_limit_ms: u64,
    storage: MetadataStorage,
    fetched_movie_metadata: tokio::sync::RwLock<HashSet<u32>>,
    fetched_series_metadata: tokio::sync::RwLock<HashSet<u32>>,
    fetched_series_key: tokio::sync::RwLock<HashSet<String>>,
}

impl TmdbClient {
    /// Creates a new TMDB client
    pub fn new(api_key: String, rate_limit_ms: u64, client: reqwest::Client, storage: MetadataStorage) -> Self {
        Self {
            api_key,
            client,
            rate_limit_ms,
            storage,
            fetched_movie_metadata: tokio::sync::RwLock::new(HashSet::new()),
            fetched_series_metadata: tokio::sync::RwLock::new(HashSet::new()),
            fetched_series_key: tokio::sync::RwLock::new(HashSet::new()),
        }
    }

    /// Searches for a movie by title and optional year
    pub async fn search_movie(&self, tmdb_id: Option<u32>, title: &str, year: Option<u32>) -> Result<Option<MediaMetadata>, String> {
        debug!("TMDB search movie: {title}");

        if let Some(movie_id) = tmdb_id {
            return self.fetch_movie_details(movie_id).await;
        }

        // Apply rate limiting
        sleep(Duration::from_millis(self.rate_limit_ms)).await;

        let mut url = match Url::parse(&format!("{TMDB_API_BASE_URL}/search/movie")) {
            Ok(url) => url,
            Err(err) => {
                error!("Failed to parse URL for tmdb movie search: {err}");
                return Err(format!("Failed to parse URL for tmdb movie search: {err}"));
            }
        };
        url.query_pairs_mut().append_pair("api_key", &self.api_key);
        url.query_pairs_mut().append_pair("query", title);
        if let Some(y) = year {
            url.query_pairs_mut().append_pair("year", y.to_string().as_str());
        }


        match self.client.get(url).send().await {
            Ok(response) => {
                if response.status().is_success() {
                    match response.json::<TmdbSearchResponse>().await {
                        Ok(search_result) => {
                            if let Some(movie) = search_result.results.first() {
                                self.fetch_movie_details(movie.id).await
                            } else {
                                debug!("No TMDB results for movie: {title}");
                                Ok(None)
                            }
                        }
                        Err(e) => {
                            error!("Failed to parse TMDB search response: {e}");
                            Err(format!("Failed to parse TMDB search response: {e}"))
                        }
                    }
                } else {
                    warn!("TMDB API error: {}", response.status());
                    Err(format!("TMDB API error: {}", response.status()))
                }
            }
            Err(e) => {
                error!("TMDB API request failed: {e}");
                Err(format!("TMDB API request failed: {e}"))
            }
        }
    }

    /// Fetches detailed movie information
    async fn fetch_movie_details(&self, movie_id: u32) -> Result<Option<MediaMetadata>, String> {
        if self.fetched_movie_metadata.read().await.contains(&movie_id) {
            return Ok(None);
        }

        sleep(Duration::from_millis(self.rate_limit_ms)).await;
        let url = format!("{TMDB_API_BASE_URL}/movie/{movie_id}?api_key={}&append_to_response=credits,videos,external_ids", self.api_key);
        let response = self.client.get(&url).send().await.map_err(|err| err.to_string())?;
        if !response.status().is_success() {
            warn!("TMDB API error fetching movie details: {}", response.status());
            return Err(format!("TMDB API error fetching movie details: {}", response.status()));
        }

        let content_bytes = response.bytes().await.map_err(|err| err.to_string())?;
        let _raw_data_path = self.storage.store_tmdb_movie_info(movie_id, &content_bytes).await.map_err(|err| err.to_string())?;
        let details: TmdbMovieDetails = serde_json::from_slice(&content_bytes).map_err(|err| err.to_string())?;
        self.fetched_movie_metadata.write().await.insert(movie_id);

        Ok(Some(MediaMetadata::Movie(details.to_meta_data())))
    }


    /// Searches for a TV series by title and optional year
    pub async fn search_series(&self, tmdb_id: Option<u32>, title: &str, year: Option<u32>) -> Result<Option<MediaMetadata>, String> {
        let key = format!("{title}-{tmdb_id:?}-{year:?}");
        if self.fetched_series_key.read().await.contains(&key) {
            return Ok(None);
        }

        if let Some(series_id) = tmdb_id {
            let result = self.fetch_series_details(series_id).await;
            if let Ok(Some(_)) = result {
                self.fetched_series_key.write().await.insert(key);
            }
            return result;
        }

        sleep(Duration::from_millis(self.rate_limit_ms)).await;

        let mut url = match Url::parse(&format!("{TMDB_API_BASE_URL}/search/tv")) {
            Ok(url) => url,
            Err(err) => {
                error!("Failed to parse URL for tmdb series search: {err}");
                return Err(format!("Failed to parse URL for tmdb series search: {err}"));
            }
        };
        url.query_pairs_mut().append_pair("api_key", &self.api_key);
        url.query_pairs_mut().append_pair("query", title);
        if let Some(y) = year {
            url.query_pairs_mut().append_pair("first_air_date_year", y.to_string().as_str());
        }

        debug!("TMDB search series: {title}");

        let result = match self.client.get(url).send().await {
            Ok(response) => {
                if response.status().is_success() {
                    match response.json::<TmdbTvSearchResponse>().await {
                        Ok(search_result) => {
                            if let Some(series) = search_result.results.first() {
                                self.fetch_series_details(series.id).await
                            } else {
                                debug!("No TMDB results for series: {title}");
                                Ok(None)
                            }
                        }
                        Err(e) => {
                            error!("Failed to parse TMDB TV search response: {e}");
                            Err(format!("Failed to parse TMDB TV search response: {e}"))
                        }
                    }
                } else {
                    warn!("TMDB API error: {}", response.status());
                    Err(format!("TMDB API error: {}", response.status()))
                }
            }
            Err(e) => {
                error!("TMDB API request failed: {e}");
                Err(format!("TMDB API request failed: {e}"))
            }
        };
        if let Ok(Some(_)) = result {
            self.fetched_series_key.write().await.insert(key);
        }
        result
    }

    /// Fetches detailed TV series information
    async fn fetch_series_details(&self, series_id: u32) -> Result<Option<MediaMetadata>, String> {
        if self.fetched_series_metadata.read().await.contains(&series_id) {
            return Ok(None);
        }

        sleep(Duration::from_millis(self.rate_limit_ms)).await;

        let url = format!("{TMDB_API_BASE_URL}/tv/{series_id}?api_key={}&append_to_response=credits,videos,external_ids", self.api_key);

        match self.client.get(&url).send().await {
            Ok(response) => {
                if response.status().is_success() {
                    let content_bytes = response.bytes().await.map_err(|err| err.to_string())?;
                    let _raw_data_path = self.storage.store_tmdb_series_info(series_id, &content_bytes).await.map_err(|err| err.to_string())?;
                    self.fetched_series_metadata.write().await.insert(series_id);

                    match serde_json::from_slice::<TmdbSeriesInfoDetails>(&content_bytes) {
                        Ok(mut series_details) => {
                            let seasons = if series_details.number_of_seasons > 0 {
                                series_details.number_of_seasons
                            } else {
                                series_details.seasons.as_ref().map(|s| u32::try_from(s.len()).unwrap_or(0)).unwrap_or(0)
                            };
                            if seasons > 0 {
                                let mut seasons_info = vec![];
                                for season in 1..=seasons {
                                    let url = format!("{TMDB_API_BASE_URL}/tv/{series_id}/season/{season}/?api_key={}&append_to_response=credits", self.api_key);
                                    match self.client.get(&url).send().await {
                                        Ok(response) => {
                                            match response.bytes().await {
                                                Ok(season_bytes) => {
                                                    let _ = self.storage.store_tmdb_series_info_season(series_id, season, &season_bytes).await;
                                                    match serde_json::from_slice::<TmdbSeriesInfoSeasonDetails>(&content_bytes) {
                                                        Ok(season_details) => {
                                                            seasons_info.push(season_details);
                                                        }
                                                        Err(err) => {
                                                            error!("Failed to parse series season details: tmdb-id: {series_id} season: {season} err: {err}");
                                                        }
                                                    }
                                                }
                                                Err(err) => {
                                                    error!("Failed to fetch series season details: tmdb-id: {series_id} season: {season} err: {err}");
                                                }
                                            }
                                        }
                                        Err(err) => error!("Failed to fetch series season details: tmdb-id: {series_id} season: {season} err: {err}"),
                                    }
                                }

                                if !seasons_info.is_empty() {
                                    series_details.episodes = Some(seasons_info);
                                }
                            }

                            Ok(Some(MediaMetadata::Series(series_details.to_meta_data())))
                        }
                        Err(e) => {
                            error!("Failed to parse TMDB series details: {e}");
                            Err(format!("Failed to parse TMDB series details: {e}"))
                        }
                    }
                } else {
                    warn!("TMDB API error fetching series details: {}", response.status());
                    Err(format!("TMDB API error fetching series details: {}", response.status()))
                }
            }
            Err(e) => {
                error!("TMDB API request failed: {e}");
                Err(format!("TMDB API request failed: {e}"))
            }
        }
    }
}
