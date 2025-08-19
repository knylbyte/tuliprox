use shared::model::{DatabaseConfigDto, DatabaseTypeDto};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DatabaseType {
    Btree,
    Postgres,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatabaseConfig {
    pub kind: DatabaseType,
    pub url: Option<String>,
}

impl From<&DatabaseConfigDto> for DatabaseConfig {
    fn from(dto: &DatabaseConfigDto) -> Self {
        let kind = match dto.kind {
            DatabaseTypeDto::Postgres => DatabaseType::Postgres,
            DatabaseTypeDto::Btree => DatabaseType::Btree,
        };
        Self { kind, url: dto.url.clone() }
    }
}

