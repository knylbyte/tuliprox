use enum_iterator::Sequence;

#[derive( Debug, Copy, Clone, serde::Serialize, serde::Deserialize, Sequence,
    PartialEq, Eq, Hash, Default)]
pub enum StrmExportStyle {
    #[serde(rename = "kodi")]
    #[default]
    Kodi,
    #[serde(rename = "plex")]
    Plex,
    #[serde(rename = "emby")]
    Emby,
    #[serde(rename = "jellyfin")]
    Jellyfin,
}