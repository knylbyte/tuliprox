use crate::app::components::config::{
    ApiConfigView, DatabaseConfigView, HdHomerunConfigView, IpCheckConfigView, MainConfigView,
    MessagingConfigView, ProxyConfigView, ReverseProxyConfigView, SchedulesConfigView,
    VideoConfigView, WebUiConfigView,
};
use crate::app::components::{Card, TabItem, TabSet};
use shared::error::TuliproxError;
use shared::info_err;
use std::fmt;
use std::str::FromStr;
use yew::prelude::*;
use yew_i18n::use_translation;

const MAIN_PAGE: &str = "main";
const API_PAGE: &str = "api";
const SCHEDULES_PAGE: &str = "schedules";
const MESSAGING_PAGE: &str = "messaging";
const WEBUI_PAGE: &str = "webui";
const REVERSE_PROXY_PAGE: &str = "reverse_proxy";
const HDHOMERUN_PAGE: &str = "hdhomerun";
const PROXY_PAGE: &str = "proxy";
const IPCHECK_PAGE: &str = "ipcheck";
const DATABASE_PAGE: &str = "database";
const VIDEO_PAGE: &str = "video";

enum ConfigPage {
    Main,
    Api,
    Schedules,
    Messaging,
    WebUi,
    ReverseProxy,
    HdHomerun,
    Proxy,
    IpCheck,
    Database,
    Video,
}

impl FromStr for ConfigPage {
    type Err = TuliproxError;

    fn from_str(s: &str) -> Result<Self, TuliproxError> {
        match s.to_lowercase().as_str() {
            MAIN_PAGE => Ok(ConfigPage::Main),
            API_PAGE => Ok(ConfigPage::Api),
            SCHEDULES_PAGE => Ok(ConfigPage::Schedules),
            MESSAGING_PAGE => Ok(ConfigPage::Messaging),
            WEBUI_PAGE => Ok(ConfigPage::WebUi),
            REVERSE_PROXY_PAGE => Ok(ConfigPage::ReverseProxy),
            HDHOMERUN_PAGE => Ok(ConfigPage::HdHomerun),
            PROXY_PAGE => Ok(ConfigPage::Proxy),
            IPCHECK_PAGE => Ok(ConfigPage::IpCheck),
            DATABASE_PAGE => Ok(ConfigPage::Database),
            VIDEO_PAGE => Ok(ConfigPage::Video),
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
            ConfigPage::Messaging => MESSAGING_PAGE,
            ConfigPage::WebUi => WEBUI_PAGE,
            ConfigPage::ReverseProxy => REVERSE_PROXY_PAGE,
            ConfigPage::HdHomerun => HDHOMERUN_PAGE,
            ConfigPage::Proxy => PROXY_PAGE,
            ConfigPage::IpCheck => IPCHECK_PAGE,
            ConfigPage::Database => DATABASE_PAGE,
            ConfigPage::Video => VIDEO_PAGE,
        };
        write!(f, "{s}")
    }
}

#[function_component]
pub fn ConfigView() -> Html {
    let translate = use_translation();
    let active_tab = use_state(|| ConfigPage::Main);

    let handle_tab_change = {
        let active_tab = active_tab.clone();
        Callback::from(move |tab_id: String| {
            if let Ok(page) = ConfigPage::from_str(&tab_id) {
                active_tab.set(page);
            }
        })
    };

    let tabs = vec![
        TabItem {
            id: ConfigPage::Main.to_string(),
            title: translate.t("LABEL.MAIN"),
            icon: "MainConfig".to_string(),
            children: html! { <MainConfigView/> },
        },
        TabItem {
            id: ConfigPage::Api.to_string(),
            title: translate.t("LABEL.API"),
            icon: "ApiConfig".to_string(),
            children: html! { <ApiConfigView/> },
        },
        TabItem {
            id: ConfigPage::Schedules.to_string(),
            title: translate.t("LABEL.SCHEDULES"),
            icon: "SchedulesConfig".to_string(),
            children: html! { <SchedulesConfigView/> },
        },
        TabItem {
            id: ConfigPage::Messaging.to_string(),
            title: translate.t("LABEL.MESSAGING"),
            icon: "MessagingConfig".to_string(),
            children: html! { <MessagingConfigView/> },
        },
        TabItem {
            id: ConfigPage::WebUi.to_string(),
            title: translate.t("LABEL.WEB_UI"),
            icon: "WebUiConfig".to_string(),
            children: html! { <WebUiConfigView/> },
        },
        TabItem {
            id: ConfigPage::ReverseProxy.to_string(),
            title: translate.t("LABEL.REVERSE_PROXY"),
            icon: "ReverseProxyConfig".to_string(),
            children: html! { <ReverseProxyConfigView/> },
        },
        TabItem {
            id: ConfigPage::HdHomerun.to_string(),
            title: translate.t("LABEL.HDHOMERUN_CONFIG"),
            icon: "HdHomerunConfig".to_string(),
            children: html! { <HdHomerunConfigView/> },
        },
        TabItem {
            id: ConfigPage::Proxy.to_string(),
            title: translate.t("LABEL.PROXY"),
            icon: "ProxyConfig".to_string(),
            children: html! { <ProxyConfigView/> },
        },
        TabItem {
            id: ConfigPage::IpCheck.to_string(),
            title: translate.t("LABEL.IP_CHECK"),
            icon: "IpCheckConfig".to_string(),
            children: html! { <IpCheckConfigView/> },
        },
        TabItem {
            id: ConfigPage::Database.to_string(),
            title: translate.t("LABEL.DATABASE"),
            icon: "DatabaseConfig".to_string(),
            children: html! { <DatabaseConfigView/> },
        },
        TabItem {
            id: ConfigPage::Video.to_string(),
            title: translate.t("LABEL.VIDEO"),
            icon: "VideoConfig".to_string(),
            children: html! { <VideoConfigView/> },
        },
    ];

    html! {
        <div class="tp__config-view">
            <div class="tp__config-view__header">
                <h1>{ translate.t("LABEL.CONFIG") } </h1>
            </div>
            <div class="tp__config-view__body">
            <Card>
                 <TabSet tabs={tabs} active_tab={Some((*active_tab).to_string())}
                     on_tab_change={Some(handle_tab_change)}
                     class="tp__config-view__tabset"/>
            </Card>
            </div>
        </div>
    }
}
