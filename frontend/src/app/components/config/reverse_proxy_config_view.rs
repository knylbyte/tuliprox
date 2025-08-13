use yew::prelude::*;
use yew_i18n::use_translation;
use shared::model::{CacheConfigDto, RateLimitConfigDto, StreamConfigDto};
use crate::app::context::ConfigContext;
use crate::{config_field, config_field_bool, config_field_bool_empty, config_field_empty, config_field_optional};
use crate::app::components::Card;

#[function_component]
pub fn ReverseProxyConfigView() -> Html {
    let translate = use_translation();
    let config_ctx = use_context::<ConfigContext>().expect("ConfigContext not found");

    let render_cache = |config: Option<&CacheConfigDto>| {
        match config {
            Some(entry) => html! {
            <Card class="tp__config-view__card">
                <h1>{translate.t("LABEL.CACHE")}</h1>
                { config_field_bool!(entry, translate.t("LABEL.ENABLED"), enabled) }
                { config_field_optional!(entry, translate.t("LABEL.SIZE"), size) }
                { config_field_optional!(entry, translate.t("LABEL.DIRECTORY"), dir) }
            </Card>
            },
            None => html! {
            <Card class="tp__config-view__card">
                <h1>{translate.t("LABEL.CACHE")}</h1>
                { config_field_empty!(translate.t("LABEL.ENABLED")) }
                { config_field_empty!(translate.t("LABEL.SIZE")) }
                { config_field_empty!(translate.t("LABEL.DIRECTORY")) }
            </Card>
          },
        }
    };
    let render_stream = |config: Option<&StreamConfigDto>| {
        //pub buffer: Option<StreamBufferConfigDto>,
        match config {
            Some(entry) => html! {
            <Card class="tp__config-view__card">
                <h1>{translate.t("LABEL.STREAM")}</h1>
                { config_field_bool!(entry, translate.t("LABEL.RETRY"), retry) }
                { config_field_optional!(entry, translate.t("LABEL.THROTTLE"), throttle) }
                { config_field!(entry, translate.t("LABEL.GRACE_PERIOD_MILLIS"), grace_period_millis) }
                { config_field!(entry, translate.t("LABEL.GRACE_PERIOD_TIMEOUT_SECS"), grace_period_timeout_secs) }
                { config_field!(entry, translate.t("LABEL.FORCED_RETRY_INTERVAL_SECS"), forced_retry_interval_secs) }
                { config_field!(entry, translate.t("LABEL.THROTTLE_KBPS"), throttle_kbps) }
            </Card>
            },
            None => html! {
            <Card class="tp__config-view__card">
                <h1>{translate.t("LABEL.STREAM")}</h1>
                { config_field_empty!(translate.t("LABEL.RETRY")) }
                { config_field_empty!(translate.t("LABEL.THROTTLE")) }
                { config_field_empty!(translate.t("LABEL.GRACE_PERIOD_MILLIS")) }
                { config_field_empty!(translate.t("LABEL.GRACE_PERIOD_TIMEOUT_SECS")) }
                { config_field_empty!(translate.t("LABEL.FORCED_RETRY_INTERVAL_SECS")) }
                { config_field_empty!(translate.t("LABEL.THROTTLE_KBPS")) }
            </Card>
          },
        }
    };

    let render_rate_limit = |config: Option<&RateLimitConfigDto>| {
        match config {
            Some(entry) => html! {
            <Card class="tp__config-view__card">
                <h1>{translate.t("LABEL.RATE_LIMIT")}</h1>
                { config_field_bool!(entry, translate.t("LABEL.ENABLED"), enabled) }
                { config_field!(entry, translate.t("LABEL.PERIOD_MILLIS"), period_millis) }
                { config_field!(entry, translate.t("LABEL.BURST_SIZE"), burst_size) }
            </Card>
            },
            None => html! {
            <Card class="tp__config-view__card">
                <h1>{translate.t("LABEL.RATE_LIMIT")}</h1>
                { config_field_empty!(translate.t("LABEL.ENABLED")) }
                { config_field_empty!(translate.t("LABEL.PERIOD_MILLIS")) }
                { config_field_empty!(translate.t("LABEL.BURST_SIZE")) }
            </Card>
          },
        }
    };

    let render_empty = || {
        html! {
          <>
            <div class="tp__reverse-proxy-config-view__header tp__config-view-page__header">
              { config_field_bool_empty!(translate.t("LABEL.RESOURCE_REWRITE_DISABLED")) }
              { config_field_bool_empty!(translate.t("LABEL.DISABLE_REFERER_HEADER")) }
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
                            { config_field_bool!(reverse_proxy, translate.t("LABEL.RESOURCE_REWRITE_DISABLED"), resource_rewrite_disabled) }
                            { config_field_bool!(reverse_proxy, translate.t("LABEL.DISABLE_REFERER_HEADER"), disable_referer_header) }
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