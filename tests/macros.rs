#![cfg(feature = "macros")]

use std::sync::Arc;

use a3s_boot::{
    controller, injectable, BootApplication, BootError, BootRequest, BootResponse,
    ControllerDefinition, Module, ModuleRef, ProviderDefinition, Result, SseEvent,
};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};

#[injectable]
#[derive(Debug)]
struct MacroCatsService;

impl MacroCatsService {
    fn find_one(&self, id: &str) -> MacroCatDto {
        MacroCatDto {
            id: id.to_string(),
            name: "Milo".to_string(),
        }
    }

    fn create(&self, dto: MacroCreateCatDto) -> MacroCatDto {
        MacroCatDto {
            id: "generated".to_string(),
            name: dto.name,
        }
    }
}

#[derive(Debug)]
struct MacroCatsController {
    cats: Arc<MacroCatsService>,
}

#[controller("/macro-cats")]
impl MacroCatsController {
    #[get("/{id}", raw)]
    async fn find_one_text(&self, request: BootRequest) -> Result<BootResponse> {
        let id = request.param("id").unwrap_or("unknown");
        let cat = self.cats.find_one(id);
        Ok(BootResponse::text(format!("{}:{}", cat.id, cat.name)))
    }

    #[get("/{id}/json")]
    async fn find_one_json(&self, request: BootRequest) -> Result<MacroCatDto> {
        let id = request.param("id").unwrap_or("unknown");
        Ok(self.cats.find_one(id))
    }

    #[post("/", status = 201)]
    async fn create(&self, dto: MacroCreateCatDto) -> Result<MacroCatDto> {
        Ok(self.cats.create(dto))
    }

    #[sse("/events")]
    async fn events(&self) -> Result<impl futures_core::Stream<Item = Result<SseEvent>>> {
        Ok(futures_util::stream::iter([Ok::<_, BootError>(
            SseEvent::new("Milo").with_event("cat.found"),
        )]))
    }
}

#[derive(Debug, Deserialize, Serialize)]
struct MacroCreateCatDto {
    name: String,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
struct MacroCatDto {
    id: String,
    name: String,
}

#[derive(Debug)]
struct MacroCatsModule;

impl Module for MacroCatsModule {
    fn name(&self) -> &'static str {
        "macro-cats"
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        Ok(vec![MacroCatsService.into_provider()])
    }

    fn controllers(&self, module_ref: &ModuleRef) -> Result<Vec<ControllerDefinition>> {
        let cats = module_ref.get::<MacroCatsService>()?;
        Ok(vec![Arc::new(MacroCatsController { cats }).controller()?])
    }
}

#[tokio::test]
async fn macros_register_injectable_services_and_controller_routes() {
    let app = BootApplication::builder()
        .import(MacroCatsModule)
        .build()
        .unwrap();

    assert_eq!(app.routes().len(), 4);

    let text = app
        .call(BootRequest::new(
            a3s_boot::HttpMethod::Get,
            "/macro-cats/42",
        ))
        .await
        .unwrap();
    assert_eq!(text.body_text().unwrap(), "42:Milo");

    let json = app
        .call(BootRequest::new(
            a3s_boot::HttpMethod::Get,
            "/macro-cats/42/json",
        ))
        .await
        .unwrap();
    assert_eq!(
        json.body_json::<MacroCatDto>().unwrap(),
        MacroCatDto {
            id: "42".to_string(),
            name: "Milo".to_string(),
        }
    );

    let create = BootRequest::new(a3s_boot::HttpMethod::Post, "/macro-cats")
        .with_json(&MacroCreateCatDto {
            name: "Luna".to_string(),
        })
        .unwrap();
    let created = app.call(create).await.unwrap();

    assert_eq!(created.status(), 201);
    assert_eq!(
        created.body_json::<MacroCatDto>().unwrap(),
        MacroCatDto {
            id: "generated".to_string(),
            name: "Luna".to_string(),
        }
    );

    let events = app
        .call(
            BootRequest::new(a3s_boot::HttpMethod::Get, "/macro-cats/events")
                .with_header("accept", "text/event-stream"),
        )
        .await
        .unwrap();
    let mut stream = events.into_sse_stream().unwrap();

    assert_eq!(
        String::from_utf8(stream.next().await.unwrap().unwrap().encode()).unwrap(),
        "event: cat.found\ndata: Milo\n\n"
    );
    assert!(stream.next().await.is_none());
}
