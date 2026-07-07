use a3s_boot::{
    BootRequest, BootResponse, ControllerDefinition, HttpMethod, Module, ModuleRef,
    ProviderDefinition, Result, RouteDefinition, TestingModule,
};
use std::sync::Arc;

#[derive(Debug)]
struct GreetingService {
    message: &'static str,
}

#[derive(Debug)]
struct GreetingModule;

impl Module for GreetingModule {
    fn name(&self) -> &'static str {
        "GreetingModule"
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        Ok(vec![ProviderDefinition::singleton(GreetingService {
            message: "real",
        })])
    }

    fn controllers(&self, module_ref: &ModuleRef) -> Result<Vec<ControllerDefinition>> {
        let service = module_ref.get::<GreetingService>()?;
        Ok(vec![ControllerDefinition::new("/greetings")?.route(
            RouteDefinition::get("/", move |_| {
                let service = Arc::clone(&service);
                async move { Ok(BootResponse::text(service.message)) }
            })?,
        )?])
    }
}

#[tokio::test]
async fn testing_module_overrides_providers_before_controllers_are_built() {
    let testing = TestingModule::builder()
        .import(GreetingModule)
        .override_provider(ProviderDefinition::singleton(GreetingService {
            message: "fake",
        }))
        .compile()
        .unwrap();

    assert_eq!(testing.get::<GreetingService>().unwrap().message, "fake");

    let response = testing
        .call(BootRequest::new(HttpMethod::Get, "/greetings"))
        .await
        .unwrap();

    assert_eq!(response.body_text().unwrap(), "fake");
}

#[tokio::test]
async fn testing_module_can_compile_direct_test_routes_and_providers() {
    let testing = TestingModule::builder()
        .provider(ProviderDefinition::singleton(GreetingService {
            message: "direct",
        }))
        .route(
            RouteDefinition::get("/probe", |_| async {
                BootResponse::json(&serde_json::json!({ "ok": true }))
            })
            .unwrap(),
        )
        .compile()
        .unwrap();

    assert_eq!(testing.get::<GreetingService>().unwrap().message, "direct");

    let response = testing
        .call(BootRequest::new(HttpMethod::Get, "/probe"))
        .await
        .unwrap();

    assert!(response.is_json_content_type());
    assert_eq!(
        response.body_json::<serde_json::Value>().unwrap(),
        serde_json::json!({ "ok": true })
    );
}
