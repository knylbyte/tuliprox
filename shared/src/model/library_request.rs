use serde::{Deserialize, Serialize};

// Scan result with statistics
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct LibraryScanResult {
    pub files_scanned: usize,
    pub files_added: usize,
    pub files_updated: usize,
    pub files_removed: usize,
    pub errors: usize,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub enum LibraryScanSummaryStatus {
    Success,
    Error,
}

// Response for Library scan
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct LibraryScanSummary {
    pub status: LibraryScanSummaryStatus,
    pub message: String,
    pub result: Option<LibraryScanResult>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct LibraryStatus {
    pub enabled: bool,
    pub total_items: usize,
    pub movies: usize,
    pub series: usize,
    pub path: Option<String>,
}