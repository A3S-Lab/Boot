#![cfg(feature = "macros")]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use a3s_boot::{
    injectable, BootApplication, BootError, BoxFuture, CallHandler, ProviderDefinition, Result,
    TransportContext, TransportInterceptor, TransportMessage, TransportReply,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

static REQUEST_CONSTRUCTIONS: AtomicUsize = AtomicUsize::new(0);
static REQUEST_EVENTS: Mutex<Vec<(usize, String)>> = Mutex::new(Vec::new());
static TRANSIENT_CONSTRUCTIONS: AtomicUsize = AtomicUsize::new(0);
static TRANSIENT_ATTEMPTS: Mutex<Vec<usize>> = Mutex::new(Vec::new());
static CONTEXT_STATE_CONSTRUCTIONS: AtomicUsize = AtomicUsize::new(0);
static SINGLETON_CONSTRUCTIONS: AtomicUsize = AtomicUsize::new(0);

#[derive(Debug, Deserialize)]
struct TypedPayload {
    value: String,
}

#[derive(Debug, Deserialize, PartialEq, Eq, Serialize)]
struct ControllerReply {
    controller_id: usize,
    value: String,
}

#[derive(Debug)]
struct RequestMessageController {
    id: usize,
}

impl RequestMessageController {
    fn provider() -> ProviderDefinition {
        ProviderDefinition::request_scoped::<Self, _>(|_| {
            Ok(Self {
                id: REQUEST_CONSTRUCTIONS.fetch_add(1, Ordering::SeqCst) + 1,
            })
        })
    }
}

#[a3s_boot::message_controller]
#[a3s_boot::metadata("controller", "request")]
impl RequestMessageController {
    #[a3s_boot::message_pattern("macro.context.request.typed")]
    #[a3s_boot::metadata("binding", "typed")]
    async fn typed(&self, payload: TypedPayload) -> Result<ControllerReply> {
        Ok(ControllerReply {
            controller_id: self.id,
            value: payload.value,
        })
    }

    #[a3s_boot::event_pattern("macro.context.request.event")]
    async fn event(&self, #[a3s_boot::payload("value")] value: String) -> Result<()> {
        REQUEST_EVENTS.lock().unwrap().push((self.id, value));
        Ok(())
    }
}

#[derive(Debug)]
struct TransientMessageController {
    id: usize,
    attempts: AtomicUsize,
}

impl TransientMessageController {
    fn provider() -> ProviderDefinition {
        ProviderDefinition::transient::<Self, _>(|_| {
            Ok(Self {
                id: TRANSIENT_CONSTRUCTIONS.fetch_add(1, Ordering::SeqCst) + 1,
                attempts: AtomicUsize::new(0),
            })
        })
    }
}

struct RetryOnce;

impl TransportInterceptor for RetryOnce {
    fn intercept<'a>(
        &'a self,
        _context: TransportContext,
        next: CallHandler<'a, Option<TransportReply>>,
    ) -> BoxFuture<'a, Result<Option<TransportReply>>> {
        Box::pin(async move {
            match next.handle().await {
                Ok(reply) => Ok(reply),
                Err(_) => next.handle().await,
            }
        })
    }
}

#[a3s_boot::message_controller]
impl TransientMessageController {
    #[a3s_boot::message_pattern("macro.context.transient.retry")]
    #[a3s_boot::use_interceptor(RetryOnce)]
    async fn retry(&self, #[a3s_boot::payload("value")] value: String) -> Result<ControllerReply> {
        TRANSIENT_ATTEMPTS.lock().unwrap().push(self.id);
        if self.attempts.fetch_add(1, Ordering::SeqCst) == 0 {
            return Err(BootError::Internal(
                "retry transient controller".to_string(),
            ));
        }

        Ok(ControllerReply {
            controller_id: self.id,
            value,
        })
    }
}

#[derive(Debug)]
struct ContextState {
    id: usize,
}

#[injectable]
#[derive(Debug)]
struct ContextualSingletonMessageController {
    state: Arc<ContextState>,
}

#[a3s_boot::message_controller]
impl ContextualSingletonMessageController {
    #[a3s_boot::message_pattern("macro.context.singleton.bubbled")]
    async fn current(&self, message: TransportMessage) -> Result<ControllerReply> {
        Ok(ControllerReply {
            controller_id: self.state.id,
            value: message.pattern().to_string(),
        })
    }
}

#[derive(Debug)]
struct PureSingletonMessageController {
    id: usize,
}

impl PureSingletonMessageController {
    fn provider() -> ProviderDefinition {
        ProviderDefinition::factory::<Self, _>(|_| {
            Ok(Self {
                id: SINGLETON_CONSTRUCTIONS.fetch_add(1, Ordering::SeqCst) + 1,
            })
        })
    }
}

#[a3s_boot::message_controller]
impl PureSingletonMessageController {
    #[a3s_boot::message_pattern("macro.context.singleton.pure")]
    async fn current(&self, payload: TypedPayload) -> Result<ControllerReply> {
        Ok(ControllerReply {
            controller_id: self.id,
            value: payload.value,
        })
    }
}

#[a3s_boot::module(
    name = "macro-context-message-controllers",
    providers = [
        ProviderDefinition::request_scoped::<ContextState, _>(|_| {
            Ok(ContextState {
                id: CONTEXT_STATE_CONSTRUCTIONS.fetch_add(1, Ordering::SeqCst) + 1,
            })
        }),
        RequestMessageController,
        TransientMessageController,
        ContextualSingletonMessageController,
        PureSingletonMessageController,
    ],
    message_controllers = [
        RequestMessageController,
        TransientMessageController,
        ContextualSingletonMessageController,
        PureSingletonMessageController,
    ],
)]
#[derive(Debug)]
struct ContextMessageControllerModule;

async fn dispatch_reply(
    app: &BootApplication,
    pattern: &str,
    data: serde_json::Value,
) -> ControllerReply {
    app.dispatch_message(TransportMessage::new(pattern, data))
        .await
        .unwrap()
        .unwrap()
        .data_as::<ControllerReply>()
        .unwrap()
}

#[tokio::test]
async fn module_macros_scope_message_controllers_per_dispatch() {
    REQUEST_CONSTRUCTIONS.store(0, Ordering::SeqCst);
    REQUEST_EVENTS.lock().unwrap().clear();
    TRANSIENT_CONSTRUCTIONS.store(0, Ordering::SeqCst);
    TRANSIENT_ATTEMPTS.lock().unwrap().clear();
    CONTEXT_STATE_CONSTRUCTIONS.store(0, Ordering::SeqCst);
    SINGLETON_CONSTRUCTIONS.store(0, Ordering::SeqCst);

    let app = BootApplication::builder()
        .import(ContextMessageControllerModule)
        .build()
        .unwrap();

    for pattern in [
        "macro.context.request.typed",
        "macro.context.request.event",
        "macro.context.transient.retry",
        "macro.context.singleton.bubbled",
    ] {
        assert!(app.message_pattern_for(pattern).unwrap().is_scoped());
    }
    assert!(!app
        .message_pattern_for("macro.context.singleton.pure")
        .unwrap()
        .is_scoped());

    assert_eq!(REQUEST_CONSTRUCTIONS.load(Ordering::SeqCst), 0);
    assert_eq!(TRANSIENT_CONSTRUCTIONS.load(Ordering::SeqCst), 0);
    assert_eq!(CONTEXT_STATE_CONSTRUCTIONS.load(Ordering::SeqCst), 0);
    assert_eq!(SINGLETON_CONSTRUCTIONS.load(Ordering::SeqCst), 1);

    let request_one = dispatch_reply(
        &app,
        "macro.context.request.typed",
        json!({ "value": "first" }),
    )
    .await;
    let request_two = dispatch_reply(
        &app,
        "macro.context.request.typed",
        json!({ "value": "second" }),
    )
    .await;
    assert_eq!(
        (request_one.controller_id, request_one.value.as_str()),
        (1, "first")
    );
    assert_eq!(
        (request_two.controller_id, request_two.value.as_str()),
        (2, "second")
    );

    app.emit_message(TransportMessage::new(
        "macro.context.request.event",
        json!({ "value": "observed" }),
    ))
    .await
    .unwrap();
    assert_eq!(
        REQUEST_EVENTS.lock().unwrap().as_slice(),
        &[(3, "observed".to_string())]
    );
    assert_eq!(REQUEST_CONSTRUCTIONS.load(Ordering::SeqCst), 3);

    let transient_one = dispatch_reply(
        &app,
        "macro.context.transient.retry",
        json!({ "value": "retry-one" }),
    )
    .await;
    let transient_two = dispatch_reply(
        &app,
        "macro.context.transient.retry",
        json!({ "value": "retry-two" }),
    )
    .await;
    assert_eq!(transient_one.controller_id, 1);
    assert_eq!(transient_two.controller_id, 2);
    assert_eq!(TRANSIENT_ATTEMPTS.lock().unwrap().as_slice(), &[1, 1, 2, 2]);
    assert_eq!(TRANSIENT_CONSTRUCTIONS.load(Ordering::SeqCst), 2);

    let contextual_one = dispatch_reply(
        &app,
        "macro.context.singleton.bubbled",
        json!({ "ignored": true }),
    )
    .await;
    let contextual_two = dispatch_reply(
        &app,
        "macro.context.singleton.bubbled",
        json!({ "ignored": true }),
    )
    .await;
    assert_eq!(contextual_one.controller_id, 1);
    assert_eq!(contextual_two.controller_id, 2);
    assert_eq!(CONTEXT_STATE_CONSTRUCTIONS.load(Ordering::SeqCst), 2);

    let singleton_one = dispatch_reply(
        &app,
        "macro.context.singleton.pure",
        json!({ "value": "captured-one" }),
    )
    .await;
    let singleton_two = dispatch_reply(
        &app,
        "macro.context.singleton.pure",
        json!({ "value": "captured-two" }),
    )
    .await;
    assert_eq!(singleton_one.controller_id, 1);
    assert_eq!(singleton_two.controller_id, 1);
    assert_eq!(SINGLETON_CONSTRUCTIONS.load(Ordering::SeqCst), 1);

    let request_pattern = app
        .message_pattern_for("macro.context.request.typed")
        .unwrap();
    assert_eq!(
        request_pattern.metadata_value("controller"),
        Some(&json!("request"))
    );
    assert_eq!(
        request_pattern.metadata_value("binding"),
        Some(&json!("typed"))
    );
}

#[test]
fn generated_message_controller_metadata_exposes_both_handler_modes() {
    let instance = Arc::new(RequestMessageController { id: 7 })
        .message_patterns()
        .unwrap();
    let provider = RequestMessageController::provider_message_patterns().unwrap();

    assert_eq!(instance.len(), provider.len());
    for (instance, provider) in instance.iter().zip(&provider) {
        assert!(!instance.is_scoped());
        assert!(provider.is_scoped());
        assert_eq!(instance.pattern(), provider.pattern());
        assert_eq!(instance.metadata(), provider.metadata());
    }
    assert_eq!(
        provider[0].metadata_value("controller"),
        Some(&json!("request"))
    );
    assert_eq!(provider[0].metadata_value("binding"), Some(&json!("typed")));
}
