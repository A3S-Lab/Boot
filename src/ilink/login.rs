//! QR-code login protocol operations.

use super::auth::SecretValue;
use super::client::{IlinkError, DEFAULT_QR_POLL_TIMEOUT};
use super::transport::IlinkClient;
use super::types::{CreateQrRequest, CreateQrResponse, PollQrResponse};
use super::url_policy::ValidatedBaseUrl;

const MAX_QR_IMAGE_CONTENT_BYTES: usize = 256 * 1024;
const MAX_LOCAL_TOKEN_COUNT: usize = 10;

impl IlinkClient {
    pub(super) async fn create_qr_request(
        &self,
        local_tokens: &[SecretValue],
    ) -> Result<CreateQrResponse, IlinkError> {
        if local_tokens.len() > MAX_LOCAL_TOKEN_COUNT {
            return Err(IlinkError::InvalidConfiguration("local token list"));
        }
        let mut url = self.qr_base_url.join("ilink/bot/get_bot_qrcode")?;
        url.query_pairs_mut()
            .append_pair("bot_type", &self.identity.bot_type);
        let request = self
            .http
            .post(url)
            .headers(self.identity.post_headers(None)?);
        let response: CreateQrResponse = self
            .post_json_without_timeout(
                request,
                &CreateQrRequest {
                    local_token_list: local_tokens.to_vec(),
                },
                "create_qr",
            )
            .await?;
        if response.qrcode_img_content.expose().len() > MAX_QR_IMAGE_CONTENT_BYTES
            || response.qrcode_img_content.expose().contains('\0')
        {
            return Err(IlinkError::InvalidResponse("create_qr"));
        }
        Ok(response)
    }

    pub(super) async fn poll_qr_request(
        &self,
        base_url: &ValidatedBaseUrl,
        qrcode: &SecretValue,
        verify_code: Option<&SecretValue>,
    ) -> Result<PollQrResponse, IlinkError> {
        let mut url = base_url.join("ilink/bot/get_qrcode_status")?;
        {
            let mut query = url.query_pairs_mut();
            query.append_pair("qrcode", qrcode.expose());
            if let Some(verify_code) = verify_code {
                query.append_pair("verify_code", verify_code.expose());
            }
        }
        let request = self
            .http
            .get(url)
            .headers(self.identity.application_headers()?);
        let response: PollQrResponse = match self
            .get_json(request, DEFAULT_QR_POLL_TIMEOUT, "poll_qr")
            .await
        {
            Ok(response) => response,
            Err(error) if retriable_poll_error(&error) => return Ok(PollQrResponse::waiting()),
            Err(error) => return Err(error),
        };
        match response.status {
            super::types::QrCodeStatus::Unknown => {
                return Err(IlinkError::InvalidResponse("poll_qr"));
            }
            super::types::QrCodeStatus::ScanedButRedirect => {
                let redirect_host = response
                    .redirect_host
                    .as_deref()
                    .ok_or(IlinkError::InvalidResponse("poll_qr"))?;
                self.host_policy.validate_redirect_host(redirect_host)?;
            }
            super::types::QrCodeStatus::Confirmed => {
                if response.bot_token.is_none()
                    || response.ilink_bot_id.is_none()
                    || response.ilink_user_id.is_none()
                {
                    return Err(IlinkError::InvalidResponse("poll_qr"));
                }
                let account_base_url = response
                    .baseurl
                    .as_deref()
                    .ok_or(IlinkError::InvalidResponse("poll_qr"))?;
                self.host_policy.validate(account_base_url)?;
            }
            _ => {}
        }
        Ok(response)
    }
}

fn retriable_poll_error(error: &IlinkError) -> bool {
    matches!(
        error,
        IlinkError::Timeout | IlinkError::Transport | IlinkError::HttpStatus(408 | 429 | 500..=599)
    )
}
