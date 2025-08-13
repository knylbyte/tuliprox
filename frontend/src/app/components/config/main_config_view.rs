use crate::app::components::{NoContent, ToggleSwitch};
use crate::app::ConfigContext;
use yew::prelude::*;
use yew_i18n::use_translation;

macro_rules! config_field_optional {
    ($config:expr, $translate:expr, $label:expr, $field:ident) => {
        html! {
            <div class="tp__config-field tp__config-field__text">
                <label>{$translate.t($label)}</label>
                <span>{
                    match &$config.config.$field {
                        Some(value) => value.to_string(),
                        None => String::new(),
                    }
                }</span>
            </div>
        }
    };
}

macro_rules! config_field_bool {
    ($config:expr, $translate:expr, $label:expr, $field:ident) => {
        html! {
            <div class="tp__config-field tp__config-field__bool">
                <label>{$translate.t($label)}</label>
                <ToggleSwitch value={$config.config.$field} readonly={true} />
            </div>
        }
    };
}

macro_rules! config_field {
    ($config:expr, $translate:expr, $label:expr, $field:ident) => {
        html! {
            <div class="tp__config-field tp__config-field__text">
                <label>{$translate.t($label)}</label>
                <span>{$config.config.$field.to_string()}</span>
            </div>
        }
    };
}


#[function_component]
pub fn MainConfigView() -> Html {
    let translate = use_translation();
    let config_ctx = use_context::<ConfigContext>().expect("Config context not found");

    html! {
        <div class="tp__main-config-view">
            <div class="tp__main-config-view__header">
                <h1>{ translate.t("LABEL.MAIN_CONFIG") }</h1>
            </div>
            <div class="tp__main-config-view__body">
                {
                    match config_ctx.config.as_ref() {
                        Some(config) => {
                            html!{
                            <>
                            { config_field_bool!(config, translate, "LABEL.UPDATE_ON_BOOT", update_on_boot) }
                            { config_field_bool!(config, translate, "LABEL.CONFIG_HOT_RELOAD", config_hot_reload) }
                            { config_field_bool!(config, translate, "LABEL.USER_ACCESS_CONTROL", user_access_control) }
                            { config_field!(config, translate, "LABEL.THREADS", threads) }
                            { config_field!(config, translate, "LABEL.WORKING_DIR", working_dir) }
                            { config_field_optional!(config, translate, "LABEL.MAPPING_PATH", mapping_path) }
                            { config_field_optional!(config, translate, "LABEL.BACKUP_DIR", backup_dir) }
                            { config_field_optional!(config, translate, "LABEL.USER_CONFIG_DIR", user_config_dir) }
                            { config_field_optional!(config, translate, "LABEL.SLEEP_TIMER_MINS", sleep_timer_mins) }
                            { config_field!(config, translate, "LABEL.CONNECT_TIMEOUT_SECS", connect_timeout_secs) }
                            { config_field_optional!(config, translate, "LABEL.CUSTOM_STREAM_RESPONSE_PATH", custom_stream_response_path) }
                            </>
                        }},
                        None => html! { <NoContent /> }
                    }
                }
            </div>
        </div>
    }
}