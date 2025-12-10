use serde::{Deserialize, Serialize};
use crate::error::{TuliproxError, info_err};
use crate::utils::default_as_true;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct VodConfigDto {
    #[serde(default)]
    pub enabled: bool,

    #[serde(default)]
    pub scan_directories: Vec<VodScanDirectoryDto>,

    #[serde(default)]
    pub supported_extensions: Vec<String>,

    #[serde(default)]
    pub metadata: VodMetadataConfigDto,

    #[serde(default)]
    pub classification: VodClassificationConfigDto,

    #[serde(default)]
    pub playlist: VodPlaylistConfigDto,

    #[serde(default)]
    pub file_serving: VodFileServingConfigDto,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct VodScanDirectoryDto {
    pub path: String,

    #[serde(default)]
    pub dir_type: VodDirectoryType,

    #[serde(default = "default_as_true")]
    pub recursive: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum VodDirectoryType {
    #[default]
    Auto,
    Movie,
    Series,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(deny_unknown_fields)]
pub struct VodMetadataConfigDto {
    #[serde(default)]
    pub read_existing: VodMetadataReadConfigDto,

    #[serde(default)]
    pub tmdb: VodTmdbConfigDto,

    #[serde(default = "default_as_true")]
    pub fallback_to_filename: bool,

    #[serde(default)]
    pub storage: VodMetadataStorageConfigDto,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct VodMetadataReadConfigDto {
    #[serde(default = "default_as_true")]
    pub kodi_nfo: bool,

    #[serde(default = "default_as_true")]
    pub jellyfin_metadata: bool,

    #[serde(default = "default_as_true")]
    pub plex_metadata: bool,
}

impl Default for VodMetadataReadConfigDto {
    fn default() -> Self {
        Self {
            kodi_nfo: true,
            jellyfin_metadata: true,
            plex_metadata: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct VodTmdbConfigDto {
    #[serde(default)]
    pub enabled: bool,

    #[serde(default)]
    pub api_key: String,

    #[serde(default = "default_rate_limit_ms")]
    pub rate_limit_ms: u64,

    #[serde(default = "default_cache_duration_days")]
    pub cache_duration_days: u32,

    #[serde(default = "default_language")]
    pub language: String,
}

impl Default for VodTmdbConfigDto {
    fn default() -> Self {
        Self {
            enabled: false,
            api_key: String::new(),
            rate_limit_ms: default_rate_limit_ms(),
            cache_duration_days: default_cache_duration_days(),
            language: default_language(),
        }
    }
}

fn default_rate_limit_ms() -> u64 {
    250
}

fn default_cache_duration_days() -> u32 {
    30
}

fn default_language() -> String {
    "en-US".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct VodMetadataStorageConfigDto {
    #[serde(default = "default_storage_location")]
    pub location: String,

    #[serde(default = "default_storage_formats")]
    pub formats: Vec<VodMetadataFormat>,
}

impl Default for VodMetadataStorageConfigDto {
    fn default() -> Self {
        Self {
            location: default_storage_location(),
            formats: default_storage_formats(),
        }
    }
}

fn default_storage_location() -> String {
    "./vod_metadata".to_string()
}

fn default_storage_formats() -> Vec<VodMetadataFormat> {
    vec![VodMetadataFormat::Json, VodMetadataFormat::Nfo]
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum VodMetadataFormat {
    Json,
    Nfo,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct VodClassificationConfigDto {
    #[serde(default = "default_series_patterns")]
    pub series_patterns: Vec<String>,

    #[serde(default)]
    pub series_directory_patterns: Vec<String>,
}

impl Default for VodClassificationConfigDto {
    fn default() -> Self {
        Self {
            series_patterns: default_series_patterns(),
            series_directory_patterns: Vec::new(),
        }
    }
}

fn default_series_patterns() -> Vec<String> {
    vec![
        r"S\d{2}E\d{2}".to_string(),
        r"s\d{2}e\d{2}".to_string(),
        r"\d{1,2}x\d{1,2}".to_string(),
        r"Season\s*\d+".to_string(),
        r"Episode\s*\d+".to_string(),
    ]
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct VodPlaylistConfigDto {
    #[serde(default = "default_movie_category")]
    pub movie_category: String,

    #[serde(default = "default_series_category")]
    pub series_category: String,
}

impl Default for VodPlaylistConfigDto {
    fn default() -> Self {
        Self {
            movie_category: default_movie_category(),
            series_category: default_series_category(),
        }
    }
}

fn default_movie_category() -> String {
    "Local Movies".to_string()
}

fn default_series_category() -> String {
    "Local TV Shows".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct VodFileServingConfigDto {
    #[serde(default = "default_file_serving_method")]
    pub method: String,
}

impl Default for VodFileServingConfigDto {
    fn default() -> Self {
        Self {
            method: default_file_serving_method(),
        }
    }
}

fn default_file_serving_method() -> String {
    "file".to_string()
}

impl Default for VodConfigDto {
    fn default() -> Self {
        Self {
            enabled: false,
            scan_directories: Vec::new(),
            supported_extensions: vec![
                ".mp4".to_string(),
                ".mkv".to_string(),
                ".avi".to_string(),
                ".mov".to_string(),
                ".ts".to_string(),
                ".m4v".to_string(),
                ".webm".to_string(),
            ],
            metadata: VodMetadataConfigDto::default(),
            classification: VodClassificationConfigDto::default(),
            playlist: VodPlaylistConfigDto::default(),
            file_serving: VodFileServingConfigDto::default(),
        }
    }
}

impl VodConfigDto {
    pub fn prepare(&mut self) -> Result<(), TuliproxError> {
        // Validate enabled state
        if self.enabled && self.scan_directories.is_empty() {
            return Err(info_err!("VOD enabled but no scan_directories configured".to_string()));
        }

        // Validate scan directories
        for dir in &self.scan_directories {
            if dir.path.is_empty() {
                return Err(info_err!("VOD scan directory path cannot be empty".to_string()));
            }
        }

        // Validate TMDB config if enabled
        if self.metadata.tmdb.enabled && self.metadata.tmdb.api_key.is_empty() {
            return Err(info_err!("TMDB enabled but api_key is empty".to_string()));
        }

        // Validate metadata storage location
        if self.metadata.storage.location.is_empty() {
            return Err(info_err!("Metadata storage location cannot be empty".to_string()));
        }

        // Validate file serving method
        if !matches!(self.file_serving.method.as_str(), "file" | "absolute") {
            return Err(info_err!("Invalid file_serving method. Must be 'file' or 'absolute'".to_string()));
        }

        Ok(())
    }
}
