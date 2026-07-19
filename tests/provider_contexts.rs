use a3s_boot::{
    BootApplication, BootError, BootRequest, BootResponse, ContextId, ContextIdFactory,
    FromModuleRef, HttpMethod, Module, ModuleRef, ProviderDefinition, ProviderDependency,
    ProviderRef, ProviderToken, Result, RouteDefinition,
};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Barrier, Condvar, Mutex};
use std::thread;
use std::time::Duration;

fn assert_send_sync<T: Send + Sync>() {}

#[derive(Debug)]
struct RequestValue {
    id: usize,
}

#[derive(Debug)]
struct TransientValue {
    id: usize,
}

#[derive(Debug)]
struct FirstConsumer {
    first: Arc<TransientValue>,
    second: Arc<TransientValue>,
}

#[derive(Debug)]
struct SecondConsumer {
    value: Arc<TransientValue>,
}

#[derive(Debug)]
struct LazyConsumer {
    eager: Arc<TransientValue>,
    lazy: ProviderRef<TransientValue>,
}

#[derive(Debug)]
struct PublishedLazyRoot;

#[derive(Debug)]
struct PublishedLazyDependency {
    root: Arc<PublishedLazyRoot>,
}

#[derive(Debug)]
struct StaticLazyRequestConsumer {
    request: ProviderRef<RequestValue>,
}

#[derive(Debug)]
struct ScopedLazyRequestConsumer {
    request: ProviderRef<RequestValue>,
    drops: Arc<AtomicUsize>,
}

impl Drop for ScopedLazyRequestConsumer {
    fn drop(&mut self) {
        self.drops.fetch_add(1, Ordering::SeqCst);
    }
}

#[derive(Debug)]
struct CreatedConsumer {
    first: Arc<TransientValue>,
    second: Arc<TransientValue>,
}

#[derive(Debug)]
struct SharedDependency;

#[derive(Debug)]
struct ConcurrentRootA {
    dependency: Arc<SharedDependency>,
}

#[derive(Debug)]
struct ConcurrentRootB {
    dependency: Arc<SharedDependency>,
}

#[derive(Debug)]
struct PanicRecoveryConsumer {
    dependency: Arc<RequestValue>,
}

#[derive(Debug)]
struct ParallelDependency;

#[derive(Debug)]
struct ParallelConsumer {
    dependency: Arc<ParallelDependency>,
}

#[derive(Debug)]
struct ParallelRoot {
    consumer: Arc<ParallelConsumer>,
    dependency: Arc<ParallelDependency>,
}

impl FromModuleRef for CreatedConsumer {
    fn from_module_ref(module_ref: &ModuleRef) -> Result<Self> {
        Ok(Self {
            first: module_ref.get::<TransientValue>()?,
            second: module_ref.get::<TransientValue>()?,
        })
    }

    fn provider_dependencies() -> Option<Vec<ProviderDependency>> {
        Some(vec![ProviderDependency::typed::<TransientValue>()])
    }
}

#[test]
fn context_ids_are_unique_shareable_and_exposed_by_scoped_module_refs() {
    assert_send_sync::<ContextId>();

    let first = ContextIdFactory::create();
    let first_clone = first.clone();
    let second = ContextIdFactory::create();
    let module_ref = ModuleRef::new();
    let scoped = module_ref.context_scope(&first);
    assert_eq!(first, first_clone);
    assert_ne!(first, second);
    assert_ne!(first.id(), second.id());
    assert_eq!(scoped.context_id(), Some(&first));
    assert!(format!("{first:?}").contains(&first.id().to_string()));
}

#[test]
fn explicit_context_ids_reuse_request_providers_across_resolve_calls() {
    let calls = Arc::new(AtomicUsize::new(0));
    let module_ref = ModuleRef::new();
    let provider_calls = Arc::clone(&calls);
    module_ref
        .register(ProviderDefinition::request_scoped::<RequestValue, _>(
            move |_| {
                Ok(RequestValue {
                    id: provider_calls.fetch_add(1, Ordering::SeqCst) + 1,
                })
            },
        ))
        .unwrap();
    module_ref
        .register(ProviderDefinition::named_alias(
            "request-value",
            ProviderToken::of::<RequestValue>(),
        ))
        .unwrap();

    let first_context = ContextIdFactory::create();
    let second_context = ContextIdFactory::create();
    let first = module_ref
        .resolve_with_context::<RequestValue>(&first_context)
        .unwrap();
    let same = module_ref
        .resolve_with_context::<RequestValue>(&first_context)
        .unwrap();
    let same_named = module_ref
        .resolve_named_with_context::<RequestValue>("request-value", &first_context)
        .unwrap();
    let same_scoped = module_ref
        .context_scope(&first_context)
        .get::<RequestValue>()
        .unwrap();
    let second = module_ref
        .resolve_with_context::<RequestValue>(&second_context)
        .unwrap();
    let fresh = module_ref.resolve::<RequestValue>().unwrap();

    assert_eq!(first.id, 1);
    assert!(Arc::ptr_eq(&first, &same));
    assert!(Arc::ptr_eq(&first, &same_named));
    assert!(Arc::ptr_eq(&first, &same_scoped));
    assert_eq!(second.id, 2);
    assert_eq!(fresh.id, 3);
    assert!(!Arc::ptr_eq(&first, &second));
    assert!(!Arc::ptr_eq(&second, &fresh));
    assert!(module_ref
        .resolve_optional_with_context::<FirstConsumer>(&first_context)
        .unwrap()
        .is_none());
    assert!(module_ref
        .resolve_optional_named_with_context::<FirstConsumer>("missing", &first_context)
        .unwrap()
        .is_none());
    assert_eq!(calls.load(Ordering::SeqCst), 3);
}

#[test]
fn provider_refs_resolve_in_an_explicit_context() {
    let calls = Arc::new(AtomicUsize::new(0));
    let module_ref = ModuleRef::new();
    let provider_calls = Arc::clone(&calls);
    module_ref
        .register(ProviderDefinition::request_scoped::<RequestValue, _>(
            move |_| {
                Ok(RequestValue {
                    id: provider_calls.fetch_add(1, Ordering::SeqCst) + 1,
                })
            },
        ))
        .unwrap();

    let provider_ref = module_ref.provider_ref::<RequestValue>();
    let first_context = ContextIdFactory::create();
    let second_context = ContextIdFactory::create();
    let first = provider_ref.resolve_with_context(&first_context).unwrap();
    let same = provider_ref.resolve_with_context(&first_context).unwrap();
    let second = provider_ref.resolve_with_context(&second_context).unwrap();
    let fresh = provider_ref.resolve().unwrap();

    assert!(Arc::ptr_eq(&first, &same));
    assert!(!Arc::ptr_eq(&first, &second));
    assert!(!Arc::ptr_eq(&second, &fresh));
    assert_eq!((first.id, second.id, fresh.id), (1, 2, 3));
    assert_eq!(calls.load(Ordering::SeqCst), 3);
}

#[test]
fn provider_refs_detach_the_active_construction_path() {
    let module_ref = ModuleRef::new();
    module_ref
        .register(
            ProviderDefinition::request_scoped::<PublishedLazyDependency, _>(|module_ref| {
                Ok(PublishedLazyDependency {
                    root: module_ref.get::<PublishedLazyRoot>()?,
                })
            })
            .depends_on::<PublishedLazyRoot>(),
        )
        .unwrap();

    let child_ready = Arc::new(Barrier::new(2));
    let (sender, receiver) = mpsc::channel();
    module_ref
        .register(
            ProviderDefinition::request_scoped::<PublishedLazyRoot, _>(move |module_ref| {
                let dependency = module_ref.provider_ref::<PublishedLazyDependency>();
                let thread_ready = Arc::clone(&child_ready);
                let sender = sender.clone();
                thread::spawn(move || {
                    thread_ready.wait();
                    sender.send(dependency.get()).unwrap();
                });
                child_ready.wait();
                thread::sleep(Duration::from_millis(25));
                Ok(PublishedLazyRoot)
            })
            .with_dependency(ProviderDependency::typed::<PublishedLazyDependency>().lazy()),
        )
        .unwrap();

    let context_id = ContextIdFactory::create();
    let root = module_ref
        .resolve_with_context::<PublishedLazyRoot>(&context_id)
        .unwrap();
    let dependency = receiver
        .recv_timeout(Duration::from_secs(2))
        .unwrap()
        .unwrap();

    assert!(Arc::ptr_eq(&root, &dependency.root));
}

#[test]
fn transient_providers_are_reused_per_inquirer_without_changing_root_get_compatibility() {
    let calls = Arc::new(AtomicUsize::new(0));
    let module_ref = ModuleRef::new();
    let provider_calls = Arc::clone(&calls);
    module_ref
        .register(ProviderDefinition::transient::<TransientValue, _>(
            move |_| {
                Ok(TransientValue {
                    id: provider_calls.fetch_add(1, Ordering::SeqCst) + 1,
                })
            },
        ))
        .unwrap();
    module_ref
        .register(
            ProviderDefinition::factory::<FirstConsumer, _>(|module_ref| {
                Ok(FirstConsumer {
                    first: module_ref.get::<TransientValue>()?,
                    second: module_ref.get::<TransientValue>()?,
                })
            })
            .depends_on::<TransientValue>(),
        )
        .unwrap();
    module_ref
        .register(
            ProviderDefinition::factory::<SecondConsumer, _>(|module_ref| {
                Ok(SecondConsumer {
                    value: module_ref.get::<TransientValue>()?,
                })
            })
            .depends_on::<TransientValue>(),
        )
        .unwrap();
    module_ref
        .register(
            ProviderDefinition::factory::<LazyConsumer, _>(|module_ref| {
                Ok(LazyConsumer {
                    eager: module_ref.get::<TransientValue>()?,
                    lazy: module_ref.provider_ref::<TransientValue>(),
                })
            })
            .with_dependencies([
                ProviderDependency::typed::<TransientValue>(),
                ProviderDependency::typed::<TransientValue>().lazy(),
            ]),
        )
        .unwrap();

    let first = module_ref.get::<FirstConsumer>().unwrap();
    let second = module_ref.get::<SecondConsumer>().unwrap();
    let lazy = module_ref.get::<LazyConsumer>().unwrap();
    let lazy_value = lazy.lazy.get().unwrap();
    let direct_first = module_ref.get::<TransientValue>().unwrap();
    let direct_second = module_ref.get::<TransientValue>().unwrap();

    assert!(Arc::ptr_eq(&first.first, &first.second));
    assert!(!Arc::ptr_eq(&first.first, &second.value));
    assert!(Arc::ptr_eq(&lazy.eager, &lazy_value));
    assert!(!Arc::ptr_eq(&direct_first, &direct_second));
    assert_ne!(first.first.id, second.value.id);
    assert_ne!(direct_first.id, direct_second.id);
    assert_eq!(calls.load(Ordering::SeqCst), 5);
}

#[test]
fn static_singletons_do_not_capture_the_context_that_first_resolves_them() {
    let calls = Arc::new(AtomicUsize::new(0));
    let module_ref = ModuleRef::new();
    let provider_calls = Arc::clone(&calls);
    module_ref
        .register(ProviderDefinition::request_scoped::<RequestValue, _>(
            move |_| {
                Ok(RequestValue {
                    id: provider_calls.fetch_add(1, Ordering::SeqCst) + 1,
                })
            },
        ))
        .unwrap();
    module_ref
        .register(
            ProviderDefinition::factory::<StaticLazyRequestConsumer, _>(|module_ref| {
                Ok(StaticLazyRequestConsumer {
                    request: module_ref.provider_ref::<RequestValue>(),
                })
            })
            .with_dependency(ProviderDependency::typed::<RequestValue>().lazy()),
        )
        .unwrap();

    let context_id = ContextIdFactory::create();
    let consumer = module_ref
        .resolve_with_context::<StaticLazyRequestConsumer>(&context_id)
        .unwrap();

    assert!(consumer.request.module_ref().context_id().is_none());
    assert!(matches!(
        consumer.request.get(),
        Err(BootError::Internal(message))
            if message.contains("requires an active request scope")
    ));
    assert_eq!(consumer.request.resolve().unwrap().id, 1);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn request_contexts_release_scoped_providers_that_hold_lazy_provider_refs() {
    let drops = Arc::new(AtomicUsize::new(0));
    let module_ref = ModuleRef::new();
    module_ref
        .register(ProviderDefinition::request_scoped::<RequestValue, _>(
            |_| Ok(RequestValue { id: 1 }),
        ))
        .unwrap();
    let provider_drops = Arc::clone(&drops);
    module_ref
        .register(
            ProviderDefinition::request_scoped::<ScopedLazyRequestConsumer, _>(move |module_ref| {
                Ok(ScopedLazyRequestConsumer {
                    request: module_ref.provider_ref::<RequestValue>(),
                    drops: Arc::clone(&provider_drops),
                })
            })
            .with_dependency(ProviderDependency::typed::<RequestValue>().lazy()),
        )
        .unwrap();

    let weak_consumer = {
        let context_id = ContextIdFactory::create();
        let consumer = module_ref
            .resolve_with_context::<ScopedLazyRequestConsumer>(&context_id)
            .unwrap();
        assert_eq!(consumer.request.get().unwrap().id, 1);
        let weak_consumer = Arc::downgrade(&consumer);

        drop(consumer);
        assert!(weak_consumer.upgrade().is_some());
        weak_consumer
    };

    assert!(
        weak_consumer.upgrade().is_none(),
        "the released ContextId must not be retained by its cached provider"
    );
    assert_eq!(drops.load(Ordering::SeqCst), 1);
}

#[test]
fn transient_instances_are_scoped_by_context_and_consumer() {
    let calls = Arc::new(AtomicUsize::new(0));
    let module_ref = ModuleRef::new();
    let provider_calls = Arc::clone(&calls);
    module_ref
        .register(ProviderDefinition::transient::<TransientValue, _>(
            move |_| {
                Ok(TransientValue {
                    id: provider_calls.fetch_add(1, Ordering::SeqCst) + 1,
                })
            },
        ))
        .unwrap();
    module_ref
        .register(
            ProviderDefinition::request_scoped::<FirstConsumer, _>(|module_ref| {
                Ok(FirstConsumer {
                    first: module_ref.get::<TransientValue>()?,
                    second: module_ref.get::<TransientValue>()?,
                })
            })
            .depends_on::<TransientValue>(),
        )
        .unwrap();
    module_ref
        .register(
            ProviderDefinition::request_scoped::<SecondConsumer, _>(|module_ref| {
                Ok(SecondConsumer {
                    value: module_ref.get::<TransientValue>()?,
                })
            })
            .depends_on::<TransientValue>(),
        )
        .unwrap();

    let first_context = ContextIdFactory::create();
    let second_context = ContextIdFactory::create();
    let first = module_ref
        .resolve_with_context::<FirstConsumer>(&first_context)
        .unwrap();
    let first_again = module_ref
        .resolve_with_context::<FirstConsumer>(&first_context)
        .unwrap();
    let other_consumer = module_ref
        .resolve_with_context::<SecondConsumer>(&first_context)
        .unwrap();
    let other_context = module_ref
        .resolve_with_context::<FirstConsumer>(&second_context)
        .unwrap();
    let root_first = module_ref
        .resolve_with_context::<TransientValue>(&first_context)
        .unwrap();
    let root_again = module_ref
        .resolve_with_context::<TransientValue>(&first_context)
        .unwrap();

    assert!(Arc::ptr_eq(&first, &first_again));
    assert!(Arc::ptr_eq(&first.first, &first.second));
    assert!(!Arc::ptr_eq(&first.first, &other_consumer.value));
    assert!(!Arc::ptr_eq(&first.first, &other_context.first));
    assert!(Arc::ptr_eq(&root_first, &root_again));
    assert!(!Arc::ptr_eq(&root_first, &first.first));
    assert_eq!(calls.load(Ordering::SeqCst), 4);
}

#[test]
fn module_ref_create_assigns_a_distinct_synthetic_inquirer() {
    let calls = Arc::new(AtomicUsize::new(0));
    let module_ref = ModuleRef::new();
    let provider_calls = Arc::clone(&calls);
    module_ref
        .register(ProviderDefinition::transient::<TransientValue, _>(
            move |_| {
                Ok(TransientValue {
                    id: provider_calls.fetch_add(1, Ordering::SeqCst) + 1,
                })
            },
        ))
        .unwrap();

    let first = module_ref.create::<CreatedConsumer>().unwrap();
    let second = module_ref.create::<CreatedConsumer>().unwrap();

    assert!(Arc::ptr_eq(&first.first, &first.second));
    assert!(Arc::ptr_eq(&second.first, &second.second));
    assert!(!Arc::ptr_eq(&first.first, &second.first));
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[test]
fn concurrent_resolution_in_one_context_runs_a_factory_once() {
    let calls = Arc::new(AtomicUsize::new(0));
    let module_ref = ModuleRef::new();
    let provider_calls = Arc::clone(&calls);
    module_ref
        .register(ProviderDefinition::request_scoped::<RequestValue, _>(
            move |_| {
                provider_calls.fetch_add(1, Ordering::SeqCst);
                thread::sleep(Duration::from_millis(25));
                Ok(RequestValue { id: 1 })
            },
        ))
        .unwrap();

    let context_id = ContextIdFactory::create();
    let barrier = Arc::new(Barrier::new(3));
    let handles = (0..2)
        .map(|_| {
            let module_ref = module_ref.clone();
            let context_id = context_id.clone();
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                barrier.wait();
                module_ref.resolve_with_context::<RequestValue>(&context_id)
            })
        })
        .collect::<Vec<_>>();
    barrier.wait();

    let mut values = handles
        .into_iter()
        .map(|handle| handle.join().unwrap().unwrap())
        .collect::<Vec<_>>();
    let second = values.pop().unwrap();
    let first_value = values.pop().unwrap();

    assert!(Arc::ptr_eq(&first_value, &second));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn concurrent_roots_can_share_a_dependency_without_a_false_cycle() {
    let module_ref = ModuleRef::new();
    let shared_calls = Arc::new(AtomicUsize::new(0));
    let root_calls = Arc::new(AtomicUsize::new(0));
    let provider_calls = Arc::clone(&shared_calls);
    module_ref
        .register(ProviderDefinition::request_scoped::<SharedDependency, _>(
            move |_| {
                provider_calls.fetch_add(1, Ordering::SeqCst);
                thread::sleep(Duration::from_millis(25));
                Ok(SharedDependency)
            },
        ))
        .unwrap();

    let roots_ready = Arc::new(Barrier::new(2));
    let a_ready = Arc::clone(&roots_ready);
    let a_calls = Arc::clone(&root_calls);
    module_ref
        .register(
            ProviderDefinition::request_scoped::<ConcurrentRootA, _>(move |module_ref| {
                a_calls.fetch_add(1, Ordering::SeqCst);
                a_ready.wait();
                Ok(ConcurrentRootA {
                    dependency: module_ref.get::<SharedDependency>()?,
                })
            })
            .depends_on::<SharedDependency>(),
        )
        .unwrap();
    let b_ready = Arc::clone(&roots_ready);
    let b_calls = Arc::clone(&root_calls);
    module_ref
        .register(
            ProviderDefinition::request_scoped::<ConcurrentRootB, _>(move |module_ref| {
                b_calls.fetch_add(1, Ordering::SeqCst);
                b_ready.wait();
                Ok(ConcurrentRootB {
                    dependency: module_ref.get::<SharedDependency>()?,
                })
            })
            .depends_on::<SharedDependency>(),
        )
        .unwrap();

    let context_id = ContextIdFactory::create();
    let first_ref = module_ref.clone();
    let first_context = context_id.clone();
    let first =
        thread::spawn(move || first_ref.resolve_with_context::<ConcurrentRootA>(&first_context));
    let second =
        thread::spawn(move || module_ref.resolve_with_context::<ConcurrentRootB>(&context_id));

    let first = first.join().unwrap().unwrap();
    let second = second.join().unwrap().unwrap();

    assert!(Arc::ptr_eq(&first.dependency, &second.dependency));
    assert_eq!(shared_calls.load(Ordering::SeqCst), 1);
    assert_eq!(root_calls.load(Ordering::SeqCst), 2);
}

#[test]
fn failed_context_construction_releases_the_single_flight_slot() {
    let calls = Arc::new(AtomicUsize::new(0));
    let module_ref = ModuleRef::new();
    let provider_calls = Arc::clone(&calls);
    module_ref
        .register(ProviderDefinition::request_scoped::<RequestValue, _>(
            move |_| {
                let call = provider_calls.fetch_add(1, Ordering::SeqCst) + 1;
                if call == 1 {
                    return Err(BootError::Internal("first construction failed".to_string()));
                }
                Ok(RequestValue { id: call })
            },
        ))
        .unwrap();

    let context_id = ContextIdFactory::create();
    let first = module_ref.resolve_with_context::<RequestValue>(&context_id);
    let second = module_ref
        .resolve_with_context::<RequestValue>(&context_id)
        .unwrap();
    let same = module_ref
        .resolve_with_context::<RequestValue>(&context_id)
        .unwrap();

    assert!(matches!(
        first,
        Err(BootError::Internal(message)) if message == "first construction failed"
    ));
    assert_eq!(second.id, 2);
    assert!(Arc::ptr_eq(&second, &same));
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[test]
fn panicking_context_construction_releases_waiters_and_allows_retry() {
    let calls = Arc::new(AtomicUsize::new(0));
    let first_started = Arc::new(Barrier::new(2));
    let module_ref = ModuleRef::new();
    let provider_calls = Arc::clone(&calls);
    let provider_started = Arc::clone(&first_started);
    module_ref
        .register(ProviderDefinition::request_scoped::<RequestValue, _>(
            move |_| {
                let call = provider_calls.fetch_add(1, Ordering::SeqCst) + 1;
                if call == 1 {
                    provider_started.wait();
                    thread::sleep(Duration::from_millis(25));
                    panic!("first construction panicked");
                }
                Ok(RequestValue { id: call })
            },
        ))
        .unwrap();

    let context_id = ContextIdFactory::create();
    let (panic_sender, panic_receiver) = mpsc::channel();
    let (result_sender, result_receiver) = mpsc::channel();
    let first_ref = module_ref.clone();
    let first_context = context_id.clone();
    thread::spawn(move || {
        let panicked = catch_unwind(AssertUnwindSafe(|| {
            first_ref.resolve_with_context::<RequestValue>(&first_context)
        }))
        .is_err();
        panic_sender.send(panicked).unwrap();
    });
    first_started.wait();

    thread::spawn(move || {
        let result = module_ref
            .resolve_with_context::<RequestValue>(&context_id)
            .map(|value| value.id);
        result_sender.send(result).unwrap();
    });

    assert!(panic_receiver.recv_timeout(Duration::from_secs(2)).unwrap());
    assert_eq!(
        result_receiver
            .recv_timeout(Duration::from_secs(2))
            .unwrap()
            .unwrap(),
        2
    );
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[test]
fn caught_dependency_panics_do_not_leave_stale_resolution_frames() {
    let calls = Arc::new(AtomicUsize::new(0));
    let module_ref = ModuleRef::new();
    let provider_calls = Arc::clone(&calls);
    module_ref
        .register(ProviderDefinition::request_scoped::<RequestValue, _>(
            move |_| {
                let call = provider_calls.fetch_add(1, Ordering::SeqCst) + 1;
                if call == 1 {
                    panic!("first dependency construction panicked");
                }
                Ok(RequestValue { id: call })
            },
        ))
        .unwrap();
    module_ref
        .register(
            ProviderDefinition::request_scoped::<PanicRecoveryConsumer, _>(|module_ref| {
                assert!(
                    catch_unwind(AssertUnwindSafe(|| module_ref.get::<RequestValue>())).is_err()
                );
                Ok(PanicRecoveryConsumer {
                    dependency: module_ref.get::<RequestValue>()?,
                })
            })
            .depends_on::<RequestValue>(),
        )
        .unwrap();

    let consumer = module_ref
        .resolve_with_context::<PanicRecoveryConsumer>(&ContextIdFactory::create())
        .unwrap();

    assert_eq!(consumer.dependency.id, 2);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[test]
fn parallel_resolution_branches_do_not_share_sibling_frames() {
    let module_ref = ModuleRef::new();
    let factories_ready = Arc::new(Barrier::new(2));
    let consumer_started = Arc::new((Mutex::new(false), Condvar::new()));
    let dependency_calls = Arc::new(AtomicUsize::new(0));

    let dependency_ready = Arc::clone(&factories_ready);
    let dependency_signal = Arc::clone(&consumer_started);
    let provider_calls = Arc::clone(&dependency_calls);
    module_ref
        .register(ProviderDefinition::request_scoped::<ParallelDependency, _>(
            move |_| {
                provider_calls.fetch_add(1, Ordering::SeqCst);
                dependency_ready.wait();
                let (started, ready) = &*dependency_signal;
                let mut started = started.lock().unwrap();
                while !*started {
                    started = ready.wait(started).unwrap();
                }
                drop(started);
                thread::sleep(Duration::from_millis(25));
                Ok(ParallelDependency)
            },
        ))
        .unwrap();

    let consumer_ready = Arc::clone(&factories_ready);
    let consumer_signal = Arc::clone(&consumer_started);
    module_ref
        .register(
            ProviderDefinition::request_scoped::<ParallelConsumer, _>(move |module_ref| {
                consumer_ready.wait();
                let (started, ready) = &*consumer_signal;
                *started.lock().unwrap() = true;
                ready.notify_all();
                Ok(ParallelConsumer {
                    dependency: module_ref.get::<ParallelDependency>()?,
                })
            })
            .depends_on::<ParallelDependency>(),
        )
        .unwrap();
    module_ref
        .register(
            ProviderDefinition::request_scoped::<ParallelRoot, _>(|module_ref| {
                let consumer_ref = module_ref.clone();
                let dependency_ref = module_ref.clone();
                let (consumer, dependency) = thread::scope(|scope| {
                    let consumer = scope.spawn(move || consumer_ref.get::<ParallelConsumer>());
                    let dependency =
                        scope.spawn(move || dependency_ref.get::<ParallelDependency>());
                    (consumer.join().unwrap(), dependency.join().unwrap())
                });
                Ok(ParallelRoot {
                    consumer: consumer?,
                    dependency: dependency?,
                })
            })
            .with_dependencies([
                ProviderDependency::typed::<ParallelConsumer>(),
                ProviderDependency::typed::<ParallelDependency>(),
            ]),
        )
        .unwrap();

    let root = module_ref
        .resolve_with_context::<ParallelRoot>(&ContextIdFactory::create())
        .unwrap();

    assert!(Arc::ptr_eq(&root.consumer.dependency, &root.dependency));
    assert_eq!(dependency_calls.load(Ordering::SeqCst), 1);
}

#[derive(Debug)]
struct SelfCycle;

#[test]
fn context_cache_waiting_does_not_hide_provider_cycles() {
    let module_ref = ModuleRef::new();
    module_ref
        .register(
            ProviderDefinition::transient::<SelfCycle, _>(|module_ref| {
                let _ = module_ref.get::<SelfCycle>()?;
                Ok(SelfCycle)
            })
            .depends_on::<SelfCycle>(),
        )
        .unwrap();

    let error = module_ref
        .resolve_with_context::<SelfCycle>(&ContextIdFactory::create())
        .unwrap_err();

    assert!(matches!(
        error,
        BootError::Internal(message)
            if message.contains("cyclic provider dependency detected")
                && message.contains("SelfCycle")
    ));
}

#[derive(Debug)]
struct ConcurrentCycleA {
    _dependency: Arc<ConcurrentCycleB>,
}

#[derive(Debug)]
struct ConcurrentCycleB {
    _dependency: Arc<ConcurrentCycleA>,
}

#[test]
fn concurrent_context_roots_detect_cross_thread_dependency_cycles() {
    let module_ref = ModuleRef::new();
    let barrier = Arc::new(Barrier::new(2));
    let a_calls = Arc::new(AtomicUsize::new(0));
    let b_calls = Arc::new(AtomicUsize::new(0));
    let a_barrier = Arc::clone(&barrier);
    let a_factory_calls = Arc::clone(&a_calls);
    module_ref
        .register(
            ProviderDefinition::request_scoped::<ConcurrentCycleA, _>(move |module_ref| {
                if a_factory_calls.fetch_add(1, Ordering::SeqCst) == 0 {
                    a_barrier.wait();
                }
                Ok(ConcurrentCycleA {
                    _dependency: module_ref.get::<ConcurrentCycleB>()?,
                })
            })
            .depends_on::<ConcurrentCycleB>(),
        )
        .unwrap();
    let b_barrier = Arc::clone(&barrier);
    let b_factory_calls = Arc::clone(&b_calls);
    module_ref
        .register(
            ProviderDefinition::request_scoped::<ConcurrentCycleB, _>(move |module_ref| {
                if b_factory_calls.fetch_add(1, Ordering::SeqCst) == 0 {
                    b_barrier.wait();
                }
                Ok(ConcurrentCycleB {
                    _dependency: module_ref.get::<ConcurrentCycleA>()?,
                })
            })
            .depends_on::<ConcurrentCycleA>(),
        )
        .unwrap();

    let context_id = ContextIdFactory::create();
    let (sender, receiver) = mpsc::channel();
    let first_ref = module_ref.clone();
    let first_context = context_id.clone();
    let first_sender = sender.clone();
    thread::spawn(move || {
        let result = first_ref
            .resolve_with_context::<ConcurrentCycleA>(&first_context)
            .map(|_| ());
        first_sender.send(result).unwrap();
    });
    let second_ref = module_ref.clone();
    thread::spawn(move || {
        let result = second_ref
            .resolve_with_context::<ConcurrentCycleB>(&context_id)
            .map(|_| ());
        sender.send(result).unwrap();
    });

    let first = receiver.recv_timeout(Duration::from_secs(2)).unwrap();
    let second = receiver.recv_timeout(Duration::from_secs(2)).unwrap();

    for result in [first, second] {
        assert!(matches!(
            result,
            Err(BootError::Internal(message))
                if message.contains("cyclic") && message.contains("provider dependency")
        ));
    }
}

#[derive(Debug)]
struct ContextRouteModule {
    calls: Arc<AtomicUsize>,
}

impl Module for ContextRouteModule {
    fn name(&self) -> &'static str {
        "context-route"
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        let calls = Arc::clone(&self.calls);
        Ok(vec![ProviderDefinition::request_scoped::<RequestValue, _>(
            move |_| {
                Ok(RequestValue {
                    id: calls.fetch_add(1, Ordering::SeqCst) + 1,
                })
            },
        )])
    }

    fn routes(&self) -> Result<Vec<RouteDefinition>> {
        Ok(vec![RouteDefinition::get(
            "/context-id",
            |request: BootRequest| async move {
                let context_id = request.context_id().ok_or_else(|| {
                    BootError::Internal("route request is missing a context id".to_string())
                })?;
                let discovered = ContextIdFactory::get_by_request(&request);
                if &discovered != context_id {
                    return Err(BootError::Internal(
                        "request context discovery returned a different id".to_string(),
                    ));
                }
                let first = request.get::<RequestValue>()?;
                let second = request.get::<RequestValue>()?;
                Ok(BootResponse::text(format!(
                    "{}:{}:{}",
                    context_id.id(),
                    first.id,
                    Arc::ptr_eq(&first, &second)
                )))
            },
        )?])
    }
}

#[tokio::test]
async fn http_routes_expose_one_context_id_per_request() {
    let calls = Arc::new(AtomicUsize::new(0));
    let app = BootApplication::builder()
        .import(ContextRouteModule {
            calls: Arc::clone(&calls),
        })
        .build()
        .unwrap();

    let first = app
        .call(BootRequest::new(HttpMethod::Get, "/context-id"))
        .await
        .unwrap();
    let second = app
        .call(BootRequest::new(HttpMethod::Get, "/context-id"))
        .await
        .unwrap();
    let first_body = first.body_text().unwrap();
    let second_body = second.body_text().unwrap();
    let first_parts = first_body.split(':').collect::<Vec<_>>();
    let second_parts = second_body.split(':').collect::<Vec<_>>();

    assert_ne!(first_parts[0], second_parts[0]);
    assert_eq!(&first_parts[1..], ["1", "true"]);
    assert_eq!(&second_parts[1..], ["2", "true"]);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}
