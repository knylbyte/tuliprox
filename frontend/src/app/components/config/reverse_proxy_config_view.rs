use yew::prelude::*;
use yew_i18n::use_translation;
use shared::model::{CacheConfigDto, RateLimitConfigDto, StreamConfigDto};
use crate::app::context::ConfigContext;
use crate::{config_field, config_field_bool, config_field_bool_empty, config_field_empty, config_field_optional};
use crate::app::components::Card;

const LABEL_CACHE: &str =  "LABEL.CACHE";
const LABEL_ENABLED: &str =  "LABEL.ENABLED";
const LABEL_SIZE: &str =  "LABEL.SIZE";
const LABEL_DIRECTORY: &str =  "LABEL.DIRECTORY";

const LABEL_STREAM: &str = "LABEL.STREAM";
const LABEL_RETRY: &str = "LABEL.RETRY";
const LABEL_THROTTLE: &str = "LABEL.THROTTLE";
const LABEL_GRACE_PERIOD_MILLIS: &str = "LABEL.GRACE_PERIOD_MILLIS";
const LABEL_GRACE_PERIOD_TIMEOUT_SECS: &str = "LABEL.GRACE_PERIOD_TIMEOUT_SECS";
const LABEL_FORCED_RETRY_INTERVAL_SECS: &str = "LABEL.FORCED_RETRY_INTERVAL_SECS";
const LABEL_THROTTLE_KBPS: &str = "LABEL.THROTTLE_KBPS";

const LABEL_RATE_LIMIT: &str =  "LABEL.RATE_LIMIT";
const LABEL_PERIOD_MILLIS: &str = "LABEL.PERIOD_MILLIS";
const LABEL_BURST_SIZE: &str = "LABEL.BURST_SIZE";

const LABEL_RESOURCE_REWRITE_DISABLED: &str = "LABEL.RESOURCE_REWRITE_DISABLED";
const LABEL_DISABLE_REFERER_HEADER: &str = "LABEL.DISABLE_REFERER_HEADER";


#[function_component]
pub fn ReverseProxyConfigView() -> Html {
    let translate = use_translation();
    let config_ctx = use_context::<ConfigContext>().expect("ConfigContext not found");

    let render_cache = |config: Option<&CacheConfigDto>| {
        match config {
            Some(entry) => html! {
            <Card class="tp__config-view__card">
                <h1>{translate.t(LABEL_CACHE)}</h1>
                { config_field_bool!(entry, translate.t(LABEL_ENABLED), enabled) }
                { config_field_optional!(entry, translate.t(LABEL_SIZE), size) }
                { config_field_optional!(entry, translate.t(LABEL_DIRECTORY), dir) }
            </Card>
            },
            None => html! {
            <Card class="tp__config-view__card">
                <h1>{translate.t(LABEL_CACHE)}</h1>
                { config_field_empty!(translate.t(LABEL_ENABLED)) }
                { config_field_empty!(translate.t(LABEL_SIZE)) }
                { config_field_empty!(translate.t(LABEL_DIRECTORY)) }
            </Card>
          },
        }
    };
    let render_stream = |config: Option<&StreamConfigDto>| {
        //pub buffer: Option<StreamBufferConfigDto>,
        match config {
            Some(entry) => html! {
            <Card class="tp__config-view__card">
                <h1>{translate.t(LABEL_STREAM)}</h1>
                { config_field_bool!(entry, translate.t(LABEL_RETRY), retry) }
                { config_field_optional!(entry, translate.t(LABEL_THROTTLE), throttle) }
                { config_field!(entry, translate.t(LABEL_GRACE_PERIOD_MILLIS), grace_period_millis) }
                { config_field!(entry, translate.t(LABEL_GRACE_PERIOD_TIMEOUT_SECS), grace_period_timeout_secs) }
                { config_field!(entry, translate.t(LABEL_FORCED_RETRY_INTERVAL_SECS), forced_retry_interval_secs) }
                { config_field!(entry, translate.t(LABEL_THROTTLE_KBPS), throttle_kbps) }
            </Card>
            },
            None => html! {
            <Card class="tp__config-view__card">
                <h1>{translate.t(LABEL_STREAM)}</h1>
                { config_field_empty!(translate.t(LABEL_RETRY)) }
                { config_field_empty!(translate.t(LABEL_THROTTLE)) }
                { config_field_empty!(translate.t(LABEL_GRACE_PERIOD_MILLIS)) }
                { config_field_empty!(translate.t(LABEL_GRACE_PERIOD_TIMEOUT_SECS)) }
                { config_field_empty!(translate.t(LABEL_FORCED_RETRY_INTERVAL_SECS)) }
                { config_field_empty!(translate.t(LABEL_THROTTLE_KBPS)) }
            </Card>
          },
        }
    };

    let render_rate_limit = |config: Option<&RateLimitConfigDto>| {
        match config {
            Some(entry) => html! {
            <Card class="tp__config-view__card">
                <h1>{translate.t(LABEL_RATE_LIMIT)}</h1>
                { config_field_bool!(entry, translate.t(LABEL_ENABLED), enabled) }
                { config_field!(entry, translate.t(LABEL_PERIOD_MILLIS), period_millis) }
                { config_field!(entry, translate.t(LABEL_BURST_SIZE), burst_size) }
            </Card>
            },
            None => html! {
            <Card class="tp__config-view__card">
                <h1>{translate.t(LABEL_RATE_LIMIT)}</h1>
                { config_field_empty!(translate.t(LABEL_ENABLED)) }
                { config_field_empty!(translate.t(LABEL_PERIOD_MILLIS)) }
                { config_field_empty!(translate.t(LABEL_BURST_SIZE)) }
            </Card>
          },
        }
    };

    let render_empty = || {
        html! {
          <>
            <div class="tp__reverse-proxy-config-view__header tp__config-view-page__header">
              { config_field_bool_empty!(translate.t(LABEL_RESOURCE_REWRITE_DISABLED)) }
              { config_field_bool_empty!(translate.t(LABEL_DISABLE_REFERER_HEADER)) }
            </div>
            <div class="tp__reverse-proxy-config-view__body tp__config-view-page__body">
             {render_cache(None)}
             {render_rate_limit(None)}
             {render_stream(None)}
            </div>
          </>
        }
    };

    html! {
        <div class="tp__reverse-proxy-config-view tp__config-view-page">
            {
                if let Some(config) = &config_ctx.config {
                    if let Some(reverse_proxy) = &config.config.reverse_proxy {
                        html! {
                        <>
                          <div class="tp__reverse-proxy-config-view__header tp__config-view-page__header">
                            { config_field_bool!(reverse_proxy, translate.t(LABEL_RESOURCE_REWRITE_DISABLED), resource_rewrite_disabled) }
                            { config_field_bool!(reverse_proxy, translate.t(LABEL_DISABLE_REFERER_HEADER), disable_referer_header) }
                          </div>
                          <div class="tp__reverse-proxy-config-view__body tp__config-view-page__body">
                            { render_cache(reverse_proxy.cache.as_ref()) }
                            { render_rate_limit(reverse_proxy.rate_limit.as_ref()) }
                            { render_stream(reverse_proxy.stream.as_ref()) }
                          </div>
                        </>
                        }
                    } else {
                       {render_empty()}
                    }
                } else {
                   {render_empty()}
                }
            }
        </div>
    }
}