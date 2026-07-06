use a3s_boot::{
    AxumAdapter, BootApplication, BootResponse, ControllerDefinition, Module, ModuleRef,
    ProviderDefinition, Result,
};

#[derive(Debug)]
struct GreetingService;

impl GreetingService {
    fn hello(&self) -> &'static str {
        "Hello from A3S Boot"
    }
}

#[derive(Debug)]
struct AppModule;

impl Module for AppModule {
    fn name(&self) -> &'static str {
        "app"
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        Ok(vec![ProviderDefinition::singleton(GreetingService)])
    }

    fn controllers(&self, module_ref: &ModuleRef) -> Result<Vec<ControllerDefinition>> {
        let greeting = module_ref.get::<GreetingService>()?;

        Ok(vec![ControllerDefinition::new("/")?.get(
            "/",
            move |_| {
                let greeting = greeting.clone();
                async move { Ok(BootResponse::text(greeting.hello())) }
            },
        )?])
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let app = BootApplication::builder().import(AppModule).build()?;
    app.serve_with(&AxumAdapter::new(), ([127, 0, 0, 1], 3000).into())
        .await
}
