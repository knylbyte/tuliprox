use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Source of metadata information
#[derive(Default, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MetadataSource {
    #[default]
    /// Metadata from TMDB API
    Tmdb,
    /// Metadata from Kodi NFO file
    KodiNfo,
    /// Metadata from Jellyfin/Emby metadata files
    JellyfinEmby,
    /// Metadata from Plex metadata files
    Plex,
    /// Metadata parsed from filename
    FilenameParsed,
    /// Manually entered metadata
    Manual,
}

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct ExternalVideoMetadata {
    pub name: String, //"Official Trailer",
    pub key: String,
    pub site: String, // "YouTube",
    pub video_type: String, // "Trailer", "Teaser"
    pub official: bool,
}

/// Movie metadata
#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct MovieMetadata {
    pub title: String,
    // Original title (if different from title)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original_title: Option<String>,
    // Release year
    #[serde(skip_serializing_if = "Option::is_none")]
    pub year: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plot: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tagline: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime: Option<u32>,
    // MPAA rating (e.g., "PG-13", "R")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mpaa: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub imdb_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tmdb_id: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tvdb_id: Option<u32>,
    // Rating (0.0 - 10.0)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rating: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub genres: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub directors: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub writers: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actors: Option<Vec<Actor>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub studios: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub poster: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fanart: Option<String>,
    pub source: MetadataSource,
    pub last_updated: i64,
    pub videos: Option<Vec<ExternalVideoMetadata>>,
}

/// Series/TV show metadata
#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct SeriesMetadata {
    pub title: String,
    // Original title (if different)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original_title: Option<String>,
    // First aired year
    #[serde(skip_serializing_if = "Option::is_none")]
    pub year: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plot: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mpaa: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub imdb_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tmdb_id: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tvdb_id: Option<u32>,
    // Rating (0.0 - 10.0)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rating: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub genres: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actors: Option<Vec<Actor>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub studios: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub poster: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fanart: Option<String>,
    // Status (e.g., "Continuing", "Ended")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub episodes: Option<Vec<EpisodeMetadata>>,
    pub source: MetadataSource,
    /// Last updated timestamp (Unix epoch)
    pub last_updated: i64,
}

/// Episode metadata for TV series
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpisodeMetadata {
    pub title: String,
    pub season: u32,
    pub episode: u32,
    /// Aired date (ISO 8601 format)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aired: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plot: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime: Option<u32>,
    // Rating (0.0 - 10.0)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rating: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thumb: Option<String>,
    pub file_path: String,
}

/// Actor information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Actor {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thumb: Option<String>,
}

/// Complete video metadata (either movie or series)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum MediaMetadata {
    #[serde(rename = "movie")]
    Movie(MovieMetadata),
    #[serde(rename = "series")]
    Series(SeriesMetadata),
}

impl MediaMetadata {
    pub fn title(&self) -> &str {
        match self {
            MediaMetadata::Movie(m) => &m.title,
            MediaMetadata::Series(s) => &s.title,
        }
    }

    pub fn year(&self) -> Option<u32> {
        match self {
            MediaMetadata::Movie(m) => m.year,
            MediaMetadata::Series(s) => s.year,
        }
    }

    pub fn imdb_id(&self) -> Option<&str> {
        match self {
            MediaMetadata::Movie(m) => m.imdb_id.as_deref(),
            MediaMetadata::Series(s) => s.imdb_id.as_deref(),
        }
    }

    pub fn tmdb_id(&self) -> Option<u32> {
        match self {
            MediaMetadata::Movie(m) => m.tmdb_id,
            MediaMetadata::Series(s) => s.tmdb_id,
        }
    }

    pub fn poster(&self) -> Option<&str> {
        match self {
            MediaMetadata::Movie(m) => m.poster.as_deref(),
            MediaMetadata::Series(s) => s.poster.as_deref(),
        }
    }

    pub fn source(&self) -> &MetadataSource {
        match self {
            MediaMetadata::Movie(m) => &m.source,
            MediaMetadata::Series(s) => &s.source,
        }
    }

    pub fn last_updated(&self) -> i64 {
        match self {
            MediaMetadata::Movie(m) => m.last_updated,
            MediaMetadata::Series(s) => s.last_updated,
        }
    }

    pub fn is_movie(&self) -> bool {
        matches!(self, MediaMetadata::Movie(_))
    }

    pub fn is_series(&self) -> bool {
        matches!(self, MediaMetadata::Series(_))
    }
}

/// Metadata cache entry that links a file to its metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetadataCacheEntry {
    pub uuid: String,
    pub file_path: String,
    pub file_size: u64,
    pub file_modified: i64,
    pub metadata: MediaMetadata,
}

impl MetadataCacheEntry {
    /// Creates a new cache entry with a generated UUID
    pub fn new(
        file_path: String,
        file_size: u64,
        file_modified: i64,
        metadata: MediaMetadata,
    ) -> Self {
        Self {
            uuid: Self::generate_uuid(),
            file_path,
            file_size,
            file_modified,
            metadata,
        }
    }

    /// Generates a simple UUID-like identifier
    fn generate_uuid() -> String {
        Uuid::new_v4().to_string()
    }

    /// Checks if the file has been modified since this entry was created
    pub fn is_file_modified(&self, current_size: u64, current_modified: i64) -> bool {
        self.file_size != current_size || self.file_modified != current_modified
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_movie_metadata_creation() {
        let movie = MovieMetadata {
            title: "The Matrix".to_string(),
            original_title: None,
            year: Some(1999),
            plot: Some("A computer hacker learns about the true nature of reality.".to_string()),
            tagline: Some("Welcome to the Real World".to_string()),
            runtime: Some(136),
            mpaa: Some("R".to_string()),
            imdb_id: Some("tt0133093".to_string()),
            tmdb_id: Some(603),
            rating: Some(8.7),
            genres: Some(vec!["Action".to_string(), "Sci-Fi".to_string()]),
            directors: Some(vec!["Lana Wachowski".to_string(), "Lilly Wachowski".to_string()]),
            studios: Some(vec!["Warner Bros.".to_string()]),
            source: MetadataSource::Tmdb,
            last_updated: 0,
            ..MovieMetadata::default()
        };

        assert_eq!(movie.title, "The Matrix");
        assert_eq!(movie.year, Some(1999));
    }

    #[test]
    fn test_video_metadata_accessors() {
        let movie_meta = MediaMetadata::Movie(MovieMetadata {
            title: "Inception".to_string(),
            year: Some(2010),
            imdb_id: Some("tt1375666".to_string()),
            tmdb_id: Some(27205),
            fanart: None,
            source: MetadataSource::Tmdb,
            last_updated: 0,
            ..MovieMetadata::default()
        });

        assert_eq!(movie_meta.title(), "Inception");
        assert_eq!(movie_meta.year(), Some(2010));
        assert_eq!(movie_meta.imdb_id(), Some("tt1375666"));
        assert_eq!(movie_meta.tmdb_id(), Some(27205));
        assert!(movie_meta.is_movie());
        assert!(!movie_meta.is_series());
    }
}
