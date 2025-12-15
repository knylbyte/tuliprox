use regex::Regex;
use shared::model::{LibraryConfigDto, LibraryContentType, LibraryMetadataFormat};
use crate::model::{macros};

#[derive(Debug, Clone, Default)]
pub struct LibraryScanDirectory {
    pub enabled: bool,
    pub path: String,
    pub content_type: LibraryContentType,
    pub recursive: bool,
}

#[derive(Debug, Clone)]
pub struct LibraryMetadataConfig {
    pub path: String,
    pub read_existing: LibraryMetadataReadConfig,
    pub tmdb: LibraryTmdbConfig,
    pub fallback_to_filename: bool,
    pub formats: Vec<LibraryMetadataFormat>,
}

#[derive(Debug, Clone)]
pub struct LibraryMetadataReadConfig {
    pub kodi: bool,
    pub jellyfin: bool,
    pub plex: bool,
}

#[derive(Debug, Clone)]
pub struct LibraryTmdbConfig {
    pub enabled: bool,
    pub api_key: Option<String>,
    pub rate_limit_ms: u64,
    pub cache_duration_days: u32,
    pub language: String,
}

#[derive(Debug, Clone)]
pub struct LibraryClassificationConfig {
    pub series_patterns: Vec<Regex>,
    pub series_directory_patterns: Vec<Regex>,
}

#[derive(Debug, Clone)]
pub struct LibraryPlaylistConfig {
    pub movie_category: String,
    pub series_category: String,
}


#[derive(Debug, Clone)]
pub struct LibraryConfig {
    pub enabled: bool,
    pub scan_directories: Vec<LibraryScanDirectory>,
    pub supported_extensions: Vec<String>,
    pub metadata: LibraryMetadataConfig,
    pub classification: LibraryClassificationConfig,
    pub playlist: LibraryPlaylistConfig,
}

impl Default for LibraryConfig {
    fn default() -> Self {
        Self::from(&LibraryConfigDto::default())
    }
}

macros::from_impl!(LibraryConfig);

impl From<&LibraryConfigDto> for LibraryConfig {
    fn from(dto: &LibraryConfigDto) -> Self {
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

        Self {
            enabled: dto.enabled,
            scan_directories: dto
                .scan_directories
                .iter()
                .map(|d| LibraryScanDirectory {
                    enabled: d.enabled,
                    path: d.path.clone(),
                    content_type: d.content_type,
                    recursive: d.recursive,
                })
                .collect(),
            supported_extensions: dto
                .supported_extensions
                .iter()
                .map(|ext| ext.to_lowercase())
                .collect(),
            metadata: LibraryMetadataConfig {
                path: dto.metadata.path.clone(),
                read_existing: LibraryMetadataReadConfig {
                    kodi: dto.metadata.read_existing.kodi,
                    jellyfin: dto.metadata.read_existing.jellyfin,
                    plex: dto.metadata.read_existing.plex,
                },
                tmdb: LibraryTmdbConfig {
                    enabled: dto.metadata.tmdb.enabled,
                    api_key: dto.metadata.tmdb.api_key.clone(),
                    rate_limit_ms: dto.metadata.tmdb.rate_limit_ms,
                    cache_duration_days: dto.metadata.tmdb.cache_duration_days,
                    language: dto.metadata.tmdb.language.clone(),
                },
                fallback_to_filename: dto.metadata.fallback_to_filename,
                formats: dto.metadata.formats.clone(),
            },
            classification: LibraryClassificationConfig {
                series_patterns,
                series_directory_patterns,
            },
            playlist: LibraryPlaylistConfig {
                movie_category: dto.playlist.movie_category.clone(),
                series_category: dto.playlist.series_category.clone(),
            },
        }
    }
}
