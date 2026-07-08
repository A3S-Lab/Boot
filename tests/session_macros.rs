#![cfg(all(feature = "macros", feature = "session"))]

use std::sync::Arc;
use std::time::Duration;

use a3s_boot::{
    controller, injectable, BootApplication, BootRequest, ControllerDefinition, HttpMethod, Module,
    Result, Session, SessionManager, SessionModule, SessionOptions,
};
#[allow(unused_imports)]
use a3s_boot::{get, session};

#[injectable]
#[derive(Debug)]
struct SessionController;

#[controller("/sessions")]
impl SessionController {
    #[get("/login")]
    async fn login(&self, #[session] session: Session) -> Result<String> {
        session.set("user_id", &"u1")?;
        Ok(session.id().to_string())
    }

    #[get("/profile")]
    async fn profile(&self, #[session] session: Session) -> Result<String> {
        let user_id = session
            .get::<String>("user_id")?
            .unwrap_or_else(|| "anonymous".to_string());
        Ok(user_id)
    }

    #[get("/optional")]
    async fn optional(&self, #[session] session: Option<Session>) -> Result<String> {
        let label = if session.is_some() { "session" } else { "none" };
        Ok(label.to_string())
    }

    #[get("/logout")]
    async fn logout(&self, #[session] session: Session) -> Result<String> {
        session.destroy()?;
        Ok("logged out".to_string())
    }
}

#[derive(Debug)]
struct SessionFeatureModule;

impl Module for SessionFeatureModule {
    fn name(&self) -> &'static str {
        "session-feature"
    }

    fn controllers(&self, _module_ref: &a3s_boot::ModuleRef) -> Result<Vec<ControllerDefinition>> {
        Ok(vec![Arc::new(SessionController).controller()?])
    }
}

#[tokio::test]
async fn session_macro_extracts_request_bound_session_handles() {
    let manager = SessionManager::in_memory(
        SessionOptions::new()
            .with_cookie_name("sid")
            .with_ttl(Duration::from_secs(60)),
    );
    let app = BootApplication::builder()
        .use_global_session_module(SessionModule::from_manager("sessions", manager))
        .import(SessionFeatureModule)
        .build()
        .unwrap();

    let login = app
        .call(BootRequest::new(HttpMethod::Get, "/sessions/login"))
        .await
        .unwrap();
    let cookie = cookie_pair(login.header_values("set-cookie")[0]);
    let profile = app
        .call(BootRequest::new(HttpMethod::Get, "/sessions/profile").with_header("cookie", cookie))
        .await
        .unwrap();

    assert_eq!(profile.body_json::<String>().unwrap(), "u1");
    assert!(login.header_values("set-cookie")[0].starts_with("sid="));
}

#[tokio::test]
async fn session_macro_supports_optional_sessions() {
    let app_without_sessions = BootApplication::builder()
        .import(SessionFeatureModule)
        .build()
        .unwrap();
    let missing = app_without_sessions
        .call(BootRequest::new(HttpMethod::Get, "/sessions/optional"))
        .await
        .unwrap();
    assert_eq!(missing.body_json::<String>().unwrap(), "none");

    let app_with_sessions = BootApplication::builder()
        .use_global_session_module(SessionModule::in_memory("sessions"))
        .import(SessionFeatureModule)
        .build()
        .unwrap();
    let present = app_with_sessions
        .call(BootRequest::new(HttpMethod::Get, "/sessions/optional"))
        .await
        .unwrap();
    assert_eq!(present.body_json::<String>().unwrap(), "session");
}

#[tokio::test]
async fn session_macro_destroy_clears_session_cookie() {
    let app = BootApplication::builder()
        .use_global_session_module(SessionModule::in_memory_with_options(
            "sessions",
            SessionOptions::new().with_cookie_name("sid"),
        ))
        .import(SessionFeatureModule)
        .build()
        .unwrap();

    let login = app
        .call(BootRequest::new(HttpMethod::Get, "/sessions/login"))
        .await
        .unwrap();
    let cookie = cookie_pair(login.header_values("set-cookie")[0]);
    let logout = app
        .call(BootRequest::new(HttpMethod::Get, "/sessions/logout").with_header("cookie", cookie))
        .await
        .unwrap();

    assert_eq!(logout.body_json::<String>().unwrap(), "logged out");
    assert!(logout.header_values("set-cookie")[0].starts_with("sid=;"));
    assert!(logout.header_values("set-cookie")[0].contains("Max-Age=0"));
}

fn cookie_pair(set_cookie: &str) -> String {
    set_cookie
        .split(';')
        .next()
        .expect("set-cookie header should contain a cookie pair")
        .to_string()
}
