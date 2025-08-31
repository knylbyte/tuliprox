use yew::prelude::*;
use yew_i18n::use_translation;
use shared::model::{
    CacheConfigDto, RateLimitConfigDto, StreamConfigDto, ReverseProxyConfigDto,
};
use crate::app::context::ConfigContext;
use crate::app::components::config::config_view_context::ConfigViewContext;
use crate::app::components::config::config_page::ConfigForm;
use crate::app::components::config::macros::HasFormData;
use crate::app::components::Card;
use crate::{config_field, config_field_bool, config_field_bool_empty, config_field_empty, config_field_optional,
            edit_field_bool, edit_field_number, edit_field_number_u64, edit_field_text_option, generate_form_reducer};

const LABEL_CACHE: &str = "LABEL.CACHE";
const LABEL_ENABLED: &str = "LABEL.ENABLED";
const LABEL_SIZE: &str = "LABEL.SIZE";
const LABEL_DIRECTORY: &str = "LABEL.DIRECTORY";

const LABEL_STREAM: &str = "LABEL.STREAM";
const LABEL_RETRY: &str = "LABEL.RETRY";
const LABEL_THROTTLE: &str = "LABEL.THROTTLE";
const LABEL_GRACE_PERIOD_MILLIS: &str = "LABEL.GRACE_PERIOD_MILLIS";
const LABEL_GRACE_PERIOD_TIMEOUT_SECS: &str = "LABEL.GRACE_PERIOD_TIMEOUT_SECS";
const LABEL_FORCED_RETRY_INTERVAL_SECS: &str = "LABEL.FORCED_RETRY_INTERVAL_SECS";
const LABEL_THROTTLE_KBPS: &str = "LABEL.THROTTLE_KBPS";

const LABEL_RATE_LIMIT: &str = "LABEL.RATE_LIMIT";
const LABEL_PERIOD_MILLIS: &str = "LABEL.PERIOD_MILLIS";
const LABEL_BURST_SIZE: &str = "LABEL.BURST_SIZE";

const LABEL_RESOURCE_REWRITE_DISABLED: &str = "LABEL.RESOURCE_REWRITE_DISABLED";
const LABEL_DISABLE_REFERER_HEADER: &str = "LABEL.DISABLE_REFERER_HEADER";

generate_form_reducer!(
    state: CacheConfigFormState { form: CacheConfigDto },
    action_name: CacheConfigFormAction,
    fields {
        Enabled => enabled: bool,
        Size => size: Option<String>,
        Dir => dir: Option<String>,
    }
);

generate_form_reducer!(
    state: RateLimitConfigFormState { form: RateLimitConfigDto },
    action_name: RateLimitConfigFormAction,
    fields {
        Enabled => enabled: bool,
        PeriodMillis => period_millis: u64,
        BurstSize => burst_size: u32,
    }
);

generate_form_reducer!(
    state: StreamConfigFormState { form: StreamConfigDto },
    action_name: StreamConfigFormAction,
    fields {
        Retry => retry: bool,
        Throttle => throttle: Option<String>,
        GracePeriodMillis => grace_period_millis: u64,
        GracePeriodTimeoutSecs => grace_period_timeout_secs: u64,
        ForcedRetryIntervalSecs => forced_retry_interval_secs: u32,
        ThrottleKbps => throttle_kbps: u64,
    }
);

generate_form_reducer!(
    state: ReverseProxyConfigFormState { form: ReverseProxyConfigDto },
    action_name: ReverseProxyConfigFormAction,
    fields {
        ResourceRewriteDisabled => resource_rewrite_disabled: bool,
        DisableRefererHeader => disable_referer_header: bool,
    }
);

#[function_component]
pub fn ReverseProxyConfigView() -> Html {
    let translate = use_translation();
    let config_ctx = use_context::<ConfigContext>().expect("ConfigContext not found");
    let config_view_ctx = use_context::<ConfigViewContext>().expect("ConfigViewContext not found");

    let reverse_proxy_state: UseReducerHandle<ReverseProxyConfigFormState> = use_reducer(|| {
        ReverseProxyConfigFormState { form: ReverseProxyConfigDto::default(), modified: false }
    });
    let cache_state: UseReducerHandle<CacheConfigFormState> = use_reducer(|| {
        CacheConfigFormState { form: CacheConfigDto::default(), modified: false }
    });
    let rate_limit_state: UseReducerHandle<RateLimitConfigFormState> = use_reducer(|| {
        RateLimitConfigFormState { form: RateLimitConfigDto::default(), modified: false }
    });
    let stream_state: UseReducerHandle<StreamConfigFormState> = use_reducer(|| {
        StreamConfigFormState { form: StreamConfigDto::default(), modified: false }
    });

    {
        let on_form_change = config_view_ctx.on_form_change.clone();
        let reverse_proxy_state = reverse_proxy_state.clone();
        let cache_state = cache_state.clone();
        let rate_limit_state = rate_limit_state.clone();
        let stream_state = stream_state.clone();

        use_effect_with(
            (reverse_proxy_state, cache_state, rate_limit_state, stream_state),
            move |(rp, cache, rl, stream)| {
                let mut form = rp.form.clone();
                form.cache = Some(cache.form.clone());
                form.rate_limit = Some(rl.form.clone());
                form.stream = Some(stream.form.clone());

                let modified = rp.modified || cache.modified || rl.modified || stream.modified;
                on_form_change.emit(ConfigForm::ReverseProxy(modified, form));
            },
        );
    }

    {
        let reverse_proxy_state = reverse_proxy_state.clone();
        let cache_state = cache_state.clone();
        let rate_limit_state = rate_limit_state.clone();
        let stream_state = stream_state.clone();

        let reverse_proxy_cfg = config_ctx.config.as_ref().and_then(|c| c.config.reverse_proxy.clone());
        use_effect_with((reverse_proxy_cfg, config_view_ctx.edit_mode.clone()), move |(cfg, _mode)| {
            if let Some(rp) = cfg {
                reverse_proxy_state.dispatch(ReverseProxyConfigFormAction::SetAll((*rp).clone()));
                cache_state.dispatch(CacheConfigFormAction::SetAll(rp.cache.as_ref().map_or_else(CacheConfigDto::default, |c| c.clone())));
                rate_limit_state.dispatch(RateLimitConfigFormAction::SetAll(rp.rate_limit.as_ref().map_or_else(RateLimitConfigDto::default, |rl| rl.clone())));
                stream_state.dispatch(StreamConfigFormAction::SetAll(rp.stream.as_ref().map_or_else(StreamConfigDto::default, |s| s.clone())));
            } else {
                reverse_proxy_state.dispatch(ReverseProxyConfigFormAction::SetAll(ReverseProxyConfigDto::default()));
                cache_state.dispatch(CacheConfigFormAction::SetAll(CacheConfigDto::default()));
                rate_limit_state.dispatch(RateLimitConfigFormAction::SetAll(RateLimitConfigDto::default()));
                stream_state.dispatch(StreamConfigFormAction::SetAll(StreamConfigDto::default()));
            }
            || ()
        });
    }

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

    let render_view_mode = || {
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
                render_empty()
            }
        } else {
            render_empty()
        }
    };

    let render_edit_mode = || html! {
        <>
          <div class="tp__reverse-proxy-config-view__header tp__config-view-page__header">
            { edit_field_bool!(reverse_proxy_state, translate.t(LABEL_RESOURCE_REWRITE_DISABLED), resource_rewrite_disabled, ReverseProxyConfigFormAction::ResourceRewriteDisabled) }
            { edit_field_bool!(reverse_proxy_state, translate.t(LABEL_DISABLE_REFERER_HEADER), disable_referer_header, ReverseProxyConfigFormAction::DisableRefererHeader) }
          </div>
          <div class="tp__reverse-proxy-config-view__body tp__config-view-page__body">
            <Card class="tp__config-view__card">
                <h1>{translate.t(LABEL_CACHE)}</h1>
                { edit_field_bool!(cache_state, translate.t(LABEL_ENABLED), enabled, CacheConfigFormAction::Enabled) }
                { edit_field_text_option!(cache_state, translate.t(LABEL_SIZE), size, CacheConfigFormAction::Size) }
                { edit_field_text_option!(cache_state, translate.t(LABEL_DIRECTORY), dir, CacheConfigFormAction::Dir) }
            </Card>
            <Card class="tp__config-view__card">
                <h1>{translate.t(LABEL_RATE_LIMIT)}</h1>
                { edit_field_bool!(rate_limit_state, translate.t(LABEL_ENABLED), enabled, RateLimitConfigFormAction::Enabled) }
                { edit_field_number_u64!(rate_limit_state, translate.t(LABEL_PERIOD_MILLIS), period_millis, RateLimitConfigFormAction::PeriodMillis) }
                { edit_field_number!(rate_limit_state, translate.t(LABEL_BURST_SIZE), burst_size, RateLimitConfigFormAction::BurstSize) }
            </Card>
            <Card class="tp__config-view__card">
                <h1>{translate.t(LABEL_STREAM)}</h1>
                { edit_field_bool!(stream_state, translate.t(LABEL_RETRY), retry, StreamConfigFormAction::Retry) }
                { edit_field_text_option!(stream_state, translate.t(LABEL_THROTTLE), throttle, StreamConfigFormAction::Throttle) }
                { edit_field_number_u64!(stream_state, translate.t(LABEL_GRACE_PERIOD_MILLIS), grace_period_millis, StreamConfigFormAction::GracePeriodMillis) }
                { edit_field_number_u64!(stream_state, translate.t(LABEL_GRACE_PERIOD_TIMEOUT_SECS), grace_period_timeout_secs, StreamConfigFormAction::GracePeriodTimeoutSecs) }
                { edit_field_number!(stream_state, translate.t(LABEL_FORCED_RETRY_INTERVAL_SECS), forced_retry_interval_secs, StreamConfigFormAction::ForcedRetryIntervalSecs) }
                { edit_field_number_u64!(stream_state, translate.t(LABEL_THROTTLE_KBPS), throttle_kbps, StreamConfigFormAction::ThrottleKbps) }
            </Card>
          </div>
        </>
    };

    html! {
        <div class="tp__reverse-proxy-config-view tp__config-view-page">
            {
                if *config_view_ctx.edit_mode {
                    render_edit_mode()
                } else {
                    render_view_mode()
                }
            }
        </div>
    }
}
