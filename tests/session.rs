#![cfg(feature = "session")]

use a3s_boot::{
    BootApplication, BootRequest, BootResponse, HttpMethod, RouteDefinition, SessionManager,
    SessionModule, SessionOptions,
};
use std::time::Duration;

#[tokio::test]
async fn session_module_persists_session_data_with_a_cookie() {
    let manager = SessionManager::in_memory(
        SessionOptions::new()
            .with_cookie_name("sid")
            .with_ttl(Duration::from_secs(60)),
    );
    let login_manager = manager.clone();
    let profile_manager = manager.clone();
    let app = BootApplication::builder()
        .use_global_session_module(SessionModule::from_manager("sessions", manager))
        .route(
            RouteDefinition::get("/login", move |request: BootRequest| {
                let manager = login_manager.clone();
                async move {
                    let session_id = manager.require_session_id(&request)?;
                    manager.set(&session_id, "user_id", &"u1")?;
                    Ok(BootResponse::text("logged in"))
                }
            })
            .unwrap(),
        )
        .route(
            RouteDefinition::get("/profile", move |request: BootRequest| {
                let manager = profile_manager.clone();
                async move {
                    let session_id = manager.require_session_id(&request)?;
                    let user_id = manager
                        .get::<String>(&session_id, "user_id")?
                        .unwrap_or_else(|| "anonymous".to_string());
                    Ok(BootResponse::text(user_id))
                }
            })
            .unwrap(),
        )
        .build()
        .unwrap();

    let login = app
        .call(BootRequest::new(HttpMethod::Get, "/login"))
        .await
        .unwrap();
    let cookie = cookie_pair(login.header_values("set-cookie")[0]);
    let profile = app
        .call(BootRequest::new(HttpMethod::Get, "/profile").with_header("cookie", cookie))
        .await
        .unwrap();

    assert_eq!(login.body_text().unwrap(), "logged in");
    assert!(login.header_values("set-cookie")[0].starts_with("sid="));
    assert!(login.header_values("set-cookie")[0].contains("HttpOnly"));
    assert_eq!(profile.body_text().unwrap(), "u1");
}

#[tokio::test]
async fn session_cookie_is_not_written_until_session_has_data() {
    let app = BootApplication::builder()
        .use_global_session_module(SessionModule::in_memory("sessions"))
        .route(RouteDefinition::get("/ping", |_| async { Ok(BootResponse::text("pong")) }).unwrap())
        .build()
        .unwrap();

    let response = app
        .call(BootRequest::new(HttpMethod::Get, "/ping"))
        .await
        .unwrap();

    assert_eq!(response.body_text().unwrap(), "pong");
    assert!(response.header_values("set-cookie").is_empty());
}

#[tokio::test]
async fn session_destroy_clears_existing_cookie() {
    let manager = SessionManager::in_memory(SessionOptions::new().with_cookie_name("sid"));
    let create_manager = manager.clone();
    let destroy_manager = manager.clone();
    let app = BootApplication::builder()
        .use_global_session_module(SessionModule::from_manager("sessions", manager))
        .route(
            RouteDefinition::get("/create", move |request: BootRequest| {
                let manager = create_manager.clone();
                async move {
                    let session_id = manager.require_session_id(&request)?;
                    manager.set(&session_id, "name", &"Milo")?;
                    Ok(BootResponse::text("created"))
                }
            })
            .unwrap(),
        )
        .route(
            RouteDefinition::get("/logout", move |request: BootRequest| {
                let manager = destroy_manager.clone();
                async move {
                    let session_id = manager.require_session_id(&request)?;
                    manager.destroy(&session_id)?;
                    Ok(BootResponse::text("logged out"))
                }
            })
            .unwrap(),
        )
        .build()
        .unwrap();

    let created = app
        .call(BootRequest::new(HttpMethod::Get, "/create"))
        .await
        .unwrap();
    let cookie = cookie_pair(created.header_values("set-cookie")[0]);
    let logout = app
        .call(BootRequest::new(HttpMethod::Get, "/logout").with_header("cookie", cookie))
        .await
        .unwrap();

    assert_eq!(logout.body_text().unwrap(), "logged out");
    assert!(logout.header_values("set-cookie")[0].starts_with("sid=;"));
    assert!(logout.header_values("set-cookie")[0].contains("Max-Age=0"));
}

#[test]
fn session_module_supports_named_and_global_exports() {
    let app = BootApplication::builder()
        .use_global_session_module(
            SessionModule::in_memory("sessions")
                .named("web-session")
                .global(),
        )
        .build()
        .unwrap();

    let manager = app.get_named::<SessionManager>("web-session").unwrap();

    assert_eq!(manager.options().cookie_name(), "a3s.sid");
}

fn cookie_pair(set_cookie: &str) -> String {
    set_cookie
        .split(';')
        .next()
        .expect("set-cookie header should contain a cookie pair")
        .to_string()
}
