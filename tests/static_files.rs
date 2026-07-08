#![cfg(feature = "static")]

use a3s_boot::{BootApplication, BootError, BootRequest, HttpMethod, StaticModule};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[tokio::test]
async fn static_module_serves_files_under_serve_root() {
    let temp = TempDir::new("static-module-serves-files");
    temp.write("public/app.css", "body { color: #222; }");
    temp.write("public/index.html", "<main>A3S</main>");

    let app = BootApplication::builder()
        .import(StaticModule::new("static", temp.path().join("public")).with_serve_root("/assets"))
        .build()
        .unwrap();

    let response = app
        .call(BootRequest::new(HttpMethod::Get, "/assets/app.css"))
        .await
        .unwrap();
    let index = app
        .call(BootRequest::new(HttpMethod::Get, "/assets"))
        .await
        .unwrap();

    assert_eq!(response.status(), 200);
    assert_eq!(response.content_type(), Some("text/css; charset=utf-8"));
    assert_eq!(response.body_text().unwrap(), "body { color: #222; }");
    assert_eq!(index.body_text().unwrap(), "<main>A3S</main>");
}

#[tokio::test]
async fn static_module_supports_head_and_cache_control() {
    let temp = TempDir::new("static-module-head");
    temp.write("public/app.js", "console.log('boot');");

    let app = BootApplication::builder()
        .import(
            StaticModule::new("static", temp.path().join("public"))
                .with_serve_root("/assets")
                .with_cache_control("public, max-age=60"),
        )
        .build()
        .unwrap();

    let response = app
        .call(BootRequest::new(HttpMethod::Head, "/assets/app.js"))
        .await
        .unwrap();

    assert_eq!(response.status(), 200);
    assert_eq!(response.body(), b"");
    assert_eq!(response.content_length().unwrap(), Some(20));
    assert_eq!(
        response.content_type(),
        Some("text/javascript; charset=utf-8")
    );
    assert_eq!(response.header("cache-control"), Some("public, max-age=60"));
}

#[tokio::test]
async fn static_module_can_fallback_to_spa_index() {
    let temp = TempDir::new("static-module-fallback");
    temp.write("dist/index.html", "<div id=\"app\"></div>");

    let app = BootApplication::builder()
        .import(
            StaticModule::new("static", temp.path().join("dist"))
                .with_serve_root("/")
                .with_fallback_file("index.html"),
        )
        .build()
        .unwrap();

    let response = app
        .call(BootRequest::new(HttpMethod::Get, "/dashboard/settings"))
        .await
        .unwrap();

    assert_eq!(response.status(), 200);
    assert_eq!(response.content_type(), Some("text/html; charset=utf-8"));
    assert_eq!(response.body_text().unwrap(), "<div id=\"app\"></div>");
}

#[tokio::test]
async fn static_module_rejects_traversal_and_dotfiles() {
    let temp = TempDir::new("static-module-security");
    temp.write("public/index.html", "ok");
    temp.write("public/.env", "secret");

    let app = BootApplication::builder()
        .import(StaticModule::new("static", temp.path().join("public")))
        .build()
        .unwrap();

    let traversal = app
        .call(BootRequest::new(HttpMethod::Get, "/../Cargo.toml"))
        .await
        .unwrap_err();
    let dotfile = app
        .call(BootRequest::new(HttpMethod::Get, "/.env"))
        .await
        .unwrap_err();

    assert!(matches!(traversal, BootError::Forbidden(_)));
    assert!(matches!(dotfile, BootError::Forbidden(_)));
}

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(name: &str) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("a3s-boot-{name}-{timestamp}"));
        fs::create_dir_all(&path).unwrap();
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn write(&self, relative_path: &str, content: &str) {
        let path = self.path.join(relative_path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, content).unwrap();
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
