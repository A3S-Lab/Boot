use a3s_boot::{
    BootApplication, BootRequest, BootResponse, ControllerDefinition, HttpMethod, Module,
    ModuleRef, Result, RouteDefinition, StringTemplateViewEngine, ViewModule, ViewRenderer,
};
use serde::Serialize;
use serde_json::json;
use std::sync::Arc;

#[derive(Debug, Serialize)]
struct CatView {
    id: String,
    name: String,
}

#[tokio::test]
async fn view_renderer_renders_string_templates_to_html_responses() {
    let renderer = ViewRenderer::new(
        StringTemplateViewEngine::new()
            .with_template("cats/show", "<h1>{{ name }}</h1><p>{{ id }}</p>"),
    );

    let response = renderer
        .render_response(
            "cats/show",
            &CatView {
                id: "cat-1".to_string(),
                name: "Milo".to_string(),
            },
        )
        .await
        .unwrap();

    assert_eq!(response.status(), 200);
    assert_eq!(response.content_type(), Some("text/html; charset=utf-8"));
    assert_eq!(response.body_text().unwrap(), "<h1>Milo</h1><p>cat-1</p>");
}

#[tokio::test]
async fn route_view_helpers_resolve_view_renderer_from_request_scope() {
    let app = BootApplication::builder()
        .import(
            ViewModule::new(
                "views",
                StringTemplateViewEngine::new().with_template(
                    "cats/show",
                    "<h1>{{ cat.name }}</h1><p>{{ request_id }}</p>",
                ),
            )
            .global(),
        )
        .route(
            RouteDefinition::get_view(
                "/cats/{id}",
                "cats/show",
                |request: BootRequest| async move {
                    Ok(json!({
                        "request_id": request.header("x-request-id").unwrap_or("missing"),
                        "cat": {
                            "id": request.param("id").unwrap_or("unknown"),
                            "name": "Milo",
                        }
                    }))
                },
            )
            .unwrap(),
        )
        .build()
        .unwrap();

    let response = app
        .call(BootRequest::new(HttpMethod::Get, "/cats/1").with_header("x-request-id", "request-1"))
        .await
        .unwrap();

    assert_eq!(response.status(), 200);
    assert_eq!(response.content_type(), Some("text/html; charset=utf-8"));
    assert_eq!(
        response.body_text().unwrap(),
        "<h1>Milo</h1><p>request-1</p>"
    );
}

#[tokio::test]
async fn view_module_exports_renderer_to_importing_modules() {
    #[derive(Debug)]
    struct CatsModule {
        views: ViewModule,
    }

    impl Module for CatsModule {
        fn name(&self) -> &'static str {
            "cats"
        }

        fn imports(&self) -> Vec<Arc<dyn Module>> {
            vec![Arc::new(self.views.clone())]
        }

        fn controllers(&self, _module_ref: &ModuleRef) -> Result<Vec<ControllerDefinition>> {
            Ok(vec![ControllerDefinition::new("/cats")?.get_view(
                "/",
                "cats/index",
                |_| async {
                    Ok(CatView {
                        id: "cat-1".to_string(),
                        name: "Milo".to_string(),
                    })
                },
            )?])
        }
    }

    let app = BootApplication::builder()
        .import(CatsModule {
            views: ViewModule::new(
                "views",
                StringTemplateViewEngine::new()
                    .with_template("cats/index", "<h1>{{ name }}</h1><small>{{ id }}</small>"),
            ),
        })
        .build()
        .unwrap();

    let response = app
        .call(BootRequest::new(HttpMethod::Get, "/cats"))
        .await
        .unwrap();

    assert_eq!(
        response.body_text().unwrap(),
        "<h1>Milo</h1><small>cat-1</small>"
    );
}

#[tokio::test]
async fn view_routes_report_missing_renderers() {
    let app = BootApplication::builder()
        .route(
            RouteDefinition::get_view("/", "home", |_| async { Ok(json!({ "name": "Milo" })) })
                .unwrap(),
        )
        .build()
        .unwrap();

    let error = app
        .call(BootRequest::new(HttpMethod::Get, "/"))
        .await
        .unwrap_err();

    assert!(error.to_string().contains("a3s_boot::view::ViewRenderer"));
}

#[test]
fn html_responses_use_html_content_type() {
    let response = BootResponse::html("<h1>Milo</h1>");

    assert_eq!(response.status(), 200);
    assert_eq!(response.content_type(), Some("text/html; charset=utf-8"));
    assert_eq!(response.body(), b"<h1>Milo</h1>");
}
