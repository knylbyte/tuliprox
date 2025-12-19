use log::{debug, error, info, warn};

use crate::library::classifier::{MediaClassification, MediaClassifier};
use crate::library::metadata::{MediaMetadata, MetadataSource, MovieMetadata, SeriesMetadata};
use crate::library::nfo_reader::NfoReader;
use crate::library::scanner::ScannedMediaFile;
use crate::library::tmdb_client::TmdbClient;
use crate::library::{MediaGroup, MetadataStorage, MovieDbId, TMDB_API_KEY};
use crate::model::LibraryConfig;

// Metadata resolver that tries multiple sources to get video metadata
pub struct MetadataResolver {
    tmdb_client: Option<TmdbClient>,
    fallback_to_filename: bool,
}

impl MetadataResolver {
    // Creates a new metadata resolver from configuration
    pub fn from_config(config: &LibraryConfig, client: reqwest::Client, storage: MetadataStorage) -> Self {
        let tmdb_client = if config.metadata.tmdb.enabled {
            let api_key = config.metadata.tmdb.api_key.as_ref().map_or_else(|| TMDB_API_KEY.to_string(), ToString::to_string);
            Some(TmdbClient::new(api_key, config.metadata.tmdb.rate_limit_ms, client, storage))
        } else {
            None
        };

        Self {
            tmdb_client,
            fallback_to_filename: config.metadata.fallback_to_filename,
        }
    }

    // Resolves metadata for a video file using multiple sources
    pub async fn resolve(&self, group: &MediaGroup) -> Option<MediaMetadata> {
        debug!("Resolving metadata for: {group}");

        // Step 1: Classify the file
        let (Some(file), classification) = (match group {
            MediaGroup::Movie { file } => (Some(file), MediaClassification::Movie),
            MediaGroup::Series { show_key: _, episodes } => {
                let episode = episodes.first()?;
                    (Some(&episode.file), MediaClassification::Series { season: None, episode: None })
            },
        }) else { return None };

        // Step 2: Try TMDB if enabled
        if let Some(ref tmdb) = self.tmdb_client {
            match self.resolve_from_tmdb(file, &classification, tmdb).await {
                Ok(Some(metadata)) => {
                    info!("Found TMDB metadata for: {}", file.file_path);
                    return Some(metadata);
                }
                Ok(None) => {
                    return None;
                }
                Err(err) => error!("Error resolving TMDB metadata: {err}"),
            }
        }

        // TODO series implementation missing
        if classification == MediaClassification::Movie {
            // Step 3: Try to read existing NFO file
            if let Some(metadata) = NfoReader::read_metadata(&file.path).await {
                info!("Found NFO metadata for: {}", file.file_path);
                return Some(metadata);
            }
        }

        // Step 4: Fallback to filename parsing
        if self.fallback_to_filename {
            info!("Using filename-based metadata for: {}", file.file_path);
            Some(Self::resolve_from_filename(file, &classification))
        } else {
            warn!("No metadata found for: {}", file.file_path);
            None
        }
    }

    // Attempts to resolve metadata from TMDB
    async fn resolve_from_tmdb(&self, file: &ScannedMediaFile, classification: &MediaClassification, tmdb: &TmdbClient) -> Result<Option<MediaMetadata>, String> {
        match classification {
            MediaClassification::Movie => {
                let (moviedb_ids, title, year) = MediaClassifier::extract_movie_search_info(file);
                let tmdb_id = MovieDbId::get_tmdb_id(moviedb_ids.as_ref());
                tmdb.search_movie(tmdb_id, &title, year).await
            }
            MediaClassification::Series { .. } => {
                let (seriesdb_id, show_name) = MediaClassifier::extract_show_name(file);
                let tmdb_id = MovieDbId::get_tmdb_id(seriesdb_id.as_ref());
                debug!("Searching TMDB for series: {show_name}");
                // Try to extract year from parent directory if available
                let year = file.path.parent()
                    .and_then(|p| p.file_name())
                    .and_then(|n| n.to_str())
                    .and_then(|s| {
                        // Look for 4-digit year in directory name
                        s.chars()
                            .collect::<Vec<_>>()
                            .windows(4)
                            .find_map(|w| {
                                let year_str: String = w.iter().collect();
                                year_str.parse::<u32>().ok()
                                    .filter(|&y| (1900..=2100).contains(&y))
                            })
                    });
                tmdb.search_series(tmdb_id, &show_name, year).await
            }
        }
    }

    // Creates basic metadata from filename parsing
    fn resolve_from_filename(
        file: &ScannedMediaFile,
        classification: &MediaClassification,
    ) -> MediaMetadata {
        let timestamp = chrono::Utc::now().timestamp();

        match classification {
            MediaClassification::Movie => {
                let (moviedb_id, title, year) = MediaClassifier::extract_movie_search_info(file);
                MediaMetadata::Movie(MovieMetadata {
                    title,
                    year,
                    tmdb_id: MovieDbId::get_tmdb_id(moviedb_id.as_ref()),
                    tvdb_id: MovieDbId::get_tvdb_id(moviedb_id.as_ref()),
                    source: MetadataSource::FilenameParsed,
                    last_updated: timestamp,
                    ..MovieMetadata::default()
                })
            }
            MediaClassification::Series { season: _, episode: _ } => {
                let (_moviedb_id, show_name) = MediaClassifier::extract_show_name(file);
                MediaMetadata::Series(SeriesMetadata {
                    title: show_name,
                    source: MetadataSource::FilenameParsed,
                    last_updated: timestamp,
                    ..SeriesMetadata::default()
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{LibraryClassificationConfig, LibraryMetadataConfig, LibraryMetadataReadConfig, LibraryPlaylistConfig, LibraryTmdbConfig};
    use std::path::PathBuf;
    use std::time::Duration;

    fn create_test_config(tmdb_enabled: bool) -> LibraryConfig {
        LibraryConfig {
            enabled: true,
            scan_directories: vec![],
            supported_extensions: vec![],
            metadata: LibraryMetadataConfig {
                path: "/tmp/vod".to_string(),
                read_existing: LibraryMetadataReadConfig {
                    kodi: true,
                    jellyfin: true,
                    plex: true,
                },
                tmdb: LibraryTmdbConfig {
                    enabled: true,
                    api_key: if tmdb_enabled {
                        Some("test_key".to_string())
                    } else {
                        None
                    },
                    rate_limit_ms: 250,
                    cache_duration_days: 0,
                    language: "en-US".to_string(),
                },
                fallback_to_filename: true,
                formats: vec![],
            },
            classification: LibraryClassificationConfig {
                series_patterns: vec![],
                series_directory_patterns: vec![],
            },
            playlist: LibraryPlaylistConfig {
                movie_category: "Movies".to_string(),
                series_category: "Series".to_string(),
            },
        }
    }

    fn create_test_file(name: &str) -> ScannedMediaFile {
        ScannedMediaFile {
            path: PathBuf::from(format!("/test/{}", name)),
            file_path: format!("/test/{}", name),
            file_name: name.to_string(),
            extension: "mkv".to_string(),
            size_bytes: 1024,
            modified_timestamp: 0,
        }
    }

    #[tokio::test]
    async fn test_resolve_from_filename_movie() {
        let config = create_test_config(false);
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        let resolver = MetadataResolver::from_config(&config, client, MetadataStorage::new(PathBuf::from("/tmp")));
        let file = create_test_file("The.Matrix.1999.1080p.mkv");
        let group = MediaGroup::Movie { file };

        let metadata = resolver.resolve(&group).await;
        assert!(metadata.is_some());

        if let Some(MediaMetadata::Movie(movie)) = metadata {
            assert_eq!(movie.title, "The Matrix");
            assert_eq!(movie.year, Some(1999));
            assert_eq!(movie.source, MetadataSource::FilenameParsed);
        } else {
            panic!("Expected movie metadata");
        }
    }

    #[tokio::test]
    async fn test_fallback_disabled() {
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        let mut config = create_test_config(false);
        config.metadata.fallback_to_filename = false;
        let resolver = MetadataResolver::from_config(&config, client, MetadataStorage::new(PathBuf::from("/tmp")));
        let file = create_test_file("Unknown.Movie.mkv");
        let group = MediaGroup::Movie { file };

        let metadata = resolver.resolve(&group).await;
        assert!(metadata.is_none());
    }
}
