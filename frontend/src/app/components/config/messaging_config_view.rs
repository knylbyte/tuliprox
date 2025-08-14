use crate::app::components::{Card, Chip};
use crate::app::context::ConfigContext;
use crate::{config_field, config_field_child, config_field_empty, config_field_hide, config_field_optional};
use shared::model::{PushoverMessagingConfigDto, RestMessagingConfigDto, TelegramMessagingConfigDto};
use yew::prelude::*;
use yew_i18n::use_translation;

#[function_component]
pub fn MessagingConfigView() -> Html {
    let translate = use_translation();
    let config_ctx = use_context::<ConfigContext>().expect("ConfigContext not found");

    let render_telegram = |telegram: Option<&TelegramMessagingConfigDto>| {
        match telegram {
            Some(entry) => html! {
            <Card class="tp__config-view__card">
                <h1>{translate.t("LABEL.TELEGRAM")}</h1>
                { config_field!(entry, translate.t("LABEL.BOT_TOKEN"), bot_token) }
                { config_field_child!(translate.t("LABEL.CHAT_ID"), {
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
               <h1>{translate.t("LABEL.TELEGRAM")}</h1>
               { config_field_empty!(translate.t("LABEL.BOT_TOKEN")) }
               { config_field_empty!(translate.t("LABEL.CHAT_IDS")) }
            </Card>
          },
        }
    };

    let render_rest = |rest: Option<&RestMessagingConfigDto>| {
        match rest {
            Some(entry) => html! {
            <Card class="tp__config-view__card">
                <h1>{translate.t("LABEL.REST")}</h1>
                { config_field!(entry, translate.t("LABEL.URL"), url) }
            </Card>
          },
            None => html! {
            <Card class="tp__config-view__card">
                <h1>{translate.t("LABEL.REST")}</h1>
                { config_field_empty!(translate.t("LABEL.URL")) }
            </Card>
          },
        }
    };

    let render_pushover = |pushover: Option<&PushoverMessagingConfigDto>| {
        match pushover {
            Some(entry) => html! {
            <Card class="tp__config-view__card">
                <h1>{translate.t("LABEL.PUSHOVER")}</h1>
                { config_field_optional!(entry, translate.t("LABEL.URL"), url) }
                { config_field_hide!(entry, translate.t("LABEL.TOKEN"), token) }
                { config_field!(entry, translate.t("LABEL.USER"), user) }
            </Card>
            },
            None => html! {
            <Card class="tp__config-view__card">
                <h1>{translate.t("LABEL.PUSHOVER")}</h1>
                { config_field_empty!(translate.t("LABEL.URL")) }
                { config_field_empty!(translate.t("LABEL.TOKEN")) }
                { config_field_empty!(translate.t("LABEL.USER")) }
            </Card>
          },
        }
    };

    let render_empty = || {
        html! {
          <>
            <div class="tp__messaging-config-view__header tp__config-view-page__header">
             { config_field_empty!(translate.t("LABEL.NOTIFY_ON")) }
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
                          { config_field_child!(translate.t("LABEL.NOTIFY_ON"), {
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