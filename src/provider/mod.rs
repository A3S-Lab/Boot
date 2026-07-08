mod definition;
mod module_ref;
mod token;

use std::any::Any;

pub use definition::{
    FromModuleRef, ProviderDefinition, ProviderOnApplicationBootstrap,
    ProviderOnApplicationShutdown, ProviderOnModuleInit, ProviderScope,
};
pub use module_ref::ModuleRef;
pub use token::ProviderToken;

pub(crate) type AnyProvider = dyn Any + Send + Sync;
