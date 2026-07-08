#![cfg(feature = "database")]

use a3s_boot::{
    BootApplication, BootError, Database, DatabaseModule, DatabaseRow, InMemoryDatabaseBackend,
    Module, ModuleRef, ProviderDefinition, Result,
};
use serde_json::json;
use std::sync::Arc;

#[tokio::test]
async fn database_executes_and_queries_through_in_memory_backend() {
    let backend = InMemoryDatabaseBackend::new()
        .with_query_result(
            "select id, name from cats where id = ?",
            [DatabaseRow::new()
                .with("id", &1_u64)
                .unwrap()
                .with("name", &"Milo")
                .unwrap()],
        )
        .unwrap();
    let database = Database::new(backend.clone());

    let result = database
        .execute("insert into cats(name) values (?)", [json!("Milo")])
        .await
        .unwrap();
    let rows = database
        .query("select id, name from cats where id = ?", [json!(1)])
        .await
        .unwrap();

    assert_eq!(result.rows_affected(), 1);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].get::<u64>("id").unwrap(), Some(1));
    assert_eq!(rows[0].get::<String>("name").unwrap(), Some("Milo".into()));

    let executed = backend.executed().unwrap();
    let queried = backend.queried().unwrap();
    assert_eq!(executed[0].sql(), "insert into cats(name) values (?)");
    assert_eq!(executed[0].params(), &[json!("Milo")]);
    assert_eq!(queried[0].sql(), "select id, name from cats where id = ?");
    assert_eq!(queried[0].params(), &[json!(1)]);
}

#[tokio::test]
async fn database_transactions_commit_and_rollback() {
    let backend = InMemoryDatabaseBackend::new();
    let database = Database::new(backend.clone());

    let value = database
        .transaction(|transaction| async move {
            transaction
                .execute("insert into cats(name) values (?)", [json!("Milo")])
                .await?;
            transaction
                .execute("insert into cats(name) values (?)", [json!("Otis")])
                .await?;
            Ok(2_u64)
        })
        .await
        .unwrap();
    let error = database
        .transaction(|transaction| async move {
            transaction
                .execute("insert into cats(name) values (?)", [json!("Bad")])
                .await?;
            Err::<(), _>(BootError::BadRequest("invalid cat".to_string()))
        })
        .await
        .unwrap_err();

    let transactions = backend.transactions().unwrap();
    assert_eq!(value, 2);
    assert!(matches!(error, BootError::BadRequest(message) if message == "invalid cat"));
    assert_eq!(transactions.len(), 2);
    assert!(transactions[0].committed());
    assert!(!transactions[0].rolled_back());
    assert_eq!(transactions[0].statements().len(), 2);
    assert!(!transactions[1].committed());
    assert!(transactions[1].rolled_back());
    assert_eq!(transactions[1].statements().len(), 1);
}

#[tokio::test]
async fn database_module_exports_database_to_importing_modules() {
    #[derive(Debug)]
    struct CatsRepository {
        database: Arc<Database>,
    }

    impl CatsRepository {
        async fn names(&self) -> Result<Vec<String>> {
            let rows = self
                .database
                .query("select name from cats", Vec::<serde_json::Value>::new())
                .await?;
            rows.into_iter()
                .map(|row| {
                    row.get::<String>("name")?
                        .ok_or_else(|| BootError::Internal("missing name column".to_string()))
                })
                .collect()
        }
    }

    #[derive(Debug)]
    struct CatsModule {
        database: DatabaseModule,
    }

    impl Module for CatsModule {
        fn name(&self) -> &'static str {
            "cats"
        }

        fn imports(&self) -> Vec<Arc<dyn Module>> {
            vec![Arc::new(self.database.clone())]
        }

        fn providers(&self) -> Result<Vec<ProviderDefinition>> {
            Ok(vec![ProviderDefinition::factory::<CatsRepository, _>(
                |module_ref: &ModuleRef| {
                    Ok(CatsRepository {
                        database: module_ref.get::<Database>()?,
                    })
                },
            )])
        }
    }

    let backend = InMemoryDatabaseBackend::new()
        .with_query_result(
            "select name from cats",
            [DatabaseRow::new().with("name", &"Milo").unwrap()],
        )
        .unwrap();
    let app = BootApplication::builder()
        .import(CatsModule {
            database: DatabaseModule::from_backend("database", backend.clone()),
        })
        .build()
        .unwrap();

    let repository = app.get::<CatsRepository>().unwrap();
    assert_eq!(repository.names().await.unwrap(), ["Milo".to_string()]);
    assert_eq!(backend.queried().unwrap()[0].sql(), "select name from cats");
}

#[tokio::test]
async fn database_module_supports_named_and_global_exports() {
    #[derive(Debug)]
    struct UsesNamedDatabase {
        database: DatabaseModule,
    }

    impl Module for UsesNamedDatabase {
        fn name(&self) -> &'static str {
            "uses-named-database"
        }

        fn imports(&self) -> Vec<Arc<dyn Module>> {
            vec![Arc::new(self.database.clone())]
        }

        fn providers(&self) -> Result<Vec<ProviderDefinition>> {
            Ok(vec![ProviderDefinition::factory::<UsesDatabase, _>(
                |module_ref: &ModuleRef| {
                    Ok(UsesDatabase {
                        database: module_ref.get_named::<Database>("analytics")?,
                    })
                },
            )])
        }
    }

    #[derive(Debug)]
    struct UsesGlobalDatabase;

    impl Module for UsesGlobalDatabase {
        fn name(&self) -> &'static str {
            "uses-global-database"
        }

        fn providers(&self) -> Result<Vec<ProviderDefinition>> {
            Ok(vec![ProviderDefinition::factory::<UsesDatabase, _>(
                |module_ref: &ModuleRef| {
                    Ok(UsesDatabase {
                        database: module_ref.get::<Database>()?,
                    })
                },
            )])
        }
    }

    #[derive(Debug)]
    struct UsesDatabase {
        database: Arc<Database>,
    }

    let named = BootApplication::builder()
        .import(UsesNamedDatabase {
            database: DatabaseModule::in_memory("analytics").named("analytics"),
        })
        .build()
        .unwrap();
    let global = BootApplication::builder()
        .import(DatabaseModule::in_memory("global-database").global())
        .import(UsesGlobalDatabase)
        .build()
        .unwrap();

    let named_service = named.get::<UsesDatabase>().unwrap();
    named_service
        .database
        .query("select 1", Vec::<serde_json::Value>::new())
        .await
        .unwrap();
    assert!(named.get_named::<Database>("analytics").is_ok());
    assert!(named.get_optional::<Database>().unwrap().is_none());
    let global_service = global.get::<UsesDatabase>().unwrap();
    global_service
        .database
        .query("select 1", Vec::<serde_json::Value>::new())
        .await
        .unwrap();
    assert!(global.get::<Database>().is_ok());
}
