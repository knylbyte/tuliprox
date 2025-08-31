use crate::app::components::config::config_page::{ConfigForm, ConfigPage};
use crate::app::components::config::config_view_context::ConfigViewContext;
use crate::app::components::config::{ApiConfigView, HdHomerunConfigView, IpCheckConfigView, MainConfigView, MessagingConfigView, ProxyConfigView, ReverseProxyConfigView, SchedulesConfigView, VideoConfigView, WebUiConfigView};
use crate::app::components::{Card, TabItem, TabSet, TextButton};
use crate::html_if;
use std::str::FromStr;
use log::warn;
use yew::prelude::*;
use yew_i18n::use_translation;

macro_rules! collect_modified {
    ($forms:expr, [ $($field:ident),+ $(,)? ]) => {{
        let mut modified = Vec::new();
        $(
            if $forms.$field.as_ref().is_some_and(|s| s.is_modified()) {
                modified.push($forms.$field.clone());
            }
        )+
        modified
    }};
}

fn config_form_to_config_page(form: &ConfigForm) -> ConfigPage {
    match form {
        ConfigForm::Main(_, _) => ConfigPage::Main,
        ConfigForm::Api(_, _) => ConfigPage::Api,
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
        let edit_mode = edit_mode.clone();

        use_memo((form_state, edit_mode), move |(forms, edit)| {
            let modified = collect_modified!(forms, [
            main, api, schedules, video, messaging, web_ui,
            reverse_proxy, hd_homerun, proxy, ipcheck
        ])
                .iter()
                .filter_map(|maybe_form| maybe_form.as_ref().map(config_form_to_config_page))
                .collect::<Vec<ConfigPage>>();

            let tab_configs = vec![
                (ConfigPage::Main, "LABEL.MAIN", html! { <MainConfigView/> }, "MainConfig"),
                (ConfigPage::Api, "LABEL.API", html! { <ApiConfigView/> }, "ApiConfig"),
                (ConfigPage::Schedules, "LABEL.SCHEDULES", html! { <SchedulesConfigView/> }, "SchedulesConfig"),
                (ConfigPage::Messaging, "LABEL.MESSAGING", html! { <MessagingConfigView/> }, "MessagingConfig"),
                (ConfigPage::WebUi, "LABEL.WEB_UI", html! { <WebUiConfigView/> }, "WebUiConfig"),
                (ConfigPage::ReverseProxy, "LABEL.REVERSE_PROXY", html! { <ReverseProxyConfigView/> }, "ReverseProxyConfig"),
                (ConfigPage::HdHomerun, "LABEL.HDHOMERUN_CONFIG", html! { <HdHomerunConfigView/> }, "HdHomerunConfig"),
                (ConfigPage::Proxy, "LABEL.PROXY", html! { <ProxyConfigView/> }, "ProxyConfig"),
                (ConfigPage::IpCheck, "LABEL.IP_CHECK", html! { <IpCheckConfigView/> }, "IpCheckConfig"),
                (ConfigPage::Video, "LABEL.VIDEO", html! { <VideoConfigView/> }, "VideoConfig"),
            ];

            let editing = **edit;
            tab_configs.into_iter().map(|(page, label, children, icon)| {
                let is_modified = editing && modified.contains(&page);
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
        let get_form_state = form_state.clone();
        Callback::from(move |_| {
            let forms = &*get_form_state;
            let modified = collect_modified!(forms, [
                    main, api, schedules, video, messaging, web_ui,
                    reverse_proxy, hd_homerun, proxy, ipcheck
                ]);
            warn!("Modified: {modified:?}");
        })
    };

    let on_form_change = {
        let set_form_state = form_state.clone();
        Callback::from(move |form_data: ConfigForm| {
            let mut new_state = (*set_form_state).clone();

            match form_data {
                ConfigForm::Main(_, _) => new_state.main = Some(form_data),
                ConfigForm::Api(_, _) => new_state.api = Some(form_data),
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


    let context = ConfigViewContext {
        edit_mode: edit_mode.clone(),
        on_form_change: on_form_change.clone(),
    };

    html! {
        <ContextProvider<ConfigViewContext> context={context}>
        <div class="tp__config-view">
            <div class="tp__config-view__header">
                <h1>{ translate.t("LABEL.CONFIG") } </h1>
                <TextButton name="config_edit"
                    class={ if *edit_mode { "secondary" } else { "primary" }}
                    icon={ if *edit_mode { "Unlocked" } else { "Locked" }}
                    title={ if *edit_mode { translate.t("LABEL.EDIT") } else { translate.t("LABEL.VIEW") }}
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
                        title={ translate.t("LABEL.SAVE")}
                        onclick={handle_save_config}></TextButton>
                    </div>
                })}
            </Card>
            </div>
        </div>
        </ContextProvider<ConfigViewContext>>
    }
}
