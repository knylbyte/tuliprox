#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default, PartialEq)]
pub struct PlaylistClusterCategoriesDto {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub live: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vod: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub series: Option<Vec<String>>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default, PartialEq)]
pub struct PlaylistCategoriesDto {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub xtream: Option<PlaylistClusterCategoriesDto>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub m3u: Option<PlaylistClusterCategoriesDto>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default, PartialEq)]
pub struct PlaylistClusterBouquetDto {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub live: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vod: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub series: Option<Vec<String>>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default, PartialEq)]
pub struct PlaylistBouquetDto {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub xtream: Option<PlaylistClusterBouquetDto>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub m3u: Option<PlaylistClusterBouquetDto>,
}