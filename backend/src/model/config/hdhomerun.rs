use shared::model::{HdHomeRunConfigDto, HdHomeRunDeviceConfigDto};
use crate::model::macros;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HdHomeRunDeviceConfig {
    pub friendly_name: String,
    pub manufacturer: String,
    pub model_name: String,
    pub model_number: String,
    pub firmware_name: String,
    pub firmware_version: String,
    pub device_id: String,
    pub device_type: String,
    pub device_udn: String,
    pub name: String,
    pub port: u16,
    pub tuner_count: u8,
    pub t_username: String,
    pub t_enabled: bool,
}

macros::from_impl!(HdHomeRunDeviceConfig);
impl From<&HdHomeRunDeviceConfigDto> for HdHomeRunDeviceConfig {
    fn from(dto: &HdHomeRunDeviceConfigDto) -> Self {
        Self {
            friendly_name: dto.friendly_name.clone(),
            manufacturer: dto.manufacturer.clone(),
            model_name: dto.model_name.clone(),
            model_number: dto.model_number.clone(),
            firmware_name: dto.firmware_name.clone(),
            firmware_version: dto.firmware_version.clone(),
            device_id: dto.device_id.clone(),
            device_type: dto.device_type.clone(),
            device_udn: dto.device_udn.clone(),
            name: dto.name.clone(),
            port: dto.port,
            tuner_count: dto.tuner_count,
            t_username: String::new(),
            t_enabled: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
pub struct HdHomeRunConfig {
    pub enabled: bool,
    pub auth: bool,
    pub ssdp_discovery: bool,
    pub proprietary_discovery: bool,
    pub devices: Vec<HdHomeRunDeviceConfig>,
}

macros::from_impl!(HdHomeRunConfig);
impl From<&HdHomeRunConfigDto> for HdHomeRunConfig {
    fn from(dto: &HdHomeRunConfigDto) -> Self {
        Self {
            enabled: dto.enabled,
            auth: dto.auth,
            ssdp_discovery: dto.ssdp_discovery,
            proprietary_discovery: dto.proprietary_discovery,
            devices: dto.devices.iter().map(Into::into).collect(),
        }
    }
}