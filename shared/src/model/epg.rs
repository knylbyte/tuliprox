use std::sync::Arc;
use chrono::Utc;
use serde::{Deserialize, Serialize};

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
    pub id: Arc<str>,
    pub title: Option<Arc<str>>,
    pub icon: Option<Arc<str>>,
    pub programmes: Vec<EpgProgramme>,
}

impl EpgChannel {
    pub fn new(id: Arc<str>) -> Self {
        Self {
            id,
            title: None,
            icon: None,
            programmes: Vec::new(),
        }
    }

    pub fn get_programme_with_limit(&self, limit: u32) -> Vec<&EpgProgramme> {
        let now = Utc::now().timestamp();

        // find index for first relevant entry
        let start_idx = self.programmes.iter()
            .position(|p| (p.start <= now && now <= p.stop) || (p.start >= now))
            .unwrap_or(self.programmes.len()); // nothing found, empty response

        // slice from start_idx, max. limit
        self.programmes
            .iter()
            .skip(start_idx)
            .take(limit as usize)
            .collect()
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct EpgProgramme {
    pub start: i64,
    pub stop: i64,
    pub title: Option<Arc<str>>,
    pub desc: Option<Arc<str>>,
    #[serde(skip)]
    pub channel: Arc<str>,
}

impl EpgProgramme {
    pub fn new(start: i64, stop: i64, channel: Arc<str>) -> Self {
        Self {
            start,
            stop,
            channel,
            title: None,
            desc: None
        }
    }
}
