use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DatabaseTypeDto {
    Btree,
    Postgres,
}

impl Default for DatabaseTypeDto {
    fn default() -> Self {
        Self::Btree
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DatabaseConfigDto {
    #[serde(default)]
    pub kind: DatabaseTypeDto,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

