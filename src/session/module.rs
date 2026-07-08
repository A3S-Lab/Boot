use super::manager::SessionManager;
use super::options::SessionOptions;
use crate::{Module, ProviderDefinition, ProviderToken, Result};
use std::fmt;
use std::sync::Arc;

/// Module that registers and exports a [`SessionManager`] provider.
#[derive(Clone)]
pub struct SessionModule {
    name: &'static str,
    token: ProviderToken,
    manager: SessionManager,
    global: bool,
}

impl fmt::Debug for SessionModule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SessionModule")
            .field("name", &self.name)
            .field("token", &self.token)
            .field("manager", &self.manager)
            .field("global", &self.global)
            .finish_non_exhaustive()
    }
}

impl SessionModule {
    pub fn in_memory(name: &'static str) -> Self {
        Self::from_manager(name, SessionManager::in_memory(SessionOptions::new()))
    }

    pub fn in_memory_with_options(name: &'static str, options: SessionOptions) -> Self {
        Self::from_manager(name, SessionManager::in_memory(options))
    }

    pub fn from_manager(name: &'static str, manager: SessionManager) -> Self {
        Self {
            name,
            token: ProviderToken::of::<SessionManager>(),
            manager,
            global: false,
        }
    }

    pub fn manager(&self) -> SessionManager {
        self.manager.clone()
    }

    pub fn named(mut self, token: impl Into<String>) -> Self {
        self.token = ProviderToken::named(token);
        self
    }

    pub fn global(mut self) -> Self {
        self.global = true;
        self
    }
}

impl Module for SessionModule {
    fn name(&self) -> &'static str {
        self.name
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        Ok(vec![ProviderDefinition::named_from_arc(
            self.token.as_str(),
            Arc::new(self.manager.clone()),
        )])
    }

    fn exports(&self) -> Result<Vec<ProviderToken>> {
        Ok(vec![self.token.clone()])
    }

    fn is_global(&self) -> bool {
        self.global
    }
}
