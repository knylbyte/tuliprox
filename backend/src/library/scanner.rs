use log::{debug, error, info, warn};
use std::path::{Path, PathBuf};
use tokio::fs;
use tokio::io;

use crate::model::VodConfig;

/// Represents a discovered video file with its metadata
#[derive(Debug, Clone)]
pub struct ScannedVideoFile {
    pub path: PathBuf,
    pub file_name: String,
    pub extension: String,
    pub size_bytes: u64,
    pub modified_timestamp: i64,
}

impl ScannedVideoFile {
    /// Creates a new `ScannedVideoFile` from a path and metadata
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
            path,
            file_name,
            extension,
            size_bytes: metadata.len(),
            modified_timestamp,
        })
    }
}

/// Video file scanner for local VOD directories
pub struct VodScanner {
    config: VodConfig,
}

impl VodScanner {
    /// Creates a new `VodScanner` with the given configuration
    pub fn new(config: VodConfig) -> Self {
        Self { config }
    }

    /// Scans all configured directories for video files
    pub async fn scan_all(&self) -> Result<Vec<ScannedVideoFile>, io::Error> {
        if !self.config.enabled {
            info!("VOD scanning is disabled");
            return Ok(Vec::new());
        }

        let mut all_files = Vec::new();

        for scan_dir in &self.config.scan_directories {
            // if !scan_dir.enabled {
            //     debug!("Skipping disabled scan directory: {}", scan_dir.path);
            //     continue;
            // }

            info!("Scanning directory: {}", scan_dir.path);
            match self.scan_directory(&scan_dir.path).await {
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

    /// Recursively scans a single directory for video files
    async fn scan_directory(&self, dir_path: &str) -> io::Result<Vec<ScannedVideoFile>> {
        let path = Path::new(dir_path);

        if !path.exists() {
            warn!("Directory does not exist: {dir_path}");
            return Ok(Vec::new());
        }

        if !path.is_dir() {
            warn!("Path is not a directory: {dir_path}");
            return Ok(Vec::new());
        }

        let mut files = Vec::new();
        self.scan_directory_recursive(path, &mut files).await?;
        Ok(files)
    }

    /// Internal recursive directory scanning implementation
    fn scan_directory_recursive<'a>(
        &'a self,
        path: &'a Path,
        files: &'a mut Vec<ScannedVideoFile>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = io::Result<()>> + Send + 'a>> {
        Box::pin(async move {
            let mut entries = fs::read_dir(path).await?;

            while let Some(entry) = entries.next_entry().await? {
                let entry_path = entry.path();
                let metadata = entry.metadata().await?;

                if metadata.is_dir() {
                    // Recursively scan subdirectories
                    if let Err(err) = self.scan_directory_recursive(&entry_path, files).await {
                        error!("Failed to scan subdirectory {}: {}", entry_path.display(), err);
                    }
                } else if metadata.is_file() {
                    // Check if file has a supported video extension
                    if let Some(ext) = entry_path.extension().and_then(|e| e.to_str()) {
                        let ext_lower = ext.to_lowercase();
                        if self.config.supported_extensions.contains(&ext_lower) {
                            match ScannedVideoFile::from_path(entry_path.clone()).await {
                                Ok(video_file) => {
                                    debug!("Found video file: {}", video_file.path.display());
                                    files.push(video_file);
                                }
                                Err(err) => {
                                    error!("Failed to read metadata for {}: {}", entry_path.display(), err);
                                }
                            }
                        }
                    }
                }
            }

            Ok(())
        })
    }

    /// Checks if a file has been modified since a given timestamp
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

    /// Checks if a file still exists
    pub async fn file_exists(path: &Path) -> bool {
        fs::try_exists(path).await.unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn create_test_config() -> VodConfig {
        VodConfig {
            enabled: true,
            scan_directories: vec![],
            supported_extensions: HashSet::from_iter(vec![
                "mp4".to_string(),
                "mkv".to_string(),
                "avi".to_string(),
            ]),
            metadata: crate::model::VodMetadataConfig {
                storage_location: "/tmp/vod_metadata".to_string(),
                tmdb_enabled: false,
                tmdb_api_key: None,
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

    #[tokio::test]
    async fn test_scanner_creation() {
        let config = create_test_config();
        let scanner = VodScanner::new(config);
        assert!(scanner.config.enabled);
    }

    #[tokio::test]
    async fn test_disabled_scanner() {
        let mut config = create_test_config();
        config.enabled = false;
        let scanner = VodScanner::new(config);
        let result = scanner.scan_all().await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 0);
    }
}
