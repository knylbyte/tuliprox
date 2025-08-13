use crate::app::components::{NoContent};
use crate::app::ConfigContext;
use yew::prelude::*;
use yew_i18n::use_translation;
use crate::{config_field, config_field_bool, config_field_optional};

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
                            { config_field_bool!(config.config, translate.t("LABEL.UPDATE_ON_BOOT"), update_on_boot) }
                            { config_field_bool!(config.config, translate.t("LABEL.CONFIG_HOT_RELOAD"), config_hot_reload) }
                            { config_field_bool!(config.config, translate.t("LABEL.USER_ACCESS_CONTROL"), user_access_control) }
                            { config_field!(config.config, translate.t("LABEL.THREADS"), threads) }
                            { config_field!(config.config, translate.t("LABEL.WORKING_DIR"), working_dir) }
                            { config_field_optional!(config.config, translate.t("LABEL.MAPPING_PATH"), mapping_path) }
                            { config_field_optional!(config.config, translate.t("LABEL.BACKUP_DIR"), backup_dir) }
                            { config_field_optional!(config.config, translate.t("LABEL.USER_CONFIG_DIR"), user_config_dir) }
                            { config_field_optional!(config.config, translate.t("LABEL.SLEEP_TIMER_MINS"), sleep_timer_mins) }
                            { config_field!(config.config, translate.t("LABEL.CONNECT_TIMEOUT_SECS"), connect_timeout_secs) }
                            { config_field_optional!(config.config, translate.t("LABEL.CUSTOM_STREAM_RESPONSE_PATH"), custom_stream_response_path) }
                            </>
                        }},
                        None => html! { <NoContent /> }
                    }
                }
            </div>
        </div>
    }
}