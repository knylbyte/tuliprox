use shared::error::TuliproxError;
use shared::info_err;
use shared::model::{ConfigApiDto, HdHomeRunConfigDto, IpCheckConfigDto, MainConfigDto, MessagingConfigDto,
                    ProxyConfigDto, ReverseProxyConfigDto, SchedulesConfigDto, VideoConfigDto, WebUiConfigDto};
use std::fmt;
use std::str::FromStr;

const MAIN_PAGE: &str = "main";
const API_PAGE: &str = "api";
const SCHEDULES_PAGE: &str = "schedules";
const MESSAGING_PAGE: &str = "messaging";
const WEBUI_PAGE: &str = "webui";
const REVERSE_PROXY_PAGE: &str = "reverse_proxy";
const HDHOMERUN_PAGE: &str = "hdhomerun";
const PROXY_PAGE: &str = "proxy";
const IPCHECK_PAGE: &str = "ipcheck";
const VIDEO_PAGE: &str = "video";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum ConfigPage {
    Main,
    Api,
    Schedules,
    Video,
    Messaging,
    WebUi,
    ReverseProxy,
    HdHomerun,
    Proxy,
    IpCheck,
}

impl FromStr for ConfigPage {
    type Err = TuliproxError;

    fn from_str(s: &str) -> Result<Self, TuliproxError> {
        match s.to_lowercase().as_str() {
            MAIN_PAGE => Ok(ConfigPage::Main),
            API_PAGE => Ok(ConfigPage::Api),
            SCHEDULES_PAGE => Ok(ConfigPage::Schedules),
            VIDEO_PAGE => Ok(ConfigPage::Video),
            MESSAGING_PAGE => Ok(ConfigPage::Messaging),
            WEBUI_PAGE => Ok(ConfigPage::WebUi),
            REVERSE_PROXY_PAGE => Ok(ConfigPage::ReverseProxy),
            HDHOMERUN_PAGE => Ok(ConfigPage::HdHomerun),
            PROXY_PAGE => Ok(ConfigPage::Proxy),
            IPCHECK_PAGE => Ok(ConfigPage::IpCheck),
            _ => Err(info_err!(format!("Unknown config page: {s}"))),
        }
    }
}

impl fmt::Display for ConfigPage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            ConfigPage::Main => MAIN_PAGE,
            ConfigPage::Api => API_PAGE,
            ConfigPage::Schedules => SCHEDULES_PAGE,
            ConfigPage::Video => VIDEO_PAGE,
            ConfigPage::Messaging => MESSAGING_PAGE,
            ConfigPage::WebUi => WEBUI_PAGE,
            ConfigPage::ReverseProxy => REVERSE_PROXY_PAGE,
            ConfigPage::HdHomerun => HDHOMERUN_PAGE,
            ConfigPage::Proxy => PROXY_PAGE,
            ConfigPage::IpCheck => IPCHECK_PAGE,
        };
        write!(f, "{s}")
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ConfigForm {
    Main(bool, MainConfigDto),
    Api(bool, ConfigApiDto),
    Schedules(bool, SchedulesConfigDto),
    Video(bool, VideoConfigDto),
    Messaging(bool, MessagingConfigDto),
    WebUi(bool, WebUiConfigDto),
    ReverseProxy(bool, ReverseProxyConfigDto),
    HdHomerun(bool, HdHomeRunConfigDto),
    Proxy(bool, ProxyConfigDto),
    IpCheck(bool, IpCheckConfigDto),
}

impl ConfigForm {
    pub(crate) fn is_modified(&self) -> bool {
        matches!(self, ConfigForm::Main(true, _)
              | ConfigForm::Api(true, _)
              | ConfigForm::Schedules(true, _)
              | ConfigForm::Video(true, _)
              | ConfigForm::Messaging(true, _)
              | ConfigForm::WebUi(true, _)
              | ConfigForm::ReverseProxy(true, _)
              | ConfigForm::HdHomerun(true, _)
              | ConfigForm::Proxy(true, _)
              | ConfigForm::IpCheck(true, _))
    }
}
