
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct ScheduleConfig {
    #[serde(default)]
    pub schedule: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub targets: Option<Vec<String>>,
}
