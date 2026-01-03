use crate::model::MessagingConfig;
use crate::utils::{telegram_create_instance, telegram_send_message, SendMessageOption, SendMessageParseMode};
use chrono::Utc;
use handlebars::Handlebars;
use log::{debug, error};
use reqwest::{header, Method};
use serde_json::{json, Value};
use shared::model::MsgKind;
use shared::utils::json_str_to_markdown;
use std::borrow::Cow;
use std::str::FromStr;
use std::sync::LazyLock;

fn is_enabled(kind: MsgKind, cfg: &MessagingConfig) -> bool {
    cfg.notify_on.contains(&kind)
}

static HANDLEBARS: LazyLock<Handlebars> = LazyLock::new(Handlebars::new);
fn render_template(template: Option<&str>, msg: &str, kind: MsgKind) -> String {
    let timestamp = Utc::now().to_rfc3339();

    let mut data = json!({
        "message": msg,
        "kind": kind.to_string(),
        "timestamp": timestamp,
    });

    if let Ok(json_val) = serde_json::from_str::<Value>(msg) {
        if let Some(obj) = data.as_object_mut() {
            obj.insert("event".to_string(), json_val);
        }
    }

    match template {
        Some(t) => {
            match HANDLEBARS.render_template(t, &data) {
                Ok(rendered) => rendered,
                Err(e) => {
                    error!("Failed to render template: {e}");
                    msg.to_string()
                }
            }
        }
        None => msg.to_string(),
    }
}

async fn send_rest_message(client: &reqwest::Client, msg: &str, kind: MsgKind, messaging: &MessagingConfig) {
    if let Some(rest) = &messaging.rest {
        let body = render_template(rest.template.as_deref(), msg, kind);
        let method = Method::from_str(&rest.method).unwrap_or(Method::POST);

        let mut rb = client.request(method, &rest.url);

        let has_content_type = rest.headers.keys().any(|k| k.eq_ignore_ascii_case("content-type"));
        if !has_content_type {
            rb = rb.header(header::CONTENT_TYPE, mime::APPLICATION_JSON.to_string());
        }

        for (key, value) in &rest.headers {
            rb = rb.header(key, value);
        }

        match rb.body(body).send().await {
            Ok(response) => {
                if response.status().is_success() {
                    debug!("Message sent successfully to rest api");
                } else {
                    error!("Failed to send message to rest api, status code {}", response.status());
                }
            }
            Err(e) => error!("Message wasn't sent to rest api because of: {e}"),
        }
    }
}

async fn send_discord_message(client: &reqwest::Client, msg: &str, kind: MsgKind, messaging: &MessagingConfig) {
    if let Some(discord) = &messaging.discord {
        let body = if let Some(template) = &discord.template {
            render_template(Some(template), msg, kind)
        } else {
            json!({ "content": msg }).to_string()
        };

        match client
            .post(&discord.url)
            .header(header::CONTENT_TYPE, mime::APPLICATION_JSON.to_string())
            .body(body)
            .send()
            .await
        {
            Ok(response) => {
                if response.status().is_success() {
                    debug!("Message sent successfully to Discord");
                } else {
                    error!("Failed to send message to Discord, status code {}", response.status());
                }
            }
            Err(e) => error!("Message wasn't sent to Discord because of: {e}"),
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
                send_rest_message(client, msg, kind, messaging),
                send_pushover_message(client, msg, messaging),
                send_discord_message(client, msg, kind, messaging)
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

#[cfg(test)]
mod tests {
    use super::*;
    use shared::model::MsgKind;

    #[test]
    fn test_render_template_simple() {
        let msg = "Hello World";
        let kind = MsgKind::Info;
        let rendered = render_template(Some("Message: {{message}}, Kind: {{kind}}"), msg, kind);
        assert!(rendered.contains("Message: Hello World"));
        assert!(rendered.contains("Kind: Info"));
    }

    #[test]
    fn test_render_template_json() {
        let msg = r#"{"name": "test", "value": 123}"#;
        let kind = MsgKind::Watch;
        let rendered = render_template(Some("Added: {{event.name}}"), msg, kind);
        assert_eq!(rendered, "Added: test");
    }

    #[test]
    fn test_render_template_none() {
        let msg = "Hello World";
        let kind = MsgKind::Info;
        let rendered = render_template(None, msg, kind);
        assert_eq!(rendered, "Hello World");
    }

    #[test]
    fn test_render_template_invalid_syntax() {
        let msg = "Hello World";
        let kind = MsgKind::Info;
        // Unclosed handlebars expression
        let rendered = render_template(Some("Message: {{message"), msg, kind);
        assert_eq!(rendered, "Hello World");
    }
}
