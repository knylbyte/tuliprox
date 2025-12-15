use serde::{Deserialize, Serialize};

/// Source of metadata information
#[derive(Default, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MetadataSource {
    /// Metadata from Kodi NFO file
    #[default]
    KodiNfo,
    /// Metadata from Jellyfin/Emby metadata files
    JellyfinEmby,
    /// Metadata from Plex metadata files
    Plex,
    /// Metadata from TMDB API
    Tmdb,
    /// Metadata parsed from filename
    FilenameParsed,
    /// Manually entered metadata
    Manual,
}

/// Movie metadata
#[derive(Default, Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MovieMetadata {
    pub title: String,
    // Original title (if different from title)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original_title: Option<String>,
    // Release year
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub year: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plot: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tagline: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime: Option<u32>,
    // MPAA rating (e.g., "PG-13", "R")
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mpaa: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub imdb_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tmdb_id: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tvdb_id: Option<u32>,
    // Rating (0.0 - 10.0)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rating: Option<f64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub genres: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub directors: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub writers: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actors: Vec<Actor>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub studios: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub poster: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fanart: Option<String>,
    pub source: MetadataSource,
    pub last_updated: i64,
}

/// Series/TV show metadata
#[derive(Default, Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SeriesMetadata {
    pub title: String,
    // Original title (if different)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original_title: Option<String>,
    // First aired year
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub year: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plot: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mpaa: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub imdb_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tmdb_id: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tvdb_id: Option<u32>,
    // Rating (0.0 - 10.0)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rating: Option<f64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub genres: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actors: Vec<Actor>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub studios: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub poster: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fanart: Option<String>,
    // Status (e.g., "Continuing", "Ended")
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub episodes: Vec<EpisodeMetadata>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aired: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plot: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime: Option<u32>,
    // Rating (0.0 - 10.0)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rating: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thumb: Option<String>,
    pub file_path: String,
}

/// Actor information
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Actor {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thumb: Option<String>,
}

/// Complete video metadata (either movie or series)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
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
        use std::time::{SystemTime, UNIX_EPOCH};
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();

        // Create a simple unique ID from timestamp and random value
        format!("{:x}-{:x}", timestamp, fastrand::u64(..))
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
            tvdb_id: None,
            rating: Some(8.7),
            genres: vec!["Action".to_string(), "Sci-Fi".to_string()],
            directors: vec!["Lana Wachowski".to_string(), "Lilly Wachowski".to_string()],
            writers: vec![],
            actors: vec![],
            studios: vec!["Warner Bros.".to_string()],
            poster: None,
            fanart: None,
            source: MetadataSource::Tmdb,
            last_updated: 0,
        };

        assert_eq!(movie.title, "The Matrix");
        assert_eq!(movie.year, Some(1999));
    }

    #[test]
    fn test_video_metadata_accessors() {
        let movie_meta = MediaMetadata::Movie(MovieMetadata {
            title: "Inception".to_string(),
            original_title: None,
            year: Some(2010),
            plot: None,
            tagline: None,
            runtime: None,
            mpaa: None,
            imdb_id: Some("tt1375666".to_string()),
            tmdb_id: Some(27205),
            tvdb_id: None,
            rating: None,
            genres: vec![],
            directors: vec![],
            writers: vec![],
            actors: vec![],
            studios: vec![],
            poster: None,
            fanart: None,
            source: MetadataSource::Tmdb,
            last_updated: 0,
        });

        assert_eq!(movie_meta.title(), "Inception");
        assert_eq!(movie_meta.year(), Some(2010));
        assert_eq!(movie_meta.imdb_id(), Some("tt1375666"));
        assert_eq!(movie_meta.tmdb_id(), Some(27205));
        assert!(movie_meta.is_movie());
        assert!(!movie_meta.is_series());
    }
}
