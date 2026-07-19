use super::AnyProvider;
use crate::{BootError, Result};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::thread::ThreadId;

static NEXT_PROVIDER_CACHE_KEY: AtomicU64 = AtomicU64::new(1);
static NEXT_PROVIDER_BUILD_ID: AtomicU64 = AtomicU64::new(1);
static PROVIDER_WAIT_GRAPH: OnceLock<Mutex<HashMap<ThreadId, ProviderWaitEdge>>> = OnceLock::new();

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct ProviderCacheKey(u64);

impl ProviderCacheKey {
    pub(crate) fn next() -> Self {
        Self(NEXT_PROVIDER_CACHE_KEY.fetch_add(1, Ordering::Relaxed))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum ProviderInstanceKey {
    Provider(ProviderCacheKey),
    Transient {
        provider: ProviderCacheKey,
        inquirer: ProviderCacheKey,
    },
}

impl From<ProviderCacheKey> for ProviderInstanceKey {
    fn from(value: ProviderCacheKey) -> Self {
        Self::Provider(value)
    }
}

#[derive(Clone, Default)]
pub(crate) struct ProviderCache {
    slots: Arc<Mutex<BTreeMap<ProviderInstanceKey, Arc<ProviderCacheSlot>>>>,
}

#[derive(Default)]
struct ProviderCacheSlot {
    state: Mutex<ProviderCacheSlotState>,
    ready: Condvar,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ProviderBuild {
    id: u64,
    builder: ThreadId,
}

impl ProviderBuild {
    fn new(builder: ThreadId) -> Self {
        Self {
            id: NEXT_PROVIDER_BUILD_ID.fetch_add(1, Ordering::Relaxed),
            builder,
        }
    }
}

#[derive(Default)]
enum ProviderCacheSlotState {
    #[default]
    Vacant,
    Building(ProviderBuild),
    Ready(Arc<AnyProvider>),
}

#[derive(Debug, Clone, Copy)]
struct ProviderWaitEdge {
    builder: ThreadId,
    build_id: u64,
}

struct ProviderWaitRegistration {
    waiter: ThreadId,
    build_id: u64,
}

impl ProviderWaitRegistration {
    fn new(build: ProviderBuild) -> Result<Self> {
        let waiter = std::thread::current().id();
        let mut graph = provider_wait_graph()
            .lock()
            .map_err(|_| BootError::Internal("provider wait graph lock is poisoned".to_string()))?;
        let mut cursor = build.builder;
        let mut visited = HashSet::new();
        while let Some(edge) = graph.get(&cursor).copied() {
            let next = edge.builder;
            if next == waiter || !visited.insert(cursor) {
                return Err(BootError::Internal(
                    "cyclic concurrent provider dependency detected".to_string(),
                ));
            }
            cursor = next;
        }
        if cursor == waiter {
            return Err(BootError::Internal(
                "cyclic concurrent provider dependency detected".to_string(),
            ));
        }
        if graph.contains_key(&waiter) {
            return Err(BootError::Internal(
                "provider resolver thread is already waiting".to_string(),
            ));
        }
        graph.insert(
            waiter,
            ProviderWaitEdge {
                builder: build.builder,
                build_id: build.id,
            },
        );
        Ok(Self {
            waiter,
            build_id: build.id,
        })
    }
}

impl Drop for ProviderWaitRegistration {
    fn drop(&mut self) {
        let mut graph = provider_wait_graph()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if graph
            .get(&self.waiter)
            .is_some_and(|edge| edge.build_id == self.build_id)
        {
            graph.remove(&self.waiter);
        }
    }
}

struct ProviderBuildReset {
    slot: Arc<ProviderCacheSlot>,
    build: ProviderBuild,
    armed: bool,
}

impl ProviderBuildReset {
    fn new(slot: Arc<ProviderCacheSlot>, build: ProviderBuild) -> Self {
        Self {
            slot,
            build,
            armed: true,
        }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for ProviderBuildReset {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }

        let mut state = self
            .slot
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if finish_provider_build(&mut state, self.build, ProviderCacheSlotState::Vacant) {
            self.slot.ready.notify_all();
        }
    }
}

impl ProviderCache {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn contains(&self, key: impl Into<ProviderInstanceKey>) -> Result<bool> {
        Ok(self.get(key)?.is_some())
    }

    pub(crate) fn get(
        &self,
        key: impl Into<ProviderInstanceKey>,
    ) -> Result<Option<Arc<AnyProvider>>> {
        let slot = self.slot(key.into())?;
        let state = slot
            .state
            .lock()
            .map_err(|_| BootError::Internal("provider cache lock is poisoned".to_string()))?;
        Ok(match &*state {
            ProviderCacheSlotState::Ready(value) => Some(Arc::clone(value)),
            ProviderCacheSlotState::Vacant | ProviderCacheSlotState::Building(_) => None,
        })
    }

    pub(crate) fn insert(
        &self,
        key: impl Into<ProviderInstanceKey>,
        value: Arc<AnyProvider>,
    ) -> Result<()> {
        let slot = self.slot(key.into())?;
        let mut state = slot
            .state
            .lock()
            .map_err(|_| BootError::Internal("provider cache lock is poisoned".to_string()))?;
        let active_build = match &*state {
            ProviderCacheSlotState::Building(build) => Some(build.id),
            ProviderCacheSlotState::Vacant | ProviderCacheSlotState::Ready(_) => None,
        };
        *state = ProviderCacheSlotState::Ready(value);
        if let Some(build_id) = active_build {
            clear_provider_waits(build_id);
        }
        slot.ready.notify_all();
        Ok(())
    }

    pub(crate) fn get_or_try_insert_with<F>(
        &self,
        key: impl Into<ProviderInstanceKey>,
        build: F,
    ) -> Result<Arc<AnyProvider>>
    where
        F: FnOnce() -> Result<Arc<AnyProvider>>,
    {
        let slot = self.slot(key.into())?;
        let mut build = Some(build);

        loop {
            let mut state = slot
                .state
                .lock()
                .map_err(|_| BootError::Internal("provider cache lock is poisoned".to_string()))?;
            match &*state {
                ProviderCacheSlotState::Ready(value) => return Ok(Arc::clone(value)),
                ProviderCacheSlotState::Building(build) => {
                    let waiting = ProviderWaitRegistration::new(*build)?;
                    state = slot.ready.wait(state).map_err(|_| {
                        BootError::Internal("provider cache lock is poisoned".to_string())
                    })?;
                    drop(state);
                    drop(waiting);
                }
                ProviderCacheSlotState::Vacant => {
                    let active_build = ProviderBuild::new(std::thread::current().id());
                    *state = ProviderCacheSlotState::Building(active_build);
                    drop(state);

                    let mut reset = ProviderBuildReset::new(Arc::clone(&slot), active_build);
                    let result = build.take().ok_or_else(|| {
                        BootError::Internal(
                            "provider cache builder was already consumed".to_string(),
                        )
                    })?();

                    let mut state = slot.state.lock().map_err(|_| {
                        BootError::Internal("provider cache lock is poisoned".to_string())
                    })?;
                    match result {
                        Ok(value) => {
                            if !finish_provider_build(
                                &mut state,
                                active_build,
                                ProviderCacheSlotState::Ready(Arc::clone(&value)),
                            ) {
                                return Err(BootError::Internal(
                                    "provider cache build state changed unexpectedly".to_string(),
                                ));
                            }
                            reset.disarm();
                            slot.ready.notify_all();
                            return Ok(value);
                        }
                        Err(error) => {
                            if !finish_provider_build(
                                &mut state,
                                active_build,
                                ProviderCacheSlotState::Vacant,
                            ) {
                                return Err(BootError::Internal(
                                    "provider cache build state changed unexpectedly".to_string(),
                                ));
                            }
                            reset.disarm();
                            slot.ready.notify_all();
                            return Err(error);
                        }
                    }
                }
            }
        }
    }

    fn slot(&self, key: ProviderInstanceKey) -> Result<Arc<ProviderCacheSlot>> {
        let mut slots = self
            .slots
            .lock()
            .map_err(|_| BootError::Internal("provider cache lock is poisoned".to_string()))?;
        Ok(Arc::clone(slots.entry(key).or_default()))
    }
}

fn provider_wait_graph() -> &'static Mutex<HashMap<ThreadId, ProviderWaitEdge>> {
    PROVIDER_WAIT_GRAPH.get_or_init(|| Mutex::new(HashMap::new()))
}

fn clear_provider_waits(build_id: u64) {
    provider_wait_graph()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .retain(|_, edge| edge.build_id != build_id);
}

fn finish_provider_build(
    state: &mut ProviderCacheSlotState,
    build: ProviderBuild,
    finished: ProviderCacheSlotState,
) -> bool {
    if !matches!(
        state,
        ProviderCacheSlotState::Building(active) if active.id == build.id
    ) {
        return false;
    }

    *state = finished;
    clear_provider_waits(build.id);
    true
}

pub(crate) fn new_provider_cache() -> ProviderCache {
    ProviderCache::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn completed_build_stops_participating_in_cycles_before_waiter_drop() {
        let (id_sender, id_receiver) = mpsc::channel();
        let (build_sender, build_receiver) = mpsc::channel();
        let (result_sender, result_receiver) = mpsc::channel();
        let builder_thread = std::thread::spawn(move || {
            id_sender.send(std::thread::current().id()).unwrap();
            let reverse_build = build_receiver.recv().unwrap();
            let result = ProviderWaitRegistration::new(reverse_build).map(drop);
            result_sender.send(result).unwrap();
        });

        let builder = id_receiver.recv_timeout(Duration::from_secs(2)).unwrap();
        let completed_build = ProviderBuild::new(builder);
        let slot = ProviderCacheSlot::default();
        *slot.state.lock().unwrap() = ProviderCacheSlotState::Building(completed_build);

        let lingering_registration = ProviderWaitRegistration::new(completed_build).unwrap();
        {
            let graph = provider_wait_graph().lock().unwrap();
            assert_eq!(
                graph
                    .get(&std::thread::current().id())
                    .map(|edge| edge.build_id),
                Some(completed_build.id)
            );
        }

        let value: Arc<AnyProvider> = Arc::new(());
        assert!(finish_provider_build(
            &mut slot.state.lock().unwrap(),
            completed_build,
            ProviderCacheSlotState::Ready(value),
        ));
        assert!(!provider_wait_graph()
            .lock()
            .unwrap()
            .contains_key(&std::thread::current().id()));

        build_sender
            .send(ProviderBuild::new(std::thread::current().id()))
            .unwrap();
        result_receiver
            .recv_timeout(Duration::from_secs(2))
            .unwrap()
            .unwrap();

        drop(lingering_registration);
        builder_thread.join().unwrap();
    }
}
