use crate::tuliprox_error::{create_tuliprox_error_result, info_err, TuliproxError, TuliproxErrorKind};
use shared::utils::CONSTANTS;
use log::warn;
use regex::Regex;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EpgSource {
    pub(crate) url: String,
    #[serde(default)]
    pub priority: i16,
    #[serde(default)]
    pub logo_override: bool,
}

impl EpgSource {
    pub fn prepare(&mut self) {
        self.url = self.url.trim().to_string();
    }

    pub fn is_valid(&self) -> bool {
        !self.url.is_empty()
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum EpgNamePrefix {
    #[default]
    Ignore,
    Suffix(String),
    Prefix(String),
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EpgSmartMatchConfig {
    #[serde(default)]
    pub enabled: bool,
    pub normalize_regex: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strip: Option<Vec<String>>,
    #[serde(default)]
    pub name_prefix: EpgNamePrefix,
    #[serde(default)]
    pub name_prefix_separator: Option<Vec<char>>,
    #[serde(default)]
    pub fuzzy_matching: bool,
    #[serde(default)]
    pub match_threshold: u16,
    #[serde(default)]
    pub best_match_threshold: u16,
    #[serde(skip)]
    pub t_strip: Vec<String>,
    #[serde(skip)]
    pub t_normalize_regex: Option<Regex>,
    #[serde(skip)]
    pub t_name_prefix_separator: Vec<char>,

}

impl EpgSmartMatchConfig {
    /// Creates a new enabled `EpgSmartMatchConfig` with default settings and prepares it.
    ///
    /// Returns an error if preparation fails.
    ///
    /// # Examples
    ///
    /// ```
    /// let config = EpgSmartMatchConfig::new().unwrap();
    /// assert!(config.enabled);
    /// ```
    pub fn new() -> Result<Self, TuliproxError> {
        let mut this = Self { enabled: true, ..Self::default() };
        this.prepare()?;
        Ok(this)
    }

    /// # Panics
    ///
    /// Prepares the EPG smart match configuration by validating thresholds, compiling normalization regex, and setting default values as needed.
    ///
    /// Adjusts match thresholds to valid ranges, compiles the normalization regex, and sets default strip values and name prefix separators if not provided. Returns an error if the normalization regex is invalid.
    ///
    /// # Returns
    ///
    /// `Ok(())` if preparation succeeds, or an `TuliproxError` if regex compilation fails.
    pub fn prepare(&mut self) -> Result<(), TuliproxError> {
        if !self.enabled {
            return Ok(());
        }

        self.t_name_prefix_separator = match &self.name_prefix_separator {
            None => vec![':', '|', '-'],
            Some(list) => list.clone(),
        };

        if self.match_threshold == 0 {
            self.match_threshold = 80;
        } else if self.match_threshold < 10 {
            warn!("match_threshold is less than 10%, setting to 10%");
            self.match_threshold = 10;
        } else if self.match_threshold > 100 {
            warn!("match_threshold is more than 100%, setting to 80%");
            self.match_threshold = 100;
        }

        if self.best_match_threshold == 0 || self.best_match_threshold > 100 || self.best_match_threshold < self.match_threshold {
            self.best_match_threshold = 99;
        }

        self.t_normalize_regex = match self.normalize_regex.as_ref() {
            None => Some(CONSTANTS.re_epg_normalize.clone()),
            Some(regstr) => {
                let re = regex::Regex::new(regstr.as_str());
                if re.is_err() {
                    return create_tuliprox_error_result!(TuliproxErrorKind::Info, "cant parse regex: {}", regstr);
                }
                Some(re.unwrap())
            }
        };

        match &self.strip {
            Some(list) => self.t_strip = list.iter().map(|s| s.to_lowercase()).collect(),
            None => self.t_strip = ["3840p", "uhd", "fhd", "hd", "sd", "4k", "plus", "raw", "full hd"].iter().map(std::string::ToString::to_string).collect(),
        }
        Ok(())
    }
}

impl Default for EpgSmartMatchConfig {
    fn default() -> Self {
        let mut instance = EpgSmartMatchConfig {
            enabled: false,
            normalize_regex: None,
            strip: None,
            name_prefix: EpgNamePrefix::default(),
            name_prefix_separator: None,
            fuzzy_matching: false,
            match_threshold: 0,
            best_match_threshold: 0,
            t_strip: Vec::default(),
            t_normalize_regex: None,
            t_name_prefix_separator: Vec::default(),
        };
        let _ = instance.prepare();
        instance
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EpgConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sources: Option<Vec<EpgSource>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub smart_match: Option<EpgSmartMatchConfig>,
    #[serde(skip)]
    pub t_sources: Vec<EpgSource>,
    #[serde(skip)]
    pub t_smart_match: EpgSmartMatchConfig,
}

impl EpgConfig {
    pub fn prepare<F>(&mut self, create_auto_url: F, include_computed: bool) -> Result<(), TuliproxError>
    where
        F: Fn() -> Result<String, String>,
    {
        if include_computed {
            self.t_sources = Vec::new();
            if let Some(epg_sources) = self.sources.as_mut() {
                for epg_source in epg_sources {
                    epg_source.prepare();
                    if epg_source.is_valid() {
                        if include_computed && epg_source.url.eq_ignore_ascii_case("auto") {
                            let auto_url = create_auto_url();
                            match auto_url {
                                Ok(provider_url) => {
                                    self.t_sources.push(EpgSource {
                                        url: provider_url,
                                        priority: epg_source.priority,
                                        logo_override: epg_source.logo_override,
                                    });
                                }
                                Err(err) => return Err(info_err!(err))
                            }
                        } else {
                            self.t_sources.push(epg_source.clone());
                        }
                    }
                }
            }

            self.t_smart_match = match self.smart_match.as_mut() {
                None => {
                    let mut normalize: EpgSmartMatchConfig = EpgSmartMatchConfig::default();
                    normalize.prepare()?;
                    normalize
                }
                Some(normalize_cfg) => {
                    let mut normalize: EpgSmartMatchConfig = normalize_cfg.clone();
                    normalize.prepare()?;
                    normalize
                }
            };
        }
        Ok(())
    }
}