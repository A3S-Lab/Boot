//! Tencent Weixin iLink protocol support.
//!
//! This module implements the HTTP/JSON contract used by Tencent's
//! `openclaw-weixin` SDK. Product applications remain responsible for storing
//! credentials, exposing user-facing APIs, and deciding what remote actions a
//! bound Weixin account may perform.

mod auth;
mod client;
mod login;
mod messages;
mod transport;
mod types;
mod updates;
mod url_policy;

use std::sync::Arc;

use crate::{BootError, Module, ProviderDefinition, ProviderToken, Result as BootResult};

pub use auth::{ClientVersionError, SecretValue, SecretValueError};
pub use client::{IlinkAuth, IlinkError};
pub use transport::{IlinkClient, IlinkLoginTransport, IlinkMessagingTransport};
pub use types::{
    CreateQrResponse, GetConfigResponse, GetUpdatesResponse, NotifyResponse, PollQrResponse,
    QrCodeStatus, SendMessageResponse, SendTypingResponse, WeixinMessage, MESSAGE_STATE_FINISH,
    MESSAGE_TYPE_USER,
};
pub use url_policy::{IlinkUrlError, ValidatedBaseUrl};

/// Tencent's public iLink application identity used by the official SDK.
pub const WEIXIN_ILINK_APP_ID: &str = "bot";

/// Tencent's iLink bot type used by the official Weixin channel.
pub const WEIXIN_ILINK_BOT_TYPE: &str = "3";

/// Wire-contract version verified against Tencent `openclaw-weixin` v2.4.6.
pub const WEIXIN_ILINK_PROTOCOL_VERSION: &str = "2.4.6";

/// Fixed Tencent endpoint used to create and initially poll login QR codes.
pub const WEIXIN_ILINK_BASE_URL: &str = "https://ilinkai.weixin.qq.com";

const PRIMARY_ILINK_HOST: &str = "ilinkai.weixin.qq.com";
const TENCENT_ILINK_HOST_SUFFIX: &str = "qq.com";

/// Configuration used to build an [`IlinkClient`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IlinkClientOptions {
    app_id: String,
    bot_type: String,
    protocol_version: String,
    bot_agent: String,
    base_url: String,
    allowed_hosts: Vec<String>,
    allowed_host_suffixes: Vec<String>,
}

impl IlinkClientOptions {
    /// Build the Tencent-compatible defaults for a product-specific bot agent.
    ///
    /// `bot_agent` identifies the host product for observability. It does not
    /// replace Tencent's fixed `iLink-App-Id` protocol header.
    pub fn weixin(bot_agent: impl Into<String>) -> Self {
        Self {
            app_id: WEIXIN_ILINK_APP_ID.to_string(),
            bot_type: WEIXIN_ILINK_BOT_TYPE.to_string(),
            protocol_version: WEIXIN_ILINK_PROTOCOL_VERSION.to_string(),
            bot_agent: bot_agent.into(),
            base_url: WEIXIN_ILINK_BASE_URL.to_string(),
            allowed_hosts: vec![PRIMARY_ILINK_HOST.to_string()],
            allowed_host_suffixes: vec![TENCENT_ILINK_HOST_SUFFIX.to_string()],
        }
    }

    /// Add an exact HTTPS host that Tencent may return for account routing.
    pub fn with_allowed_host(mut self, host: impl Into<String>) -> Self {
        let host = host.into();
        if !self
            .allowed_hosts
            .iter()
            .any(|current| current.eq_ignore_ascii_case(&host))
        {
            self.allowed_hosts.push(host);
        }
        self
    }
}

impl IlinkClient {
    /// Build a concrete iLink client from validated options.
    pub fn from_options(options: IlinkClientOptions) -> Result<Self, IlinkError> {
        let identity = client::IlinkClientIdentity::new(
            options.app_id,
            options.bot_type,
            &options.protocol_version,
            options.bot_agent,
        )?;
        let host_policy = url_policy::IlinkHostPolicy::production_with_suffixes(
            &options.allowed_hosts,
            &options.allowed_host_suffixes,
        )?;
        Self::new(identity, host_policy, &options.base_url)
    }

    /// Build the Tencent-compatible Weixin client used by product hosts.
    pub fn weixin(bot_agent: impl Into<String>) -> Result<Self, IlinkError> {
        Self::from_options(IlinkClientOptions::weixin(bot_agent))
    }
}

/// A3S Boot module that exports one validated [`IlinkClient`] provider.
#[derive(Clone, Debug)]
pub struct IlinkModule {
    options: IlinkClientOptions,
}

impl IlinkModule {
    pub fn new(options: IlinkClientOptions) -> Self {
        Self { options }
    }

    pub fn weixin(bot_agent: impl Into<String>) -> Self {
        Self::new(IlinkClientOptions::weixin(bot_agent))
    }
}

impl Module for IlinkModule {
    fn name(&self) -> &'static str {
        "a3s-boot-ilink"
    }

    fn providers(&self) -> BootResult<Vec<ProviderDefinition>> {
        let client = IlinkClient::from_options(self.options.clone()).map_err(|error| {
            BootError::Internal(format!("failed to configure iLink client: {error}"))
        })?;
        Ok(vec![ProviderDefinition::from_arc(Arc::new(client))])
    }

    fn exports(&self) -> BootResult<Vec<ProviderToken>> {
        Ok(vec![ProviderToken::of::<IlinkClient>()])
    }
}

#[cfg(test)]
mod tests;
