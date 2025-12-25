#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ConfigPaths {
    pub config_path: String,
    pub config_file_path: String,
    pub sources_file_path: String,
    pub mapping_file_path: Option<String>,
    pub api_proxy_file_path: String,
    #[serde(default)]
    pub custom_stream_response_path: Option<String>,
}
