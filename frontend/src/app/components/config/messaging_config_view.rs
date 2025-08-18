use crate::app::components::{Card, Chip};
use crate::app::context::ConfigContext;
use crate::{config_field, config_field_child, config_field_empty, config_field_hide, config_field_optional};
use shared::model::{PushoverMessagingConfigDto, RestMessagingConfigDto, TelegramMessagingConfigDto};
use yew::prelude::*;
use yew_i18n::use_translation;

const LABEL_NOTIFY_ON: &str = "LABEL.NOTIFY_ON";
const LABEL_TELEGRAM: &str = "LABEL.TELEGRAM";
const LABEL_PUSHOVER: &str = "LABEL.PUSHOVER";
const LABEL_REST: &str = "LABEL.REST";
const LABEL_BOT_TOKEN: &str = "LABEL.BOT_TOKEN";
const LABEL_CHAT_IDS: &str = "LABEL.CHAT_IDS";
const LABEL_URL: &str = "LABEL.URL";
const LABEL_TOKEN: &str = "LABEL.TOKEN";
const LABEL_USER: &str = "LABEL.USER";


#[function_component]
pub fn MessagingConfigView() -> Html {
    let translate = use_translation();
    let config_ctx = use_context::<ConfigContext>().expect("ConfigContext not found");

    let render_telegram = |telegram: Option<&TelegramMessagingConfigDto>| {
        match telegram {
            Some(entry) => html! {
            <Card class="tp__config-view__card">
                <h1>{translate.t("LABEL.TELEGRAM")}</h1>
                { config_field!(entry, translate.t(LABEL_BOT_TOKEN), bot_token) }
                { config_field_child!(translate.t(LABEL_CHAT_IDS), {
                    html! {
                        <div class="tp__config-view__tags">
                            {
                                if entry.chat_ids.is_empty() {
                                    html! {}
                                } else {
                                    html! { for entry.chat_ids.iter().map(|t| html! { <Chip label={t.clone()} /> }) }
                                }
                            }
                        </div>
                    }
                })}
            </Card>
          },
            None => html! {
            <Card class="tp__config-view__card">
               <h1>{translate.t(LABEL_TELEGRAM)}</h1>
               { config_field_empty!(translate.t(LABEL_BOT_TOKEN)) }
               { config_field_empty!(translate.t(LABEL_CHAT_IDS)) }
            </Card>
          },
        }
    };

    let render_rest = |rest: Option<&RestMessagingConfigDto>| {
        match rest {
            Some(entry) => html! {
            <Card class="tp__config-view__card">
                <h1>{translate.t(LABEL_REST)}</h1>
                { config_field!(entry, translate.t(LABEL_URL), url) }
            </Card>
          },
            None => html! {
            <Card class="tp__config-view__card">
                <h1>{translate.t(LABEL_REST)}</h1>
                { config_field_empty!(translate.t(LABEL_URL)) }
            </Card>
          },
        }
    };

    let render_pushover = |pushover: Option<&PushoverMessagingConfigDto>| {
        match pushover {
            Some(entry) => html! {
            <Card class="tp__config-view__card">
                <h1>{translate.t(LABEL_PUSHOVER)}</h1>
                { config_field_optional!(entry, translate.t(LABEL_URL), url) }
                { config_field_hide!(entry, translate.t(LABEL_TOKEN), token) }
                { config_field!(entry, translate.t(LABEL_USER), user) }
            </Card>
            },
            None => html! {
            <Card class="tp__config-view__card">
                <h1>{translate.t(LABEL_PUSHOVER)}</h1>
                { config_field_empty!(translate.t(LABEL_URL)) }
                { config_field_empty!(translate.t(LABEL_TOKEN)) }
                { config_field_empty!(translate.t(LABEL_USER)) }
            </Card>
          },
        }
    };

    let render_empty = || {
        html! {
          <>
            <div class="tp__messaging-config-view__header tp__config-view-page__header">
             { config_field_empty!(translate.t(LABEL_NOTIFY_ON)) }
            </div>
            <div class="tp__messaging-config-view__body tp__config-view-page__body">
             {render_telegram(None)}
             {render_rest(None)}
             {render_pushover(None)}
            </div>
          </>
        }
    };

    html! {
        <div class="tp__messaging-config-view tp__config-view-page">
            {
                if let Some(config) = &config_ctx.config {
                    if let Some(messaging) = &config.config.messaging {
                        html! {
                          <>
                        <div class="tp__messaging-config-view__header tp__config-view-page__header">
                          { config_field_child!(translate.t(LABEL_NOTIFY_ON), {
                             html! { <div class="tp__messaging-config-view__notify-on">
                                { for messaging.notify_on.iter().map(|t| html! { <Chip label={t.to_string()} /> }) }
                            </div> }
                          })}
                        </div>
                        <div class="tp__messaging-config-view__body tp__config-view-page__body">
                          {render_telegram(messaging.telegram.as_ref())}
                          {render_rest(messaging.rest.as_ref())}
                          {render_pushover(messaging.pushover.as_ref())}
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