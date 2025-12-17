use log::debug;
use regex::Regex;
use std::path::Path;
use shared::utils::CONSTANTS;
use crate::model::LibraryConfig;
use crate::library::scanner::ScannedMediaFile;

fn clear_filename(file_name: &str) -> String {
    // Remove quality indicators (1080p, 720p, BluRay, etc.)
    let mut cleaned = CONSTANTS.re_classifier_quality.replace(file_name, "").to_string();

    // Clean up special characters
    cleaned = cleaned.replace(['.', '_', '-'], " ").trim().to_string();

    // remove brackets
    cleaned = CONSTANTS.re_classifier_brackets_info.replace_all(&cleaned, "").to_string();

    // put space between camel case
    cleaned = CONSTANTS.re_classifier_camel_case.replace_all(&cleaned, "$1 $2").to_string();

    // Remove multiple spaces
    CONSTANTS.re_whitespace.replace_all(&cleaned, " ").trim().to_string()
}

#[derive(Debug, Copy, Clone, Eq, Hash, PartialEq)]
pub enum MovieDbId {
    Tmdb(u32),
    Tvdb(u32),
}

impl MovieDbId {
    fn get_id<F>(ids: Option<&Vec<MovieDbId>>, f: F) -> Option<u32>
    where
        F: Fn(&MovieDbId) -> Option<u32>,
    {
        ids.as_ref()?.iter().find_map(f)
    }

    pub fn get_tmdb_id(ids: Option<&Vec<MovieDbId>>) -> Option<u32> {
        Self::get_id(ids, |id| {
            if let MovieDbId::Tmdb(val) = id { Some(*val) } else { None }
        })
    }

    pub fn get_tvdb_id(ids: Option<&Vec<MovieDbId>>) -> Option<u32> {
        Self::get_id(ids, |id| {
            if let MovieDbId::Tvdb(val) = id { Some(*val) } else { None }
        })
    }
}

/// Classification result for a video file
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MediaClassification {
    Movie,
    Series {
        season: Option<u32>,
        episode: Option<u32>,
    },
}

impl MediaClassification {
    pub fn is_movie(&self) -> bool {
        matches!(self, MediaClassification::Movie)
    }

    pub fn is_series(&self) -> bool {
        matches!(self, MediaClassification::Series { .. })
    }
}

/// Classifier for determining if a video file is a movie or series
pub struct MediaClassifier {
    series_patterns: Vec<Regex>,
}

impl MediaClassifier {
    /// Creates a new classifier from the VOD configuration
    pub fn from_config(config: &LibraryConfig) -> Self {
        Self {
            series_patterns: config.classification.series_patterns.clone(),
        }
    }

    /// Classifies a video file as either Movie or Series
    pub fn classify(&self, file: &ScannedMediaFile) -> MediaClassification {
        let file_name = &file.file_name;
        let parent_path = file.path.parent().and_then(|p| p.to_str()).unwrap_or("");

        // Check if any series pattern matches
        for pattern in &self.series_patterns {
            if pattern.is_match(file_name) || pattern.is_match(parent_path) {
                debug!("File '{file_name}' matched series pattern: {pattern}");
                return Self::extract_series_info(file_name, parent_path);
            }
        }

        // If no series pattern matches, classify as movie
        debug!("File '{file_name}' classified as Movie (no series pattern match)");
        MediaClassification::Movie
    }

    /// Extracts season and episode information from file name or path
    fn extract_series_info(file_name: &str, parent_path: &str) -> MediaClassification {
        let combined = format!("{parent_path} {file_name}");

        // Try to extract season and episode numbers
        // Common patterns: S01E02, s01e02, 1x02, Season 1 Episode 2, etc.
        let season = Self::extract_season(&combined);
        let episode = Self::extract_episode(&combined);

        if season.is_some() || episode.is_some() {
            debug!("Extracted series info - Season: {season:?}, Episode: {episode:?}");
        }

        MediaClassification::Series { season, episode }
    }

    /// Extracts season number from text using common patterns
    fn extract_season(text: &str) -> Option<u32> {
        // Patterns: S01, s01, Season 1, season 1, Season1, etc.
        CONSTANTS.re_classifier_season
            .captures(text)
            .and_then(|cap| cap.get(1))
            .and_then(|m| m.as_str().parse::<u32>().ok())
    }

    /// Extracts episode number from text using common patterns
    fn extract_episode(text: &str) -> Option<u32> {
        // Patterns: E02, e02, Episode 2, episode 2, x02, etc.
        CONSTANTS.re_classifier_episode
            .captures(text)
            .and_then(|cap| cap.get(1))
            .and_then(|m| m.as_str().parse::<u32>().ok())
    }

    /// Extracts show name from file path for series
    /// Removes season/episode patterns and cleans up the name
    pub fn extract_show_name(file: &ScannedMediaFile) -> (Option<Vec<MovieDbId>>, String) {
        let file_name = Path::new(&file.file_name)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(&file.file_name);

        let moviedb_ids: Vec<MovieDbId> = CONSTANTS.re_classifier_moviedb_id.captures_iter(file_name)
            .filter_map(|caps| {
                let id = caps[2].parse::<u32>().ok()?;
                match &caps[1].to_lowercase()[..] {
                    "tmdb" => Some(MovieDbId::Tmdb(id)),
                    "tvdb" => Some(MovieDbId::Tvdb(id)),
                    _ => None,
                }
            })
            .collect();


        // Remove common series patterns
        let cleaned = CONSTANTS.re_classifier_cleanup.replace(file_name, "").trim().to_string();
        // Clean up remaining special characters
        ((!moviedb_ids.is_empty()).then_some(moviedb_ids), clear_filename(&cleaned))
    }

    /// Extracts movie title from file path
    /// Attempts to extract tmdb-id, tvdb-id, year and remove quality tags
    pub fn extract_movie_search_info(file: &ScannedMediaFile) -> (Option<Vec<MovieDbId>>, String, Option<u32>) {
        let file_name = Path::new(&file.file_name)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(&file.file_name);

        let moviedb_ids: Vec<MovieDbId> = CONSTANTS.re_classifier_moviedb_id.captures_iter(file_name)
            .filter_map(|caps| {
                let id = caps[2].parse::<u32>().ok()?;
                match &caps[1].to_lowercase()[..] {
                    "tmdb" => Some(MovieDbId::Tmdb(id)),
                    "tvdb" => Some(MovieDbId::Tvdb(id)),
                    _ => None,
                }
            })
            .collect();

        // Try to extract year (4 digits in parentheses or standalone)
        let year = CONSTANTS.re_classifier_year
            .captures(file_name)
            .and_then(|cap| cap.get(1))
            .and_then(|m| m.as_str().parse::<u32>().ok())
            .filter(|&y| (1900..=2100).contains(&y)); // Validate year range

        // Remove year and everything after it, and quality tags
        let mut title = file_name.to_string();
        if let Some(mat) = CONSTANTS.re_classifier_year.find(file_name) {
            title = file_name[..mat.start()].to_string();
        }

        title = clear_filename(&title);

        ((!moviedb_ids.is_empty()).then_some(moviedb_ids), title, year)
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
    fn test_season_extraction() {
        assert_eq!(MediaClassifier::extract_season("S01E02"), Some(1));
        assert_eq!(MediaClassifier::extract_season("s03e05"), Some(3));
        assert_eq!(MediaClassifier::extract_season("Season 2 Episode 1"), Some(2));
        assert_eq!(MediaClassifier::extract_season("season_05_episode_03"), Some(5));
    }

    #[test]
    fn test_episode_extraction() {
        assert_eq!(MediaClassifier::extract_episode("S01E02"), Some(2));
        assert_eq!(MediaClassifier::extract_episode("s03e05"), Some(5));
        assert_eq!(MediaClassifier::extract_episode("Episode 12"), Some(12));
        assert_eq!(MediaClassifier::extract_episode("1x15"), Some(15));
    }

    #[test]
    fn test_extract_show_name() {
        let file = create_test_file("Breaking.Bad.S01E01.mkv", "/tv/Breaking.Bad");
        let show_name = MediaClassifier::extract_show_name(&file);
        assert_eq!(show_name, "Breaking Bad");
    }

    #[test]
    fn test_extract_movie_title() {
        let file = create_test_file("The.Matrix.1999.1080p.BluRay.mkv", "/movies");
        let (_moviedb_id, title, year) = MediaClassifier::extract_movie_search_info(&file);
        assert_eq!(title, "The Matrix");
        assert_eq!(year, Some(1999));
    }

    #[test]
    fn test_extract_movie_title_without_year() {
        let file = create_test_file("Inception.1080p.BluRay.mkv", "/movies");
        let (tmdbid, title, year) = MediaClassifier::extract_movie_search_info(&file);
        assert_eq!(tmdbid, None);
        assert_eq!(title, "Inception");
        assert_eq!(year, None);
    }

    #[test]
    fn test_classify_series() {
        let series_pattern = Regex::new(r"(?i)s\d+e\d+").unwrap();
        let classifier = MediaClassifier {
            series_patterns: vec![series_pattern],
        };

        let file = create_test_file("Breaking.Bad.S01E01.mkv", "/tv");
        let classification = classifier.classify(&file);
        assert!(classification.is_series());

        if let MediaClassification::Series { season, episode } = classification {
            assert_eq!(season, Some(1));
            assert_eq!(episode, Some(1));
        }
    }

    #[test]
    fn test_classify_movie() {
        let series_pattern = Regex::new(r"(?i)s\d+e\d+").unwrap();
        let classifier = MediaClassifier {
            series_patterns: vec![series_pattern],
        };

        let file = create_test_file("The.Matrix.1999.mkv", "/movies");
        let classification = classifier.classify(&file);
        assert!(classification.is_movie());
    }
}
