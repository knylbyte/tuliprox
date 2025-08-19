use shared::model::{DatabaseConfigDto, DatabaseTypeDto, PostgresConfigDto};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DatabaseType {
    Btree,
    Postgres,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatabaseConfig {
    pub enabled: bool,
    pub kind: DatabaseType,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostgresConfig {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: String,
    pub database: String,
    pub sslmode: String,
    pub max_connections: Option<u32>,
}

impl From<&DatabaseConfigDto> for DatabaseConfig {
    fn from(dto: &DatabaseConfigDto) -> Self {
        let kind = match dto.kind {
            DatabaseTypeDto::Postgres => DatabaseType::Postgres,
            DatabaseTypeDto::Btree => DatabaseType::Btree,
        };
        Self {
            enabled: dto.enabled,
            kind,
        }
    }
}

impl From<&PostgresConfigDto> for PostgresConfig {
    fn from(dto: &PostgresConfigDto) -> Self {
        Self {
            host: dto.host.clone(),
            port: dto.port,
            user: dto.user.clone(),
            password: dto.password.clone(),
            database: dto.database.clone(),
            sslmode: dto.sslmode.clone(),
            max_connections: dto.max_connections,
        }
    }
}

impl DatabaseConfig {
    pub fn url(&self, postgresql: Option<&PostgresConfig>) -> Option<String> {
        if !self.enabled {
            return None;
        }
        match (&self.kind, postgresql) {
            (DatabaseType::Postgres, Some(pg)) => Some(format!(
                "postgresql://{}:{}@{}:{}/{}?sslmode={}",
                pg.user, pg.password, pg.host, pg.port, pg.database, pg.sslmode
            )),
            _ => None,
        }
    }
}
