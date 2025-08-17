use yew::prelude::*;
use yew_i18n::use_translation;
use crate::app::context::ConfigContext;
use crate::{config_field_empty, config_field_optional};

#[function_component]
pub fn IpCheckConfigView() -> Html {
    let translate = use_translation();
    let config_ctx = use_context::<ConfigContext>().expect("ConfigContext not found");

    let render_empty = || html! {
            html! {
              <>
                { config_field_empty!(translate.t("LABEL.URL")) }
                { config_field_empty!(translate.t("LABEL.URL_IPV4")) }
                { config_field_empty!(translate.t("LABEL.URL_IPV6")) }
                { config_field_empty!(translate.t("LABEL.PATTERN_IPV4")) }
                { config_field_empty!(translate.t("LABEL.PATTERN_IPV6")) }
              </>
            }
    };

    html! {
        <div class="tp__ipcheck-config-view tp__config-view-page">
            <div class="tp__ipcheck-config-view__body tp__config-view-page__body">
            {
                if let Some(config) = &config_ctx.config {
                    if let Some(ipcheck) = &config.config.ipcheck {
                        html! {
                          <>
                            { config_field_optional!(ipcheck, translate.t("LABEL.URL"),  url) }
                            { config_field_optional!(ipcheck, translate.t("LABEL.URL_IPV4"),  url_ipv4) }
                            { config_field_optional!(ipcheck, translate.t("LABEL.URL_IPV6"),  url_ipv6) }
                            { config_field_optional!(ipcheck, translate.t("LABEL.PATTERN_IPV4"),  pattern_ipv4) }
                            { config_field_optional!(ipcheck, translate.t("LABEL.PATTERN_IPV6"),  pattern_ipv6) }
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
        </div>
    }
}