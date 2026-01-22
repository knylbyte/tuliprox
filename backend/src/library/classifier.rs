use crate::library::scanner::ScannedMediaFile;
use crate::library::{SeriesKey};
use crate::ptt::{ptt_parse_title, PttMetadata};

// Classification result for a video file
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MediaClassification {
    Movie {
        metadata: PttMetadata
    },
    Series {
        key: SeriesKey,
        episode: u32,
        season: u32,
        metadata: PttMetadata,
    },
}

impl MediaClassification {
    pub fn is_movie(&self) -> bool {
        matches!(self, MediaClassification::Movie { .. })
    }

    pub fn is_series(&self) -> bool {
        matches!(self, MediaClassification::Series { .. })
    }
}

/// Classifier for determining if a video file is a movie or series
pub struct MediaClassifier {
}

impl MediaClassifier {
    // Classifies a video file as either Movie or Series
    pub fn classify(file: &ScannedMediaFile) -> MediaClassification {
        let file_name = &file.file_name;

        let ptt_metadata = ptt_parse_title(file_name);

        if let (Some(episode), Some(season)) = (ptt_metadata.episodes.first(), ptt_metadata.seasons.first()) {
            MediaClassification::Series {
                key: SeriesKey {
                    title: ptt_metadata.title.clone(),
                    year: ptt_metadata.year,
                    tmdb_id: ptt_metadata.tmdb,
                },
                episode: *episode,
                season: *season,
                metadata: ptt_metadata,
            }
        } else {
            MediaClassification::Movie {metadata: ptt_metadata }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn create_test_file(file_name: &str, parent_path: &str) -> ScannedMediaFile {
        ScannedMediaFile {
            path: PathBuf::from(parent_path).join(file_name),
            file_path: parent_path.to_string(),
            file_name: file_name.to_string(),
            extension: "mkv".to_string(),
            size_bytes: 1024,
            modified_timestamp: 0,
        }
    }

    #[test]
    fn test_extract_show_name() {
        let file = create_test_file("Breaking.Bad.S01E01.mkv", "/tv/Breaking.Bad");
        let classification = MediaClassifier::classify(&file);
        match classification {
            MediaClassification::Movie {  .. } => {

            }
            MediaClassification::Series { key, episode, season, metadata, .. } => {
                assert_eq!(key.title, "Breaking Bad");
                assert_eq!(episode, 1);
                assert_eq!(season, 1);
                assert_eq!(metadata.extension, Some("mkv".to_string()));
            }
        }
    }

    #[test]
    fn test_extract_movie_title() {
        let file = create_test_file("The.Matrix.1999.1080p.BluRay.mkv", "/movies");
        let classification = MediaClassifier::classify(&file);
        match classification {
            MediaClassification::Movie { metadata } => {
                assert_eq!(metadata.title, "The Matrix");
                assert_eq!(metadata.year, Some(1999));
                assert_eq!(metadata.extension, Some("mkv".to_string()));

            }
            MediaClassification::Series { .. } => {
            }
        }
    }

    #[test]
    fn test_extract_movie_title_without_year() {
        let file = create_test_file("Inception.1080p.BluRay.mkv", "/movies");
        let classification = MediaClassifier::classify(&file);
        match classification {
            MediaClassification::Movie { metadata } => {
                assert_eq!(metadata.title, "Inception");
                assert_eq!(metadata.year, None);
                assert_eq!(metadata.extension, Some("mkv".to_string()));
            }
            MediaClassification::Series { .. } => {
            }
        }
    }
}
