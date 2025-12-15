use crate::library::metadata::{Actor, MediaMetadata, MetadataSource, MovieMetadata, SeriesMetadata};
use log::{debug, error, warn};
use serde::Deserialize;
use tokio::time::{sleep, Duration};
use url::Url;

pub const TMDB_API_KEY: &str = "4219e299c89411838049ab0dab19ebd5";

// TODO make this configurable in Library tmdb config
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
    pub async fn search_movie(&self, tmdb_id: Option<u32>, title: &str, year: Option<u32>) -> Option<MediaMetadata> {
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
                return None;
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
                                None
                            }
                        }
                        Err(e) => {
                            error!("Failed to parse TMDB search response: {e}");
                            None
                        }
                    }
                } else {
                    warn!("TMDB API error: {}", response.status());
                    None
                }
            }
            Err(e) => {
                error!("TMDB API request failed: {e}");
                None
            }
        }
    }

    /// Fetches detailed movie information
    async fn fetch_movie_details(&self, movie_id: u32) -> Option<MediaMetadata> {
        sleep(Duration::from_millis(self.rate_limit_ms)).await;

        let url = format!("{TMDB_API_BASE_URL}/movie/{movie_id}?api_key={}&append_to_response=credits", self.api_key);

        match self.client.get(&url).send().await {
            Ok(response) => {
                if response.status().is_success() {
                    match response.json::<TmdbMovieDetails>().await {
                        Ok(details) => Some(MediaMetadata::Movie(MovieMetadata {
                            title: details.title,
                            original_title: Some(details.original_title),
                            year: details.release_date.split('-').next().and_then(|y| y.parse().ok()),
                            plot: Some(details.overview),
                            tagline: details.tagline,
                            runtime: Some(details.runtime),
                            mpaa: None, // TMDB doesn't provide MPAA rating in basic response
                            imdb_id: details.imdb_id,
                            tmdb_id: Some(details.id),
                            tvdb_id: None,
                            rating: Some(details.vote_average),
                            genres: details.genres.as_ref().and_then(|list| {
                                let result: Vec<String> = list.iter().map(|g| g.name.clone()).collect();
                                if result.is_empty() {
                                    None
                                } else {
                                    Some(result)
                                }
                            }),
                            directors: details
                                .credits
                                .as_ref()
                                .and_then(|c| {
                                    let list: Vec<String> = c.crew
                                        .iter()
                                        .filter(|crew| crew.job == "Director")
                                        .map(|crew| crew.name.clone())
                                        .collect();
                                    if list.is_empty() {
                                        None
                                    } else {
                                        Some(list)
                                    }
                                }),
                            writers: details
                                .credits
                                .as_ref()
                                .and_then(|c| {
                                    let list: Vec<String> = c.crew
                                        .iter()
                                        .filter(|crew| crew.job == "Writer" || crew.job == "Screenplay")
                                        .map(|crew| crew.name.clone())
                                        .collect();
                                    if list.is_empty() {
                                        None
                                    } else {
                                        Some(list)
                                    }
                                }),
                            actors: details
                                .credits
                                .as_ref()
                                .and_then(|c| {
                                    let actors: Vec<Actor> = c.cast
                                        .iter()
                                        .take(10) // Limit to top 10 actors
                                        .map(|actor| Actor {
                                            name: actor.name.clone(),
                                            role: Some(actor.character.clone()),
                                            thumb: actor
                                                .profile_path
                                                .as_ref()
                                                .map(|p| format!("{TMDB_IMAGE_BASE_URL}{p}")),
                                        })
                                        .collect();

                                    if actors.is_empty() {
                                        None
                                    } else {
                                        Some(actors)
                                    }
                                }),
                            studios:
                            details.production_companies.as_ref().and_then(|list| {
                                let result: Vec<String> = list.iter().map(|n| n.name.clone()).collect();
                                if result.is_empty() {
                                    None
                                } else {
                                    Some(result)
                                }
                            }),
                            poster: details
                                .poster_path
                                .map(|p| format!("{TMDB_IMAGE_BASE_URL}{p}")),
                            fanart: details
                                .backdrop_path
                                .map(|p| format!("{TMDB_IMAGE_BASE_URL}{p}")),
                            source: MetadataSource::Tmdb,
                            last_updated: chrono::Utc::now().timestamp(),
                        })),
                        Err(e) => {
                            error!("Failed to parse TMDB movie details: {e}");
                            None
                        }
                    }
                } else {
                    warn!("TMDB API error fetching movie details: {}", response.status());
                    None
                }
            }
            Err(e) => {
                error!("TMDB API request failed: {e}");
                None
            }
        }
    }

    /// Searches for a TV series by title and optional year
    pub async fn search_series(&self, title: &str, year: Option<u32>) -> Option<MediaMetadata> {
        sleep(Duration::from_millis(self.rate_limit_ms)).await;

        let mut url = match Url::parse(&format!("{TMDB_API_BASE_URL}/search/tv")) {
            Ok(url) => url,
            Err(err) => {
                error!("Failed to parse URL for tmdb series search: {err}");
                return None;
            }
        };
        url.query_pairs_mut().append_pair("api_key", &self.api_key);
        url.query_pairs_mut().append_pair("query", title);
        if let Some(y) = year {
            url.query_pairs_mut().append_pair("first_air_date_year", y.to_string().as_str());
        }

        debug!("TMDB search series: {title}");

        match self.client.get(url).send().await {
            Ok(response) => {
                if response.status().is_success() {
                    match response.json::<TmdbTvSearchResponse>().await {
                        Ok(search_result) => {
                            if let Some(series) = search_result.results.first() {
                                self.fetch_series_details(series.id).await
                            } else {
                                debug!("No TMDB results for series: {title}");
                                None
                            }
                        }
                        Err(e) => {
                            error!("Failed to parse TMDB TV search response: {e}");
                            None
                        }
                    }
                } else {
                    warn!("TMDB API error: {}", response.status());
                    None
                }
            }
            Err(e) => {
                error!("TMDB API request failed: {e}");
                None
            }
        }
    }

    /// Fetches detailed TV series information
    async fn fetch_series_details(&self, series_id: u32) -> Option<MediaMetadata> {
        sleep(Duration::from_millis(self.rate_limit_ms)).await;

        let url = format!("{TMDB_API_BASE_URL}/tv/{series_id}?api_key={}&append_to_response=credits", self.api_key);

        match self.client.get(&url).send().await {
            Ok(response) => {
                if response.status().is_success() {
                    match response.json::<TmdbSeriesDetails>().await {
                        Ok(details) => Some(MediaMetadata::Series(SeriesMetadata {
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
                            genres: details.genres.as_ref().and_then(|list| {
                                let result: Vec<String> = list.iter().map(|g| g.name.clone()).collect();
                                if result.is_empty() {
                                    None
                                } else {
                                    Some(result)
                                }
                            }),
                            actors: details
                                .credits
                                .as_ref()
                                .and_then(|c| {
                                    let actors: Vec<Actor> = c.cast
                                        .iter()
                                        .take(10) // Limit to top 10 actors
                                        .map(|actor| Actor {
                                            name: actor.name.clone(),
                                            role: Some(actor.character.clone()),
                                            thumb: actor
                                                .profile_path
                                                .as_ref()
                                                .map(|p| format!("{TMDB_IMAGE_BASE_URL}{p}")),
                                        })
                                        .collect();

                                    if actors.is_empty() {
                                        None
                                    } else {
                                        Some(actors)
                                    }
                                }),
                            studios: details.networks.as_ref().and_then(|list| {
                                let result: Vec<String> = list.iter().map(|n| n.name.clone()).collect();
                                if result.is_empty() {
                                    None
                                } else {
                                    Some(result)
                                }
                            }),
                            poster: details
                                .poster_path
                                .map(|p| format!("{TMDB_IMAGE_BASE_URL}{p}")),
                            fanart: details
                                .backdrop_path
                                .map(|p| format!("{TMDB_IMAGE_BASE_URL}{p}")),
                            status: Some(details.status),
                            episodes: None, // Episodes would need separate API calls
                            source: MetadataSource::Tmdb,
                            last_updated: chrono::Utc::now().timestamp(),
                        })),
                        Err(e) => {
                            error!("Failed to parse TMDB series details: {e}");
                            None
                        }
                    }
                } else {
                    warn!("TMDB API error fetching series details: {}", response.status());
                    None
                }
            }
            Err(e) => {
                error!("TMDB API request failed: {e}");
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
    #[serde(default)]
    overview: String,
    tagline: Option<String>,
    #[serde(default)]
    release_date: String,
    #[serde(default)]
    runtime: u32,
    vote_average: f64,
    imdb_id: Option<String>,
    poster_path: Option<String>,
    backdrop_path: Option<String>,
    genres: Option<Vec<TmdbGenre>>,
    production_companies: Option<Vec<TmdbCompany>>,
    credits: Option<TmdbCredits>,
}

#[derive(Debug, Deserialize)]
struct TmdbSeriesDetails {
    id: u32,
    name: String,
    original_name: String,
    #[serde(default)]
    overview: String,
    #[serde(default)]
    first_air_date: String,
    vote_average: f64,
    poster_path: Option<String>,
    backdrop_path: Option<String>,
    status: String,
    genres: Option<Vec<TmdbGenre>>,
    networks: Option<Vec<TmdbNetwork>>,
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
