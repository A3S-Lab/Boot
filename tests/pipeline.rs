use a3s_boot::{
    BootApplication, BootError, BootErrorKind, BootRequest, BootResponse, BoxFuture, CallHandler,
    ControllerDefinition, ExecutionContext, ExecutionInterceptor, Guard, HttpMethod, Interceptor,
    MessagePatternDefinition, Middleware, MiddlewareConsumer, MiddlewareOutcome, MiddlewareRoute,
    Module, ModuleRef, Result, RouteDefinition, TransportMessage, TransportReply,
    WebSocketGatewayDefinition, WebSocketMessage,
};
use serde_json::json;
use std::sync::Arc;

#[tokio::test]
async fn route_pipeline_runs_guards_interceptors_pipes_and_filters() {
    struct HeaderInterceptor;

    impl Interceptor for HeaderInterceptor {
        fn after(
            &self,
            _context: ExecutionContext,
            response: BootResponse,
        ) -> BoxFuture<'static, Result<BootResponse>> {
            Box::pin(async move { Ok(response.with_header("x-boot", "ok")) })
        }
    }

    let route = RouteDefinition::post("/", |request: BootRequest| async move {
        Ok(BootResponse::text(request.text()?))
    })
    .unwrap()
    .with_pipe(|request: BootRequest| async move { Ok(request.with_body("transformed")) })
    .with_guard(|context: ExecutionContext| async move { Ok(context.request_path == "/") })
    .with_interceptor(HeaderInterceptor)
    .with_filter(|_, error: BootError| async move {
        Ok(Some(BootResponse::text(error.to_string()).with_status(500)))
    });

    let response = route
        .call(BootRequest::new(HttpMethod::Post, "/").with_body("raw"))
        .await
        .unwrap();

    assert_eq!(response.body, b"transformed");
    assert_eq!(
        response.headers.get("x-boot").map(String::as_str),
        Some("ok")
    );
}

#[tokio::test]
async fn middleware_runs_before_pipes_and_can_mutate_requests() {
    let route = RouteDefinition::post("/", |request: BootRequest| async move {
        Ok(BootResponse::text(format!(
            "{}:{}",
            request.header("x-middleware").unwrap_or("missing"),
            request.text()?
        )))
    })
    .unwrap()
    .with_middleware(|request: BootRequest| async move {
        Ok(MiddlewareOutcome::next(
            request
                .with_header("x-middleware", "route")
                .with_body("from-middleware"),
        ))
    })
    .with_pipe(|request: BootRequest| async move {
        let body = request.text()?;
        Ok(request.with_body(format!("{body}+pipe")))
    });

    let response = route
        .call(BootRequest::new(HttpMethod::Post, "/").with_body("raw"))
        .await
        .unwrap();

    assert_eq!(response.body, b"route:from-middleware+pipe");
}

#[tokio::test]
async fn middleware_can_short_circuit_before_guards() {
    let guard_calls = Arc::new(std::sync::Mutex::new(0usize));
    let guard_log = Arc::clone(&guard_calls);
    let route = RouteDefinition::get("/", |_| async { Ok(BootResponse::text("unreachable")) })
        .unwrap()
        .with_middleware(|_| async {
            Ok(MiddlewareOutcome::response(
                BootResponse::text("middleware").with_status(202),
            ))
        })
        .with_guard(move |_| {
            let guard_log = Arc::clone(&guard_log);
            async move {
                *guard_log.lock().unwrap() += 1;
                Ok(false)
            }
        });

    let response = route
        .call(BootRequest::new(HttpMethod::Get, "/"))
        .await
        .unwrap();

    assert_eq!(response.status(), 202);
    assert_eq!(response.body, b"middleware");
    assert_eq!(*guard_calls.lock().unwrap(), 0);
}

#[tokio::test]
async fn middleware_consumer_applies_and_excludes_module_routes() -> Result<()> {
    #[derive(Debug)]
    struct ConsumerModule {
        log: Arc<std::sync::Mutex<Vec<String>>>,
    }

    impl Module for ConsumerModule {
        fn name(&self) -> &'static str {
            "consumer"
        }

        fn route_prefix(&self) -> Option<&str> {
            Some("/api")
        }

        fn configure(
            &self,
            consumer: &mut MiddlewareConsumer,
            _module_ref: &ModuleRef,
        ) -> Result<()> {
            let log = Arc::clone(&self.log);
            consumer
                .apply(move |request: BootRequest| {
                    let log = Arc::clone(&log);
                    async move {
                        log.lock().unwrap().push(format!(
                            "{} {}",
                            request.method.as_str(),
                            request.path
                        ));
                        Ok(MiddlewareOutcome::next(
                            request.with_header("x-consumer", "yes"),
                        ))
                    }
                })
                .exclude([MiddlewareRoute::get("/cats/internal")?])
                .for_routes([MiddlewareRoute::get("/cats/{id}")?])
        }

        fn controllers(&self, _module_ref: &ModuleRef) -> Result<Vec<ControllerDefinition>> {
            Ok(vec![ControllerDefinition::new("/cats")?
                .get("/internal", |request: BootRequest| async move {
                    Ok(BootResponse::text(format!(
                        "internal:{}",
                        request.header("x-consumer").unwrap_or("missing")
                    )))
                })?
                .get("/{id}", |request: BootRequest| async move {
                    Ok(BootResponse::text(format!(
                        "get:{}",
                        request.header("x-consumer").unwrap_or("missing")
                    )))
                })?
                .post("/{id}", |request: BootRequest| async move {
                    Ok(BootResponse::text(format!(
                        "post:{}",
                        request.header("x-consumer").unwrap_or("missing")
                    )))
                })?])
        }
    }

    let log = Arc::new(std::sync::Mutex::new(Vec::new()));
    let app = BootApplication::builder()
        .global_prefix("/v1")
        .import(ConsumerModule {
            log: Arc::clone(&log),
        })
        .build()?;

    let matched = app
        .call(BootRequest::new(HttpMethod::Get, "/v1/api/cats/42"))
        .await?;
    let excluded = app
        .call(BootRequest::new(HttpMethod::Get, "/v1/api/cats/internal"))
        .await?;
    let method_miss = app
        .call(BootRequest::new(HttpMethod::Post, "/v1/api/cats/42"))
        .await?;

    assert_eq!(matched.body, b"get:yes");
    assert_eq!(excluded.body, b"internal:missing");
    assert_eq!(method_miss.body, b"post:missing");
    assert_eq!(log.lock().unwrap().as_slice(), ["GET /v1/api/cats/42"]);
    Ok(())
}

#[tokio::test]
async fn dynamic_module_can_configure_middleware_consumer() -> Result<()> {
    let app = BootApplication::builder()
        .import(
            a3s_boot::DynamicModule::new("dynamic-consumer")
                .route_prefix("/dynamic")
                .route(RouteDefinition::get(
                    "/health",
                    |request: BootRequest| async move {
                        Ok(BootResponse::text(
                            request.header("x-dynamic").unwrap_or("missing"),
                        ))
                    },
                )?)
                .configure_middleware(|consumer| {
                    consumer
                        .apply(|request: BootRequest| async move {
                            Ok(MiddlewareOutcome::next(
                                request.with_header("x-dynamic", "yes"),
                            ))
                        })
                        .for_routes([MiddlewareRoute::get("/health")?])
                })?,
        )
        .build()?;

    let response = app
        .call(BootRequest::new(HttpMethod::Get, "/dynamic/health"))
        .await?;

    assert_eq!(response.body, b"yes");
    Ok(())
}

#[tokio::test]
async fn middleware_consumer_can_apply_to_all_module_routes() -> Result<()> {
    let app = BootApplication::builder()
        .import(
            a3s_boot::DynamicModule::new("all-consumer")
                .route(RouteDefinition::get(
                    "/a",
                    |request: BootRequest| async move {
                        Ok(BootResponse::text(
                            request.header("x-all").unwrap_or("missing"),
                        ))
                    },
                )?)
                .route(RouteDefinition::get(
                    "/b",
                    |request: BootRequest| async move {
                        Ok(BootResponse::text(
                            request.header("x-all").unwrap_or("missing"),
                        ))
                    },
                )?)
                .configure_middleware(|consumer| {
                    consumer
                        .apply(|request: BootRequest| async move {
                            Ok(MiddlewareOutcome::next(request.with_header("x-all", "yes")))
                        })
                        .for_all_routes()
                })?,
        )
        .build()?;

    let first = app.call(BootRequest::new(HttpMethod::Get, "/a")).await?;
    let second = app.call(BootRequest::new(HttpMethod::Get, "/b")).await?;

    assert_eq!(first.body, b"yes");
    assert_eq!(second.body, b"yes");
    Ok(())
}

#[tokio::test]
async fn route_can_use_protocol_neutral_execution_interceptor() {
    let log = Arc::new(std::sync::Mutex::new(Vec::new()));
    let route = RouteDefinition::get("/", |_| async { Ok(BootResponse::text("ok")) })
        .unwrap()
        .with_execution_interceptor(SharedExecutionInterceptor {
            log: Arc::clone(&log),
        });

    let response = route
        .call(BootRequest::new(HttpMethod::Get, "/"))
        .await
        .unwrap();

    assert_eq!(response.body, b"ok");
    assert_eq!(
        log.lock().unwrap().as_slice(),
        ["before:http:/", "after:http:/"]
    );
}

#[tokio::test]
async fn global_execution_interceptors_apply_to_http_websocket_and_transport() -> Result<()> {
    let log = Arc::new(std::sync::Mutex::new(Vec::new()));
    let guard_log = Arc::new(std::sync::Mutex::new(Vec::new()));
    let app = BootApplication::builder()
        .use_global_execution_guard(ProtocolLoggingGuard {
            log: Arc::clone(&guard_log),
        })
        .use_global_execution_interceptor(SharedExecutionInterceptor {
            log: Arc::clone(&log),
        })
        .route(RouteDefinition::get("/http", |_| async {
            Ok(BootResponse::text("ok"))
        })?)
        .gateway(
            WebSocketGatewayDefinition::new("/ws")?.subscribe("ping", |_| async {
                Ok(WebSocketMessage::text("pong", "ok"))
            })?,
        )
        .message_pattern(MessagePatternDefinition::request(
            "transport",
            |message: TransportMessage| async move { Ok(TransportReply::new(message.data)) },
        )?)
        .build()?;

    let response = app.call(BootRequest::new(HttpMethod::Get, "/http")).await?;
    assert_eq!(response.body, b"ok");

    let reply = app
        .gateway_for("/ws")
        .unwrap()
        .dispatch(
            BootRequest::new(HttpMethod::Get, "/ws"),
            WebSocketMessage::text("ping", "payload"),
        )
        .await?
        .unwrap();
    assert_eq!(reply, WebSocketMessage::text("pong", "ok"));

    let reply = app
        .dispatch_message(a3s_boot::TransportMessage::new(
            "transport",
            json!({ "ok": true }),
        ))
        .await?
        .unwrap();
    assert_eq!(reply.data(), &json!({ "ok": true }));

    assert_eq!(
        log.lock().unwrap().as_slice(),
        [
            "before:http:/http",
            "after:http:/http",
            "before:websocket:/ws",
            "after:websocket:/ws",
            "before:transport:transport",
            "after:transport:transport"
        ]
    );
    assert_eq!(
        guard_log.lock().unwrap().as_slice(),
        [
            "guard:http:/http",
            "guard:websocket:/ws",
            "guard:transport:transport"
        ]
    );
    Ok(())
}

#[tokio::test]
async fn route_filters_handle_middleware_errors() {
    let route = RouteDefinition::get("/", |_| async { Ok(BootResponse::text("unreachable")) })
        .unwrap()
        .with_middleware(|_| async { Err(BootError::BadRequest("bad middleware".to_string())) })
        .with_filter(|context: ExecutionContext, error: BootError| async move {
            Ok(Some(
                BootResponse::text(format!("{}: {error}", context.route_path)).with_status(400),
            ))
        });

    let response = route
        .call(BootRequest::new(HttpMethod::Get, "/"))
        .await
        .unwrap();

    assert_eq!(response.status(), 400);
    assert_eq!(response.body, b"/: bad request: bad middleware");
}

#[tokio::test]
async fn route_filters_handle_pipe_errors() {
    let route = RouteDefinition::post("/", |_| async { Ok(BootResponse::text("unreachable")) })
        .unwrap()
        .with_pipe(|_| async { Err(BootError::BadRequest("invalid input".to_string())) })
        .with_filter(|context: ExecutionContext, error: BootError| async move {
            Ok(Some(
                BootResponse::text(format!("{}: {error}", context.route_path)).with_status(400),
            ))
        });

    let response = route
        .call(BootRequest::new(HttpMethod::Post, "/").with_body("raw"))
        .await
        .unwrap();

    assert_eq!(response.status, 400);
    assert_eq!(response.body, b"/: bad request: invalid input");
}

#[tokio::test]
async fn route_filters_see_the_request_snapshot_before_the_failing_pipe() {
    let route = RouteDefinition::get("/", |_| async { Ok(BootResponse::text("unreachable")) })
        .unwrap()
        .with_pipe(|request: BootRequest| async move {
            Ok(request.with_header("x-pipeline-stage", "first"))
        })
        .with_pipe(|_| async { Err(BootError::BadRequest("second pipe failed".to_string())) })
        .with_filter(|context: ExecutionContext, _| async move {
            Ok(Some(BootResponse::text(
                context
                    .request
                    .header("x-pipeline-stage")
                    .unwrap_or("missing"),
            )))
        });

    let response = route
        .call(BootRequest::new(HttpMethod::Get, "/"))
        .await
        .unwrap();

    assert_eq!(response.body, b"first");
}

#[tokio::test]
async fn retried_interceptor_errors_reset_stale_pipe_filter_context() {
    struct RetryOnce;

    impl Interceptor for RetryOnce {
        fn intercept<'a>(
            &'a self,
            _context: ExecutionContext,
            next: CallHandler<'a>,
        ) -> BoxFuture<'a, Result<BootResponse>> {
            Box::pin(async move {
                match next.handle().await {
                    Ok(response) => Ok(response),
                    Err(_) => next.handle().await,
                }
            })
        }
    }

    struct FailSecondBefore {
        calls: Arc<std::sync::atomic::AtomicUsize>,
    }

    impl Interceptor for FailSecondBefore {
        fn before(&self, _context: ExecutionContext) -> BoxFuture<'static, Result<()>> {
            let calls = Arc::clone(&self.calls);
            Box::pin(async move {
                if calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
                    Ok(())
                } else {
                    Err(BootError::Internal("second before failed".to_string()))
                }
            })
        }
    }

    let before_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let route = RouteDefinition::get("/", |_| async { Ok(BootResponse::text("unreachable")) })
        .unwrap()
        .with_interceptor(RetryOnce)
        .with_interceptor(FailSecondBefore {
            calls: Arc::clone(&before_calls),
        })
        .with_pipe(|request: BootRequest| async move {
            Ok(request.with_header("x-pipeline-stage", "stale"))
        })
        .with_pipe(|_| async { Err(BootError::BadRequest("retry".to_string())) })
        .with_filter(|context: ExecutionContext, error: BootError| async move {
            Ok(Some(BootResponse::text(format!(
                "{}:{error}",
                context
                    .request
                    .header("x-pipeline-stage")
                    .unwrap_or("missing")
            ))))
        });

    let response = route
        .call(BootRequest::new(HttpMethod::Get, "/"))
        .await
        .unwrap();

    assert_eq!(
        response.body,
        b"missing:internal error: second before failed"
    );
    assert_eq!(before_calls.load(std::sync::atomic::Ordering::SeqCst), 2);
}

#[tokio::test]
async fn route_filters_handle_interceptor_errors() {
    struct FailingAfterInterceptor;

    impl Interceptor for FailingAfterInterceptor {
        fn after(
            &self,
            _context: ExecutionContext,
            _response: BootResponse,
        ) -> BoxFuture<'static, Result<BootResponse>> {
            Box::pin(async { Err(BootError::Internal("response failed".to_string())) })
        }
    }

    let route = RouteDefinition::get("/", |_| async { Ok(BootResponse::text("ok")) })
        .unwrap()
        .with_interceptor(FailingAfterInterceptor)
        .with_filter(|context: ExecutionContext, error: BootError| async move {
            Ok(Some(
                BootResponse::text(format!("{}: {error}", context.route_path)).with_status(500),
            ))
        });

    let response = route
        .call(BootRequest::new(HttpMethod::Get, "/"))
        .await
        .unwrap();

    assert_eq!(response.status, 500);
    assert_eq!(response.body, b"/: internal error: response failed");
}

#[tokio::test]
async fn around_interceptors_can_recover_pipe_errors_before_filters() {
    struct RecoverBadRequest;

    impl Interceptor for RecoverBadRequest {
        fn intercept<'a>(
            &'a self,
            _context: ExecutionContext,
            next: CallHandler<'a>,
        ) -> BoxFuture<'a, Result<BootResponse>> {
            Box::pin(async move {
                match next.handle().await {
                    Err(BootError::BadRequest(message)) => {
                        Ok(BootResponse::text(format!("recovered: {message}")).with_status(422))
                    }
                    result => result,
                }
            })
        }
    }

    let filter_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let filter_counter = Arc::clone(&filter_calls);
    let route = RouteDefinition::post("/", |_| async { Ok(BootResponse::text("unreachable")) })
        .unwrap()
        .with_interceptor(RecoverBadRequest)
        .with_pipe(|_| async { Err(BootError::BadRequest("invalid input".to_string())) })
        .with_filter(move |_, _| {
            let filter_counter = Arc::clone(&filter_counter);
            async move {
                filter_counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Ok(Some(BootResponse::text("filtered").with_status(400)))
            }
        });

    let response = route
        .call(BootRequest::new(HttpMethod::Post, "/"))
        .await
        .unwrap();

    assert_eq!(response.status(), 422);
    assert_eq!(response.body, b"recovered: invalid input");
    assert_eq!(filter_calls.load(std::sync::atomic::Ordering::SeqCst), 0);
}

#[tokio::test]
async fn around_interceptors_can_retry_the_downstream_pipeline() {
    struct RetryOnce;

    impl Interceptor for RetryOnce {
        fn intercept<'a>(
            &'a self,
            _context: ExecutionContext,
            next: CallHandler<'a>,
        ) -> BoxFuture<'a, Result<BootResponse>> {
            Box::pin(async move {
                match next.handle().await {
                    Ok(response) => Ok(response),
                    Err(_) => next.handle().await,
                }
            })
        }
    }

    struct CountDownstreamInterceptor {
        before_calls: Arc<std::sync::atomic::AtomicUsize>,
        after_calls: Arc<std::sync::atomic::AtomicUsize>,
    }

    impl Interceptor for CountDownstreamInterceptor {
        fn before(&self, _context: ExecutionContext) -> BoxFuture<'static, Result<()>> {
            let calls = Arc::clone(&self.before_calls);
            Box::pin(async move {
                calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Ok(())
            })
        }

        fn after(
            &self,
            _context: ExecutionContext,
            response: BootResponse,
        ) -> BoxFuture<'static, Result<BootResponse>> {
            let calls = Arc::clone(&self.after_calls);
            Box::pin(async move {
                calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Ok(response)
            })
        }
    }

    let pipe_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let pipe_counter = Arc::clone(&pipe_calls);
    let handler_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let handler_counter = Arc::clone(&handler_calls);
    let inner_before_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let inner_after_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let route = RouteDefinition::get("/", move |_| {
        let handler_counter = Arc::clone(&handler_counter);
        async move {
            let attempt = handler_counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if attempt == 0 {
                return Err(BootError::Internal("try again".to_string()));
            }
            Ok(BootResponse::text("ok"))
        }
    })
    .unwrap()
    .with_interceptor(RetryOnce)
    .with_interceptor(CountDownstreamInterceptor {
        before_calls: Arc::clone(&inner_before_calls),
        after_calls: Arc::clone(&inner_after_calls),
    })
    .with_pipe(move |request: BootRequest| {
        let pipe_counter = Arc::clone(&pipe_counter);
        async move {
            pipe_counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(request)
        }
    });

    let response = route
        .call(BootRequest::new(HttpMethod::Get, "/"))
        .await
        .unwrap();

    assert_eq!(response.body, b"ok");
    assert_eq!(pipe_calls.load(std::sync::atomic::Ordering::SeqCst), 2);
    assert_eq!(handler_calls.load(std::sync::atomic::Ordering::SeqCst), 2);
    assert_eq!(
        inner_before_calls.load(std::sync::atomic::Ordering::SeqCst),
        2
    );
    assert_eq!(
        inner_after_calls.load(std::sync::atomic::Ordering::SeqCst),
        1
    );
}

#[tokio::test]
async fn call_handler_rejects_concurrent_calls_and_resets_after_cancellation() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<CallHandler<'static>>();

    let attempts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let observed = Arc::clone(&attempts);
    let handler = CallHandler::from_fn(move || {
        let observed = Arc::clone(&observed);
        async move {
            let attempt = observed.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if attempt == 0 {
                std::future::pending::<()>().await;
            }
            Ok(BootResponse::text("completed"))
        }
    });

    let running_handler = handler.clone();
    let running = tokio::spawn(async move { running_handler.handle().await });
    while attempts.load(std::sync::atomic::Ordering::SeqCst) == 0 {
        tokio::task::yield_now().await;
    }

    let error = handler.handle().await.unwrap_err();
    assert!(matches!(
        error,
        BootError::Internal(message) if message == "call handler is already running"
    ));

    running.abort();
    assert!(running.await.unwrap_err().is_cancelled());

    let response = handler.handle().await.unwrap();
    assert_eq!(response.body, b"completed");
    assert_eq!(attempts.load(std::sync::atomic::Ordering::SeqCst), 2);
}

#[tokio::test]
async fn around_interceptor_short_circuits_still_unwind_outer_legacy_hooks() {
    struct OuterHeader;

    impl Interceptor for OuterHeader {
        fn after(
            &self,
            _context: ExecutionContext,
            response: BootResponse,
        ) -> BoxFuture<'static, Result<BootResponse>> {
            Box::pin(async move { Ok(response.with_header("x-outer", "yes")) })
        }
    }

    struct ShortCircuit;

    impl Interceptor for ShortCircuit {
        fn intercept<'a>(
            &'a self,
            _context: ExecutionContext,
            _next: CallHandler<'a>,
        ) -> BoxFuture<'a, Result<BootResponse>> {
            Box::pin(async { Ok(BootResponse::text("cached").with_status(202)) })
        }
    }

    let handler_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let handler_counter = Arc::clone(&handler_calls);
    let route = RouteDefinition::get("/", move |_| {
        let handler_counter = Arc::clone(&handler_counter);
        async move {
            handler_counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(BootResponse::text("handler"))
        }
    })
    .unwrap()
    .with_interceptor(OuterHeader)
    .with_interceptor(ShortCircuit);

    let response = route
        .call(BootRequest::new(HttpMethod::Get, "/"))
        .await
        .unwrap();

    assert_eq!(response.status(), 202);
    assert_eq!(response.body, b"cached");
    assert_eq!(response.header("x-outer"), Some("yes"));
    assert_eq!(handler_calls.load(std::sync::atomic::Ordering::SeqCst), 0);
}

#[tokio::test]
async fn unrecovered_around_interceptor_errors_reach_filters_once() {
    struct MapToConflict;

    impl Interceptor for MapToConflict {
        fn intercept<'a>(
            &'a self,
            _context: ExecutionContext,
            next: CallHandler<'a>,
        ) -> BoxFuture<'a, Result<BootResponse>> {
            Box::pin(async move {
                match next.handle().await {
                    Err(BootError::BadRequest(message)) => {
                        Err(BootError::Conflict(format!("mapped: {message}")))
                    }
                    result => result,
                }
            })
        }
    }

    let filter_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let filter_counter = Arc::clone(&filter_calls);
    let route = RouteDefinition::get("/", |_| async {
        Err(BootError::BadRequest("invalid".to_string()))
    })
    .unwrap()
    .with_interceptor(MapToConflict)
    .with_filter(move |_, error: BootError| {
        let filter_counter = Arc::clone(&filter_counter);
        async move {
            filter_counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(Some(BootResponse::text(error.to_string()).with_status(409)))
        }
    });

    let response = route
        .call(BootRequest::new(HttpMethod::Get, "/"))
        .await
        .unwrap();

    assert_eq!(response.status(), 409);
    assert_eq!(response.body, b"resource conflict: mapped: invalid");
    assert_eq!(filter_calls.load(std::sync::atomic::Ordering::SeqCst), 1);
}

#[tokio::test]
async fn route_filters_can_decline_and_next_filter_sees_original_error() {
    let log = Arc::new(std::sync::Mutex::new(Vec::new()));
    let catch_log = Arc::clone(&log);
    let decline_log = Arc::clone(&log);
    let route = RouteDefinition::get("/", |_| async {
        Err(BootError::BadRequest("bad input".to_string()))
    })
    .unwrap()
    .with_filter(move |context: ExecutionContext, error: BootError| {
        let catch_log = Arc::clone(&catch_log);
        async move {
            catch_log.lock().unwrap().push(format!("catch:{error}"));
            Ok(Some(
                BootResponse::text(format!("{}: {error}", context.route_path)).with_status(400),
            ))
        }
    })
    .with_filter(move |_, _| {
        let decline_log = Arc::clone(&decline_log);
        async move {
            decline_log.lock().unwrap().push("decline".to_string());
            Ok(None)
        }
    });

    let response = route
        .call(BootRequest::new(HttpMethod::Get, "/"))
        .await
        .unwrap();

    assert_eq!(response.status, 400);
    assert_eq!(response.body, b"/: bad request: bad input");
    assert_eq!(
        log.lock().unwrap().as_slice(),
        ["decline", "catch:bad request: bad input"]
    );
}

#[tokio::test]
async fn route_filters_return_original_error_when_all_decline() {
    let route = RouteDefinition::get("/", |_| async {
        Err(BootError::BadRequest("bad input".to_string()))
    })
    .unwrap()
    .with_filter(|_, _| async { Ok(None) });

    let error = route
        .call(BootRequest::new(HttpMethod::Get, "/"))
        .await
        .unwrap_err();

    assert!(matches!(error, BootError::BadRequest(message) if message == "bad input"));
}

#[tokio::test]
async fn catch_filters_only_handle_matching_error_kinds() {
    let bad_request_route = RouteDefinition::get("/bad", |_| async {
        Err(BootError::BadRequest("bad input".to_string()))
    })
    .unwrap()
    .with_filter(|context: ExecutionContext, error: BootError| async move {
        Ok(Some(
            BootResponse::text(format!("fallback:{}:{error}", context.route_path)).with_status(500),
        ))
    })
    .with_catch_filter(
        [BootErrorKind::BadRequest],
        |context: ExecutionContext, error: BootError| async move {
            Ok(Some(
                BootResponse::text(format!("bad:{}:{error}", context.route_path)).with_status(400),
            ))
        },
    );

    let internal_route = RouteDefinition::get("/internal", |_| async {
        Err(BootError::Internal("boom".to_string()))
    })
    .unwrap()
    .with_filter(|context: ExecutionContext, error: BootError| async move {
        Ok(Some(
            BootResponse::text(format!("fallback:{}:{error}", context.route_path)).with_status(500),
        ))
    })
    .with_catch_filter(
        [BootErrorKind::BadRequest],
        |context: ExecutionContext, error: BootError| async move {
            Ok(Some(
                BootResponse::text(format!("bad:{}:{error}", context.route_path)).with_status(400),
            ))
        },
    );

    let conflict_route = RouteDefinition::get("/conflict", |_| async {
        Err(BootError::Conflict("duplicate cat".to_string()))
    })
    .unwrap()
    .with_filter(|context: ExecutionContext, error: BootError| async move {
        Ok(Some(
            BootResponse::text(format!("fallback:{}:{error}", context.route_path)).with_status(500),
        ))
    })
    .with_catch_filter(
        [BootErrorKind::Conflict],
        |context: ExecutionContext, error: BootError| async move {
            Ok(Some(
                BootResponse::text(format!("conflict:{}:{error}", context.route_path))
                    .with_status(409),
            ))
        },
    );

    let bad_response = bad_request_route
        .call(BootRequest::new(HttpMethod::Get, "/bad"))
        .await
        .unwrap();
    let internal_response = internal_route
        .call(BootRequest::new(HttpMethod::Get, "/internal"))
        .await
        .unwrap();
    let conflict_response = conflict_route
        .call(BootRequest::new(HttpMethod::Get, "/conflict"))
        .await
        .unwrap();

    assert_eq!(bad_response.status(), 400);
    assert_eq!(
        bad_response.body_text().unwrap(),
        "bad:/bad:bad request: bad input"
    );
    assert_eq!(internal_response.status(), 500);
    assert_eq!(
        internal_response.body_text().unwrap(),
        "fallback:/internal:internal error: boom"
    );
    assert_eq!(conflict_response.status(), 409);
    assert_eq!(
        conflict_response.body_text().unwrap(),
        "conflict:/conflict:resource conflict: duplicate cat"
    );
}

#[tokio::test]
async fn global_catch_filters_apply_to_matching_error_kinds() {
    let app = BootApplication::builder()
        .use_global_filter(|context: ExecutionContext, error: BootError| async move {
            Ok(Some(
                BootResponse::text(format!("fallback:{}:{error}", context.route_path))
                    .with_status(500),
            ))
        })
        .use_global_catch_filter(
            [BootErrorKind::Unauthorized],
            |context: ExecutionContext, error: BootError| async move {
                Ok(Some(
                    BootResponse::text(format!("auth:{}:{error}", context.route_path))
                        .with_status(401),
                ))
            },
        )
        .route(
            RouteDefinition::get("/private", |_| async {
                Err(BootError::Unauthorized("missing token".to_string()))
            })
            .unwrap(),
        )
        .build()
        .unwrap();

    let response = app
        .call(BootRequest::new(HttpMethod::Get, "/private"))
        .await
        .unwrap();

    assert_eq!(response.status(), 401);
    assert_eq!(
        response.body_text().unwrap(),
        "auth:/private:request was unauthorized: missing token"
    );
}

#[derive(Clone)]
struct TraceInterceptor {
    name: &'static str,
    log: Arc<std::sync::Mutex<Vec<String>>>,
}

impl TraceInterceptor {
    fn new(name: &'static str, log: Arc<std::sync::Mutex<Vec<String>>>) -> Self {
        Self { name, log }
    }
}

impl Interceptor for TraceInterceptor {
    fn before(&self, _context: ExecutionContext) -> BoxFuture<'static, Result<()>> {
        let name = self.name;
        let log = Arc::clone(&self.log);
        Box::pin(async move {
            log.lock().unwrap().push(format!("before:{name}"));
            Ok(())
        })
    }

    fn after(
        &self,
        _context: ExecutionContext,
        response: BootResponse,
    ) -> BoxFuture<'static, Result<BootResponse>> {
        let name = self.name;
        let log = Arc::clone(&self.log);
        Box::pin(async move {
            log.lock().unwrap().push(format!("after:{name}"));
            Ok(response)
        })
    }
}

#[derive(Clone)]
struct SharedExecutionInterceptor {
    log: Arc<std::sync::Mutex<Vec<String>>>,
}

#[derive(Clone)]
struct ProtocolLoggingGuard {
    log: Arc<std::sync::Mutex<Vec<String>>>,
}

impl Guard for ProtocolLoggingGuard {
    fn can_activate(&self, context: ExecutionContext) -> BoxFuture<'static, Result<bool>> {
        let log = Arc::clone(&self.log);
        Box::pin(async move {
            log.lock().unwrap().push(format!(
                "guard:{}:{}",
                context.protocol().as_str(),
                context.route_path
            ));
            Ok(true)
        })
    }
}

impl ExecutionInterceptor for SharedExecutionInterceptor {
    fn before(&self, context: ExecutionContext) -> BoxFuture<'static, Result<()>> {
        let log = Arc::clone(&self.log);
        Box::pin(async move {
            log.lock().unwrap().push(format!(
                "before:{}:{}",
                context.protocol().as_str(),
                context.route_path
            ));
            Ok(())
        })
    }

    fn after(&self, context: ExecutionContext) -> BoxFuture<'static, Result<()>> {
        let log = Arc::clone(&self.log);
        Box::pin(async move {
            log.lock().unwrap().push(format!(
                "after:{}:{}",
                context.protocol().as_str(),
                context.route_path
            ));
            Ok(())
        })
    }
}

fn trace_middleware(
    name: &'static str,
    log: Arc<std::sync::Mutex<Vec<String>>>,
) -> impl Fn(BootRequest) -> std::future::Ready<Result<MiddlewareOutcome>> + Send + Sync + 'static {
    move |request| {
        log.lock().unwrap().push(format!("middleware:{name}"));
        std::future::ready(Ok(MiddlewareOutcome::next(request)))
    }
}

#[derive(Debug)]
struct PipelineModule {
    log: Arc<std::sync::Mutex<Vec<String>>>,
}

impl Module for PipelineModule {
    fn name(&self) -> &'static str {
        "pipeline"
    }

    fn controllers(&self, _module_ref: &ModuleRef) -> Result<Vec<ControllerDefinition>> {
        let log = Arc::clone(&self.log);
        Ok(vec![ControllerDefinition::new("/pipeline")?
            .with_interceptor(TraceInterceptor::new("controller", Arc::clone(&self.log)))
            .get("/", move |_| {
                let log = Arc::clone(&log);
                async move {
                    log.lock().unwrap().push("handler".to_string());
                    Ok(BootResponse::text("ok"))
                }
            })?])
    }
}

#[tokio::test]
async fn global_and_controller_interceptors_wrap_route_in_order() {
    let log = Arc::new(std::sync::Mutex::new(Vec::new()));
    let app = BootApplication::builder()
        .use_global_interceptor(TraceInterceptor::new("global", Arc::clone(&log)))
        .import(PipelineModule {
            log: Arc::clone(&log),
        })
        .build()
        .unwrap();

    let response = app.routes()[0]
        .call(BootRequest::new(HttpMethod::Get, "/pipeline"))
        .await
        .unwrap();

    assert_eq!(response.body, b"ok");
    assert_eq!(
        log.lock().unwrap().as_slice(),
        [
            "before:global",
            "before:controller",
            "handler",
            "after:controller",
            "after:global"
        ]
    );
}

#[derive(Debug)]
struct LateControllerPipelineModule {
    log: Arc<std::sync::Mutex<Vec<String>>>,
}

impl Module for LateControllerPipelineModule {
    fn name(&self) -> &'static str {
        "late-controller-pipeline"
    }

    fn controllers(&self, _module_ref: &ModuleRef) -> Result<Vec<ControllerDefinition>> {
        let log = Arc::clone(&self.log);
        Ok(vec![ControllerDefinition::new("/late-pipeline")?
            .get("/", move |_| {
                let log = Arc::clone(&log);
                async move {
                    log.lock().unwrap().push("handler".to_string());
                    Ok(BootResponse::text("ok"))
                }
            })?
            .with_interceptor(TraceInterceptor::new("first", Arc::clone(&self.log)))
            .with_interceptor(TraceInterceptor::new(
                "second",
                Arc::clone(&self.log),
            ))])
    }
}

#[tokio::test]
async fn controller_pipeline_applies_to_existing_routes() {
    let log = Arc::new(std::sync::Mutex::new(Vec::new()));
    let app = BootApplication::builder()
        .import(LateControllerPipelineModule {
            log: Arc::clone(&log),
        })
        .build()
        .unwrap();

    let response = app
        .call(BootRequest::new(HttpMethod::Get, "/late-pipeline"))
        .await
        .unwrap();

    assert_eq!(response.body, b"ok");
    assert_eq!(
        log.lock().unwrap().as_slice(),
        [
            "before:first",
            "before:second",
            "handler",
            "after:second",
            "after:first"
        ]
    );
}

#[derive(Debug)]
struct MiddlewareOrderModule {
    log: Arc<std::sync::Mutex<Vec<String>>>,
}

impl Module for MiddlewareOrderModule {
    fn name(&self) -> &'static str {
        "middleware-order"
    }

    fn middleware(&self) -> Vec<Arc<dyn Middleware>> {
        vec![Arc::new(trace_middleware("module", Arc::clone(&self.log)))]
    }

    fn controllers(&self, _module_ref: &ModuleRef) -> Result<Vec<ControllerDefinition>> {
        let log = Arc::clone(&self.log);
        let pipe_log = Arc::clone(&self.log);
        let guard_log = Arc::clone(&self.log);
        let handler_log = Arc::clone(&self.log);

        let route = RouteDefinition::get("/", move |_| {
            let handler_log = Arc::clone(&handler_log);
            async move {
                handler_log.lock().unwrap().push("handler".to_string());
                Ok(BootResponse::text("ok"))
            }
        })?
        .with_middleware(trace_middleware("route", Arc::clone(&log)))
        .with_pipe(move |request: BootRequest| {
            let pipe_log = Arc::clone(&pipe_log);
            async move {
                pipe_log.lock().unwrap().push("pipe".to_string());
                Ok(request)
            }
        })
        .with_guard(move |_| {
            let guard_log = Arc::clone(&guard_log);
            async move {
                guard_log.lock().unwrap().push("guard".to_string());
                Ok(true)
            }
        })
        .with_interceptor(TraceInterceptor::new("route", Arc::clone(&self.log)));

        Ok(vec![ControllerDefinition::new("/middleware-order")?
            .with_middleware(trace_middleware("controller", Arc::clone(&self.log)))
            .route(route)?])
    }
}

#[tokio::test]
async fn middleware_runs_before_guards_interceptors_pipes_and_handlers() {
    let log = Arc::new(std::sync::Mutex::new(Vec::new()));
    let app = BootApplication::builder()
        .use_global_middleware(trace_middleware("global", Arc::clone(&log)))
        .import(MiddlewareOrderModule {
            log: Arc::clone(&log),
        })
        .build()
        .unwrap();

    let response = app
        .call(BootRequest::new(HttpMethod::Get, "/middleware-order"))
        .await
        .unwrap();

    assert_eq!(response.body, b"ok");
    assert_eq!(
        log.lock().unwrap().as_slice(),
        [
            "middleware:global",
            "middleware:module",
            "middleware:controller",
            "middleware:route",
            "guard",
            "before:route",
            "pipe",
            "handler",
            "after:route"
        ]
    );
}

#[tokio::test]
async fn route_scoped_middleware_only_applies_to_that_route() {
    let app = BootApplication::builder()
        .route(
            RouteDefinition::get("/scoped", |request: BootRequest| async move {
                Ok(BootResponse::text(
                    request.header("x-route").unwrap_or("missing"),
                ))
            })
            .unwrap()
            .with_middleware(|request: BootRequest| async move {
                Ok(MiddlewareOutcome::next(
                    request.with_header("x-route", "scoped"),
                ))
            }),
        )
        .route(
            RouteDefinition::get("/plain", |request: BootRequest| async move {
                Ok(BootResponse::text(
                    request.header("x-route").unwrap_or("missing"),
                ))
            })
            .unwrap(),
        )
        .build()
        .unwrap();

    let scoped = app
        .call(BootRequest::new(HttpMethod::Get, "/scoped"))
        .await
        .unwrap();
    let plain = app
        .call(BootRequest::new(HttpMethod::Get, "/plain"))
        .await
        .unwrap();

    assert_eq!(scoped.body, b"scoped");
    assert_eq!(plain.body, b"missing");
}

#[derive(Debug)]
struct GuardedControllerModule;

impl Module for GuardedControllerModule {
    fn name(&self) -> &'static str {
        "guarded-controller"
    }

    fn controllers(&self, _module_ref: &ModuleRef) -> Result<Vec<ControllerDefinition>> {
        Ok(vec![ControllerDefinition::new("/items")?
            .get("/", |_| async { Ok(BootResponse::text("ok")) })?])
    }
}

#[tokio::test]
async fn global_guards_and_filters_apply_to_controller_routes() {
    let app = BootApplication::builder()
        .use_global_guard(|_| async { Ok(false) })
        .use_global_filter(|context: ExecutionContext, error: BootError| async move {
            Ok(Some(
                BootResponse::text(format!("{}: {error}", context.route_path)).with_status(403),
            ))
        })
        .import(GuardedControllerModule)
        .build()
        .unwrap();

    let response = app.routes()[0]
        .call(BootRequest::new(HttpMethod::Get, "/items"))
        .await
        .unwrap();

    assert_eq!(response.status, 403);
    assert_eq!(
        String::from_utf8(response.body).unwrap(),
        "/items: request was forbidden: GET /items"
    );
}

#[tokio::test]
async fn application_call_method_not_allowed_errors_use_route_filters() {
    let app = BootApplication::builder()
        .use_global_filter(|context: ExecutionContext, error: BootError| async move {
            Ok(Some(
                BootResponse::text(format!("{}: {error}", context.route_path)).with_status(405),
            ))
        })
        .route(
            RouteDefinition::get("/items/{id}", |_| async {
                Ok(BootResponse::text("unreachable"))
            })
            .unwrap(),
        )
        .build()
        .unwrap();

    let response = app
        .call(BootRequest::new(HttpMethod::Post, "/items/hammer"))
        .await
        .unwrap();

    assert_eq!(response.status, 405);
    assert_eq!(
        String::from_utf8(response.body).unwrap(),
        "/items/{id}: method is not allowed: POST /items/hammer"
    );
}

#[tokio::test]
async fn application_call_path_param_decode_errors_use_route_filters() {
    let app = BootApplication::builder()
        .use_global_filter(|context: ExecutionContext, error: BootError| async move {
            Ok(Some(
                BootResponse::text(format!("{}: {error}", context.route_path)).with_status(400),
            ))
        })
        .route(
            RouteDefinition::get("/files/{path}", |_| async {
                Ok(BootResponse::text("unreachable"))
            })
            .unwrap(),
        )
        .build()
        .unwrap();

    let response = app
        .call(BootRequest::new(HttpMethod::Get, "/files/%ZZ"))
        .await
        .unwrap();

    assert_eq!(response.status, 400);
    assert_eq!(
        String::from_utf8(response.body).unwrap(),
        "/files/{path}: bad request: invalid percent encoding: %ZZ"
    );
}

#[tokio::test]
async fn guard_forbidden_errors_use_actual_request_path() {
    let route = RouteDefinition::get("/items/{id}", |_| async {
        Ok(BootResponse::text("unreachable"))
    })
    .unwrap()
    .with_guard(|_| async { Ok(false) });

    let error = route
        .call(BootRequest::new(HttpMethod::Get, "/items/hammer"))
        .await
        .unwrap_err();

    assert!(matches!(
        error,
        BootError::Forbidden(message) if message == "GET /items/hammer"
    ));
}

#[tokio::test]
async fn guard_unauthorized_errors_use_route_filters() {
    let route = RouteDefinition::get("/private", |_| async {
        Ok(BootResponse::text("unreachable"))
    })
    .unwrap()
    .with_guard(|context: ExecutionContext| async move {
        let _token = context.request.require_bearer_token()?;
        Ok(true)
    })
    .with_filter(|context: ExecutionContext, error: BootError| async move {
        Ok(Some(
            BootResponse::text(format!("{}: {error}", context.route_path)).with_status(401),
        ))
    });

    let response = route
        .call(BootRequest::new(HttpMethod::Get, "/private"))
        .await
        .unwrap();
    let ok = route
        .call(
            BootRequest::new(HttpMethod::Get, "/private")
                .with_header("authorization", "Bearer token-123"),
        )
        .await
        .unwrap();

    assert_eq!(response.status, 401);
    assert_eq!(
        String::from_utf8(response.body).unwrap(),
        "/private: request was unauthorized: missing bearer token"
    );
    assert_eq!(ok.body, b"unreachable");
}

#[tokio::test]
async fn guards_can_read_route_metadata_from_execution_context() {
    let route = RouteDefinition::get("/admin", |_| async { Ok(BootResponse::text("admin")) })
        .unwrap()
        .with_metadata_value("roles", json!(["admin"]))
        .with_guard(|context: ExecutionContext| async move {
            let roles = context
                .metadata_as::<Vec<String>>("roles")?
                .unwrap_or_default();
            Ok(roles.iter().any(|role| role == "admin"))
        });

    let response = route
        .call(BootRequest::new(HttpMethod::Get, "/admin"))
        .await
        .unwrap();

    assert_eq!(response.body, b"admin");
}

#[tokio::test]
async fn guard_session_cookie_errors_use_route_filters() {
    let route = RouteDefinition::get("/session", |_| async {
        Ok(BootResponse::text("session ok"))
    })
    .unwrap()
    .with_guard(|context: ExecutionContext| async move {
        let _session = context.request.require_cookie("session")?;
        Ok(true)
    })
    .with_filter(|context: ExecutionContext, error: BootError| async move {
        Ok(Some(
            BootResponse::text(format!("{}: {error}", context.route_path)).with_status(401),
        ))
    });

    let response = route
        .call(BootRequest::new(HttpMethod::Get, "/session"))
        .await
        .unwrap();
    let ok = route
        .call(BootRequest::new(HttpMethod::Get, "/session").with_header("cookie", "session=abc"))
        .await
        .unwrap();

    assert_eq!(response.status, 401);
    assert_eq!(
        String::from_utf8(response.body).unwrap(),
        "/session: request was unauthorized: missing cookie: session"
    );
    assert_eq!(ok.body, b"session ok");
}
