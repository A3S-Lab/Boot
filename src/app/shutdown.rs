use crate::{BootError, BoxFuture, Result};
use futures_util::StreamExt;
use std::collections::BTreeSet;
use std::fmt;

/// Shutdown signal names understood by Nest-style shutdown hooks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ShutdownSignal {
    Sigint,
    Sigterm,
    Sigquit,
    Sighup,
    Sigusr2,
}

impl ShutdownSignal {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Sigint => "SIGINT",
            Self::Sigterm => "SIGTERM",
            Self::Sigquit => "SIGQUIT",
            Self::Sighup => "SIGHUP",
            Self::Sigusr2 => "SIGUSR2",
        }
    }

    pub fn default_signals() -> Vec<Self> {
        vec![Self::Sigint, Self::Sigterm]
    }
}

impl fmt::Display for ShutdownSignal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Wait for one configured operating-system shutdown signal.
pub async fn wait_for_shutdown_signal<I>(signals: I) -> Result<ShutdownSignal>
where
    I: IntoIterator<Item = ShutdownSignal>,
{
    let signals = normalize_shutdown_signals(signals);
    let mut futures = futures_util::stream::FuturesUnordered::new();
    for signal in signals {
        futures.push(shutdown_signal_future(signal)?);
    }

    futures
        .next()
        .await
        .unwrap_or_else(|| Err(BootError::Internal("no shutdown signals configured".into())))
}

pub(crate) fn normalize_shutdown_signals<I>(signals: I) -> Vec<ShutdownSignal>
where
    I: IntoIterator<Item = ShutdownSignal>,
{
    let signals = signals.into_iter().collect::<BTreeSet<_>>();
    if signals.is_empty() {
        ShutdownSignal::default_signals()
    } else {
        signals.into_iter().collect()
    }
}

fn shutdown_signal_future(
    signal: ShutdownSignal,
) -> Result<BoxFuture<'static, Result<ShutdownSignal>>> {
    match signal {
        ShutdownSignal::Sigint => Ok(Box::pin(async move {
            tokio::signal::ctrl_c().await?;
            Ok(ShutdownSignal::Sigint)
        })),
        #[cfg(unix)]
        ShutdownSignal::Sigterm => unix_shutdown_signal_future(
            ShutdownSignal::Sigterm,
            tokio::signal::unix::SignalKind::terminate(),
        ),
        #[cfg(unix)]
        ShutdownSignal::Sigquit => unix_shutdown_signal_future(
            ShutdownSignal::Sigquit,
            tokio::signal::unix::SignalKind::quit(),
        ),
        #[cfg(unix)]
        ShutdownSignal::Sighup => unix_shutdown_signal_future(
            ShutdownSignal::Sighup,
            tokio::signal::unix::SignalKind::hangup(),
        ),
        #[cfg(unix)]
        ShutdownSignal::Sigusr2 => unix_shutdown_signal_future(
            ShutdownSignal::Sigusr2,
            tokio::signal::unix::SignalKind::user_defined2(),
        ),
        #[cfg(not(unix))]
        signal => Ok(Box::pin(async move {
            Err(BootError::Internal(format!(
                "shutdown signal {signal} is only supported on Unix platforms"
            )))
        })),
    }
}

#[cfg(unix)]
fn unix_shutdown_signal_future(
    signal: ShutdownSignal,
    kind: tokio::signal::unix::SignalKind,
) -> Result<BoxFuture<'static, Result<ShutdownSignal>>> {
    let mut stream = tokio::signal::unix::signal(kind)?;
    Ok(Box::pin(async move {
        let _ = stream.recv().await;
        Ok(signal)
    }))
}
