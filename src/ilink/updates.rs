//! Update polling, typing, and lifecycle notification operations.

use std::time::Duration;

use super::auth::SecretValue;
use super::client::{ensure_api_success, IlinkAuth, IlinkError, DEFAULT_CONFIG_TIMEOUT};
use super::transport::IlinkClient;
use super::types::{GetConfigRequest, GetConfigResponse, SendTypingRequest, SendTypingResponse};
use super::types::{GetUpdatesRequest, GetUpdatesResponse, NotifyRequest, NotifyResponse};

const MAX_UPDATE_MESSAGES: usize = 256;
const MAX_MESSAGE_ITEMS: usize = 32;
const MAX_INBOUND_TEXT_BYTES: usize = 16 * 1024;
const MAX_INBOUND_ID_BYTES: usize = 256;
const MAX_SERVER_LONG_POLL_MS: u64 = 60_000;

impl IlinkClient {
    pub(super) async fn get_updates_request(
        &self,
        auth: &IlinkAuth,
        update_cursor: &str,
        long_poll_timeout: Duration,
    ) -> Result<GetUpdatesResponse, IlinkError> {
        if update_cursor.len() > 64 * 1024 {
            return Err(IlinkError::InvalidConfiguration("update cursor"));
        }
        let request = self.authenticated_post(auth, "ilink/bot/getupdates")?;
        let response: GetUpdatesResponse = match self
            .post_json(
                request,
                &GetUpdatesRequest {
                    get_updates_buf: update_cursor.to_string(),
                    base_info: self.identity.base_info(),
                },
                Self::bounded_long_poll_timeout(long_poll_timeout)?,
                "get_updates",
            )
            .await
        {
            Ok(response) => response,
            Err(IlinkError::Timeout) => {
                let update_cursor = if update_cursor.is_empty() {
                    None
                } else {
                    Some(
                        SecretValue::new(update_cursor.to_string())
                            .map_err(|_| IlinkError::InvalidConfiguration("update cursor"))?,
                    )
                };
                return Ok(GetUpdatesResponse {
                    ret: Some(0),
                    errcode: None,
                    errmsg: None,
                    messages: Vec::new(),
                    update_cursor,
                    long_polling_timeout_ms: None,
                });
            }
            Err(error) => return Err(error),
        };
        ensure_api_success("get_updates", response.ret, response.errcode)?;
        validate_updates_response(&response)?;
        Ok(response)
    }

    pub(super) async fn get_config_request(
        &self,
        auth: &IlinkAuth,
        owner_id: Option<&SecretValue>,
        context_token: Option<&SecretValue>,
    ) -> Result<GetConfigResponse, IlinkError> {
        let request = self.authenticated_post(auth, "ilink/bot/getconfig")?;
        let response: GetConfigResponse = self
            .post_json(
                request,
                &GetConfigRequest {
                    base_info: self.identity.base_info(),
                    ilink_user_id: owner_id.cloned(),
                    context_token: context_token.cloned(),
                },
                DEFAULT_CONFIG_TIMEOUT,
                "get_config",
            )
            .await?;
        ensure_api_success("get_config", response.ret, None)?;
        Ok(response)
    }

    pub(super) async fn send_typing_request(
        &self,
        auth: &IlinkAuth,
        owner_id: &SecretValue,
        typing_ticket: &SecretValue,
        status: i32,
    ) -> Result<SendTypingResponse, IlinkError> {
        if !matches!(status, 1 | 2) {
            return Err(IlinkError::InvalidConfiguration("typing status"));
        }
        let request = self.authenticated_post(auth, "ilink/bot/sendtyping")?;
        let response: SendTypingResponse = self
            .post_json(
                request,
                &SendTypingRequest {
                    ilink_user_id: owner_id.clone(),
                    typing_ticket: typing_ticket.clone(),
                    status,
                    base_info: self.identity.base_info(),
                },
                DEFAULT_CONFIG_TIMEOUT,
                "send_typing",
            )
            .await?;
        ensure_api_success("send_typing", response.ret, None)?;
        Ok(response)
    }

    pub(super) async fn notify_request(
        &self,
        auth: &IlinkAuth,
        start: bool,
    ) -> Result<NotifyResponse, IlinkError> {
        let (endpoint, operation) = if start {
            ("ilink/bot/msg/notifystart", "notify_start")
        } else {
            ("ilink/bot/msg/notifystop", "notify_stop")
        };
        let request = self.authenticated_post(auth, endpoint)?;
        let response: NotifyResponse = self
            .post_json(
                request,
                &NotifyRequest {
                    base_info: self.identity.base_info(),
                },
                DEFAULT_CONFIG_TIMEOUT,
                operation,
            )
            .await?;
        ensure_api_success(operation, response.ret, None)?;
        Ok(response)
    }
}

pub(super) fn validate_updates_response(response: &GetUpdatesResponse) -> Result<(), IlinkError> {
    if response.messages.len() > MAX_UPDATE_MESSAGES
        || response
            .long_polling_timeout_ms
            .is_some_and(|timeout| timeout == 0 || timeout > MAX_SERVER_LONG_POLL_MS)
    {
        return Err(IlinkError::InvalidResponse("get_updates"));
    }
    for message in &response.messages {
        if message.item_list.len() > MAX_MESSAGE_ITEMS
            || message
                .client_id
                .as_deref()
                .is_some_and(|value| value.len() > MAX_INBOUND_ID_BYTES || value.contains('\0'))
            || message
                .run_id
                .as_deref()
                .is_some_and(|value| value.len() > MAX_INBOUND_ID_BYTES || value.contains('\0'))
        {
            return Err(IlinkError::InvalidResponse("get_updates"));
        }
        for item in &message.item_list {
            if item
                .msg_id
                .as_deref()
                .is_some_and(|value| value.len() > MAX_INBOUND_ID_BYTES || value.contains('\0'))
                || item
                    .text_item
                    .as_ref()
                    .and_then(|text| text.text.as_deref())
                    .is_some_and(|text| text.len() > MAX_INBOUND_TEXT_BYTES || text.contains('\0'))
            {
                return Err(IlinkError::InvalidResponse("get_updates"));
            }
        }
    }
    Ok(())
}
