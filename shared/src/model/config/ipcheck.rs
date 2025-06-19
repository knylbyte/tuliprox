#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct IpCheckConfigDto {
    /// URL that may return both IPv4 and IPv6 in one response
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,

    /// Dedicated URL to fetch only IPv4
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url_ipv4: Option<String>,

    /// Dedicated URL to fetch only IPv6
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url_ipv6: Option<String>,

    /// Optional regex pattern to extract IPv4
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pattern_ipv4: Option<String>,

    /// Optional regex pattern to extract IPv6
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pattern_ipv6: Option<String>,

}