use serde::{Deserialize, Serialize};
use crate::error::{TuliproxError, info_err};
use crate::utils::default_as_true;

pub fn default_metadata_path() -> String {
   "library_metadata".to_string()
}
fn default_tmdb_rate_limit_ms() -> u64 {
    250
}
fn default_tmdb_cache_duration_days() -> u32 {
    30
}
fn default_tmdb_language() -> String {
    "en-US".to_string()
}
fn default_storage_formats() -> Vec<LibraryMetadataFormat> {
    vec![]
}
fn default_movie_category() -> String {
    "Local Movies".to_string()
}
fn default_series_category() -> String {
    "Local TV Shows".to_string()
}
fn default_supported_extensions() -> Vec<String> {
    [
        "mp4",
        "mkv",
        "avi",
        "mov",
        "ts",
        "m4v",
        "webm",
    ].iter().map(ToString::to_string).collect()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(deny_unknown_fields)]
pub struct LibraryConfigDto {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub scan_directories: Vec<LibraryScanDirectoryDto>,
    #[serde(default="default_supported_extensions")]
    pub supported_extensions: Vec<String>,
    #[serde(default)]
    pub metadata: LibraryMetadataConfigDto,
    #[serde(default)]
    pub playlist: LibraryPlaylistConfigDto,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct LibraryScanDirectoryDto {
    #[serde(default = "default_as_true")]
    pub enabled: bool,
    pub path: String,
    #[serde(default)]
    pub content_type: LibraryContentType,
    #[serde(default = "default_as_true")]
    pub recursive: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum LibraryContentType {
    #[default]
    Auto,
    Movie,
    Series,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(deny_unknown_fields)]
pub struct LibraryMetadataConfigDto {
    #[serde(default = "default_metadata_path")]
    pub path: String,
    #[serde(default)]
    pub read_existing: LibraryMetadataReadConfigDto,
    #[serde(default)]
    pub tmdb: LibraryTmdbConfigDto,
    #[serde(default = "default_as_true")]
    pub fallback_to_filename: bool,
    #[serde(default = "default_storage_formats")]
    pub formats: Vec<LibraryMetadataFormat>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct LibraryMetadataReadConfigDto {
    #[serde(default = "default_as_true")]
    pub kodi: bool,
    #[serde(default = "default_as_true")]
    pub jellyfin: bool,
    #[serde(default = "default_as_true")]
    pub plex: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(deny_unknown_fields)]
pub struct LibraryTmdbConfigDto {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default = "default_tmdb_rate_limit_ms")]
    pub rate_limit_ms: u64,
    #[serde(default = "default_tmdb_cache_duration_days")]
    pub cache_duration_days: u32,
    #[serde(default = "default_tmdb_language")]
    pub language: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LibraryMetadataFormat {
    Nfo,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(deny_unknown_fields)]
pub struct LibraryPlaylistConfigDto {
    #[serde(default = "default_movie_category")]
    pub movie_category: String,
    #[serde(default = "default_series_category")]
    pub series_category: String,
}

impl LibraryConfigDto {
    pub fn prepare(&mut self) -> Result<(), TuliproxError> {
        // Validate enabled state
        if self.enabled && self.scan_directories.is_empty() {
            return Err(info_err!("Library enabled but no scan_directories configured".to_string()));
        }

        // Validate scan directories
        for dir in &self.scan_directories {
            if dir.path.is_empty() {
                return Err(info_err!("Library scan directory path cannot be empty".to_string()));
            }
        }

        // Validate metadata storage location
        if self.metadata.path.is_empty() {
            return Err(info_err!("Library Metadata storage location cannot be empty".to_string()));
        }

        Ok(())
    }
}
