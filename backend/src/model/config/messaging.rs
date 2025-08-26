use shared::model::{MessagingConfigDto, MsgKind, PushoverMessagingConfigDto, RestMessagingConfigDto, TelegramMessagingConfigDto};
use crate::model::macros;

#[derive(Debug, Clone)]
pub struct TelegramMessagingConfig {
    pub bot_token: String,
    pub chat_ids: Vec<String>,
}

macros::from_impl!(TelegramMessagingConfig);
impl From<&TelegramMessagingConfigDto>  for TelegramMessagingConfig {
    fn from(dto: &TelegramMessagingConfigDto) -> Self {
        Self {
            bot_token: dto.bot_token.clone(),
            chat_ids: dto.chat_ids.clone(),
        }
    }
}

impl From<&TelegramMessagingConfig>  for TelegramMessagingConfigDto {
    fn from(instance: &TelegramMessagingConfig) -> Self {
        Self {
            bot_token: instance.bot_token.clone(),
            chat_ids: instance.chat_ids.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct RestMessagingConfig {
    pub url: String,
}

macros::from_impl!(RestMessagingConfig);
impl From<&RestMessagingConfigDto> for RestMessagingConfig {
    fn from(dto: &RestMessagingConfigDto) -> Self {
        Self {
            url: dto.url.clone(),
        }
    }
}

impl From<&RestMessagingConfig> for RestMessagingConfigDto {
    fn from(instance: &RestMessagingConfig) -> Self {
        Self {
            url: instance.url.clone(),
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
}

macros::from_impl!(MessagingConfig);
impl From<&MessagingConfigDto> for MessagingConfig {
    fn from(dto: &MessagingConfigDto) -> Self {
        Self {
            notify_on: dto.notify_on.clone(),
            telegram: dto.telegram.as_ref().map(Into::into),
            rest: dto.rest.as_ref().map(Into::into),
            pushover: dto.pushover.as_ref().map(Into::into),
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
        }
    }
}