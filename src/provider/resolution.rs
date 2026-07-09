use super::ProviderToken;
use crate::{BootError, Result};
use std::sync::{Arc, RwLock};

pub(crate) type ProviderResolutionStack = Arc<RwLock<Vec<ProviderToken>>>;

pub(crate) fn new_resolution_stack() -> ProviderResolutionStack {
    Arc::new(RwLock::new(Vec::new()))
}

pub(crate) fn enter_resolution_stack(
    resolution_stack: &ProviderResolutionStack,
    token: &ProviderToken,
) -> Result<()> {
    let mut stack = resolution_stack.write().map_err(|_| {
        BootError::Internal("provider resolution stack lock is poisoned".to_string())
    })?;
    if let Some(index) = stack.iter().position(|active| active == token) {
        let mut chain = stack[index..].to_vec();
        chain.push(token.clone());
        let chain = chain
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(" -> ");
        return Err(BootError::Internal(format!(
            "cyclic provider dependency detected: {chain}"
        )));
    }

    stack.push(token.clone());
    Ok(())
}

pub(crate) fn exit_resolution_stack(resolution_stack: &ProviderResolutionStack) -> Result<()> {
    let mut stack = resolution_stack.write().map_err(|_| {
        BootError::Internal("provider resolution stack lock is poisoned".to_string())
    })?;
    stack
        .pop()
        .ok_or_else(|| BootError::Internal("provider resolution stack underflow".to_string()))?;
    Ok(())
}
