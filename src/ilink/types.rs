//! JSON wire types used by the iLink protocol.

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize};

use super::auth::SecretValue;

pub const MESSAGE_TYPE_USER: i32 = 1;
pub(super) const MESSAGE_TYPE_BOT: i32 = 2;
pub(super) const MESSAGE_ITEM_TYPE_TEXT: i32 = 1;
pub const MESSAGE_STATE_FINISH: i32 = 2;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct BaseInfo {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) channel_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) bot_agent: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(super) struct CreateQrRequest {
    pub(super) local_token_list: Vec<SecretValue>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct CreateQrResponse {
    pub qrcode: SecretValue,
    pub qrcode_img_content: SecretValue,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QrCodeStatus {
    Wait,
    Scaned,
    Confirmed,
    Expired,
    ScanedButRedirect,
    NeedVerifycode,
    VerifyCodeBlocked,
    BindedRedirect,
    #[serde(other)]
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct PollQrResponse {
    pub status: QrCodeStatus,
    #[serde(default, deserialize_with = "deserialize_optional_secret")]
    pub bot_token: Option<SecretValue>,
    #[serde(default, deserialize_with = "deserialize_optional_secret")]
    pub ilink_bot_id: Option<SecretValue>,
    #[serde(default, deserialize_with = "deserialize_optional_secret")]
    pub ilink_user_id: Option<SecretValue>,
    #[serde(default)]
    pub baseurl: Option<String>,
    #[serde(default)]
    pub redirect_host: Option<String>,
}

impl PollQrResponse {
    pub(super) fn waiting() -> Self {
        Self {
            status: QrCodeStatus::Wait,
            bot_token: None,
            ilink_bot_id: None,
            ilink_user_id: None,
            baseurl: None,
            redirect_host: None,
        }
    }
}

#[derive(Clone, Default, PartialEq, Eq, Serialize)]
pub(super) struct GetUpdatesRequest {
    pub(super) get_updates_buf: String,
    #[serde(default)]
    pub(super) base_info: BaseInfo,
}

impl fmt::Debug for GetUpdatesRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GetUpdatesRequest")
            .field("has_update_cursor", &!self.get_updates_buf.is_empty())
            .field("base_info", &self.base_info)
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq, Deserialize)]
pub struct GetUpdatesResponse {
    #[serde(default)]
    pub(super) ret: Option<i64>,
    #[serde(default)]
    pub(super) errcode: Option<i64>,
    #[serde(default)]
    pub(super) errmsg: Option<String>,
    #[serde(default, rename = "msgs")]
    pub messages: Vec<WeixinMessage>,
    #[serde(
        default,
        rename = "get_updates_buf",
        deserialize_with = "deserialize_optional_secret"
    )]
    pub update_cursor: Option<SecretValue>,
    #[serde(default, rename = "longpolling_timeout_ms")]
    pub long_polling_timeout_ms: Option<u64>,
}

impl fmt::Debug for GetUpdatesResponse {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GetUpdatesResponse")
            .field("ret", &self.ret)
            .field("errcode", &self.errcode)
            .field("message_count", &self.messages.len())
            .field("has_update_cursor", &self.update_cursor.is_some())
            .field("long_polling_timeout_ms", &self.long_polling_timeout_ms)
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq, Deserialize)]
pub struct WeixinMessage {
    #[serde(default)]
    pub seq: Option<u64>,
    #[serde(default)]
    pub message_id: Option<u64>,
    #[serde(default, deserialize_with = "deserialize_optional_secret")]
    pub from_user_id: Option<SecretValue>,
    #[serde(default, deserialize_with = "deserialize_optional_secret")]
    pub to_user_id: Option<SecretValue>,
    #[serde(default)]
    pub client_id: Option<String>,
    #[serde(default)]
    pub create_time_ms: Option<u64>,
    #[serde(default)]
    pub update_time_ms: Option<u64>,
    #[serde(default, deserialize_with = "deserialize_optional_secret")]
    pub session_id: Option<SecretValue>,
    #[serde(default, deserialize_with = "deserialize_optional_secret")]
    pub group_id: Option<SecretValue>,
    #[serde(default)]
    pub message_type: Option<i32>,
    #[serde(default)]
    pub message_state: Option<i32>,
    #[serde(default)]
    pub(super) item_list: Vec<MessageItem>,
    #[serde(default, deserialize_with = "deserialize_optional_secret")]
    pub context_token: Option<SecretValue>,
    #[serde(default)]
    pub run_id: Option<String>,
}

impl WeixinMessage {
    pub fn text(&self) -> Option<&str> {
        self.item_list.iter().find_map(|item| {
            (item.item_type == Some(MESSAGE_ITEM_TYPE_TEXT))
                .then(|| item.text_item.as_ref()?.text.as_deref())
                .flatten()
        })
    }
}

impl fmt::Debug for WeixinMessage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WeixinMessage")
            .field("seq", &self.seq)
            .field("message_id", &self.message_id)
            .field("message_type", &self.message_type)
            .field("message_state", &self.message_state)
            .field("item_count", &self.item_list.len())
            .field("has_context_token", &self.context_token.is_some())
            .finish()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct MessageItem {
    #[serde(default, rename = "type", skip_serializing_if = "Option::is_none")]
    pub(super) item_type: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) create_time_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) update_time_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) is_completed: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) msg_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) text_item: Option<TextItem>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct TextItem {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) text: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(super) struct SendMessageRequest {
    pub(super) msg: OutboundWeixinMessage,
    pub(super) base_info: BaseInfo,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(super) struct OutboundWeixinMessage {
    pub(super) from_user_id: String,
    pub(super) to_user_id: SecretValue,
    pub(super) client_id: String,
    pub(super) message_type: i32,
    pub(super) message_state: i32,
    pub(super) item_list: Vec<MessageItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) context_token: Option<SecretValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) run_id: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
pub struct SendMessageResponse {
    #[serde(default)]
    pub(super) ret: Option<i64>,
    #[serde(default)]
    pub(super) errmsg: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub(super) struct GetConfigRequest {
    #[serde(default)]
    pub(super) base_info: BaseInfo,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) ilink_user_id: Option<SecretValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) context_token: Option<SecretValue>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
pub struct GetConfigResponse {
    #[serde(default)]
    pub(super) ret: Option<i64>,
    #[serde(default)]
    pub(super) errmsg: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_secret")]
    pub(super) typing_ticket: Option<SecretValue>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(super) struct SendTypingRequest {
    pub(super) ilink_user_id: SecretValue,
    pub(super) typing_ticket: SecretValue,
    pub(super) status: i32,
    pub(super) base_info: BaseInfo,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
pub struct SendTypingResponse {
    #[serde(default)]
    pub(super) ret: Option<i64>,
    #[serde(default)]
    pub(super) errmsg: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub(super) struct NotifyRequest {
    #[serde(default)]
    pub(super) base_info: BaseInfo,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
pub struct NotifyResponse {
    #[serde(default)]
    pub(super) ret: Option<i64>,
    #[serde(default)]
    pub(super) errmsg: Option<String>,
}

fn deserialize_optional_secret<'de, D>(deserializer: D) -> Result<Option<SecretValue>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<String>::deserialize(deserializer)?;
    value
        .filter(|value| !value.is_empty())
        .map(SecretValue::new)
        .transpose()
        .map_err(serde::de::Error::custom)
}
