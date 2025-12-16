use log::{debug, error, info, warn};
use std::collections::HashMap;
use shared::model::{LibraryMetadataFormat, LibraryScanResult};
use crate::api::model::create_http_client;
use crate::model::{AppConfig, LibraryConfig};
use crate::library::metadata::{MetadataCacheEntry, MediaMetadata};
use crate::library::metadata_resolver::MetadataResolver;
use crate::library::metadata_storage::MetadataStorage;
use crate::library::scanner::{ScannedMediaFile, LibraryScanner};

/// VOD processor that orchestrates scanning, classification, metadata resolution, and storage
pub struct LibraryProcessor {
    config: LibraryConfig,
    scanner: LibraryScanner,
    resolver: MetadataResolver,
    storage: MetadataStorage,
}

impl LibraryProcessor {
    /// Creates a new Library processor from application config
    pub fn from_app_config(app_config: &AppConfig) -> Option<Self> {
        let client = create_http_client(app_config);
        app_config.config.load().library.as_ref().map(|lib_cfg| Self::new(lib_cfg.clone(), client))
    }

    /// Creates a new Library processor with the given configuration
    pub fn new(config: LibraryConfig, client: reqwest::Client) -> Self {
        let storage_path = std::path::PathBuf::from(&config.metadata.path);
        let scanner = LibraryScanner::new(config.clone());
        let resolver = MetadataResolver::from_config(&config, client);
        let storage = MetadataStorage::new(storage_path);

        Self {
            config,
            scanner,
            resolver,
            storage,
        }
    }

    /// Performs a full Library scan
    pub async fn scan(&self, force_rescan: bool) -> Result<LibraryScanResult, std::io::Error> {
        info!("Starting Library scan (force_rescan: {force_rescan})");

        // Initialize storage
        self.storage.initialize().await?;

        // Load existing metadata cache
        let existing_entries = self.storage.load_all().await;
        let existing_map: HashMap<_, _> = existing_entries
            .iter()
            .map(|e| (e.file_path.clone(), e.clone()))
            .collect();

        // Scan for video files
        let scanned_files = self.scanner.scan_all().await?;
        info!("Scanned {} video files", scanned_files.len());

        let mut result = LibraryScanResult {
            files_scanned: scanned_files.len(),
            files_added: 0,
            files_updated: 0,
            files_removed: 0,
            errors: 0,
        };

        // Process each scanned file
        for file in &scanned_files {
            match self.process_file(file, &existing_map, force_rescan).await {
                Ok(action) => match action {
                    ProcessAction::Added => result.files_added += 1,
                    ProcessAction::Updated => result.files_updated += 1,
                    ProcessAction::Unchanged => {}
                },
                Err(e) => {
                    error!("Error processing {}: {e}", file.file_path);
                    result.errors += 1;
                }
            }
        }

        // Cleanup orphaned entries (files that no longer exist)
        let scanned_paths: std::collections::HashSet<_> = scanned_files.iter().map(|f| f.file_path.as_str()).collect();

        for entry in existing_entries {
            if !scanned_paths.contains(entry.file_path.as_str()) {
                debug!("Removing orphaned entry for: {}", entry.file_path);
                if let Err(e) = self.storage.delete_by_uuid(&entry.uuid).await {
                    error!("Failed to delete orphaned entry: {e}");
                } else {
                    result.files_removed += 1;
                }
            }
        }

        info!("Library scan completed: {result:?}");
        Ok(result)
    }

    // Processes a single video file
    async fn process_file(
        &self,
        file: &ScannedMediaFile,
        existing_map: &HashMap<String, MetadataCacheEntry>,
        force_rescan: bool,
    ) -> Result<ProcessAction, std::io::Error> {
        // Check if file already exists in cache
        if let Some(existing_entry) = existing_map.get(&file.file_path) {
            // Check if file has been modified
            if !force_rescan && !existing_entry.is_file_modified(file.size_bytes, file.modified_timestamp) {
                debug!("File unchanged, skipping: {}", file.file_path);
                return Ok(ProcessAction::Unchanged);
            }

            debug!("File modified, updating metadata: {}", file.file_path);
            // Reuse existing UUID
            let metadata = self.resolve_metadata(file).await?;
            let entry = MetadataCacheEntry {
                uuid: existing_entry.uuid.clone(),
                file_path: file.file_path.clone(),
                file_size: file.size_bytes,
                file_modified: file.modified_timestamp,
                metadata,
            };

            self.storage.store(&entry).await?;
            self.write_metadata_files(&entry).await?;
            return Ok(ProcessAction::Updated);
        }

        debug!("New file, resolving metadata: {}", file.file_path);
        let metadata = self.resolve_metadata(file).await?;

        let entry = MetadataCacheEntry::new(
            file.file_path.clone(),
            file.size_bytes,
            file.modified_timestamp,
            metadata,
        );

        self.storage.store(&entry).await?;
        self.write_metadata_files(&entry).await?;
        Ok(ProcessAction::Added)
    }

    /// Resolves metadata for a video file
    async fn resolve_metadata(&self, file: &ScannedMediaFile) -> Result<MediaMetadata, std::io::Error> {
        self.resolver
            .resolve(file)
            .await
            .ok_or_else(|| std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("Could not resolve metadata for {}", file.file_path),
            ))
    }

    /// Writes metadata files (JSON, NFO) based on configuration
    async fn write_metadata_files(&self, entry: &MetadataCacheEntry) -> Result<(), std::io::Error> {
        // JSON is always written by storage.store()
        // TODO should we remove json from formats ?
        // Write NFO if enabled
        if self.config.metadata.formats.contains(&LibraryMetadataFormat::Nfo) {
            if let Err(e) = self.storage.write_nfo(entry).await {
                warn!("Failed to write NFO for {}: {e}", entry.file_path);
            }
        }

        Ok(())
    }

    /// Gets all cached metadata entries
    pub async fn get_all_entries(&self) -> Vec<MetadataCacheEntry> {
        self.storage.load_all().await
    }

}

/// Action taken when processing a file
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProcessAction {
    Added,
    Updated,
    Unchanged,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scan_result_creation() {
        let result = LibraryScanResult {
            files_scanned: 100,
            files_added: 50,
            files_updated: 20,
            files_removed: 5,
            errors: 2,
        };

        assert_eq!(result.files_scanned, 100);
        assert_eq!(result.files_added, 50);
    }
}
