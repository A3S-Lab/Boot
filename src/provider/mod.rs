mod cache;
mod definition;
mod entry;
mod module_ref;
mod provider_ref;
mod resolution;
mod token;

use std::any::Any;

pub use definition::{
    FromModuleRef, ProviderBeforeApplicationShutdown, ProviderDefinition,
    ProviderOnApplicationBootstrap, ProviderOnApplicationShutdown, ProviderOnModuleDestroy,
    ProviderOnModuleInit, ProviderScope,
};
pub use module_ref::ModuleRef;
pub use provider_ref::ProviderRef;
pub use token::ProviderToken;

pub(crate) type AnyProvider = dyn Any + Send + Sync;
