#![cfg(feature = "events")]

use a3s_boot::{BootApplication, EventContext, EventEmitter, EventEnvelope, EventModule, Result};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

#[derive(Debug, Deserialize, Serialize)]
struct CatEvent {
    name: String,
}

#[cfg(feature = "macros")]
#[derive(Debug)]
struct MacroEventHandlers {
    observed: Arc<Mutex<Vec<String>>>,
}

#[cfg(feature = "macros")]
#[a3s_boot::event_listener]
impl MacroEventHandlers {
    #[a3s_boot::on_event("cat.created")]
    async fn cat_created(&self, payload: CatEvent, context: EventContext) -> Result<()> {
        let emitter = context.get::<EventEmitter>()?;
        self.observed.lock().unwrap().push(format!(
            "created:{}:{}",
            payload.name,
            emitter.listener_count()?
        ));
        Ok(())
    }

    #[a3s_boot::on_event("cat.*")]
    async fn cat_event(&self, event: EventEnvelope) -> Result<()> {
        self.observed
            .lock()
            .unwrap()
            .push(format!("wildcard:{}", event.name()));
        Ok(())
    }
}

#[cfg(feature = "macros")]
#[tokio::test]
async fn event_listener_macros_register_typed_payload_and_envelope_handlers() {
    let observed = Arc::new(Mutex::new(Vec::new()));
    let handlers = Arc::new(MacroEventHandlers {
        observed: Arc::clone(&observed),
    });
    let app = BootApplication::builder()
        .import(
            EventModule::in_process("events").listeners(Arc::clone(&handlers).event_listeners()),
        )
        .build()
        .unwrap();
    let emitter = app.get::<EventEmitter>().unwrap();

    let count = emitter
        .emit(
            "cat.created",
            &CatEvent {
                name: "Milo".to_string(),
            },
        )
        .await
        .unwrap();

    assert_eq!(count, 2);
    assert_eq!(
        observed.lock().unwrap().as_slice(),
        ["created:Milo:2", "wildcard:cat.created"]
    );
}

#[tokio::test]
async fn event_module_exports_emitter_and_dispatches_registered_listeners() {
    let observed = Arc::new(Mutex::new(Vec::new()));
    let listener_observed = Arc::clone(&observed);
    let app = BootApplication::builder()
        .import(EventModule::in_process("events").listener(
            "cat.created",
            move |event: EventEnvelope, _| {
                let observed = Arc::clone(&listener_observed);
                async move {
                    let payload = event.data_as::<CatEvent>()?;
                    observed.lock().unwrap().push(payload.name);
                    Ok(())
                }
            },
        ))
        .build()
        .unwrap();
    let emitter = app.get::<EventEmitter>().unwrap();

    let count = emitter
        .emit(
            "cat.created",
            &CatEvent {
                name: "Milo".to_string(),
            },
        )
        .await
        .unwrap();

    assert_eq!(count, 1);
    assert_eq!(observed.lock().unwrap().as_slice(), ["Milo"]);
}

#[tokio::test]
async fn event_emitter_supports_wildcard_patterns_in_registration_order() {
    let observed = Arc::new(Mutex::new(Vec::new()));
    let first_observed = Arc::clone(&observed);
    let second_observed = Arc::clone(&observed);
    let app = BootApplication::builder()
        .import(
            EventModule::in_process("events")
                .listener("cat.*", move |event: EventEnvelope, _| {
                    let observed = Arc::clone(&first_observed);
                    async move {
                        observed
                            .lock()
                            .unwrap()
                            .push(format!("wildcard:{}", event.name()));
                        Ok(())
                    }
                })
                .listener("cat.created", move |event: EventEnvelope, _| {
                    let observed = Arc::clone(&second_observed);
                    async move {
                        observed
                            .lock()
                            .unwrap()
                            .push(format!("exact:{}", event.name()));
                        Ok(())
                    }
                }),
        )
        .build()
        .unwrap();
    let emitter = app.get::<EventEmitter>().unwrap();

    let count = emitter
        .emit(
            "cat.created",
            &CatEvent {
                name: "Luna".to_string(),
            },
        )
        .await
        .unwrap();
    emitter
        .emit(
            "dog.created",
            &CatEvent {
                name: "Ada".to_string(),
            },
        )
        .await
        .unwrap();

    assert_eq!(count, 2);
    assert_eq!(
        observed.lock().unwrap().as_slice(),
        ["wildcard:cat.created", "exact:cat.created"]
    );
}

#[tokio::test]
async fn event_listener_context_can_resolve_the_emitter_provider() {
    let observed = Arc::new(Mutex::new(Vec::new()));
    let listener_observed = Arc::clone(&observed);
    let app = BootApplication::builder()
        .import(EventModule::in_process("events").listener(
            "cat.created",
            move |_event: EventEnvelope, context: EventContext| {
                let observed = Arc::clone(&listener_observed);
                async move {
                    let emitter = context.get::<EventEmitter>()?;
                    observed
                        .lock()
                        .unwrap()
                        .push(emitter.listener_count()?.to_string());
                    Ok(())
                }
            },
        ))
        .build()
        .unwrap();
    let emitter = app.get::<EventEmitter>().unwrap();

    emitter
        .emit(
            "cat.created",
            &CatEvent {
                name: "Nori".to_string(),
            },
        )
        .await
        .unwrap();

    assert_eq!(observed.lock().unwrap().as_slice(), ["1"]);
}

#[test]
fn event_module_supports_named_and_global_exports() {
    let app = BootApplication::builder()
        .import(
            EventModule::in_process("events")
                .named("application-events")
                .global(),
        )
        .build()
        .unwrap();

    let emitter = app.get_named::<EventEmitter>("application-events").unwrap();

    assert_eq!(emitter.listener_count().unwrap(), 0);
}
