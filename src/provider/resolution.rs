use super::cache::ProviderCacheKey;
use super::ProviderToken;
use crate::{BootError, Result};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[derive(Clone)]
pub(crate) struct ProviderResolutionFrame {
    cache_key: ProviderCacheKey,
    token: ProviderToken,
    active: Arc<AtomicBool>,
}

impl ProviderResolutionFrame {
    fn is_active(&self) -> bool {
        self.active.load(Ordering::Acquire)
    }
}

pub(crate) type ProviderResolutionStack = Arc<Vec<ProviderResolutionFrame>>;

pub(crate) struct ProviderResolutionGuard {
    active: Arc<AtomicBool>,
}

impl Drop for ProviderResolutionGuard {
    fn drop(&mut self) {
        self.active.store(false, Ordering::Release);
    }
}

pub(crate) fn new_resolution_stack() -> ProviderResolutionStack {
    Arc::new(Vec::new())
}

pub(crate) fn resolution_stack_is_empty(resolution_stack: &ProviderResolutionStack) -> bool {
    !resolution_stack
        .iter()
        .any(ProviderResolutionFrame::is_active)
}

pub(crate) fn ensure_not_resolving(
    resolution_stack: &ProviderResolutionStack,
    cache_key: ProviderCacheKey,
    token: &ProviderToken,
) -> Result<()> {
    let active_frames = resolution_stack
        .iter()
        .filter(|frame| frame.is_active())
        .collect::<Vec<_>>();
    if let Some(index) = active_frames
        .iter()
        .position(|active| active.cache_key == cache_key)
    {
        let mut chain = active_frames[index..]
            .iter()
            .map(|frame| frame.token.to_string())
            .collect::<Vec<_>>();
        chain.push(token.to_string());
        return Err(BootError::Internal(format!(
            "cyclic provider dependency detected: {}",
            chain.join(" -> ")
        )));
    }

    Ok(())
}

pub(crate) fn enter_resolution_stack(
    resolution_stack: &ProviderResolutionStack,
    cache_key: ProviderCacheKey,
    token: &ProviderToken,
) -> Result<(ProviderResolutionStack, ProviderResolutionGuard)> {
    ensure_not_resolving(resolution_stack, cache_key, token)?;

    let active = Arc::new(AtomicBool::new(true));
    let mut frames = resolution_stack
        .iter()
        .filter(|frame| frame.is_active())
        .cloned()
        .collect::<Vec<_>>();
    frames.push(ProviderResolutionFrame {
        cache_key,
        token: token.clone(),
        active: Arc::clone(&active),
    });
    Ok((Arc::new(frames), ProviderResolutionGuard { active }))
}

pub(crate) fn resolution_chain_with(
    resolution_stack: &ProviderResolutionStack,
    token: &ProviderToken,
) -> String {
    let mut chain = resolution_stack
        .iter()
        .filter(|frame| frame.is_active())
        .map(|frame| frame.token.clone())
        .collect::<Vec<_>>();
    chain.push(token.clone());
    chain
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(" -> ")
}
