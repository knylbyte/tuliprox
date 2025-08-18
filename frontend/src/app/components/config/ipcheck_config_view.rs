use yew::prelude::*;
use yew_i18n::use_translation;
use crate::app::context::ConfigContext;
use crate::{config_field_empty, config_field_optional};

const LABEL_URL: &str =  "LABEL.URL";
const LABEL_URL_IPV4: &str =  "LABEL.URL_IPV4";
const LABEL_URL_IPV6: &str =  "LABEL.URL_IPV6";
const LABEL_PATTERN_IPV4: &str =  "LABEL.PATTERN_IPV4";
const LABEL_PATTERN_IPV6: &str =  "LABEL.PATTERN_IPV6";


#[function_component]
pub fn IpCheckConfigView() -> Html {
    let translate = use_translation();
    let config_ctx = use_context::<ConfigContext>().expect("ConfigContext not found");

    let render_empty = || {
            html! {
              <>
                { config_field_empty!(translate.t(LABEL_URL)) }
                { config_field_empty!(translate.t(LABEL_URL_IPV4)) }
                { config_field_empty!(translate.t(LABEL_URL_IPV6)) }
                { config_field_empty!(translate.t(LABEL_PATTERN_IPV4)) }
                { config_field_empty!(translate.t(LABEL_PATTERN_IPV6)) }
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
                            { config_field_optional!(ipcheck, translate.t(LABEL_URL),  url) }
                            { config_field_optional!(ipcheck, translate.t(LABEL_URL_IPV4),  url_ipv4) }
                            { config_field_optional!(ipcheck, translate.t(LABEL_URL_IPV6),  url_ipv6) }
                            { config_field_optional!(ipcheck, translate.t(LABEL_PATTERN_IPV4),  pattern_ipv4) }
                            { config_field_optional!(ipcheck, translate.t(LABEL_PATTERN_IPV6),  pattern_ipv6) }
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