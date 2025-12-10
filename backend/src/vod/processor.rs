use log::{debug, error, info, warn};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::model::{AppConfig, VodConfig};
use crate::vod::metadata::{MetadataCacheEntry, VideoMetadata};
use crate::vod::metadata_resolver::MetadataResolver;
use crate::vod::metadata_storage::MetadataStorage;
use crate::vod::scanner::{ScannedVideoFile, VodScanner};

/// VOD processor that orchestrates scanning, classification, metadata resolution, and storage
pub struct VodProcessor {
    config: VodConfig,
    scanner: VodScanner,
    resolver: MetadataResolver,
    storage: MetadataStorage,
    next_virtual_id: Arc<RwLock<u16>>,
}

/// Scan result with statistics
#[derive(Debug, Clone)]
pub struct VodScanResult {
    pub files_scanned: usize,
    pub files_added: usize,
    pub files_updated: usize,
    pub files_removed: usize,
    pub errors: usize,
}

impl VodProcessor {
    /// Creates a new VOD processor from application config
    pub fn from_app_config(app_config: &AppConfig) -> Option<Self> {
        let vod_config = app_config.vod.load_full()?;
        if !vod_config.enabled {
            return None;
        }

        Some(Self::new(vod_config.as_ref().clone()))
    }

    /// Creates a new VOD processor with the given configuration
    pub fn new(config: VodConfig) -> Self {
        let storage_path = std::path::PathBuf::from(&config.metadata.storage_location);
        let scanner = VodScanner::new(config.clone());
        let resolver = MetadataResolver::from_config(&config);
        let storage = MetadataStorage::new(storage_path);

        Self {
            config,
            scanner,
            resolver,
            storage,
            next_virtual_id: Arc::new(RwLock::new(1)),
        }
    }

    /// Performs a full VOD scan
    pub async fn scan(&self, force_rescan: bool) -> Result<VodScanResult, std::io::Error> {
        info!("Starting VOD scan (force_rescan: {})", force_rescan);

        // Initialize storage
        self.storage.initialize().await?;

        // Load existing metadata cache
        let existing_entries = self.storage.load_all().await;
        let existing_map: HashMap<_, _> = existing_entries
            .iter()
            .map(|e| (e.file_path.clone(), e.clone()))
            .collect();

        // Find highest virtual ID
        let max_virtual_id = existing_entries
            .iter()
            .map(|e| e.virtual_id)
            .max()
            .unwrap_or(0);
        *self.next_virtual_id.write().await = max_virtual_id + 1;

        info!("Existing cache entries: {}, next virtual ID: {}", existing_entries.len(), max_virtual_id + 1);

        // Scan for video files
        let scanned_files = self.scanner.scan_all().await?;
        info!("Scanned {} video files", scanned_files.len());

        let mut result = VodScanResult {
            files_scanned: scanned_files.len(),
            files_added: 0,
            files_updated: 0,
            files_removed: 0,
            errors: 0,
        };

        // Process each scanned file
        for file in scanned_files {
            match self.process_file(&file, &existing_map, force_rescan).await {
                Ok(action) => match action {
                    ProcessAction::Added => result.files_added += 1,
                    ProcessAction::Updated => result.files_updated += 1,
                    ProcessAction::Unchanged => {}
                },
                Err(e) => {
                    error!("Error processing {}: {}", file.file_name, e);
                    result.errors += 1;
                }
            }
        }

        // Cleanup orphaned entries (files that no longer exist)
        let scanned_paths: std::collections::HashSet<_> =
            scanned_files.iter().map(|f| f.path.clone()).collect();

        for entry in existing_entries {
            if !scanned_paths.contains(&entry.file_path) {
                info!("Removing orphaned entry for: {}", entry.file_path.display());
                if let Err(e) = self.storage.delete_by_uuid(&entry.uuid).await {
                    error!("Failed to delete orphaned entry: {}", e);
                } else {
                    result.files_removed += 1;
                }
            }
        }

        info!("VOD scan completed: {:?}", result);
        Ok(result)
    }

    /// Processes a single video file
    async fn process_file(
        &self,
        file: &ScannedVideoFile,
        existing_map: &HashMap<std::path::PathBuf, MetadataCacheEntry>,
        force_rescan: bool,
    ) -> Result<ProcessAction, std::io::Error> {
        // Check if file already exists in cache
        if let Some(existing_entry) = existing_map.get(&file.path) {
            // Check if file has been modified
            if !force_rescan && !existing_entry.is_file_modified(file.size_bytes, file.modified_timestamp) {
                debug!("File unchanged, skipping: {}", file.file_name);
                return Ok(ProcessAction::Unchanged);
            }

            info!("File modified, updating metadata: {}", file.file_name);
            // Reuse existing UUID and virtual ID
            let metadata = self.resolve_metadata(file).await?;
            let entry = MetadataCacheEntry {
                uuid: existing_entry.uuid.clone(),
                file_path: file.path.clone(),
                file_size: file.size_bytes,
                file_modified: file.modified_timestamp,
                metadata,
                virtual_id: existing_entry.virtual_id,
            };

            self.storage.store(&entry).await?;
            self.write_metadata_files(&entry).await?;
            return Ok(ProcessAction::Updated);
        }

        // New file - resolve metadata and assign virtual ID
        info!("New file, resolving metadata: {}", file.file_name);
        let metadata = self.resolve_metadata(file).await?;
        let virtual_id = self.allocate_virtual_id().await;

        let entry = MetadataCacheEntry::new(
            file.path.clone(),
            file.size_bytes,
            file.modified_timestamp,
            metadata,
            virtual_id,
        );

        self.storage.store(&entry).await?;
        self.write_metadata_files(&entry).await?;
        Ok(ProcessAction::Added)
    }

    /// Resolves metadata for a video file
    async fn resolve_metadata(&self, file: &ScannedVideoFile) -> Result<VideoMetadata, std::io::Error> {
        self.resolver
            .resolve(file)
            .await
            .ok_or_else(|| std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("Could not resolve metadata for {}", file.file_name),
            ))
    }

    /// Allocates the next available virtual ID
    async fn allocate_virtual_id(&self) -> u16 {
        let mut next_id = self.next_virtual_id.write().await;
        let id = *next_id;
        *next_id += 1;
        id
    }

    /// Writes metadata files (JSON, NFO) based on configuration
    async fn write_metadata_files(&self, entry: &MetadataCacheEntry) -> Result<(), std::io::Error> {
        // JSON is always written by storage.store()

        // Write NFO if enabled
        if self.config.metadata.write_nfo {
            if let Err(e) = self.storage.write_nfo(entry).await {
                warn!("Failed to write NFO for {}: {}", entry.file_path.display(), e);
            }
        }

        Ok(())
    }

    /// Gets all cached metadata entries
    pub async fn get_all_entries(&self) -> Vec<MetadataCacheEntry> {
        self.storage.load_all().await
    }

    /// Gets metadata for a specific file path
    pub async fn get_entry_by_path(&self, path: &std::path::Path) -> Option<MetadataCacheEntry> {
        self.storage.load_by_path(path).await
    }

    /// Gets metadata by virtual ID
    pub async fn get_entry_by_virtual_id(&self, virtual_id: u16) -> Option<MetadataCacheEntry> {
        let entries = self.storage.load_all().await;
        entries.into_iter().find(|e| e.virtual_id == virtual_id)
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
        let result = VodScanResult {
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
