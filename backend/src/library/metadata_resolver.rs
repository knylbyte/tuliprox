use log::{debug, info, warn};

use crate::model::VodConfig;
use crate::library::classifier::{VideoClassification, VodClassifier};
use crate::library::metadata::{MetadataSource, MovieMetadata, SeriesMetadata, VideoMetadata};
use crate::library::nfo_reader::NfoReader;
use crate::library::scanner::ScannedVideoFile;
use crate::library::tmdb_client::TmdbClient;

/// Metadata resolver that tries multiple sources to get video metadata
pub struct MetadataResolver {
    classifier: VodClassifier,
    tmdb_client: Option<TmdbClient>,
    fallback_to_filename: bool,
}

impl MetadataResolver {
    /// Creates a new metadata resolver from configuration
    pub fn from_config(config: &VodConfig) -> Self {
        let tmdb_client = if config.metadata.tmdb.enabled {
            Some(TmdbClient::new(config.metadata.tmdb.api_key.clone(), config.metadata.tmdb.rate_limit_ms))
        } else {
            None
        };

        Self {
            classifier: VodClassifier::from_config(config),
            tmdb_client,
            fallback_to_filename: config.metadata.fallback_to_filename,
        }
    }

    /// Resolves metadata for a video file using multiple sources
    pub async fn resolve(&self, file: &ScannedVideoFile) -> Option<VideoMetadata> {
        debug!("Resolving metadata for: {}", file.file_name);

        // Step 1: Try to read existing NFO file
        if let Some(metadata) = NfoReader::read_metadata(&file.path).await {
            info!("Found NFO metadata for: {}", file.file_name);
            return Some(metadata);
        }

        // Step 2: Classify the file
        let classification = self.classifier.classify(file);
        debug!("Classified {} as: {:?}", file.file_name, classification);

        // Step 3: Try TMDB if enabled
        if let Some(ref tmdb) = self.tmdb_client {
            if let Some(metadata) = self.resolve_from_tmdb(file, &classification, tmdb).await {
                info!("Found TMDB metadata for: {}", file.file_name);
                return Some(metadata);
            }
        }

        // Step 4: Fallback to filename parsing
        if self.fallback_to_filename {
            info!("Using filename-based metadata for: {}", file.file_name);
            Some(Self::resolve_from_filename(file, &classification))
        } else {
            warn!("No metadata found for: {}", file.file_name);
            None
        }
    }

    /// Attempts to resolve metadata from TMDB
    async fn resolve_from_tmdb(
        &self,
        file: &ScannedVideoFile,
        classification: &VideoClassification,
        tmdb: &TmdbClient,
    ) -> Option<VideoMetadata> {
        match classification {
            VideoClassification::Movie => {
                let (title, year) = VodClassifier::extract_movie_title(file);
                debug!("Searching TMDB for movie: {title} ({year:?})");
                tmdb.search_movie(&title, year).await
            }
            VideoClassification::Series { .. } => {
                let show_name = VodClassifier::extract_show_name(file);
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
                tmdb.search_series(&show_name, year).await
            }
        }
    }

    /// Creates basic metadata from filename parsing
    fn resolve_from_filename(
        file: &ScannedVideoFile,
        classification: &VideoClassification,
    ) -> VideoMetadata {
        let timestamp = chrono::Utc::now().timestamp();

        match classification {
            VideoClassification::Movie => {
                let (title, year) = VodClassifier::extract_movie_title(file);
                VideoMetadata::Movie(MovieMetadata {
                    title,
                    original_title: None,
                    year,
                    plot: None,
                    tagline: None,
                    runtime: None,
                    mpaa: None,
                    imdb_id: None,
                    tmdb_id: None,
                    rating: None,
                    genres: Vec::new(),
                    directors: Vec::new(),
                    writers: Vec::new(),
                    actors: Vec::new(),
                    studios: Vec::new(),
                    poster: None,
                    fanart: None,
                    source: MetadataSource::FilenameParsed,
                    last_updated: timestamp,
                })
            }
            VideoClassification::Series { season: _, episode: _ } => {
                let show_name = VodClassifier::extract_show_name(file);
                VideoMetadata::Series(SeriesMetadata {
                    title: show_name,
                    original_title: None,
                    year: None,
                    plot: None,
                    mpaa: None,
                    imdb_id: None,
                    tmdb_id: None,
                    tvdb_id: None,
                    rating: None,
                    genres: Vec::new(),
                    actors: Vec::new(),
                    studios: Vec::new(),
                    poster: None,
                    fanart: None,
                    status: None,
                    episodes: Vec::new(), // Single episode would be added during processing
                    source: MetadataSource::FilenameParsed,
                    last_updated: timestamp,
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::path::PathBuf;

    fn create_test_config(tmdb_enabled: bool) -> VodConfig {
        VodConfig {
            enabled: true,
            scan_directories: vec![],
            supported_extensions: HashSet::new(),
            metadata: crate::model::VodMetadataConfig {
                storage_location: "/tmp/vod".to_string(),
                tmdb_enabled,
                tmdb_api_key: if tmdb_enabled {
                    Some("test_key".to_string())
                } else {
                    None
                },
                tmdb_rate_limit_ms: 250,
                fallback_to_filename_parsing: true,
                write_json: false,
                write_nfo: false,
            },
            classification: crate::model::VodClassificationConfig {
                series_patterns: vec![],
            },
            playlist: crate::model::VodPlaylistConfig {
                movie_group_name: "Movies".to_string(),
                series_group_name: "Series".to_string(),
            },
            file_serving: crate::model::VodFileServingConfig {
                method: crate::model::VodFileServingMethod::XtreamApi,
            },
        }
    }

    fn create_test_file(name: &str) -> ScannedVideoFile {
        ScannedVideoFile {
            path: PathBuf::from(format!("/test/{}", name)),
            file_name: name.to_string(),
            extension: "mkv".to_string(),
            size_bytes: 1024,
            modified_timestamp: 0,
        }
    }

    #[tokio::test]
    async fn test_resolve_from_filename_movie() {
        let config = create_test_config(false);
        let resolver = MetadataResolver::from_config(&config);
        let file = create_test_file("The.Matrix.1999.1080p.mkv");

        let metadata = resolver.resolve(&file).await;
        assert!(metadata.is_some());

        if let Some(VideoMetadata::Movie(movie)) = metadata {
            assert_eq!(movie.title, "The Matrix");
            assert_eq!(movie.year, Some(1999));
            assert_eq!(movie.source, MetadataSource::FilenameParsed);
        } else {
            panic!("Expected movie metadata");
        }
    }

    #[tokio::test]
    async fn test_fallback_disabled() {
        let mut config = create_test_config(false);
        config.metadata.fallback_to_filename_parsing = false;
        let resolver = MetadataResolver::from_config(&config);
        let file = create_test_file("Unknown.Movie.mkv");

        let metadata = resolver.resolve(&file).await;
        assert!(metadata.is_none());
    }
}
