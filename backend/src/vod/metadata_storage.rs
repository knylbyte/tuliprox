use log::{debug, error, info, warn};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tokio::fs;
use tokio::io::AsyncWriteExt;

use crate::vod::metadata::{MetadataCacheEntry, VideoMetadata};

/// Metadata storage for local VOD files
/// Stores metadata as JSON files with UUID-based filenames
pub struct MetadataStorage {
    storage_dir: PathBuf,
}

impl MetadataStorage {
    /// Creates a new metadata storage instance
    pub fn new(storage_dir: PathBuf) -> Self {
        Self { storage_dir }
    }

    /// Initializes the storage directory
    pub async fn initialize(&self) -> std::io::Result<()> {
        if !self.storage_dir.exists() {
            info!("Creating metadata storage directory: {}", self.storage_dir.display());
            fs::create_dir_all(&self.storage_dir).await?;
        }
        Ok(())
    }

    /// Stores metadata for a video file
    pub async fn store(&self, entry: &MetadataCacheEntry) -> std::io::Result<()> {
        let file_path = self.get_metadata_file_path(&entry.uuid);

        debug!("Storing metadata for {}: {}", entry.file_path.display(), file_path.display());

        let json = serde_json::to_string_pretty(entry)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

        let mut file = fs::File::create(&file_path).await?;
        file.write_all(json.as_bytes()).await?;
        file.flush().await?;

        Ok(())
    }

    /// Loads metadata for a specific UUID
    pub async fn load_by_uuid(&self, uuid: &str) -> Option<MetadataCacheEntry> {
        let file_path = self.get_metadata_file_path(uuid);

        if !fs::try_exists(&file_path).await.unwrap_or(false) {
            return None;
        }

        match fs::read_to_string(&file_path).await {
            Ok(content) => match serde_json::from_str::<MetadataCacheEntry>(&content) {
                Ok(entry) => Some(entry),
                Err(e) => {
                    error!("Failed to parse metadata file {}: {}", file_path.display(), e);
                    None
                }
            },
            Err(e) => {
                error!("Failed to read metadata file {}: {}", file_path.display(), e);
                None
            }
        }
    }

    /// Loads metadata for a specific file path
    pub async fn load_by_path(&self, file_path: &Path) -> Option<MetadataCacheEntry> {
        // This requires scanning all metadata files to find the one with matching file_path
        // For better performance, we should maintain a separate index
        let entries = self.load_all().await;
        entries
            .into_iter()
            .find(|entry| entry.file_path == file_path)
    }

    /// Loads all metadata entries from storage
    pub async fn load_all(&self) -> Vec<MetadataCacheEntry> {
        let mut entries = Vec::new();

        let mut read_dir = match fs::read_dir(&self.storage_dir).await {
            Ok(dir) => dir,
            Err(e) => {
                error!("Failed to read metadata directory: {}", e);
                return entries;
            }
        };

        while let Ok(Some(dir_entry)) = read_dir.next_entry().await {
            let path = dir_entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("json") {
                if let Ok(content) = fs::read_to_string(&path).await {
                    match serde_json::from_str::<MetadataCacheEntry>(&content) {
                        Ok(entry) => entries.push(entry),
                        Err(e) => {
                            error!("Failed to parse metadata file {}: {}", path.display(), e);
                        }
                    }
                }
            }
        }

        debug!("Loaded {} metadata entries from storage", entries.len());
        entries
    }

    /// Deletes metadata for a specific UUID
    pub async fn delete_by_uuid(&self, uuid: &str) -> std::io::Result<()> {
        let file_path = self.get_metadata_file_path(uuid);

        if fs::try_exists(&file_path).await.unwrap_or(false) {
            debug!("Deleting metadata file: {}", file_path.display());
            fs::remove_file(&file_path).await?;
        }

        Ok(())
    }

    /// Deletes metadata for a specific file path
    pub async fn delete_by_path(&self, file_path: &Path) -> std::io::Result<()> {
        if let Some(entry) = self.load_by_path(file_path).await {
            self.delete_by_uuid(&entry.uuid).await?;
        }
        Ok(())
    }

    /// Cleans up metadata for files that no longer exist
    pub async fn cleanup_orphaned(&self) -> std::io::Result<usize> {
        let entries = self.load_all().await;
        let mut deleted_count = 0;

        for entry in entries {
            if !fs::try_exists(&entry.file_path).await.unwrap_or(false) {
                info!("Removing orphaned metadata for missing file: {}", entry.file_path.display());
                if let Err(e) = self.delete_by_uuid(&entry.uuid).await {
                    error!("Failed to delete orphaned metadata: {}", e);
                } else {
                    deleted_count += 1;
                }
            }
        }

        if deleted_count > 0 {
            info!("Cleaned up {} orphaned metadata entries", deleted_count);
        }

        Ok(deleted_count)
    }

    /// Builds a map of file paths to UUIDs for quick lookups
    pub async fn build_path_index(&self) -> HashMap<PathBuf, String> {
        let entries = self.load_all().await;
        entries
            .into_iter()
            .map(|entry| (entry.file_path.clone(), entry.uuid.clone()))
            .collect()
    }

    /// Builds a map of UUIDs to virtual IDs
    pub async fn build_virtual_id_map(&self) -> HashMap<String, u16> {
        let entries = self.load_all().await;
        entries
            .into_iter()
            .map(|entry| (entry.uuid.clone(), entry.virtual_id))
            .collect()
    }

    /// Gets the metadata file path for a UUID
    fn get_metadata_file_path(&self, uuid: &str) -> PathBuf {
        self.storage_dir.join(format!("{}.json", uuid))
    }

    /// Writes an NFO file for the given metadata
    pub async fn write_nfo(&self, entry: &MetadataCacheEntry) -> std::io::Result<()> {
        let nfo_content = Self::generate_nfo_content(&entry.metadata);
        let nfo_path = entry.file_path.with_extension("nfo");

        debug!("Writing NFO file: {}", nfo_path.display());

        let mut file = fs::File::create(&nfo_path).await?;
        file.write_all(nfo_content.as_bytes()).await?;
        file.flush().await?;

        Ok(())
    }

    /// Generates NFO XML content from metadata
    fn generate_nfo_content(metadata: &VideoMetadata) -> String {
        match metadata {
            VideoMetadata::Movie(movie) => {
                let mut nfo = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<movie>\n");
                nfo.push_str(&format!("  <title>{}</title>\n", Self::xml_escape(&movie.title)));

                if let Some(ref original_title) = movie.original_title {
                    nfo.push_str(&format!("  <originaltitle>{}</originaltitle>\n", Self::xml_escape(original_title)));
                }

                if let Some(year) = movie.year {
                    nfo.push_str(&format!("  <year>{}</year>\n", year));
                }

                if let Some(ref plot) = movie.plot {
                    nfo.push_str(&format!("  <plot>{}</plot>\n", Self::xml_escape(plot)));
                }

                if let Some(ref tagline) = movie.tagline {
                    nfo.push_str(&format!("  <tagline>{}</tagline>\n", Self::xml_escape(tagline)));
                }

                if let Some(runtime) = movie.runtime {
                    nfo.push_str(&format!("  <runtime>{}</runtime>\n", runtime));
                }

                if let Some(ref imdb_id) = movie.imdb_id {
                    nfo.push_str(&format!("  <imdbid>{}</imdbid>\n", Self::xml_escape(imdb_id)));
                }

                if let Some(tmdb_id) = movie.tmdb_id {
                    nfo.push_str(&format!("  <tmdbid>{}</tmdbid>\n", tmdb_id));
                }

                if let Some(rating) = movie.rating {
                    nfo.push_str(&format!("  <rating>{}</rating>\n", rating));
                }

                for genre in &movie.genres {
                    nfo.push_str(&format!("  <genre>{}</genre>\n", Self::xml_escape(genre)));
                }

                for director in &movie.directors {
                    nfo.push_str(&format!("  <director>{}</director>\n", Self::xml_escape(director)));
                }

                if let Some(ref poster) = movie.poster {
                    nfo.push_str(&format!("  <thumb>{}</thumb>\n", Self::xml_escape(poster)));
                }

                nfo.push_str("</movie>\n");
                nfo
            }
            VideoMetadata::Series(series) => {
                let mut nfo = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<tvshow>\n");
                nfo.push_str(&format!("  <title>{}</title>\n", Self::xml_escape(&series.title)));

                if let Some(year) = series.year {
                    nfo.push_str(&format!("  <year>{}</year>\n", year));
                }

                if let Some(ref plot) = series.plot {
                    nfo.push_str(&format!("  <plot>{}</plot>\n", Self::xml_escape(plot)));
                }

                if let Some(ref imdb_id) = series.imdb_id {
                    nfo.push_str(&format!("  <imdbid>{}</imdbid>\n", Self::xml_escape(imdb_id)));
                }

                if let Some(tmdb_id) = series.tmdb_id {
                    nfo.push_str(&format!("  <tmdbid>{}</tmdbid>\n", tmdb_id));
                }

                if let Some(tvdb_id) = series.tvdb_id {
                    nfo.push_str(&format!("  <tvdbid>{}</tvdbid>\n", tvdb_id));
                }

                for genre in &series.genres {
                    nfo.push_str(&format!("  <genre>{}</genre>\n", Self::xml_escape(genre)));
                }

                if let Some(ref status) = series.status {
                    nfo.push_str(&format!("  <status>{}</status>\n", Self::xml_escape(status)));
                }

                nfo.push_str("</tvshow>\n");
                nfo
            }
        }
    }

    /// Escapes XML special characters
    fn xml_escape(s: &str) -> String {
        s.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
            .replace('\'', "&apos;")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vod::metadata::{MetadataSource, MovieMetadata};

    #[tokio::test]
    async fn test_store_and_load() {
        let temp_dir = tempfile::tempdir().unwrap();
        let storage = MetadataStorage::new(temp_dir.path().to_path_buf());
        storage.initialize().await.unwrap();

        let entry = MetadataCacheEntry::new(
            PathBuf::from("/test/movie.mp4"),
            1024,
            1234567890,
            VideoMetadata::Movie(MovieMetadata {
                title: "Test Movie".to_string(),
                original_title: None,
                year: Some(2020),
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
                last_updated: 0,
            }),
            100,
        );

        // Store
        storage.store(&entry).await.unwrap();

        // Load by UUID
        let loaded = storage.load_by_uuid(&entry.uuid).await;
        assert!(loaded.is_some());
        assert_eq!(loaded.unwrap().file_path, entry.file_path);

        // Delete
        storage.delete_by_uuid(&entry.uuid).await.unwrap();
        let deleted = storage.load_by_uuid(&entry.uuid).await;
        assert!(deleted.is_none());
    }
}
