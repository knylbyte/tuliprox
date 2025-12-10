use log::debug;
use regex::Regex;
use std::path::Path;
use std::sync::OnceLock;

use crate::model::VodConfig;
use crate::vod::scanner::ScannedVideoFile;

/// Classification result for a video file
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VideoClassification {
    Movie,
    Series {
        season: Option<u32>,
        episode: Option<u32>,
    },
}

impl VideoClassification {
    pub fn is_movie(&self) -> bool {
        matches!(self, VideoClassification::Movie)
    }

    pub fn is_series(&self) -> bool {
        matches!(self, VideoClassification::Series { .. })
    }
}

/// Classifier for determining if a video file is a movie or series
pub struct VodClassifier {
    series_patterns: Vec<Regex>,
}

impl VodClassifier {
    /// Creates a new classifier from the VOD configuration
    pub fn from_config(config: &VodConfig) -> Self {
        Self {
            series_patterns: config.classification.series_patterns.clone(),
        }
    }

    /// Classifies a video file as either Movie or Series
    pub fn classify(&self, file: &ScannedVideoFile) -> VideoClassification {
        let file_name = &file.file_name;
        let parent_path = file.path.parent().and_then(|p| p.to_str()).unwrap_or("");

        // Check if any series pattern matches
        for pattern in &self.series_patterns {
            if pattern.is_match(file_name) || pattern.is_match(parent_path) {
                debug!("File '{}' matched series pattern: {}", file_name, pattern);
                return self.extract_series_info(file_name, parent_path);
            }
        }

        // If no series pattern matches, classify as movie
        debug!("File '{}' classified as Movie (no series pattern match)", file_name);
        VideoClassification::Movie
    }

    /// Extracts season and episode information from file name or path
    fn extract_series_info(&self, file_name: &str, parent_path: &str) -> VideoClassification {
        let combined = format!("{} {}", parent_path, file_name);

        // Try to extract season and episode numbers
        // Common patterns: S01E02, s01e02, 1x02, Season 1 Episode 2, etc.
        let season = Self::extract_season(&combined);
        let episode = Self::extract_episode(&combined);

        if season.is_some() || episode.is_some() {
            debug!(
                "Extracted series info - Season: {:?}, Episode: {:?}",
                season, episode
            );
        }

        VideoClassification::Series { season, episode }
    }

    /// Extracts season number from text using common patterns
    fn extract_season(text: &str) -> Option<u32> {
        // Patterns: S01, s01, Season 1, season 1, Season1, etc.
        static SEASON_REGEX: OnceLock<Regex> = OnceLock::new();
        let regex = SEASON_REGEX.get_or_init(|| {
            Regex::new(r"(?i)(?:s|season)[\s\._-]*(\d+)").unwrap()
        });

        regex
            .captures(text)
            .and_then(|cap| cap.get(1))
            .and_then(|m| m.as_str().parse::<u32>().ok())
    }

    /// Extracts episode number from text using common patterns
    fn extract_episode(text: &str) -> Option<u32> {
        // Patterns: E02, e02, Episode 2, episode 2, x02, etc.
        static EPISODE_REGEX: OnceLock<Regex> = OnceLock::new();
        let regex = EPISODE_REGEX.get_or_init(|| {
            Regex::new(r"(?i)(?:e|episode|x)[\s\._-]*(\d+)").unwrap()
        });

        regex
            .captures(text)
            .and_then(|cap| cap.get(1))
            .and_then(|m| m.as_str().parse::<u32>().ok())
    }

    /// Extracts show name from file path for series
    /// Removes season/episode patterns and cleans up the name
    pub fn extract_show_name(file: &ScannedVideoFile) -> String {
        let file_name = Path::new(&file.file_name)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(&file.file_name);

        // Remove common series patterns
        static CLEANUP_REGEX: OnceLock<Regex> = OnceLock::new();
        let cleanup = CLEANUP_REGEX.get_or_init(|| {
            Regex::new(r"(?i)[\s\._-]*(?:s\d+e\d+|\d+x\d+|season[\s\._-]*\d+|episode[\s\._-]*\d+).*$").unwrap()
        });

        let cleaned = cleanup.replace(file_name, "").trim().to_string();

        // Clean up remaining special characters
        let cleaned = cleaned
            .replace('.', " ")
            .replace('_', " ")
            .replace('-', " ");

        // Remove multiple spaces
        static SPACE_REGEX: OnceLock<Regex> = OnceLock::new();
        let space = SPACE_REGEX.get_or_init(|| Regex::new(r"\s+").unwrap());

        space.replace_all(&cleaned, " ").trim().to_string()
    }

    /// Extracts movie title from file path
    /// Attempts to extract year and remove quality tags
    pub fn extract_movie_title(file: &ScannedVideoFile) -> (String, Option<u32>) {
        let file_name = Path::new(&file.file_name)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(&file.file_name);

        // Try to extract year (4 digits in parentheses or standalone)
        static YEAR_REGEX: OnceLock<Regex> = OnceLock::new();
        let year_regex = YEAR_REGEX.get_or_init(|| {
            Regex::new(r"[\(\[]?(\d{4})[\)\]]?").unwrap()
        });

        let year = year_regex
            .captures(file_name)
            .and_then(|cap| cap.get(1))
            .and_then(|m| m.as_str().parse::<u32>().ok())
            .filter(|&y| y >= 1900 && y <= 2100); // Validate year range

        // Remove year and everything after it, and quality tags
        let mut title = file_name.to_string();
        if let Some(y) = year {
            if let Some(pos) = title.find(&y.to_string()) {
                title = title[..pos].to_string();
            }
        }

        // Remove quality indicators (1080p, 720p, BluRay, etc.)
        static QUALITY_REGEX: OnceLock<Regex> = OnceLock::new();
        let quality_regex = QUALITY_REGEX.get_or_init(|| {
            Regex::new(r"(?i)[\s\._-]*(1080p|720p|480p|2160p|4K|BluRay|BRRip|WEB-DL|WEBRip|HDTV|DVDRip).*$").unwrap()
        });
        title = quality_regex.replace(&title, "").to_string();

        // Clean up special characters
        title = title
            .replace('.', " ")
            .replace('_', " ")
            .replace('-', " ")
            .trim()
            .to_string();

        // Remove multiple spaces
        static SPACE_REGEX: OnceLock<Regex> = OnceLock::new();
        let space_regex = SPACE_REGEX.get_or_init(|| Regex::new(r"\s+").unwrap());
        title = space_regex.replace_all(&title, " ").trim().to_string();

        (title, year)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn create_test_file(file_name: &str, parent_path: &str) -> ScannedVideoFile {
        ScannedVideoFile {
            path: PathBuf::from(parent_path).join(file_name),
            file_name: file_name.to_string(),
            extension: "mkv".to_string(),
            size_bytes: 1024,
            modified_timestamp: 0,
        }
    }

    #[test]
    fn test_season_extraction() {
        assert_eq!(VodClassifier::extract_season("S01E02"), Some(1));
        assert_eq!(VodClassifier::extract_season("s03e05"), Some(3));
        assert_eq!(VodClassifier::extract_season("Season 2 Episode 1"), Some(2));
        assert_eq!(VodClassifier::extract_season("season_05_episode_03"), Some(5));
    }

    #[test]
    fn test_episode_extraction() {
        assert_eq!(VodClassifier::extract_episode("S01E02"), Some(2));
        assert_eq!(VodClassifier::extract_episode("s03e05"), Some(5));
        assert_eq!(VodClassifier::extract_episode("Episode 12"), Some(12));
        assert_eq!(VodClassifier::extract_episode("1x15"), Some(15));
    }

    #[test]
    fn test_extract_show_name() {
        let file = create_test_file("Breaking.Bad.S01E01.mkv", "/tv/Breaking.Bad");
        let show_name = VodClassifier::extract_show_name(&file);
        assert_eq!(show_name, "Breaking Bad");
    }

    #[test]
    fn test_extract_movie_title() {
        let file = create_test_file("The.Matrix.1999.1080p.BluRay.mkv", "/movies");
        let (title, year) = VodClassifier::extract_movie_title(&file);
        assert_eq!(title, "The Matrix");
        assert_eq!(year, Some(1999));
    }

    #[test]
    fn test_extract_movie_title_without_year() {
        let file = create_test_file("Inception.1080p.BluRay.mkv", "/movies");
        let (title, year) = VodClassifier::extract_movie_title(&file);
        assert_eq!(title, "Inception");
        assert_eq!(year, None);
    }

    #[test]
    fn test_classify_series() {
        let series_pattern = Regex::new(r"(?i)s\d+e\d+").unwrap();
        let classifier = VodClassifier {
            series_patterns: vec![series_pattern],
        };

        let file = create_test_file("Breaking.Bad.S01E01.mkv", "/tv");
        let classification = classifier.classify(&file);
        assert!(classification.is_series());

        if let VideoClassification::Series { season, episode } = classification {
            assert_eq!(season, Some(1));
            assert_eq!(episode, Some(1));
        }
    }

    #[test]
    fn test_classify_movie() {
        let series_pattern = Regex::new(r"(?i)s\d+e\d+").unwrap();
        let classifier = VodClassifier {
            series_patterns: vec![series_pattern],
        };

        let file = create_test_file("The.Matrix.1999.mkv", "/movies");
        let classification = classifier.classify(&file);
        assert!(classification.is_movie());
    }
}
