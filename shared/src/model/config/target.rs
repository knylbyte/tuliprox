use crate::model::{ClusterFlags, ConfigRenameDto, ConfigSortDto, ProcessingOrder, StrmExportStyle, TargetType, TraktConfigDto};
use crate::utils::{default_as_true, default_resolve_delay_secs, default_as_default};
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct ConfigTargetOptionsDto {
    #[serde(default)]
    pub ignore_logo: bool,
    #[serde(default)]
    pub share_live_streams: bool,
    #[serde(default)]
    pub remove_duplicates: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub force_redirect: Option<ClusterFlags>,
}

#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct XtreamTargetOutputDto {
    #[serde(default = "default_as_true")]
    pub skip_live_direct_source: bool,
    #[serde(default = "default_as_true")]
    pub skip_video_direct_source: bool,
    #[serde(default = "default_as_true")]
    pub skip_series_direct_source: bool,
    #[serde(default)]
    pub resolve_series: bool,
    #[serde(default = "default_resolve_delay_secs")]
    pub resolve_series_delay: u16,
    #[serde(default)]
    pub resolve_vod: bool,
    #[serde(default = "default_resolve_delay_secs")]
    pub resolve_vod_delay: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trakt: Option<TraktConfigDto>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct M3uTargetOutputDto {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
    #[serde(default)]
    pub include_type_in_url: bool,
    #[serde(default)]
    pub mask_redirect_url: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StrmTargetOutputDto {
    pub directory: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    #[serde(default)]
    pub style: StrmExportStyle,
    #[serde(default)]
    pub flat: bool,
    #[serde(default)]
    pub underscore_whitespace: bool,
    #[serde(default)]
    pub cleanup: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strm_props: Option<Vec<String>>,
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HdHomeRunTargetOutputDto {
    pub device: String,
    pub username: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub use_output: Option<TargetType>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields, tag = "type", rename_all = "lowercase")]
pub enum TargetOutputDto {
    Xtream(XtreamTargetOutputDto),
    M3u(M3uTargetOutputDto),
    Strm(StrmTargetOutputDto),
    HdHomeRun(HdHomeRunTargetOutputDto),
}


#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct ConfigTargetDto {
    #[serde(skip)]
    pub id: u16,
    #[serde(default = "default_as_true")]
    pub enabled: bool,
    #[serde(default = "default_as_default")]
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options: Option<ConfigTargetOptionsDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sort: Option<ConfigSortDto>,
    pub filter: String,
    #[serde(default)]
    pub output: Vec<TargetOutputDto>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rename: Option<Vec<ConfigRenameDto>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mapping: Option<Vec<String>>,
    #[serde(default)]
    pub processing_order: ProcessingOrder,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub watch: Option<Vec<String>>,
}


