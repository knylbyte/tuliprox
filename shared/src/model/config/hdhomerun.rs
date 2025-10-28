use std::collections::HashSet;
use log::warn;
use crate::create_tuliprox_error_result;
use crate::error::{TuliproxError, TuliproxErrorKind};
use crate::utils::{default_as_true, generate_hdhr_device_id, generate_hdhr_device_id_from_base, validate_hdhr_device_id, hash_string, hex_encode};

fn default_friendly_name() -> String { String::from("TuliproxTV") }
fn default_manufacturer() -> String { String::from("Silicondust") }
fn default_model_name() -> String { String::from("HDTC-2US") }
fn default_firmware_name() -> String { String::from("hdhomeruntc_atsc") }
fn default_firmware_version() -> String { String::from("20170930") }
fn default_device_type() -> String { String::from("urn:schemas-upnp-org:device:MediaServer:1") }
fn default_device_udn() -> String { String::from("uuid:12345678-90ab-cdef-1234-567890abcdef::urn:dial-multicast:com.silicondust.hdhomerun") }

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct HdHomeRunDeviceConfigDto {
    #[serde(default = "default_friendly_name")]
    pub friendly_name: String,
    #[serde(default = "default_manufacturer")]
    pub manufacturer: String,
    #[serde(default = "default_model_name")]
    pub model_name: String,
    #[serde(default = "default_model_name")]
    pub model_number: String,
    #[serde(default = "default_firmware_name")]
    pub firmware_name: String,
    #[serde(default = "default_firmware_version")]
    pub firmware_version: String,
    #[serde(default)]
    pub device_id: String,
    #[serde(default = "default_device_type")]
    pub device_type: String,
    #[serde(default = "default_device_udn")]
    pub device_udn: String,
    pub name: String,
    #[serde(default)]
    pub port: u16,
    #[serde(default)]
    pub tuner_count: u8,
}

impl Default for HdHomeRunDeviceConfigDto {
    fn default() -> Self {
        Self {
            friendly_name: default_friendly_name(),
            manufacturer: default_manufacturer(),
            model_name: default_model_name(),
            model_number: default_model_name(),
            firmware_name: default_firmware_name(),
            firmware_version: default_firmware_version(),
            device_id: String::new(),
            device_type: default_device_type(),
            device_udn: default_device_udn(),
            name: String::new(),
            port: 0,
            tuner_count: 0,
        }
    }
}

impl HdHomeRunDeviceConfigDto {
    pub fn prepare(&mut self, device_num: u8) -> Result<(), TuliproxError> {
        self.name = self.name.trim().to_string();
        if self.name.is_empty() {
            self.name = format!("device{device_num}");
            warn!("Device name empty, assigned new name: {}", self.name);
        }

        if self.tuner_count == 0 {
            self.tuner_count = 1;
        }

        if device_num > 0 && self.friendly_name == default_friendly_name() {
            self.friendly_name = format!("{} {}", self.friendly_name, device_num);
        }

        // --- UDN Logic ---
        // If UDN is the default value or empty, generate a stable UUID from the device name.
        if self.device_udn == default_device_udn() || self.device_udn.is_empty() {
            let hash = hash_string(&self.name);
            // Format the hash into a valid UUID string
            let p1 = hex_encode(&hash[0..4]);
            let p2 = hex_encode(&hash[4..6]);
            let p3 = hex_encode(&hash[6..8]);
            let p4 = hex_encode(&hash[8..10]);
            let p5 = hex_encode(&hash[10..16]);
            self.device_udn = format!("{p1}-{p2}-{p3}-{p4}-{p5}");
            warn!("HDHomeRun device '{}' is missing a unique device_udn. A new one has been generated: {}", self.name, self.device_udn);
        } else {
            // Ensure only the UUID part is stored.
            if let Some(uuid_part) = self.device_udn.strip_prefix("uuid:") {
                self.device_udn = uuid_part.split("::").next().unwrap_or(uuid_part).to_string();
            }
        }

        // --- Device ID Logic ---
        if self.device_id.is_empty() {
            self.device_id = generate_hdhr_device_id();
            warn!("HDHomeRun device '{}' is missing a device_id. A new one has been generated: {}", self.name, self.device_id);
        } else if !validate_hdhr_device_id(&self.device_id) {
            let old_id = self.device_id.clone();
            self.device_id = generate_hdhr_device_id_from_base(&self.device_id);
            warn!("HDHomeRun device '{}' has an invalid device_id '{}'. A valid one has been generated: {}", self.name, old_id, self.device_id);
        }
        Ok(())
    }
}


#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct HdHomeRunConfigDto {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub auth: bool,
    #[serde(default = "default_as_true")]
    pub ssdp_discovery: bool,
    #[serde(default = "default_as_true")]
    pub proprietary_discovery: bool,
    pub devices: Vec<HdHomeRunDeviceConfigDto>,
}

impl HdHomeRunConfigDto {
    pub fn is_empty(&self) -> bool {
        !self.enabled && !self.auth && self.devices.is_empty() && !self.ssdp_discovery && !self.proprietary_discovery
    }

    pub fn clean(&mut self) {
        // This method can be left empty if no cleanup is required.
        // It's only included to satisfy the frontend compiler.
    }

    pub fn prepare(&mut self, api_port: u16)  -> Result<(), TuliproxError> {
        let mut names = HashSet::new();
        let mut ports = HashSet::new();
        let mut device_ids = HashSet::new();
        ports.insert(api_port);
        for (device_num, device) in (0_u8..).zip(self.devices.iter_mut()) {
            device.prepare(device_num)?;
            if !names.insert(device.name.clone()) {
                return create_tuliprox_error_result!(TuliproxErrorKind::Info, "HdHomeRun duplicate device name {}", device.name);
            }
            if device.port > 0 && !ports.insert(device.port) {
                return create_tuliprox_error_result!(TuliproxErrorKind::Info, "HdHomeRun duplicate port {}", device.port);
            }
            if !device_ids.insert(device.device_id.clone()) {
                return create_tuliprox_error_result!(TuliproxErrorKind::Info, "HdHomeRun duplicate device_id {}", device.device_id);
            }
        }
        let mut current_port = api_port.saturating_add(1);
        for device in &mut self.devices {
            if device.port == 0 {
                while ports.contains(&current_port) || current_port == 0 {
                  current_port = current_port.wrapping_add(1);
                  if current_port == api_port { // full cycle guard
                    return create_tuliprox_error_result!(TuliproxErrorKind::Info, "No free port available for HdHomeRun devices");
                  }
                }

                device.port = current_port;
                ports.insert(current_port);
                current_port += 1;
            }
        }
        Ok(())
    }
}