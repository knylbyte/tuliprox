use crate::model::MsgKind;
use crate::utils::is_blank_optional_string;

#[derive(Default, Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TelegramMessagingConfigDto {
    pub bot_token: String,
    pub chat_ids: Vec<String>,
    #[serde(default)]
    pub markdown: bool,
}

impl TelegramMessagingConfigDto {
    pub fn is_empty(&self) -> bool {
        self.bot_token.trim().is_empty() && self.chat_ids.is_empty()
    }
}

#[derive(Default, Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RestMessagingConfigDto {
    pub url: String,
}

impl RestMessagingConfigDto {
    pub fn is_empty(&self) -> bool {
        self.url.trim().is_empty()
    }
}

#[derive(Default, Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PushoverMessagingConfigDto {
    pub url: Option<String>,
    pub token: String,
    pub user: String,
}

impl PushoverMessagingConfigDto {
    pub fn is_empty(&self) -> bool {
        is_blank_optional_string(self.url.as_deref())
            && self.token.trim().is_empty()
            && self.user.trim().is_empty()
    }
}

#[derive(Default, Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct MessagingConfigDto {
    #[serde(default)]
    pub notify_on: Vec<MsgKind>,
    #[serde(default)]
    pub telegram: Option<TelegramMessagingConfigDto>,
    #[serde(default)]
    pub rest: Option<RestMessagingConfigDto>,
    #[serde(default)]
    pub pushover: Option<PushoverMessagingConfigDto>,
}

impl MessagingConfigDto {
    pub fn is_empty(&self) -> bool {
        self.notify_on.is_empty()
            && (self.telegram.is_none() || self.telegram.as_ref().is_some_and(|c| c.is_empty()))
            && (self.rest.is_none()  || self.rest.as_ref().is_some_and(|c| c.is_empty()))
            && (self.pushover.is_none() || self.pushover.as_ref().is_some_and(|c| c.is_empty()))
    }

    pub fn clean(&mut self) {
        if self.telegram.as_ref().is_some_and(|c| c.is_empty()) {
            self.telegram = None;
        }
        if self.rest.as_ref().is_some_and(|c| c.is_empty()) {
            self.rest = None;
        }
        if self.pushover.as_ref().is_some_and(|c| c.is_empty()) {
            self.pushover = None;
        }

    }
}