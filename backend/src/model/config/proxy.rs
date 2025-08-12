use shared::model::{ProxyPoolConfigDto, ProxyServerConfigDto};
use crate::model::macros;

#[derive(Debug, Clone)]
pub struct ProxyServerConfig {
    pub url: String,
    pub username: Option<String>,
    pub password: Option<String>,
    pub weight: u8,
}

#[derive(Debug, Clone)]
pub struct ProxyPoolConfig {
    pub interval_secs: u64,
    pub proxies: Vec<ProxyServerConfig>,
}

macros::from_impl!(ProxyServerConfig);
macros::from_impl!(ProxyPoolConfig);

impl From<&ProxyServerConfigDto> for ProxyServerConfig {
    fn from(dto: &ProxyServerConfigDto) -> Self {
        Self {
            url: dto.url.clone(),
            username: dto.username.clone(),
            password: dto.password.clone(),
            weight: dto.weight,
        }
    }
}

impl From<&ProxyServerConfig> for ProxyServerConfigDto {
    fn from(p: &ProxyServerConfig) -> Self {
        Self {
            url: p.url.clone(),
            username: p.username.clone(),
            password: p.password.clone(),
            weight: p.weight,
        }
    }
}

impl From<&ProxyPoolConfigDto> for ProxyPoolConfig {
    fn from(dto: &ProxyPoolConfigDto) -> Self {
        Self {
            interval_secs: dto.interval_secs,
            proxies: dto.proxies.iter().map(Into::into).collect(),
        }
    }
}

impl From<&ProxyPoolConfig> for ProxyPoolConfigDto {
    fn from(p: &ProxyPoolConfig) -> Self {
        Self {
            interval_secs: p.interval_secs,
            proxies: p.proxies.iter().map(Into::into).collect(),
        }
    }
}

