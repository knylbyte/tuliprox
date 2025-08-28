use crate::app::ConfigContext;
use yew::prelude::*;
use yew_i18n::use_translation;
use crate::{config_field, config_field_bool, config_field_bool_empty, config_field_empty, config_field_optional};

const LABEL_UPDATE_ON_BOOT: &str = "LABEL.UPDATE_ON_BOOT";
const LABEL_CONFIG_HOT_RELOAD: &str = "LABEL.CONFIG_HOT_RELOAD";
const LABEL_USER_ACCESS_CONTROL: &str = "LABEL.USER_ACCESS_CONTROL";
const LABEL_THREADS: &str = "LABEL.THREADS";
const LABEL_WORKING_DIR: &str = "LABEL.WORKING_DIR";
const LABEL_MAPPING_PATH: &str = "LABEL.MAPPING_PATH";
const LABEL_BACKUP_DIR: &str = "LABEL.BACKUP_DIR";
const LABEL_USER_CONFIG_DIR: &str = "LABEL.USER_CONFIG_DIR";
const LABEL_SLEEP_TIMER_MINS: &str = "LABEL.SLEEP_TIMER_MINS";
const LABEL_CONNECT_TIMEOUT_SECS: &str = "LABEL.CONNECT_TIMEOUT_SECS";
const LABEL_CUSTOM_STREAM_RESPONSE_PATH: &str = "LABEL.CUSTOM_STREAM_RESPONSE_PATH";

#[function_component]
pub fn MainConfigView() -> Html {
    let translate = use_translation();
    let config_ctx = use_context::<ConfigContext>().expect("Config context not found");

    html! {
        <div class="tp__main-config-view tp__config-view-page">
            <div class="tp__main-config-view__body tp__config-view-page__body">
                {
                    match config_ctx.config.as_ref() {
                        Some(config) => {
                            html!{
                            <>
                            { config_field_bool!(config.config, translate.t(LABEL_UPDATE_ON_BOOT), update_on_boot) }
                            { config_field_bool!(config.config, translate.t(LABEL_CONFIG_HOT_RELOAD), config_hot_reload) }
                            { config_field_bool!(config.config, translate.t(LABEL_USER_ACCESS_CONTROL), user_access_control) }
                            { config_field!(config.config, translate.t(LABEL_THREADS), threads) }
                            { config_field!(config.config, translate.t(LABEL_WORKING_DIR), working_dir) }
                            { config_field_optional!(config.config, translate.t(LABEL_MAPPING_PATH), mapping_path) }
                            { config_field_optional!(config.config, translate.t(LABEL_BACKUP_DIR), backup_dir) }
                            { config_field_optional!(config.config, translate.t(LABEL_USER_CONFIG_DIR), user_config_dir) }
                            { config_field_optional!(config.config, translate.t(LABEL_SLEEP_TIMER_MINS), sleep_timer_mins) }
                            { config_field!(config.config, translate.t(LABEL_CONNECT_TIMEOUT_SECS), connect_timeout_secs) }
                            { config_field_optional!(config.config, translate.t(LABEL_CUSTOM_STREAM_RESPONSE_PATH), custom_stream_response_path) }
                            </>
                        }},
                        None => {
                            html!{
                            <>
                            { config_field_bool_empty!(translate.t(LABEL_UPDATE_ON_BOOT)) }
                            { config_field_bool_empty!(translate.t(LABEL_CONFIG_HOT_RELOAD)) }
                            { config_field_bool_empty!(translate.t(LABEL_USER_ACCESS_CONTROL)) }
                            { config_field_empty!(translate.t(LABEL_THREADS)) }
                            { config_field_empty!(translate.t(LABEL_WORKING_DIR)) }
                            { config_field_empty!(translate.t(LABEL_MAPPING_PATH)) }
                            { config_field_empty!(translate.t(LABEL_BACKUP_DIR)) }
                            { config_field_empty!(translate.t(LABEL_USER_CONFIG_DIR)) }
                            { config_field_empty!(translate.t(LABEL_SLEEP_TIMER_MINS)) }
                            { config_field_empty!(translate.t(LABEL_CONNECT_TIMEOUT_SECS)) }
                            { config_field_empty!(translate.t(LABEL_CUSTOM_STREAM_RESPONSE_PATH)) }
                            </>
                       }
                    }
                  }
                }
            </div>
        </div>
    }
}