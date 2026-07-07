#![cfg(feature = "config")]

use a3s_boot::{
    acl_document_to_json, BootApplication, BootError, ConfigModule, Module, ProviderDefinition,
    Result, Validate,
};
use serde::Deserialize;
use serde_json::json;
use std::collections::BTreeMap;
use std::sync::Arc;

#[derive(Debug, Deserialize)]
struct AppConfig {
    database_url: String,
    port: u16,
    features: Vec<String>,
    limits: LimitsConfig,
    providers: BTreeMap<String, ProviderConfig>,
}

impl Validate for AppConfig {
    fn validate(&self) -> Result<()> {
        if self.port == 0 {
            return Err(BootError::BadRequest("port must be non-zero".to_string()));
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
struct LimitsConfig {
    body_bytes: u64,
}

#[derive(Debug, Deserialize)]
struct ProviderConfig {
    api_key: String,
    base_url: String,
}

#[derive(Debug)]
struct ConfigConsumer {
    config: Arc<AppConfig>,
}

#[derive(Debug)]
struct UsesConfigModule {
    config_module: ConfigModule<AppConfig>,
}

impl Module for UsesConfigModule {
    fn name(&self) -> &'static str {
        "uses-config"
    }

    fn imports(&self) -> Vec<Arc<dyn Module>> {
        vec![Arc::new(self.config_module.clone())]
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        Ok(vec![ProviderDefinition::factory::<ConfigConsumer, _>(
            |module_ref| {
                Ok(ConfigConsumer {
                    config: module_ref.get::<AppConfig>()?,
                })
            },
        )])
    }
}

#[derive(Debug)]
struct UsesNamedConfigModule {
    config_module: ConfigModule<AppConfig>,
}

impl Module for UsesNamedConfigModule {
    fn name(&self) -> &'static str {
        "uses-named-config"
    }

    fn imports(&self) -> Vec<Arc<dyn Module>> {
        vec![Arc::new(self.config_module.clone())]
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        Ok(vec![ProviderDefinition::factory::<ConfigConsumer, _>(
            |module_ref| {
                Ok(ConfigConsumer {
                    config: module_ref.get_named::<AppConfig>("app-config")?,
                })
            },
        )])
    }
}

#[test]
fn config_module_loads_typed_values_from_acl() {
    let module = ConfigModule::<AppConfig>::from_acl_str("app-config", app_config_acl()).unwrap();
    let app = BootApplication::builder()
        .import(UsesConfigModule {
            config_module: module,
        })
        .build()
        .unwrap();

    let consumer = app.get::<ConfigConsumer>().unwrap();

    assert_eq!(consumer.config.database_url, "postgres://localhost/app");
    assert_eq!(consumer.config.port, 3000);
    assert_eq!(consumer.config.features, ["http", "sse", "transport"]);
    assert_eq!(consumer.config.limits.body_bytes, 4096);
    assert_eq!(
        consumer.config.providers["openai"].base_url,
        "https://api.openai.com/v1"
    );
}

#[test]
fn config_module_supports_acl_env_and_concat_functions() {
    std::env::set_var("A3S_BOOT_CONFIG_TEST_DATABASE_URL", "postgres://env/app");
    let module = ConfigModule::<AppConfig>::from_acl_str(
        "app-config",
        r#"
            database_url = env("A3S_BOOT_CONFIG_TEST_DATABASE_URL")
            port = 3000
            features = [concat("trans", "port")]

            limits {
                body_bytes = 4096
            }

            providers "openai" {
                api_key = env("A3S_BOOT_CONFIG_TEST_API_KEY", "test-key")
                base_url = concat("https://", "api.openai.com", "/v1")
            }
        "#,
    )
    .unwrap();
    let app = BootApplication::builder()
        .import(module.global())
        .build()
        .unwrap();

    let config = app.get::<AppConfig>().unwrap();

    assert_eq!(config.database_url, "postgres://env/app");
    assert_eq!(config.features, ["transport"]);
    assert_eq!(config.providers["openai"].api_key, "test-key");
    assert_eq!(
        config.providers["openai"].base_url,
        "https://api.openai.com/v1"
    );
}

#[test]
fn config_module_supports_acl_files_and_named_exports() {
    let path = std::env::temp_dir().join(format!(
        "a3s-boot-config-{}-{}.acl",
        std::process::id(),
        "named"
    ));
    std::fs::write(&path, app_config_acl()).unwrap();

    let module = ConfigModule::<AppConfig>::from_acl_file("app-config", &path)
        .unwrap()
        .named("app-config");
    let app = BootApplication::builder()
        .import(UsesNamedConfigModule {
            config_module: module,
        })
        .build()
        .unwrap();
    let consumer = app.get::<ConfigConsumer>().unwrap();

    assert_eq!(consumer.config.database_url, "postgres://localhost/app");
    assert!(app.get_optional::<AppConfig>().unwrap().is_none());
    assert!(app.get_named::<AppConfig>("app-config").is_ok());

    std::fs::remove_file(path).unwrap();
}

#[test]
fn config_module_rejects_invalid_acl_and_invalid_config_shapes() {
    let syntax = ConfigModule::<AppConfig>::from_acl_str("app-config", "port = [").unwrap_err();
    let shape =
        ConfigModule::<AppConfig>::from_acl_str("app-config", "port = \"bad\"").unwrap_err();
    let validation = ConfigModule::<AppConfig>::from_validated_acl_str(
        "app-config",
        &app_config_acl().replace("port = 3000", "port = 0"),
    )
    .unwrap_err();

    assert!(
        matches!(syntax, BootError::BadRequest(message) if message.contains("invalid ACL config"))
    );
    assert!(
        matches!(shape, BootError::BadRequest(message) if message.contains("invalid ACL config shape"))
    );
    assert!(
        matches!(validation, BootError::BadRequest(message) if message == "port must be non-zero")
    );
}

#[test]
fn acl_documents_convert_to_json_values() {
    let document = a3s_acl::parse(app_config_acl()).unwrap();
    let value = acl_document_to_json(&document).unwrap();

    assert_eq!(value["database_url"], json!("postgres://localhost/app"));
    assert_eq!(value["limits"]["body_bytes"], json!(4096));
    assert_eq!(value["providers"]["openai"]["api_key"], json!("test-key"));
}

#[test]
fn acl_config_rejects_missing_env_without_default() {
    std::env::remove_var("A3S_BOOT_CONFIG_TEST_MISSING");
    let error = ConfigModule::<AppConfig>::from_acl_str(
        "app-config",
        r#"
            database_url = env("A3S_BOOT_CONFIG_TEST_MISSING")
            port = 3000
            features = []
            limits { body_bytes = 1 }
            providers "openai" {
                api_key = "test-key"
                base_url = "https://api.openai.com/v1"
            }
        "#,
    )
    .unwrap_err();

    assert!(
        matches!(error, BootError::BadRequest(message) if message.contains("missing environment variable"))
    );
}

fn app_config_acl() -> &'static str {
    r#"
        database_url = "postgres://localhost/app"
        port = 3000
        features = ["http", "sse", "transport"]

        limits {
            body_bytes = 4096
        }

        providers "openai" {
            api_key = "test-key"
            base_url = "https://api.openai.com/v1"
        }
    "#
}
