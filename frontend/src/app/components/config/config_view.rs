use crate::app::components::config::config_page::{ConfigForm, ConfigPage, LABEL_API_CONFIG, LABEL_HDHOMERUN_CONFIG, LABEL_IP_CHECK_CONFIG, LABEL_LOG_CONFIG, LABEL_MAIN_CONFIG, LABEL_MESSAGING_CONFIG, LABEL_PROXY_CONFIG, LABEL_REVERSE_PROXY_CONFIG, LABEL_SCHEDULES_CONFIG, LABEL_VIDEO_CONFIG, LABEL_WEB_UI_CONFIG};
use crate::app::components::config::config_view_context::ConfigViewContext;
use crate::app::components::config::{ApiConfigView, HdHomerunConfigView, IpCheckConfigView, LogConfigView, MainConfigView, MessagingConfigView, ProxyConfigView, ReverseProxyConfigView, SchedulesConfigView, VideoConfigView, WebUiConfigView};
use crate::app::components::{Card, TabItem, TabSet, TextButton};
use crate::html_if;
use std::str::FromStr;
use yew::platform::spawn_local;
use yew::prelude::*;
use yew_i18n::use_translation;
use shared::model::ConfigDto;
use crate::app::components::config::config_update::update_config;
use crate::app::{ConfigContext};
use crate::hooks::use_service_context;

const LABEL_CONFIG: &str = "LABEL.CONFIG";
const LABEL_EDIT: &str = "LABEL.EDIT";
const LABEL_VIEW: &str = "LABEL.VIEW";
const LABEL_SAVE: &str = "LABEL.SAVE";
const LABEL_UPDATE_GEOIP: &str = "LABEL.UPDATE_GEOIP_DB";
macro_rules! collect_modified {
    ($forms:expr, [ $($field:ident),+ $(,)? ]) => {{
        let mut modified = Vec::new();
        $(
            if let Some(form) = $forms.$field.as_ref() {
                if form.is_modified() {
                   modified.push(form.clone());
                }
            }
        )+
        modified
    }};
}

fn config_form_to_config_page(form: &ConfigForm) -> ConfigPage {
    match form {
        ConfigForm::Main(_, _) => ConfigPage::Main,
        ConfigForm::Api(_, _) => ConfigPage::Api,
        ConfigForm::Log(_, _) => ConfigPage::Log,
        ConfigForm::Schedules(_, _) => ConfigPage::Schedules,
        ConfigForm::Video(_, _) => ConfigPage::Video,
        ConfigForm::Messaging(_, _) => ConfigPage::Messaging,
        ConfigForm::WebUi(_, _) => ConfigPage::WebUi,
        ConfigForm::ReverseProxy(_, _) => ConfigPage::ReverseProxy,
        ConfigForm::HdHomerun(_, _) => ConfigPage::HdHomerun,
        ConfigForm::Proxy(_, _) => ConfigPage::Proxy,
        ConfigForm::IpCheck(_, _) => ConfigPage::IpCheck
    }
}

#[derive(Default, Debug, Clone, PartialEq)]
struct ConfigFormState {
    pub main: Option<ConfigForm>,
    pub api: Option<ConfigForm>,
    pub log: Option<ConfigForm>,
    pub schedules: Option<ConfigForm>,
    pub video: Option<ConfigForm>,
    pub messaging: Option<ConfigForm>,
    pub web_ui: Option<ConfigForm>,
    pub reverse_proxy: Option<ConfigForm>,
    pub hd_homerun: Option<ConfigForm>,
    pub proxy: Option<ConfigForm>,
    pub ipcheck: Option<ConfigForm>,
}

#[function_component]
pub fn ConfigView() -> Html {
    let translate = use_translation();
    let services_ctx = use_service_context();
    let config_ctx = use_context::<ConfigContext>().expect("ConfigContext not found");

    let active_tab = use_state(|| ConfigPage::Main);
    let edit_mode = use_state(|| false);
    let form_state = use_state(ConfigFormState::default);

    let handle_tab_change = {
        let active_tab = active_tab.clone();
        Callback::from(move |tab_id: String| {
            if let Ok(page) = ConfigPage::from_str(&tab_id) {
                active_tab.set(page);
            }
        })
    };

    let tabs = {
        let form_state = form_state.clone();
        let translate = translate.clone();
        let edit_value = *edit_mode;

        use_memo((form_state, edit_value, translate.clone()), move |(forms, editing, translate)| {
            let forms: &ConfigFormState = forms;
            let modified_pages = collect_modified!(forms, [
                main, api, log, schedules, video, messaging, web_ui,
                reverse_proxy, hd_homerun, proxy, ipcheck
            ]).iter()
                .map(config_form_to_config_page)
                .collect::<Vec<ConfigPage>>();

            let tab_configs = vec![
                (ConfigPage::Main, LABEL_MAIN_CONFIG, html! { <MainConfigView/> }, "MainConfig"),
                (ConfigPage::Api, LABEL_API_CONFIG, html! { <ApiConfigView/> }, "ApiConfig"),
                (ConfigPage::Log, LABEL_LOG_CONFIG, html! { <LogConfigView/> }, "Log"),
                (ConfigPage::Schedules, LABEL_SCHEDULES_CONFIG, html! { <SchedulesConfigView/> }, "SchedulesConfig"),
                (ConfigPage::Messaging, LABEL_MESSAGING_CONFIG, html! { <MessagingConfigView/> }, "MessagingConfig"),
                (ConfigPage::WebUi, LABEL_WEB_UI_CONFIG, html! { <WebUiConfigView/> }, "WebUiConfig"),
                (ConfigPage::ReverseProxy, LABEL_REVERSE_PROXY_CONFIG, html! { <ReverseProxyConfigView/> }, "ReverseProxyConfig"),
                (ConfigPage::HdHomerun, LABEL_HDHOMERUN_CONFIG, html! { <HdHomerunConfigView/> }, "HdHomerunConfig"),
                (ConfigPage::Proxy, LABEL_PROXY_CONFIG, html! { <ProxyConfigView/> }, "ProxyConfig"),
                (ConfigPage::IpCheck, LABEL_IP_CHECK_CONFIG, html! { <IpCheckConfigView/> }, "IpCheckConfig"),
                (ConfigPage::Video, LABEL_VIDEO_CONFIG, html! { <VideoConfigView/> }, "VideoConfig"),
            ];

            let editing = *editing;
            tab_configs.into_iter().map(|(page, label, children, icon)| {
                let is_modified = editing && modified_pages.contains(&page);
                TabItem {
                    id: page.to_string(),
                    title: translate.t(label),
                    icon: icon.to_string(),
                    children,
                    active_class: if is_modified { Some("tp__tab__modified__active".to_string()) } else { None },
                    inactive_class: if is_modified { Some("tp__tab__modified__inactive".to_string()) } else { None },
                }
            }).collect::<Vec<TabItem>>()
        })
    };

    let handle_config_edit = {
        let set_edit_mode = edit_mode.clone();
        Callback::from(move |_| {
            set_edit_mode.set(!*set_edit_mode);
        })
    };

    let handle_save_config = {
        let config_ctx = config_ctx.clone();
        let translate = translate.clone();
        let services = services_ctx.clone();
        let get_form_state = form_state.clone();
        let set_edit_mode = edit_mode.clone();

        Callback::from(move |_| {
            let forms = &*get_form_state;
            let modified_forms: Vec<ConfigForm> = collect_modified!(forms, [
                main, api, log, schedules, video, messaging, web_ui,
                reverse_proxy, hd_homerun, proxy, ipcheck
            ]);

            if !modified_forms.is_empty() {
                let mut config_dto = config_ctx.config.as_ref().map_or_else(ConfigDto::default,
                                                                            |app_cfg| app_cfg.config.clone());
                update_config(&mut config_dto, modified_forms);
                match config_dto.prepare(false) {
                    Ok(_) => {
                        let services = services.clone();
                        let translate = translate.clone();
                        let set_edit_mode = set_edit_mode.clone();
                        spawn_local(async move {
                            match services.config.save_config(config_dto).await{
                                Ok(()) => {
                                    services.toastr.success(translate.t("MESSAGES.SAVE.MAIN_CONFIG.SUCCESS"));
                                    set_edit_mode.set(false);
                                    let _cfg = services.config.get_server_config().await;
                                },
                                Err(err) => {
                                    services.toastr.error(translate.t("MESSAGES.SAVE.MAIN_CONFIG.FAIL"));
                                    services.toastr.error(err.to_string());
                                }
                            }
                        });
                    }
                    Err(err) => {
                        services.toastr.error(err.to_string());
                    }
                }
            } else {
                set_edit_mode.set(false);
            }
        })
    };

    let on_form_change = {
        let set_form_state = form_state.clone();
        Callback::from(move |form_data: ConfigForm| {
            let mut new_state = (*set_form_state).clone();

            match form_data {
                ConfigForm::Main(_, _) => new_state.main = Some(form_data),
                ConfigForm::Api(_, _) => new_state.api = Some(form_data),
                ConfigForm::Log(_, _) => new_state.log = Some(form_data),
                ConfigForm::Schedules(_, _) => new_state.schedules = Some(form_data),
                ConfigForm::Video(_, _) => new_state.video = Some(form_data),
                ConfigForm::Messaging(_, _) => new_state.messaging = Some(form_data),
                ConfigForm::WebUi(_, _) => new_state.web_ui = Some(form_data),
                ConfigForm::ReverseProxy(_, _) => new_state.reverse_proxy = Some(form_data),
                ConfigForm::HdHomerun(_, _) => new_state.hd_homerun = Some(form_data),
                ConfigForm::Proxy(_, _) => new_state.proxy = Some(form_data),
                ConfigForm::IpCheck(_, _) => new_state.ipcheck = Some(form_data),
            };
            set_form_state.set(new_state);
        })
    };


    let handle_update_content = {
        let services = services_ctx.clone();
        let translate = translate.clone();
        Callback::from(move |name: String| {
            let services = services.clone();
            let translate = translate.clone();
            wasm_bindgen_futures::spawn_local(async move {
                if name.as_str() == "update_geo_ip" {
                    match services.config.update_geoip().await {
                        Ok(_) => services.toastr.success(translate.t("MESSAGES.DOWNLOAD.GEOIP.SUCCESS")),
                        Err(_err) => services.toastr.error(translate.t("MESSAGES.DOWNLOAD.GEOIP.FAIL")),
                    }
                }
            });
        })
    };

    let context = ConfigViewContext {
        edit_mode: edit_mode.clone(),
        on_form_change: on_form_change.clone(),
    };

    let geo_ip_enabled = config_ctx.config.as_ref().is_some_and(|c| c.config.is_geoip_enabled());

    html! {
        <ContextProvider<ConfigViewContext> context={context}>
        <div class="tp__config-view">
            <div class="tp__config-view__header">
                <h1>{ translate.t(LABEL_CONFIG) } </h1>
                <div class="tp__config-view__header-tools">
                {html_if!(geo_ip_enabled, {
                    <TextButton class="tertiary" name="update_geo_ip"
                        icon="Refresh"
                        title={ translate.t(LABEL_UPDATE_GEOIP)}
                        onclick={handle_update_content.clone()}></TextButton>
                })}
                </div>
               <TextButton name="config_edit"
                    class={ if *edit_mode { "secondary" } else { "primary" }}
                    icon={ if *edit_mode { "Unlocked" } else { "Locked" }}
                    title={ if *edit_mode { translate.t(LABEL_EDIT) } else { translate.t(LABEL_VIEW) }}
                    onclick={handle_config_edit}></TextButton>

            </div>
            <div class="tp__config-view__body">
            <Card>
                 <TabSet tabs={tabs.clone()} active_tab={Some((*active_tab).to_string())}
                     on_tab_change={Some(handle_tab_change)}
                     class="tp__config-view__tabset"/>

                { html_if!(*edit_mode, {
                    <div class="tp__config-view__toolbar tp__form-page__toolbar">
                     <TextButton class="primary" name="save_config"
                        icon="Save"
                        title={ translate.t(LABEL_SAVE)}
                        onclick={handle_save_config}></TextButton>
                    </div>
                })}
            </Card>
            </div>
        </div>
        </ContextProvider<ConfigViewContext>>
    }
}
