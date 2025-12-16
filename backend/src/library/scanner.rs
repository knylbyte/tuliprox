use crate::model::{LibraryConfig, LibraryScanDirectory};
use log::{debug, error, info, trace, warn};
use std::path::{Path, PathBuf};
use tokio::fs;
use tokio::io;

/// Represents a discovered video file with its metadata
#[derive(Debug, Clone)]
pub struct ScannedMediaFile {
    pub path: PathBuf,
    pub file_path: String,
    pub file_name: String,
    pub extension: String,
    pub size_bytes: u64,
    pub modified_timestamp: i64,
}

impl ScannedMediaFile {
    /// Creates a new `ScannedMediaFile` from a path and metadata
    pub async fn from_path(path: PathBuf) -> io::Result<Self> {
        let metadata = fs::metadata(&path).await?;
        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default()
            .to_string();
        let extension = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or_default()
            .to_lowercase();

        let modified_timestamp = metadata
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(0));

        Ok(Self {
            file_path: path.display().to_string(),
            path,
            file_name,
            extension,
            size_bytes: metadata.len(),
            modified_timestamp,
        })
    }
}

/// Library file scanner for local VOD directories
pub struct LibraryScanner {
    config: LibraryConfig,
}

impl LibraryScanner {
    pub fn new(config: LibraryConfig) -> Self {
        Self { config }
    }

    pub async fn scan_all(&self) -> Result<Vec<ScannedMediaFile>, io::Error> {
        if !self.config.enabled {
            info!("Library media scanning is disabled");
            return Ok(Vec::new());
        }

        let mut all_files = Vec::new();

        for scan_dir in &self.config.scan_directories {
            if !scan_dir.enabled {
                debug!("Skipping disabled scan directory: {}", scan_dir.path);
                continue;
            }

            info!("Scanning directory: {}", scan_dir.path);
            match self.scan_directory(scan_dir).await {
                Ok(mut files) => {
                    info!("Found {} video files in {}", files.len(), scan_dir.path);
                    all_files.append(&mut files);
                }
                Err(err) => {
                    error!("Failed to scan directory {}: {err}", scan_dir.path);
                }
            }
        }

        info!("Total video files found: {}", all_files.len());
        Ok(all_files)
    }

    // Recursively scans a single directory for video files
    async fn scan_directory(&self, scan_directory: &LibraryScanDirectory) -> io::Result<Vec<ScannedMediaFile>> {
        let path = Path::new(&scan_directory.path);

        if !fs::try_exists(path).await.unwrap_or(false) {
            warn!("Directory does not exist: {}", &scan_directory.path);
            return Ok(Vec::new());
        }

        let dir_metadata = fs::metadata(path).await?;
        if !dir_metadata.is_dir() {
            warn!("Path is not a directory: {}", &scan_directory.path);
            return Ok(Vec::new());
        }

        let mut files = Vec::new();
        self.scan_directory_recursive(path, scan_directory.recursive, &mut files).await?;
        Ok(files)
    }

    fn scan_directory_recursive<'a>(
        &'a self,
        path: &'a Path,
        recursive: bool,
        files: &'a mut Vec<ScannedMediaFile>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output=io::Result<()>> + Send + 'a>> {
        Box::pin(async move {
            let mut entries = fs::read_dir(path).await?;

            while let Some(entry) = entries.next_entry().await? {
                let entry_path = entry.path();
                let metadata = match entry.metadata().await {
                    Ok(m) => m,
                    Err(err) => {
                        error!("Failed to read metadata for {}: {err}", entry_path.display());
                        continue;
                    }
                };

                if metadata.is_dir() {
                    if recursive {
                        // Recursively scan subdirectories
                        if let Err(err) = self.scan_directory_recursive(&entry_path, recursive, files).await {
                            error!("Failed to scan subdirectory {}: {err}", entry_path.display());
                        }
                    }
                } else if metadata.is_file() {
                    // Check if file has a supported video extension
                    if let Some(ext) = entry_path.extension().and_then(|e| e.to_str()) {
                        let ext_lower = ext.to_lowercase();
                        if self.config.supported_extensions.contains(&ext_lower) {
                            match ScannedMediaFile::from_path(entry_path.clone()).await {
                                Ok(video_file) => {
                                    trace!("Found video file: {}", video_file.file_path);
                                    files.push(video_file);
                                }
                                Err(err) => {
                                    error!("Failed to read metadata for {}: {err}", entry_path.display());
                                }
                            }
                        }
                    }
                }
            }

            Ok(())
        })
    }

    // Checks if a file has been modified since a given timestamp
    pub async fn is_file_modified_since(path: &Path, since_timestamp: i64) -> bool {
        match fs::metadata(path).await {
            Ok(metadata) => {
                if let Ok(modified) = metadata.modified() {
                    if let Ok(duration) = modified.duration_since(std::time::UNIX_EPOCH) {
                        return i64::try_from(duration.as_secs()).unwrap_or(0) > since_timestamp;
                    }
                }
                false
            }
            Err(_) => false,
        }
    }

    pub async fn file_exists(path: &Path) -> bool {
        fs::try_exists(path).await.unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{LibraryClassificationConfig, LibraryMetadataConfig, LibraryMetadataReadConfig, LibraryPlaylistConfig, LibraryTmdbConfig};

    fn create_test_config() -> LibraryConfig {
        LibraryConfig {
            enabled: true,
            scan_directories: vec![],
            supported_extensions: vec![
                "mp4".to_string(),
                "mkv".to_string(),
                "avi".to_string(),
            ],
            metadata: LibraryMetadataConfig {
                path: "/tmp/vod_metadata".to_string(),
                read_existing: LibraryMetadataReadConfig {
                    kodi: false,
                    jellyfin: false,
                    plex: false,
                },
                tmdb: LibraryTmdbConfig {
                    enabled: false,
                    api_key: Some(String::new()),
                    rate_limit_ms: 250,
                    cache_duration_days: 0,
                    language: String::new(),
                },
                fallback_to_filename: true,
                formats: vec![],
            },
            classification: LibraryClassificationConfig {
                series_patterns: vec![],
                series_directory_patterns: vec![],
            },
            playlist: LibraryPlaylistConfig {
                movie_category: "Local Movies".to_string(),
                series_category: "Local Series".to_string(),
            },
        }
    }

    #[tokio::test]
    async fn test_scanner_creation() {
        let config = create_test_config();
        let scanner = LibraryScanner::new(config);
        assert!(scanner.config.enabled);
    }

    #[tokio::test]
    async fn test_disabled_scanner() {
        let mut config = create_test_config();
        config.enabled = false;
        let scanner = LibraryScanner::new(config);
        let result = scanner.scan_all().await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 0);
    }
}
