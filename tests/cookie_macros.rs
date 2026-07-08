#![cfg(feature = "macros")]

use std::collections::BTreeMap;
use std::sync::Arc;

use a3s_boot::{
    controller, injectable, BootApplication, BootError, BootRequest, ControllerDefinition,
    HttpMethod, Module, OpenApiInfo, ParseIntPipe, Result,
};
#[allow(unused_imports)]
use a3s_boot::{cookie, cookies, get};
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
struct CookieReply {
    session: String,
    theme: Option<String>,
    page: u16,
    kind: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
struct CookiesReply {
    session: Option<String>,
    flag: Option<String>,
    count: usize,
}

fn normalize_cookie_kind(value: String) -> Result<String> {
    let value = value.trim();
    if value.is_empty() {
        return Err(BootError::BadRequest("cookie kind is required".to_string()));
    }

    Ok(value.to_ascii_uppercase())
}

#[injectable]
#[derive(Debug)]
struct CookieController;

#[controller("/cookies")]
impl CookieController {
    #[get("/one")]
    async fn one(
        &self,
        #[cookie("session")] session: String,
        #[cookie("theme")] theme: Option<String>,
        #[cookie("page", default = 1, pipe = ParseIntPipe)] page: u16,
        #[cookie("kind", pipe = normalize_cookie_kind)] kind: Option<String>,
    ) -> Result<CookieReply> {
        Ok(CookieReply {
            session,
            theme,
            page,
            kind,
        })
    }

    #[get("/all")]
    async fn all(&self, #[cookies] cookies: BTreeMap<String, String>) -> Result<CookiesReply> {
        Ok(CookiesReply {
            session: cookies.get("session").cloned(),
            flag: cookies.get("flag").cloned(),
            count: cookies.len(),
        })
    }
}

#[derive(Debug)]
struct CookieFeatureModule;

impl Module for CookieFeatureModule {
    fn name(&self) -> &'static str {
        "cookie-feature"
    }

    fn controllers(&self, _module_ref: &a3s_boot::ModuleRef) -> Result<Vec<ControllerDefinition>> {
        Ok(vec![Arc::new(CookieController).controller()?])
    }
}

fn cookie_app() -> BootApplication {
    BootApplication::builder()
        .import(CookieFeatureModule)
        .build()
        .unwrap()
}

#[tokio::test]
async fn cookie_macro_extracts_named_cookie_values() {
    let response = cookie_app()
        .call(
            BootRequest::new(HttpMethod::Get, "/cookies/one")
                .with_header("cookie", "session=abc; kind=tabby"),
        )
        .await
        .unwrap();

    assert_eq!(
        response.body_json::<CookieReply>().unwrap(),
        CookieReply {
            session: "abc".to_string(),
            theme: None,
            page: 1,
            kind: Some("TABBY".to_string()),
        }
    );
}

#[tokio::test]
async fn cookies_macro_extracts_all_request_cookies() {
    let response = cookie_app()
        .call(
            BootRequest::new(HttpMethod::Get, "/cookies/all")
                .with_header("cookie", "session=abc; theme=dark")
                .append_header("cookie", "flag=true"),
        )
        .await
        .unwrap();

    assert_eq!(
        response.body_json::<CookiesReply>().unwrap(),
        CookiesReply {
            session: Some("abc".to_string()),
            flag: Some("true".to_string()),
            count: 3,
        }
    );
}

#[tokio::test]
async fn cookie_macro_rejects_missing_required_cookies() {
    let error = cookie_app()
        .call(BootRequest::new(HttpMethod::Get, "/cookies/one"))
        .await
        .unwrap_err();

    assert!(
        matches!(error, BootError::BadRequest(message) if message == "missing cookie: session")
    );
}

#[test]
fn cookie_macro_documents_cookie_parameters() {
    let document = cookie_app().openapi(OpenApiInfo::new("Cookies", "1.0.0"));
    let value = serde_json::to_value(document).unwrap();
    let operation = &value["paths"]["/cookies/one"]["get"];

    assert!(operation["parameters"]
        .as_array()
        .unwrap()
        .iter()
        .any(|parameter| {
            parameter["name"] == "session"
                && parameter["in"] == "cookie"
                && parameter["required"] == true
                && parameter["schema"] == json!({ "type": "string" })
        }));
}
