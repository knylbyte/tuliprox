use std::fmt::{Display, Formatter};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum ConfigType {
    Config,
    ApiProxy,
    Mapping,
    Sources,
}

impl Display for ConfigType {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", match self {
            Self::Config => "Config",
            Self::ApiProxy => "ApiProxy",
            Self::Mapping => "Mapping",
            Self::Sources => "Sources",
        })
    }
}