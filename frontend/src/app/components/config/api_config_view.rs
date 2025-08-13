use crate::app::components::{NoContent};
use crate::app::ConfigContext;
use yew::prelude::*;
use yew_i18n::use_translation;
use crate::{config_field};

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
                            { config_field!(config.config.api, translate.t("LABEL.HOST"), host) }
                            { config_field!(config.config.api, translate.t("LABEL.PORT"), port) }
                            { config_field!(config.config.api, translate.t("LABEL.WEB_ROOT"), web_root) }
                            </>
                        }},
                        None => html! { <NoContent /> }
                    }
                }
            </div>
        </div>
    }
}