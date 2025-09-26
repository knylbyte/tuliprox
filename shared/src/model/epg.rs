use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub fn parse_xmltv_time(t: &str) -> i64 {
    let fmt = "%Y%m%d%H%M%S %z";
    DateTime::parse_from_str(t, fmt)
        .unwrap()
        .with_timezone(&Utc)
        .timestamp()
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
    pub programmes: Vec<EpgProgramme>,
}

impl EpgChannel {
    pub fn new(id: String) -> Self {
        Self {
            id,
            title: String::new(),
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
    pub fn new(start: String, stop: String, channel: String) -> Self {
        Self {
            start: parse_xmltv_time(&start),
            stop: parse_xmltv_time(&stop),
            channel,
            title: String::new(),
        }
    }
}
