#![cfg(feature = "macros")]

use std::sync::Arc;

use a3s_boot::{
    controller, injectable, BootApplication, BootRequest, BootResponse, ControllerDefinition,
    CookieOptions, HttpMethod, Module, ResponsePassthrough, Result,
};
#[allow(unused_imports)]
use a3s_boot::{get, res};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
struct ReplyDto {
    message: String,
}

#[injectable]
#[derive(Debug)]
struct ResponseController;

#[controller("/response")]
impl ResponseController {
    #[get("/json")]
    async fn json(&self, #[res] response: ResponsePassthrough) -> Result<ReplyDto> {
        response.set_status(202)?;
        response.set_header("x-response-mode", "passthrough")?;
        response.set_cookie("seen", "yes", CookieOptions::new().with_path("/"))?;
        response.delete_cookie("old", CookieOptions::new().with_path("/"))?;
        Ok(ReplyDto {
            message: "json".to_string(),
        })
    }

    #[get("/raw", raw)]
    async fn raw(&self, #[res] response: ResponsePassthrough) -> Result<BootResponse> {
        response
            .status(203)?
            .header("x-response-mode", "raw-passthrough")?;
        Ok(BootResponse::text("raw"))
    }
}

#[derive(Debug)]
struct ResponseFeatureModule;

impl Module for ResponseFeatureModule {
    fn name(&self) -> &'static str {
        "response-feature"
    }

    fn controllers(&self, _module_ref: &a3s_boot::ModuleRef) -> Result<Vec<ControllerDefinition>> {
        Ok(vec![Arc::new(ResponseController).controller()?])
    }
}

#[tokio::test]
async fn res_macro_applies_passthrough_status_headers_and_cookies_to_json_routes() {
    let app = BootApplication::builder()
        .import(ResponseFeatureModule)
        .build()
        .unwrap();

    let response = app
        .call(
            BootRequest::new(HttpMethod::Get, "/response/json")
                .with_header("accept", "application/json"),
        )
        .await
        .unwrap();

    assert_eq!(response.status, 202);
    assert_eq!(response.header("x-response-mode"), Some("passthrough"));
    assert_eq!(
        response.body_json::<ReplyDto>().unwrap(),
        ReplyDto {
            message: "json".to_string()
        }
    );

    let cookies = response.header_values("set-cookie");
    assert_eq!(cookies.len(), 2);
    assert!(cookies.iter().any(|cookie| cookie.starts_with("seen=yes;")));
    assert!(cookies.iter().any(|cookie| cookie.starts_with("old=;")));
}

#[tokio::test]
async fn res_macro_applies_passthrough_status_and_headers_to_raw_routes() {
    let app = BootApplication::builder()
        .import(ResponseFeatureModule)
        .build()
        .unwrap();

    let response = app
        .call(BootRequest::new(HttpMethod::Get, "/response/raw"))
        .await
        .unwrap();

    assert_eq!(response.status, 203);
    assert_eq!(response.header("x-response-mode"), Some("raw-passthrough"));
    assert_eq!(response.body_text().unwrap(), "raw");
}

#[test]
fn response_passthrough_rejects_invalid_response_metadata() {
    let response = ResponsePassthrough::new();

    assert!(response.set_status(99).is_err());
    assert!(response.set_header("bad header", "value").is_err());
}
