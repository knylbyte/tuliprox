use crate::app::ConfigContext;
use yew::prelude::*;
use yew_i18n::use_translation;
use crate::{config_field, config_field_empty};

const LABEL_HOST: &str =  "LABEL.HOST";
const LABEL_PORT: &str =  "LABEL.PORT";
const LABEL_WEB_ROOT: &str =  "LABEL.WEB_ROOT";


#[function_component]
pub fn ApiConfigView() -> Html {
    let translate = use_translation();
    let config_ctx = use_context::<ConfigContext>().expect("Config context not found");

    html! {
        <div class="tp__api-config-view tp__config-view-page">
            <div class="tp__api-config-view__body tp__config-view-page__body">
                {
                    match config_ctx.config.as_ref() {
                        Some(config) => {
                            html!{
                            <>
                            { config_field!(config.config.api, translate.t(LABEL_HOST), host) }
                            { config_field!(config.config.api, translate.t(LABEL_PORT), port) }
                            { config_field!(config.config.api, translate.t(LABEL_WEB_ROOT), web_root) }
                            </>
                        }},
                        None => {
                            html!{
                            <>
                            { config_field_empty!(translate.t(LABEL_HOST)) }
                            { config_field_empty!(translate.t(LABEL_PORT)) }
                            { config_field_empty!(translate.t(LABEL_WEB_ROOT)) }
                            </>
                         }
                        }
                    }
                }
            </div>
        </div>
    }
}