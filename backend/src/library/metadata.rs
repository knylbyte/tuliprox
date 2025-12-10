use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Source of metadata information
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MetadataSource {
    /// Metadata from Kodi NFO file
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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MovieMetadata {
    /// Movie title
    pub title: String,

    /// Original title (if different from title)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original_title: Option<String>,

    /// Release year
    #[serde(skip_serializing_if = "Option::is_none")]
    pub year: Option<u32>,

    /// Plot/synopsis
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plot: Option<String>,

    /// Tagline
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tagline: Option<String>,

    /// Runtime in minutes
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime: Option<u32>,

    /// MPAA rating (e.g., "PG-13", "R")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mpaa: Option<String>,

    /// `IMDb` ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub imdb_id: Option<String>,

    /// TMDB ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tmdb_id: Option<u32>,

    /// Rating (0.0 - 10.0)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rating: Option<f32>,

    /// Genres
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub genres: Vec<String>,

    /// Director(s)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub directors: Vec<String>,

    /// Writers
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub writers: Vec<String>,

    /// Actors
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actors: Vec<Actor>,

    /// Studios
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub studios: Vec<String>,

    /// Poster URL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub poster: Option<String>,

    /// Fanart URL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fanart: Option<String>,

    /// Source of this metadata
    pub source: MetadataSource,

    /// Last updated timestamp (Unix epoch)
    pub last_updated: i64,
}

/// Series/TV show metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeriesMetadata {
    /// Series title
    pub title: String,

    /// Original title (if different)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original_title: Option<String>,

    /// First aired year
    #[serde(skip_serializing_if = "Option::is_none")]
    pub year: Option<u32>,

    /// Plot/synopsis
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plot: Option<String>,

    /// `MPAA` rating
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mpaa: Option<String>,

    /// `IMDb` ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub imdb_id: Option<String>,

    /// TMDB ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tmdb_id: Option<u32>,

    /// TVDB ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tvdb_id: Option<u32>,

    /// Rating (0.0 - 10.0)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rating: Option<f32>,

    /// Genres
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub genres: Vec<String>,

    /// Actors
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actors: Vec<Actor>,

    /// Studios/Networks
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub studios: Vec<String>,

    /// Poster URL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub poster: Option<String>,

    /// Fanart URL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fanart: Option<String>,

    /// Status (e.g., "Continuing", "Ended")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,

    /// Episodes for this series
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub episodes: Vec<EpisodeMetadata>,

    /// Source of this metadata
    pub source: MetadataSource,

    /// Last updated timestamp (Unix epoch)
    pub last_updated: i64,
}

/// Episode metadata for TV series
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpisodeMetadata {
    /// Episode title
    pub title: String,

    /// Season number
    pub season: u32,

    /// Episode number
    pub episode: u32,

    /// Aired date (ISO 8601 format)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aired: Option<String>,

    /// Plot/synopsis
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plot: Option<String>,

    /// Runtime in minutes
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime: Option<u32>,

    /// Rating (0.0 - 10.0)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rating: Option<f32>,

    /// Episode thumbnail URL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thumb: Option<String>,

    /// File path for this episode
    pub file_path: PathBuf,
}

/// Actor information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Actor {
    /// Actor name
    pub name: String,

    /// Role/character name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,

    /// Thumbnail URL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thumb: Option<String>,
}

/// Complete video metadata (either movie or series)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum VideoMetadata {
    #[serde(rename = "movie")]
    Movie(MovieMetadata),

    #[serde(rename = "series")]
    Series(SeriesMetadata),
}

impl VideoMetadata {
    /// Gets the title of the video (movie or series)
    pub fn title(&self) -> &str {
        match self {
            VideoMetadata::Movie(m) => &m.title,
            VideoMetadata::Series(s) => &s.title,
        }
    }

    /// Gets the year (if available)
    pub fn year(&self) -> Option<u32> {
        match self {
            VideoMetadata::Movie(m) => m.year,
            VideoMetadata::Series(s) => s.year,
        }
    }

    /// Gets the IMDB ID (if available)
    pub fn imdb_id(&self) -> Option<&str> {
        match self {
            VideoMetadata::Movie(m) => m.imdb_id.as_deref(),
            VideoMetadata::Series(s) => s.imdb_id.as_deref(),
        }
    }

    /// Gets the TMDB ID (if available)
    pub fn tmdb_id(&self) -> Option<u32> {
        match self {
            VideoMetadata::Movie(m) => m.tmdb_id,
            VideoMetadata::Series(s) => s.tmdb_id,
        }
    }

    /// Gets the poster URL (if available)
    pub fn poster(&self) -> Option<&str> {
        match self {
            VideoMetadata::Movie(m) => m.poster.as_deref(),
            VideoMetadata::Series(s) => s.poster.as_deref(),
        }
    }

    /// Gets the metadata source
    pub fn source(&self) -> &MetadataSource {
        match self {
            VideoMetadata::Movie(m) => &m.source,
            VideoMetadata::Series(s) => &s.source,
        }
    }

    /// Gets the last updated timestamp
    pub fn last_updated(&self) -> i64 {
        match self {
            VideoMetadata::Movie(m) => m.last_updated,
            VideoMetadata::Series(s) => s.last_updated,
        }
    }

    /// Checks if this is movie metadata
    pub fn is_movie(&self) -> bool {
        matches!(self, VideoMetadata::Movie(_))
    }

    /// Checks if this is series metadata
    pub fn is_series(&self) -> bool {
        matches!(self, VideoMetadata::Series(_))
    }
}

/// Metadata cache entry that links a file to its metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetadataCacheEntry {
    /// UUID for this entry (for stable virtual IDs)
    pub uuid: String,

    /// File path
    pub file_path: PathBuf,

    /// File size in bytes (for change detection)
    pub file_size: u64,

    /// File modified timestamp (for change detection)
    pub file_modified: i64,

    /// Video metadata
    pub metadata: VideoMetadata,

    /// Virtual ID assigned to this item
    pub virtual_id: u16,
}

impl MetadataCacheEntry {
    /// Creates a new cache entry with a generated UUID
    pub fn new(
        file_path: PathBuf,
        file_size: u64,
        file_modified: i64,
        metadata: VideoMetadata,
        virtual_id: u16,
    ) -> Self {
        Self {
            uuid: Self::generate_uuid(),
            file_path,
            file_size,
            file_modified,
            metadata,
            virtual_id,
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
        let movie_meta = VideoMetadata::Movie(MovieMetadata {
            title: "Inception".to_string(),
            original_title: None,
            year: Some(2010),
            plot: None,
            tagline: None,
            runtime: None,
            mpaa: None,
            imdb_id: Some("tt1375666".to_string()),
            tmdb_id: Some(27205),
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
