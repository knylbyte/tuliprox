use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DatabaseTypeDto {
    Btree,
    #[serde(rename = "postgresql")]
    Postgres,
}

impl Default for DatabaseTypeDto {
    fn default() -> Self {
        Self::Btree
    }
}

impl fmt::Display for DatabaseTypeDto {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            DatabaseTypeDto::Btree => "btree",
            DatabaseTypeDto::Postgres => "postgresql",
        };
        write!(f, "{s}")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DatabaseConfigDto {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub kind: DatabaseTypeDto,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PostgresConfigDto {
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default)]
    pub user: String,
    #[serde(default)]
    pub password: String,
    #[serde(default)]
    pub database: String,
    #[serde(default = "default_sslmode")]
    pub sslmode: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_connections: Option<u32>,
}

const fn default_port() -> u16 {
    5432
}

fn default_host() -> String {
    "localhost".to_string()
}

fn default_sslmode() -> String {
    "disable".to_string()
}
