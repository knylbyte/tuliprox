use shared::model::{DiscordMessagingConfigDto, MessagingConfigDto, MsgKind, PushoverMessagingConfigDto, RestMessagingConfigDto, TelegramMessagingConfigDto};
use crate::model::macros;

#[derive(Debug, Clone)]
pub struct TelegramMessagingConfig {
    pub bot_token: String,
    pub chat_ids: Vec<String>,
    pub markdown: bool,
}

macros::from_impl!(TelegramMessagingConfig);
impl From<&TelegramMessagingConfigDto>  for TelegramMessagingConfig {
    fn from(dto: &TelegramMessagingConfigDto) -> Self {
        Self {
            bot_token: dto.bot_token.clone(),
            chat_ids: dto.chat_ids.clone(),
            markdown: dto.markdown,
        }
    }
}

impl From<&TelegramMessagingConfig>  for TelegramMessagingConfigDto {
    fn from(instance: &TelegramMessagingConfig) -> Self {
        Self {
            bot_token: instance.bot_token.clone(),
            chat_ids: instance.chat_ids.clone(),
            markdown: instance.markdown,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RestMessagingConfig {
    pub url: String,
    pub method: String,
    pub headers: std::collections::HashMap<String, String>,
    pub template: Option<String>,
}

macros::from_impl!(RestMessagingConfig);
impl From<&RestMessagingConfigDto> for RestMessagingConfig {
    fn from(dto: &RestMessagingConfigDto) -> Self {
        let mut headers = std::collections::HashMap::new();
        for h in &dto.headers {
            if let Some((k, v)) = h.split_once(':') {
                headers.insert(k.trim().to_string(), v.trim().to_string());
            }
        }
        Self {
            url: dto.url.clone(),
            method: dto.method.clone().unwrap_or_else(|| "POST".to_string()),
            headers,
            template: dto.template.clone(),
        }
    }
}

impl From<&RestMessagingConfig> for RestMessagingConfigDto {
    fn from(model: &RestMessagingConfig) -> Self {
        let headers = model.headers.iter()
            .map(|(k, v)| format!("{k}: {v}"))
            .collect();
        Self {
            url: model.url.clone(),
            method: Some(model.method.clone()),
            headers,
            template: model.template.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct DiscordMessagingConfig {
    pub url: String,
    pub template: Option<String>,
}

macros::from_impl!(DiscordMessagingConfig);
impl From<&DiscordMessagingConfigDto> for DiscordMessagingConfig {
    fn from(dto: &DiscordMessagingConfigDto) -> Self {
        Self {
            url: dto.url.clone(),
            template: dto.template.clone(),
        }
    }
}

impl From<&DiscordMessagingConfig> for DiscordMessagingConfigDto {
    fn from(instance: &DiscordMessagingConfig) -> Self {
        Self {
            url: instance.url.clone(),
            template: instance.template.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct PushoverMessagingConfig {
    pub url: String,
    pub token: String,
    pub user: String,
}

macros::from_impl!(PushoverMessagingConfig);
impl From<&PushoverMessagingConfigDto> for PushoverMessagingConfig {
    fn from(dto: &PushoverMessagingConfigDto) -> Self {
        Self {
            url: dto.url.as_ref().map_or_else(|| String::from("https://api.pushover.net/1/messages.json"), ToString::to_string),
            token: dto.token.clone(),
            user: dto.user.clone(),
        }
    }
}

impl From<&PushoverMessagingConfig> for PushoverMessagingConfigDto {
    fn from(instance: &PushoverMessagingConfig) -> Self {
        Self {
            url: Some(instance.url.clone()),
            token: instance.token.clone(),
            user: instance.user.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct MessagingConfig {
    pub notify_on: Vec<MsgKind>,
    pub telegram: Option<TelegramMessagingConfig>,
    pub rest: Option<RestMessagingConfig>,
    pub pushover: Option<PushoverMessagingConfig>,
    pub discord: Option<DiscordMessagingConfig>,
}

macros::from_impl!(MessagingConfig);
impl From<&MessagingConfigDto> for MessagingConfig {
    fn from(dto: &MessagingConfigDto) -> Self {
        Self {
            notify_on: dto.notify_on.clone(),
            telegram: dto.telegram.as_ref().map(Into::into),
            rest: dto.rest.as_ref().map(Into::into),
            pushover: dto.pushover.as_ref().map(Into::into),
            discord: dto.discord.as_ref().map(Into::into),
        }
    }
}

impl From<&MessagingConfig> for MessagingConfigDto {
    fn from(instance: &MessagingConfig) -> Self {
        Self {
            notify_on: instance.notify_on.clone(),
            telegram: instance.telegram.as_ref().map(Into::into),
            rest: instance.rest.as_ref().map(Into::into),
            pushover: instance.pushover.as_ref().map(Into::into),
            discord: instance.discord.as_ref().map(Into::into),
        }
    }
}