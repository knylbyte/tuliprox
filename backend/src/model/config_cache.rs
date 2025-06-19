use std::path::PathBuf;
use log::error;
use path_clean::PathClean;
use crate::tuliprox_error::{info_err, TuliproxError, TuliproxErrorKind};
use shared::utils::parse_size_base_2;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct CacheConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dir: Option<String>,
    #[serde(skip)]
    pub t_size: usize,
}

impl CacheConfig {
    pub(crate) fn prepare(&mut self, working_dir: &str) -> Result<(), TuliproxError>{
        if self.enabled {
            let work_path = PathBuf::from(working_dir);
            if self.dir.is_none() {
                self.dir = Some(work_path.join("cache").to_string_lossy().to_string());
            } else {
                let mut cache_dir = self.dir.as_ref().unwrap().to_string();
                if PathBuf::from(&cache_dir).is_relative() {
                    cache_dir = work_path.join(&cache_dir).clean().to_string_lossy().to_string();
                }
                self.dir = Some(cache_dir.to_string());
            }
            match self.size.as_ref() {
                None => self.t_size = 1024,
                Some(val) => match parse_size_base_2(val) {
                    Ok(size) => match usize::try_from(size) {
                        Ok(s) => self.t_size = s,
                        Err(err) => {
                            self.t_size = 0;
                            error!("Cache size could not be determined. Setting to 0. {err}");
                        }
                    },
                    Err(err) => { return Err(info_err!(format!("Failed to read cache size: {err}"))) }
                }
            }
        }
        Ok(())
    }
}