use shared::model::GeoIpConfigDto;
use crate::model::macros;

#[derive(Debug, Clone)]
pub struct GeoIpConfig {
    pub(crate) enabled: bool,
    pub(crate) url: String,
}

macros::from_impl!(GeoIpConfig);

impl From<&GeoIpConfigDto> for GeoIpConfig {
    fn from(dto: &GeoIpConfigDto) -> Self {
        Self {
            enabled: dto.enabled,
            url: dto.url.clone(),
        }
    }
}

impl From<&GeoIpConfig> for GeoIpConfigDto {
    fn from(instance: &GeoIpConfig) -> Self {
        Self {
            enabled: instance.enabled,
            url: instance.url.clone(),
        }
    }
}
