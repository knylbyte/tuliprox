use log::{debug, error, warn};
use serde::Deserialize;
use tokio::time::{sleep, Duration};

use crate::vod::metadata::{Actor, MetadataSource, MovieMetadata, SeriesMetadata, VideoMetadata};

/// Simple URL encoding for query parameters
fn encode_query_param(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => c.to_string(),
            ' ' => "+".to_string(),
            _ => format!("%{:02X}", c as u8),
        })
        .collect()
}

const TMDB_API_BASE_URL: &str = "https://api.themoviedb.org/3";
const TMDB_IMAGE_BASE_URL: &str = "https://image.tmdb.org/t/p/w500";

/// TMDB API client with rate limiting
pub struct TmdbClient {
    api_key: String,
    client: reqwest::Client,
    rate_limit_ms: u64,
}

impl TmdbClient {
    /// Creates a new TMDB client
    pub fn new(api_key: String, rate_limit_ms: u64) -> Self {
        Self {
            api_key,
            client: reqwest::Client::new(),
            rate_limit_ms,
        }
    }

    /// Searches for a movie by title and optional year
    pub async fn search_movie(&self, title: &str, year: Option<u32>) -> Option<VideoMetadata> {
        // Apply rate limiting
        sleep(Duration::from_millis(self.rate_limit_ms)).await;

        let mut url = format!(
            "{}/search/movie?api_key={}&query={}",
            TMDB_API_BASE_URL,
            self.api_key,
            encode_query_param(title)
        );

        if let Some(y) = year {
            url.push_str(&format!("&year={}", y));
        }

        debug!("TMDB search movie: {}", title);

        match self.client.get(&url).send().await {
            Ok(response) => {
                if response.status().is_success() {
                    match response.json::<TmdbSearchResponse>().await {
                        Ok(search_result) => {
                            if let Some(movie) = search_result.results.first() {
                                self.fetch_movie_details(movie.id).await
                            } else {
                                debug!("No TMDB results for movie: {}", title);
                                None
                            }
                        }
                        Err(e) => {
                            error!("Failed to parse TMDB search response: {}", e);
                            None
                        }
                    }
                } else {
                    warn!("TMDB API error: {}", response.status());
                    None
                }
            }
            Err(e) => {
                error!("TMDB API request failed: {}", e);
                None
            }
        }
    }

    /// Fetches detailed movie information
    async fn fetch_movie_details(&self, movie_id: u32) -> Option<VideoMetadata> {
        sleep(Duration::from_millis(self.rate_limit_ms)).await;

        let url = format!(
            "{}/movie/{}?api_key={}&append_to_response=credits",
            TMDB_API_BASE_URL, movie_id, self.api_key
        );

        match self.client.get(&url).send().await {
            Ok(response) => {
                if response.status().is_success() {
                    match response.json::<TmdbMovieDetails>().await {
                        Ok(details) => Some(VideoMetadata::Movie(MovieMetadata {
                            title: details.title,
                            original_title: Some(details.original_title),
                            year: details.release_date.split('-').next().and_then(|y| y.parse().ok()),
                            plot: Some(details.overview),
                            tagline: details.tagline,
                            runtime: Some(details.runtime),
                            mpaa: None, // TMDB doesn't provide MPAA rating in basic response
                            imdb_id: details.imdb_id,
                            tmdb_id: Some(details.id),
                            rating: Some(details.vote_average),
                            genres: details.genres.iter().map(|g| g.name.clone()).collect(),
                            directors: details
                                .credits
                                .as_ref()
                                .map(|c| {
                                    c.crew
                                        .iter()
                                        .filter(|crew| crew.job == "Director")
                                        .map(|crew| crew.name.clone())
                                        .collect()
                                })
                                .unwrap_or_default(),
                            writers: details
                                .credits
                                .as_ref()
                                .map(|c| {
                                    c.crew
                                        .iter()
                                        .filter(|crew| crew.job == "Writer" || crew.job == "Screenplay")
                                        .map(|crew| crew.name.clone())
                                        .collect()
                                })
                                .unwrap_or_default(),
                            actors: details
                                .credits
                                .as_ref()
                                .map(|c| {
                                    c.cast
                                        .iter()
                                        .take(10) // Limit to top 10 actors
                                        .map(|actor| Actor {
                                            name: actor.name.clone(),
                                            role: Some(actor.character.clone()),
                                            thumb: actor
                                                .profile_path
                                                .as_ref()
                                                .map(|p| format!("{}{}", TMDB_IMAGE_BASE_URL, p)),
                                        })
                                        .collect()
                                })
                                .unwrap_or_default(),
                            studios: details
                                .production_companies
                                .iter()
                                .map(|c| c.name.clone())
                                .collect(),
                            poster: details
                                .poster_path
                                .map(|p| format!("{}{}", TMDB_IMAGE_BASE_URL, p)),
                            fanart: details
                                .backdrop_path
                                .map(|p| format!("{}{}", TMDB_IMAGE_BASE_URL, p)),
                            source: MetadataSource::Tmdb,
                            last_updated: chrono::Utc::now().timestamp(),
                        })),
                        Err(e) => {
                            error!("Failed to parse TMDB movie details: {}", e);
                            None
                        }
                    }
                } else {
                    warn!("TMDB API error fetching movie details: {}", response.status());
                    None
                }
            }
            Err(e) => {
                error!("TMDB API request failed: {}", e);
                None
            }
        }
    }

    /// Searches for a TV series by title and optional year
    pub async fn search_series(&self, title: &str, year: Option<u32>) -> Option<VideoMetadata> {
        sleep(Duration::from_millis(self.rate_limit_ms)).await;

        let mut url = format!(
            "{}/search/tv?api_key={}&query={}",
            TMDB_API_BASE_URL,
            self.api_key,
            encode_query_param(title)
        );

        if let Some(y) = year {
            url.push_str(&format!("&first_air_date_year={}", y));
        }

        debug!("TMDB search series: {}", title);

        match self.client.get(&url).send().await {
            Ok(response) => {
                if response.status().is_success() {
                    match response.json::<TmdbTvSearchResponse>().await {
                        Ok(search_result) => {
                            if let Some(series) = search_result.results.first() {
                                self.fetch_series_details(series.id).await
                            } else {
                                debug!("No TMDB results for series: {}", title);
                                None
                            }
                        }
                        Err(e) => {
                            error!("Failed to parse TMDB TV search response: {}", e);
                            None
                        }
                    }
                } else {
                    warn!("TMDB API error: {}", response.status());
                    None
                }
            }
            Err(e) => {
                error!("TMDB API request failed: {}", e);
                None
            }
        }
    }

    /// Fetches detailed TV series information
    async fn fetch_series_details(&self, series_id: u32) -> Option<VideoMetadata> {
        sleep(Duration::from_millis(self.rate_limit_ms)).await;

        let url = format!(
            "{}/tv/{}?api_key={}&append_to_response=credits",
            TMDB_API_BASE_URL, series_id, self.api_key
        );

        match self.client.get(&url).send().await {
            Ok(response) => {
                if response.status().is_success() {
                    match response.json::<TmdbSeriesDetails>().await {
                        Ok(details) => Some(VideoMetadata::Series(SeriesMetadata {
                            title: details.name,
                            original_title: Some(details.original_name),
                            year: details
                                .first_air_date
                                .split('-')
                                .next()
                                .and_then(|y| y.parse().ok()),
                            plot: Some(details.overview),
                            mpaa: None,
                            imdb_id: None, // TMDB TV doesn't always provide IMDB ID
                            tmdb_id: Some(details.id),
                            tvdb_id: None, // TMDB doesn't provide TVDB ID directly
                            rating: Some(details.vote_average),
                            genres: details.genres.iter().map(|g| g.name.clone()).collect(),
                            actors: details
                                .credits
                                .as_ref()
                                .map(|c| {
                                    c.cast
                                        .iter()
                                        .take(10)
                                        .map(|actor| Actor {
                                            name: actor.name.clone(),
                                            role: Some(actor.character.clone()),
                                            thumb: actor
                                                .profile_path
                                                .as_ref()
                                                .map(|p| format!("{}{}", TMDB_IMAGE_BASE_URL, p)),
                                        })
                                        .collect()
                                })
                                .unwrap_or_default(),
                            studios: details.networks.iter().map(|n| n.name.clone()).collect(),
                            poster: details
                                .poster_path
                                .map(|p| format!("{}{}", TMDB_IMAGE_BASE_URL, p)),
                            fanart: details
                                .backdrop_path
                                .map(|p| format!("{}{}", TMDB_IMAGE_BASE_URL, p)),
                            status: Some(details.status),
                            episodes: Vec::new(), // Episodes would need separate API calls
                            source: MetadataSource::Tmdb,
                            last_updated: chrono::Utc::now().timestamp(),
                        })),
                        Err(e) => {
                            error!("Failed to parse TMDB series details: {}", e);
                            None
                        }
                    }
                } else {
                    warn!("TMDB API error fetching series details: {}", response.status());
                    None
                }
            }
            Err(e) => {
                error!("TMDB API request failed: {}", e);
                None
            }
        }
    }
}

// TMDB API response structures
#[derive(Debug, Deserialize)]
struct TmdbSearchResponse {
    results: Vec<TmdbMovieSearchResult>,
}

#[derive(Debug, Deserialize)]
struct TmdbMovieSearchResult {
    id: u32,
}

#[derive(Debug, Deserialize)]
struct TmdbTvSearchResponse {
    results: Vec<TmdbTvSearchResult>,
}

#[derive(Debug, Deserialize)]
struct TmdbTvSearchResult {
    id: u32,
}

#[derive(Debug, Deserialize)]
struct TmdbMovieDetails {
    id: u32,
    title: String,
    original_title: String,
    overview: String,
    tagline: Option<String>,
    release_date: String,
    runtime: u32,
    vote_average: f32,
    imdb_id: Option<String>,
    poster_path: Option<String>,
    backdrop_path: Option<String>,
    genres: Vec<TmdbGenre>,
    production_companies: Vec<TmdbCompany>,
    credits: Option<TmdbCredits>,
}

#[derive(Debug, Deserialize)]
struct TmdbSeriesDetails {
    id: u32,
    name: String,
    original_name: String,
    overview: String,
    first_air_date: String,
    vote_average: f32,
    poster_path: Option<String>,
    backdrop_path: Option<String>,
    status: String,
    genres: Vec<TmdbGenre>,
    networks: Vec<TmdbNetwork>,
    credits: Option<TmdbCredits>,
}

#[derive(Debug, Deserialize)]
struct TmdbGenre {
    name: String,
}

#[derive(Debug, Deserialize)]
struct TmdbCompany {
    name: String,
}

#[derive(Debug, Deserialize)]
struct TmdbNetwork {
    name: String,
}

#[derive(Debug, Deserialize)]
struct TmdbCredits {
    cast: Vec<TmdbCast>,
    crew: Vec<TmdbCrew>,
}

#[derive(Debug, Deserialize)]
struct TmdbCast {
    name: String,
    character: String,
    profile_path: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TmdbCrew {
    name: String,
    job: String,
}
