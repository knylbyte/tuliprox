#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ScheduleConfigDto {
    #[serde(default)]
    pub schedule: String,
    #[serde(default)]
    pub targets: Option<Vec<String>>,
}