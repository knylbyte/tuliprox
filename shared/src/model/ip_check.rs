
#[derive(Debug, serde::Serialize, serde::Deserialize, Default)]
pub struct IpCheckDto {
    #[serde(default)]
    pub ipv4: Option<String>,
    #[serde(default)]
    pub ipv6: Option<String>,
}