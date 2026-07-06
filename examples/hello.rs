use a3s_boot::{AxumAdapter, BootApplication, BootResponse, Module, RouteDefinition};

#[derive(Debug)]
struct AppModule;

impl Module for AppModule {
    fn name(&self) -> &'static str {
        "app"
    }

    fn routes(&self) -> a3s_boot::Result<Vec<RouteDefinition>> {
        Ok(vec![RouteDefinition::get("/", |_| async {
            Ok(BootResponse::text("Hello from A3S Boot"))
        })?])
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> a3s_boot::Result<()> {
    let app = BootApplication::builder().import(AppModule).build()?;
    app.serve_with(&AxumAdapter::new(), ([127, 0, 0, 1], 3000).into())
        .await
}
