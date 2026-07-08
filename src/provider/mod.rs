mod definition;
mod module_ref;
mod token;

use std::any::Any;

pub use definition::{
    FromModuleRef, ProviderBeforeApplicationShutdown, ProviderDefinition,
    ProviderOnApplicationBootstrap, ProviderOnApplicationShutdown, ProviderOnModuleDestroy,
    ProviderOnModuleInit, ProviderScope,
};
pub use module_ref::{ModuleRef, ProviderRef};
pub use token::ProviderToken;

pub(crate) type AnyProvider = dyn Any + Send + Sync;
