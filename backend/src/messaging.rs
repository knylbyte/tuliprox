use std::borrow::Cow;
use crate::model::MessagingConfig;
use crate::utils::{telegram_create_instance, telegram_send_message, SendMessageOption, SendMessageParseMode};
use log::{debug, error};
use reqwest::header;
use shared::model::MsgKind;
use shared::utils::json_str_to_markdown;

fn is_enabled(kind: MsgKind, cfg: &MessagingConfig) -> bool {
    cfg.notify_on.contains(&kind)
}

async fn send_http_post_request(client: &reqwest::Client, msg: &str, messaging: &MessagingConfig) {
    if let Some(rest) = &messaging.rest {
    let data = msg.to_owned();
        match client
            .post(&rest.url)
            .header(header::CONTENT_TYPE, mime::APPLICATION_JSON.to_string())
            .body(data)
            .send()
            .await
        {
            Ok(_) => debug!("Text message sent successfully to rest api"),
            Err(e) => error!("Text message wasn't sent to rest api because of: {e}"),
        }
    }
}

async fn send_telegram_message(client: &reqwest::Client, msg: &str, messaging: &MessagingConfig, json: bool) {
    // TODO use proxy settings
    if let Some(telegram) = &messaging.telegram {
        let (message, options) = {
            if json && telegram.markdown {
                if let Ok(md) = json_str_to_markdown(msg) {
                    (Cow::Owned(md), Some(SendMessageOption { parse_mode: SendMessageParseMode::MarkdownV2 }))
                } else {
                    (Cow::Borrowed(msg), None)
                }
            } else {
                (Cow::Borrowed(msg), None)
            }
        };

        for chat_id in &telegram.chat_ids {
            let bot = telegram_create_instance(&telegram.bot_token, chat_id);
            telegram_send_message(client, &bot, &message, options.as_ref()).await;
        }
    }
}

async fn send_pushover_message(client: &reqwest::Client, msg: &str, messaging: &MessagingConfig) {
    if let Some(pushover) = &messaging.pushover {
        let encoded_message: String = url::form_urlencoded::Serializer::new(String::new())
            .append_pair("token", pushover.token.as_str())
            .append_pair("user", pushover.user.as_str())
            .append_pair("message", msg)
            .finish();
        match client
            .post(&pushover.url)
            .header(header::CONTENT_TYPE, mime::APPLICATION_WWW_FORM_URLENCODED.to_string())
            .body(encoded_message)
            .send()
            .await
        {
            Ok(response) => {
                if response.status().is_success() {
                    debug!("Text message sent successfully to PUSHOVER, status code {}", response.status());
                } else {
                    error!("Failed to send text message to PUSHOVER, status code {}", response.status());
                }
            }
            Err(e) => error!("Text message wasn't sent to PUSHOVER api because of: {e}"),
        }
    }
}

async fn dispatch_send_message(client: &reqwest::Client, kind: MsgKind, cfg: Option<&MessagingConfig>, msg: &str, json: bool) {
    if let Some(messaging) = cfg {
        if is_enabled(kind, messaging) {
            tokio::join!(
            send_telegram_message(client, msg, messaging, json),
            send_http_post_request(client, msg, messaging),
            send_pushover_message(client, msg, messaging)
            );
        }
    }
}

pub async fn send_message_json(client: &reqwest::Client, kind: MsgKind, cfg: Option<&MessagingConfig>, msg: &str) {
    dispatch_send_message(client, kind, cfg, msg, true).await;
}

pub async fn send_message(client: &reqwest::Client, kind: MsgKind, cfg: Option<&MessagingConfig>, msg: &str) {
    dispatch_send_message(client, kind, cfg, msg, false).await;
}
