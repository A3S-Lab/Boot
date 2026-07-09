mod application;
mod builder;
mod context;
mod factory;
mod handle;
mod lazy;
mod microservice;
mod registration;
#[cfg(feature = "shutdown-hooks")]
mod shutdown;

pub use application::{BootApplication, RouteMatch};
pub use builder::BootApplicationBuilder;
pub use context::BootApplicationContext;
pub use factory::BootFactory;
pub use handle::BootApplicationHandle;
pub use lazy::{LazyLoadedModule, LazyModuleLoader};
pub use microservice::BootMicroservice;
#[cfg(feature = "shutdown-hooks")]
pub use shutdown::{wait_for_shutdown_signal, ShutdownSignal};
