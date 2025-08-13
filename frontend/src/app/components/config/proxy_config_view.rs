use yew::prelude::*;
use yew_i18n::use_translation;
use crate::app::context::ConfigContext;
use crate::{config_field, config_field_empty, config_field_optional, config_field_optional_hide};

#[function_component]
pub fn ProxyConfigView() -> Html {
    let translate = use_translation();
    let config_ctx = use_context::<ConfigContext>().expect("ConfigContext not found");

    let render_empty = || {
        html! {
          <div class="tp__proxy-config-config-view__body tp__config-view-page__body">
            { config_field_empty!(translate.t("LABEL.URL")) }
            { config_field_empty!(translate.t("LABEL.USERNAME")) }
            { config_field_empty!(translate.t("LABEL.PASSWORD")) }
          </div>
        }
    };

    html! {
        <div class="tp__proxy-config-view tp__config-view-page">
            {
                if let Some(config) = &config_ctx.config {
                    if let Some(proxy) = &config.config.proxy {
                        html! {
                         <>
                          <div class="tp__proxy-config-config-view__body tp__config-view-page__body">
                            { config_field!(proxy, translate.t("LABEL.URL"), url) }
                            { config_field_optional!(proxy, translate.t("LABEL.USERNAME"), username) }
                            { config_field_optional_hide!(proxy, translate.t("LABEL.PASSWORD"), password) }
                          </div>
                         </>
                        }
                    } else {
                        { render_empty() }
                    }
                } else {
                     { render_empty() }
                }
            }
        </div>
    }
}