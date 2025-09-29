use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub fn parse_xmltv_time(t: &str) -> Option<i64> {
    let fmt = "%Y%m%d%H%M%S %z";
    if let Ok(result) = DateTime::parse_from_str(t, fmt) {
            Some(result
            .with_timezone(&Utc)
            .timestamp())
    } else {
        None
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct EpgTv {
    pub start: i64,
    pub stop: i64,
    pub channels: Vec<EpgChannel>,
}

impl PartialEq for EpgTv {
    fn eq(&self, other: &Self) -> bool {
            self.start == other.start
            && self.stop == other.stop
        // Note: self.channels is skipped
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct EpgChannel {
    pub id: String,
    pub title: String,
    pub icon: Option<String>,
    pub programmes: Vec<EpgProgramme>,
}

impl EpgChannel {
    pub fn new(id: String) -> Self {
        Self {
            id,
            title: String::new(),
            icon: None,
            programmes: Vec::new(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct EpgProgramme {
    pub start: i64,
    pub stop: i64,
    pub channel: String,
    pub title: String,
}

impl EpgProgramme {
    pub fn new(start: i64, stop: i64, channel: String) -> Self {
        Self {
            start,
            stop,
            channel,
            title: String::new(),
        }
    }
}
