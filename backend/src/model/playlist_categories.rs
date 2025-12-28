#[derive(Debug, serde::Serialize, serde::Deserialize, Default)]
pub struct PlaylistCategories {
    #[serde(default)]
    pub live: Option<Vec<String>>,
    #[serde(default)]
    pub vod: Option<Vec<String>>,
    #[serde(default)]
    pub series: Option<Vec<String>>,
}
