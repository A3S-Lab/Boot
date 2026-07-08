#![cfg(feature = "cqrs")]

use a3s_boot::{
    BootApplication, Command, CommandBus, CqrsContext, CqrsModule, EventBus, ProviderDefinition,
    Query, QueryBus,
};
use std::sync::{Arc, Mutex};

#[derive(Debug)]
struct CatStore {
    prefix: &'static str,
}

#[derive(Debug)]
struct RenameCat {
    id: u64,
    name: String,
}

impl Command for RenameCat {
    type Output = String;
}

#[derive(Debug)]
struct FindCat {
    id: u64,
}

impl Query for FindCat {
    type Output = String;
}

#[derive(Debug, Clone)]
struct CatRenamed {
    id: u64,
    name: String,
}

#[tokio::test]
async fn cqrs_module_exports_buses_and_dispatches_commands_queries_and_events() {
    let observed = Arc::new(Mutex::new(Vec::new()));
    let first_observed = Arc::clone(&observed);
    let second_observed = Arc::clone(&observed);
    let app = BootApplication::builder()
        .import(
            CqrsModule::new("cats-cqrs")
                .provider(ProviderDefinition::singleton(CatStore { prefix: "cat" }))
                .command_handler::<RenameCat, _>(
                    |command: RenameCat, context: CqrsContext| async move {
                        let store = context.get::<CatStore>()?;
                        Ok(format!("{}:{}={}", store.prefix, command.id, command.name))
                    },
                )
                .query_handler::<FindCat, _>(|query: FindCat, context: CqrsContext| async move {
                    let store = context.get::<CatStore>()?;
                    Ok(format!("{}:{}", store.prefix, query.id))
                })
                .event_handler::<CatRenamed, _>(move |event: CatRenamed, _| {
                    let observed = Arc::clone(&first_observed);
                    async move {
                        observed
                            .lock()
                            .unwrap()
                            .push(format!("first:{}:{}", event.id, event.name));
                        Ok(())
                    }
                })
                .event_handler::<CatRenamed, _>(move |event: CatRenamed, _| {
                    let observed = Arc::clone(&second_observed);
                    async move {
                        observed
                            .lock()
                            .unwrap()
                            .push(format!("second:{}:{}", event.id, event.name));
                        Ok(())
                    }
                }),
        )
        .build()
        .unwrap();
    let command_bus = app.get::<CommandBus>().unwrap();
    let query_bus = app.get::<QueryBus>().unwrap();
    let event_bus = app.get::<EventBus>().unwrap();

    let command_result = command_bus
        .execute(RenameCat {
            id: 1,
            name: "Milo".to_string(),
        })
        .await
        .unwrap();
    let query_result = query_bus.execute(FindCat { id: 1 }).await.unwrap();
    let event_count = event_bus
        .publish(CatRenamed {
            id: 1,
            name: "Milo".to_string(),
        })
        .await
        .unwrap();

    assert_eq!(command_result, "cat:1=Milo");
    assert_eq!(query_result, "cat:1");
    assert_eq!(event_count, 2);
    assert_eq!(
        observed.lock().unwrap().as_slice(),
        ["first:1:Milo", "second:1:Milo"]
    );
}

#[tokio::test]
async fn command_and_query_buses_report_missing_handlers() {
    let command_bus = CommandBus::new();
    let query_bus = QueryBus::new();

    let command_error = command_bus
        .execute(RenameCat {
            id: 1,
            name: "Milo".to_string(),
        })
        .await
        .unwrap_err();
    let query_error = query_bus.execute(FindCat { id: 1 }).await.unwrap_err();

    assert!(command_error
        .to_string()
        .contains("command handler is not registered"));
    assert!(query_error
        .to_string()
        .contains("query handler is not registered"));
}

#[test]
fn cqrs_module_rejects_duplicate_command_and_query_handlers() {
    let command_bus = CommandBus::new();
    command_bus
        .register::<RenameCat, _>(|_: RenameCat, _: CqrsContext| async { Ok("first".to_string()) })
        .unwrap();
    let command_error = command_bus
        .register::<RenameCat, _>(|_: RenameCat, _: CqrsContext| async { Ok("second".to_string()) })
        .unwrap_err();

    let query_bus = QueryBus::new();
    query_bus
        .register::<FindCat, _>(|_: FindCat, _: CqrsContext| async { Ok("first".to_string()) })
        .unwrap();
    let query_error = query_bus
        .register::<FindCat, _>(|_: FindCat, _: CqrsContext| async { Ok("second".to_string()) })
        .unwrap_err();

    assert!(command_error
        .to_string()
        .contains("command handler is already registered"));
    assert!(query_error
        .to_string()
        .contains("query handler is already registered"));
}

#[tokio::test]
async fn cqrs_module_can_export_buses_globally() {
    let app = BootApplication::builder()
        .import(CqrsModule::new("cqrs").global())
        .build()
        .unwrap();

    assert_eq!(app.get::<CommandBus>().unwrap().handler_count().unwrap(), 0);
    assert_eq!(app.get::<QueryBus>().unwrap().handler_count().unwrap(), 0);
    assert_eq!(app.get::<EventBus>().unwrap().handler_count().unwrap(), 0);
}
