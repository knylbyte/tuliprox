use std::rc::Rc;
use std::str::FromStr;
use crate::app::components::{Card, Chip, RadioButtonGroup};
use crate::app::context::ConfigContext;
use crate::{config_field, config_field_child, config_field_empty, config_field_hide, config_field_optional, edit_field_list, edit_field_text, edit_field_text_option, generate_form_reducer};
use shared::model::{MessagingConfigDto, MsgKind, PushoverMessagingConfigDto, RestMessagingConfigDto,
                    TelegramMessagingConfigDto};
use yew::prelude::*;
use yew_i18n::use_translation;
use crate::app::components::config::config_page::ConfigForm;
use crate::app::components::config::config_view_context::ConfigViewContext;
use crate::app::components::config::macros::HasFormData;

const LABEL_NOTIFY_ON: &str = "LABEL.NOTIFY_ON";
const LABEL_TELEGRAM: &str = "LABEL.TELEGRAM";
const LABEL_PUSHOVER: &str = "LABEL.PUSHOVER";
const LABEL_REST: &str = "LABEL.REST";
const LABEL_BOT_TOKEN: &str = "LABEL.BOT_TOKEN";
const LABEL_CHAT_IDS: &str = "LABEL.CHAT_IDS";
const LABEL_URL: &str = "LABEL.URL";
const LABEL_TOKEN: &str = "LABEL.TOKEN";
const LABEL_USER: &str = "LABEL.USER";

generate_form_reducer!(
    state: TelegramMessagingConfigFormState { form: TelegramMessagingConfigDto },
    action_name: TelegramMessagingConfigFormAction,
    fields {
        BotToken => bot_token: String,
        ChatIds => chat_ids: Vec<String>,
    }
);

generate_form_reducer!(
    state: RestMessagingConfigFormState { form: RestMessagingConfigDto },
    action_name: RestMessagingConfigFormAction,
    fields {
        Url => url: String,
    }
);

generate_form_reducer!(
    state: PushoverMessagingConfigFormState { form: PushoverMessagingConfigDto },
    action_name: PushoverMessagingConfigFormAction,
    fields {
        Url => url: Option<String>,
        Token => token: String,
        User => user: String,
    }
);

generate_form_reducer!(
    state: MessagingConfigFormState { form: MessagingConfigDto },
    action_name: MessagingConfigFormAction,
    fields {
        NotifyOn => notify_on: Vec<MsgKind>,
    }
);


#[function_component]
pub fn MessagingConfigView() -> Html {
    let translate = use_translation();
    let config_ctx = use_context::<ConfigContext>().expect("ConfigContext not found");
    let config_view_ctx = use_context::<ConfigViewContext>().expect("ConfigViewContext not found");

    let telegram_state: UseReducerHandle<TelegramMessagingConfigFormState> = use_reducer(|| {
        TelegramMessagingConfigFormState { form: TelegramMessagingConfigDto::default(), modified: false }
    });
    let rest_state: UseReducerHandle<RestMessagingConfigFormState> = use_reducer(|| {
        RestMessagingConfigFormState { form: RestMessagingConfigDto::default(), modified: false }
    });

    let pushover_state: UseReducerHandle<PushoverMessagingConfigFormState> = use_reducer(|| {
        PushoverMessagingConfigFormState { form: PushoverMessagingConfigDto::default(), modified: false }
    });

    let messaging_state: UseReducerHandle<MessagingConfigFormState> = use_reducer(|| {
        MessagingConfigFormState { form: MessagingConfigDto::default(), modified: false }
    });

    let notify_on_options = use_memo((), |_| vec![
        MsgKind::Info.to_string(),
        MsgKind::Stats.to_string(),
        MsgKind::Error.to_string(),
        MsgKind::Watch.to_string(),
    ]);

    {
        let on_form_change = config_view_ctx.on_form_change.clone();
        let messaging_state = messaging_state.clone();
        let telegram_state = telegram_state.clone();
        let rest_state = rest_state.clone();
        let pushover_state = pushover_state.clone();

        use_effect_with(
            (messaging_state, telegram_state, rest_state, pushover_state),
            move |(m, t, r, p)| {
                let mut form = m.form.clone();
                form.telegram = Some(t.form.clone());
                form.rest = Some(r.form.clone());
                form.pushover = Some(p.form.clone());

                let modified = m.modified || t.modified || r.modified || p.modified;
                on_form_change.emit(ConfigForm::Messaging(modified, form));
            },
        );
    }

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

    let render_view_mode = || {
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
                render_empty()
            }
        } else {
            render_empty()
        }
    };

    let render_edit_mode = || {
        let msg_state = messaging_state.clone();
        let notify_on_selections = Rc::new(msg_state.form.notify_on.iter().map(ToString::to_string).collect());
        html! {
        <>
        <div class="tp__messaging-config-view__header tp__config-view-page__header">
            { config_field_child!(translate.t("LABEL.NOTIFY_ON"), {
               html! { <RadioButtonGroup
                    multi_select={true} none_allowed={true}
                    on_select={Callback::from(move |selections: Rc<Vec<String>>| {
                        msg_state.dispatch(MessagingConfigFormAction::NotifyOn(
                            selections.iter().filter_map(|s| MsgKind::from_str(s).ok()).collect()));
                    })}
                    options={notify_on_options.clone()}
                    selected={notify_on_selections}
                />
            }})}
        </div>
        <div class="tp__messaging-config-view__body tp__config-view-page__body">
            <Card class="tp__config-view__card">
                <h1>{translate.t(LABEL_TELEGRAM)}</h1>
                { edit_field_text!(telegram_state, translate.t(LABEL_BOT_TOKEN), bot_token, TelegramMessagingConfigFormAction::BotToken) }
                { edit_field_list!(telegram_state, translate.t(LABEL_CHAT_IDS), chat_ids, TelegramMessagingConfigFormAction::ChatIds, translate.t("LABEL.ADD_CHAT_ID")) }
            </Card>

            <Card class="tp__config-view__card">
                <h1>{translate.t(LABEL_REST)}</h1>
                { edit_field_text!(rest_state, translate.t(LABEL_URL), url, RestMessagingConfigFormAction::Url) }
            </Card>

            <Card class="tp__config-view__card">
                <h1>{translate.t(LABEL_PUSHOVER)}</h1>
                { edit_field_text_option!(pushover_state, translate.t(LABEL_URL), url, PushoverMessagingConfigFormAction::Url) }
                { edit_field_text!(pushover_state, translate.t(LABEL_TOKEN), token, PushoverMessagingConfigFormAction::Token, true) }
                { edit_field_text!(pushover_state, translate.t(LABEL_USER), user, PushoverMessagingConfigFormAction::User) }
            </Card>
        </div>
        </>
    }};

    html! {
        <div class="tp__messaging-config-view tp__config-view-page">
            { if *config_view_ctx.edit_mode { render_edit_mode() } else { render_view_mode() } }
        </div>
    }
}