use shared::model::config::{
    VodConfigDto, VodDirectoryType, VodMetadataFormat, VodScanDirectoryDto,
};
use regex::Regex;
use std::collections::HashSet;

#[derive(Debug, Clone)]
pub struct VodConfig {
    pub enabled: bool,
    pub scan_directories: Vec<VodScanDirectory>,
    pub supported_extensions: HashSet<String>,
    pub metadata: VodMetadataConfig,
    pub classification: VodClassificationConfig,
    pub playlist: VodPlaylistConfig,
    pub file_serving: VodFileServingConfig,
}

#[derive(Debug, Clone)]
pub struct VodScanDirectory {
    pub path: String,
    pub dir_type: VodDirectoryType,
    pub recursive: bool,
}

#[derive(Debug, Clone)]
pub struct VodMetadataConfig {
    pub read_existing: VodMetadataReadConfig,
    pub tmdb: VodTmdbConfig,
    pub fallback_to_filename: bool,
    pub storage: VodMetadataStorageConfig,
}

#[derive(Debug, Clone)]
pub struct VodMetadataReadConfig {
    pub kodi_nfo: bool,
    pub jellyfin_metadata: bool,
    pub plex_metadata: bool,
}

#[derive(Debug, Clone)]
pub struct VodTmdbConfig {
    pub enabled: bool,
    pub api_key: String,
    pub rate_limit_ms: u64,
    pub cache_duration_days: u32,
    pub language: String,
}

#[derive(Debug, Clone)]
pub struct VodMetadataStorageConfig {
    pub location: String,
    pub formats: Vec<VodMetadataFormat>,
    pub write_json: bool,
    pub write_nfo: bool,
}

#[derive(Debug, Clone)]
pub struct VodClassificationConfig {
    pub series_patterns: Vec<Regex>,
    pub series_directory_patterns: Vec<Regex>,
}

#[derive(Debug, Clone)]
pub struct VodPlaylistConfig {
    pub movie_category: String,
    pub series_category: String,
}

#[derive(Debug, Clone)]
pub struct VodFileServingConfig {
    pub method: VodFileServingMethod,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VodFileServingMethod {
    File,      // file:// URLs
    Absolute,  // Absolute file paths
}

impl From<&VodConfigDto> for VodConfig {
    fn from(dto: &VodConfigDto) -> Self {
        // Compile series patterns
        let series_patterns = dto
            .classification
            .series_patterns
            .iter()
            .filter_map(|pattern| Regex::new(pattern).ok())
            .collect();

        // Compile directory patterns
        let series_directory_patterns = dto
            .classification
            .series_directory_patterns
            .iter()
            .filter_map(|pattern| Regex::new(pattern).ok())
            .collect();

        // Determine which formats to write
        let write_json = dto.metadata.storage.formats.contains(&VodMetadataFormat::Json);
        let write_nfo = dto.metadata.storage.formats.contains(&VodMetadataFormat::Nfo);

        // Parse file serving method
        let method = match dto.file_serving.method.as_str() {
            "absolute" => VodFileServingMethod::Absolute,
            _ => VodFileServingMethod::File,
        };

        Self {
            enabled: dto.enabled,
            scan_directories: dto
                .scan_directories
                .iter()
                .map(|d| VodScanDirectory {
                    path: d.path.clone(),
                    dir_type: d.dir_type,
                    recursive: d.recursive,
                })
                .collect(),
            supported_extensions: dto
                .supported_extensions
                .iter()
                .map(|ext| ext.to_lowercase())
                .collect(),
            metadata: VodMetadataConfig {
                read_existing: VodMetadataReadConfig {
                    kodi_nfo: dto.metadata.read_existing.kodi_nfo,
                    jellyfin_metadata: dto.metadata.read_existing.jellyfin_metadata,
                    plex_metadata: dto.metadata.read_existing.plex_metadata,
                },
                tmdb: VodTmdbConfig {
                    enabled: dto.metadata.tmdb.enabled,
                    api_key: dto.metadata.tmdb.api_key.clone(),
                    rate_limit_ms: dto.metadata.tmdb.rate_limit_ms,
                    cache_duration_days: dto.metadata.tmdb.cache_duration_days,
                    language: dto.metadata.tmdb.language.clone(),
                },
                fallback_to_filename: dto.metadata.fallback_to_filename,
                storage: VodMetadataStorageConfig {
                    location: dto.metadata.storage.location.clone(),
                    formats: dto.metadata.storage.formats.clone(),
                    write_json,
                    write_nfo,
                },
            },
            classification: VodClassificationConfig {
                series_patterns,
                series_directory_patterns,
            },
            playlist: VodPlaylistConfig {
                movie_category: dto.playlist.movie_category.clone(),
                series_category: dto.playlist.series_category.clone(),
            },
            file_serving: VodFileServingConfig { method },
        }
    }
}

impl Default for VodConfig {
    fn default() -> Self {
        Self::from(&VodConfigDto::default())
    }
}
