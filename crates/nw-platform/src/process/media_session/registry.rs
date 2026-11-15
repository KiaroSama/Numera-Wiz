//! Fixed process-lifetime ownership, independent of every caller runtime.
use super::*;
use std::sync::{Mutex, OnceLock};

const PARTITION: u64 = 25 * 1024 * 1024;
struct ChargeCell {
    charge: Option<Box<dyn SessionCharge>>,
    mark_attempted: bool,
    callback_failed: bool,
}
struct Slot {
    shared: Arc<Shared>,
    coordinator: Option<std::thread::JoinHandle<CoordinatorDone>>,
    charge: Arc<Mutex<ChargeCell>>,
    joining: bool,
}
struct Registry {
    slots: [Option<Slot>; 2],
    next: u64,
}
static REGISTRY: OnceLock<Mutex<Registry>> = OnceLock::new();
fn registry() -> &'static Mutex<Registry> {
    REGISTRY.get_or_init(|| Mutex::new(Registry { slots: [None, None], next: 1 }))
}
fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    // No external callbacks execute while the registry lock is held.
    mutex.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

pub(super) fn admit(inputs: [ByteInput; 2], parent: DirectoryStamp, policy: SessionPolicy, charge: Box<dyn SessionCharge>, cancel: CancellationToken) -> SessionResult<Client> {
    admit_started(inputs, parent, policy, charge, cancel, start_coordinator)
}
pub(super) fn start_coordinator(shared: Arc<Shared>) -> std::io::Result<std::thread::JoinHandle<CoordinatorDone>> {
    std::thread::Builder::new().name("nw-relay-session".into()).stack_size(2 * 1024 * 1024).spawn(move || {
        while !shared.registered.load(Ordering::Acquire) { std::thread::yield_now(); }
        coordinator(shared)
    })
}
pub(super) fn admit_started(inputs: [ByteInput; 2], parent: DirectoryStamp, policy: SessionPolicy, charge: Box<dyn SessionCharge>, cancel: CancellationToken, start: impl FnOnce(Arc<Shared>) -> std::io::Result<std::thread::JoinHandle<CoordinatorDone>>) -> SessionResult<Client> {
    let _ = reconcile();
    let mut entries = lock(registry());
    let id = SessionId(entries.next);
    let next = entries.next.checked_add(1);
    if let Some(next) = next { entries.next = next; }
    let free = entries.slots.iter().position(Option::is_none);
    if next.is_none() || free.is_none() {
        drop(entries);
        let closed = ClosedSession::empty(id);
        let released = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| charge.release(&closed)));
        return Err(SessionFailure { cause: error(if released.is_err() { "SESSION_CHARGE_CALLBACK_FAILED" } else { "RESOURCE_LIMIT" }, "Owned relay admission was rejected before creating resources"), settlement: Settlement::Closed(closed) });
    }
    let shared = Arc::new(Shared::new(id, policy, cancel, inputs, parent));
    let charge = Arc::new(Mutex::new(ChargeCell { charge: Some(charge), mark_attempted: false, callback_failed: false }));
    let index = free.unwrap_or(0);
    entries.slots[index] = Some(Slot { shared: shared.clone(), coordinator: None, charge, joining: false });
    // Both charge and input ownership now live in the slot, before channel,
    // runtime, endpoint, buffer or thread creation.
    let thread = start(shared.clone());
    match thread {
        Ok(handle) => {
            if let Some(slot) = entries.slots[index].as_mut() { slot.coordinator = Some(handle); }
            shared.registered.store(true, Ordering::Release);
            drop(entries);
            Ok(Client { shared, armed: true })
        }
        Err(problem) => {
            let slot = entries.slots[index].take();
            drop(entries);
            drop(lock(&shared.mailbox).preparation.take());
            let closed = ClosedSession::empty(id);
            let callback_failed = slot.is_some_and(|slot| release_charge(&slot.charge, &closed));
            Err(SessionFailure { cause: error(if callback_failed { "SESSION_CHARGE_CALLBACK_FAILED" } else { "SESSION_THREAD_FAILED" }, &problem.to_string()), settlement: Settlement::Closed(closed) })
        }
    }
}

fn release_charge(cell: &Arc<Mutex<ChargeCell>>, closed: &ClosedSession) -> bool {
    let mut charge = lock(cell);
    let callback = charge.charge.take();
    let release_failed = callback.is_some_and(|callback| std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| callback.release(closed))).is_err());
    charge.callback_failed || release_failed
}

pub(super) fn uncertain(id: SessionId) -> bool {
    let cell = {
        let entries = lock(registry());
        entries.slots.iter().flatten().find(|slot| slot.shared.id == id).map(|slot| {
            slot.shared.retained.store(true, Ordering::Release);
            slot.charge.clone()
        })
    };
    if let Some(cell) = cell {
        let mut state = lock(&cell);
        if !state.mark_attempted {
            state.mark_attempted = true;
            if let Some(charge) = state.charge.as_mut() {
                state.callback_failed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| charge.mark_uncertain())).is_err();
            }
        }
        state.callback_failed
    } else { false }
}

pub(super) fn reconcile() -> Result<usize> {
    let mut count = 0;
    loop {
        let candidate = {
            let mut entries = lock(registry());
            entries.slots.iter_mut().flatten().find(|slot| !slot.joining && slot.coordinator.as_ref().is_some_and(std::thread::JoinHandle::is_finished)).map(|slot| {
                slot.joining = true;
                (slot.shared.clone(), slot.charge.clone(), slot.coordinator.take())
            })
        };
        let Some((shared, charge, handle)) = candidate else { break; };
        let Some(handle) = handle else { return Err(error("SESSION_SETTLEMENT_UNCERTAIN", "Coordinator join ownership was lost")); };
        let done = match handle.join() {
            Ok(done) => done,
            Err(_) => {
                uncertain(shared.id);
                return Err(error("SESSION_SETTLEMENT_UNCERTAIN", "Coordinator escaped its unwind guard"));
            }
        };
        let closed = ClosedSession { facts: Box::new(ClosedFacts { id: shared.id, pumps: done.pumps, child_spawned: done.child_spawned, child_reaped: done.child_reaped, outputs_joined: done.outputs_joined, namespace_cleaned: done.namespace_cleaned, coordinator_joined: true, stdout_bytes: shared.stdout_bytes.load(Ordering::Acquire), diagnostic_bytes: shared.diagnostic_bytes.load(Ordering::Acquire) }) };
        let callback_failed = release_charge(&charge, &closed);
        let result = if callback_failed {
            Err(SessionFailure { cause: error("SESSION_CHARGE_CALLBACK_FAILED", "A charge callback panicked; native resources are closed"), settlement: Settlement::Closed(closed) })
        } else {
            match done.output {
                Ok(output) => Ok(SessionOutput { output, closed }),
                Err(cause) => Err(SessionFailure { cause, settlement: Settlement::Closed(closed) }),
            }
        };
        lock(&shared.mailbox).finished = Some(result);
        let mut entries = lock(registry());
        if let Some(index) = entries.slots.iter().position(|slot| slot.as_ref().is_some_and(|slot| slot.shared.id == shared.id)) {
            entries.slots[index] = None;
            count += 1;
        }
    }
    Ok(count)
}

pub(super) fn status() -> RegistryStatus {
    let entries = lock(registry());
    let mut status = RegistryStatus::default();
    for slot in entries.slots.iter().flatten() {
        status.admitted += 1;
        status.charged_bytes += PARTITION;
        status.retained += usize::from(slot.shared.retained.load(Ordering::Acquire));
        status.coordinator_threads += usize::from(slot.coordinator.is_some() || slot.joining);
        status.input_threads += slot.shared.input_threads.load(Ordering::Acquire);
        status.closed_unjoined += usize::from(slot.coordinator.as_ref().is_some_and(std::thread::JoinHandle::is_finished));
    }
    status
}

#[cfg(test)]
pub(super) fn foundation_cases() {
    use std::sync::atomic::AtomicUsize;
    struct TestCharge(Arc<AtomicUsize>);
    impl SessionCharge for TestCharge {
        fn mark_uncertain(&mut self) { self.0.store(2, Ordering::Release); }
        fn release(self: Box<Self>, closed: &ClosedSession) {
            assert!(closed.coordinator_joined() || (!closed.child_spawned() && closed.pumps().iter().all(|pump| !pump.endpoint_created())));
            self.0.store(0, Ordering::Release);
        }
    }
    struct Gate(Arc<AtomicBool>);
    impl Drop for Gate { fn drop(&mut self) { self.0.store(true, Ordering::Release); } }
    let root = tempfile::tempdir().unwrap();
    let root_path = std::fs::canonicalize(root.path()).unwrap();
    let parent_path = crate::security::private_directory(&root_path.join("foundation")).unwrap();
    let inputs = || std::array::from_fn(|id| {
        let path = root_path.join(format!("foundation-source-{id}"));
        std::fs::write(&path, [id as u8]).unwrap();
        ByteInput::new(RelayInputId(id as u8), SourceStamp::capture_resolved(&path).unwrap(), 1, Sha256::digest([id as u8]).into()).unwrap()
    });
    assert_eq!(status().admitted(), 0);
    let failed_charge = Arc::new(AtomicUsize::new(1));
    let failed = admit_started(inputs(), DirectoryStamp::capture_resolved(&parent_path).unwrap(), SessionPolicy::byte_fixture(), Box::new(TestCharge(failed_charge.clone())), CancellationToken::new(), |_| Err(std::io::Error::other("Synthetic thread-start failure")));
    match failed {
        Err(SessionFailure { cause, settlement: Settlement::Closed(closed) }) => { assert_eq!(cause.code, "SESSION_THREAD_FAILED"); assert!(!closed.coordinator_joined()); assert!(closed.pumps().iter().all(|pump| !pump.endpoint_created())); },
        _ => panic!("Thread-start failure must close only an empty reservation"),
    }
    assert_eq!(failed_charge.load(Ordering::Acquire), 0);
    assert_eq!(status().admitted(), 0);
    let mut owners = Vec::new();
    for _ in 0..2 {
        let charged = Arc::new(AtomicUsize::new(1));
        let gate = Arc::new(AtomicBool::new(false));
        let mut entries = lock(registry());
        let id = SessionId(entries.next);
        entries.next = entries.next.checked_add(1).unwrap();
        let shared = Arc::new(Shared { id, policy: SessionPolicy::byte_fixture(), cancel: CancellationToken::new(), registered: AtomicBool::new(false), retained: AtomicBool::new(false), input_threads: Arc::new(AtomicUsize::new(0)), stdout_bytes: AtomicU64::new(0), diagnostic_bytes: AtomicU64::new(0), mailbox: Mutex::new(Mailbox { preparation: None, invocation: None, ready: false, spawned: false, cause: None, finished: None, direct: false, endpoints: None, preparation_control: None }) });
        let index = entries.slots.iter().position(Option::is_none).unwrap();
        entries.slots[index] = Some(Slot { shared: shared.clone(), coordinator: None, charge: Arc::new(Mutex::new(ChargeCell { charge: Some(Box::new(TestCharge(charged.clone()))), mark_attempted: false, callback_failed: false })), joining: false });
        let worker_shared = shared.clone();
        let worker_gate = gate.clone();
        let handle = std::thread::Builder::new().stack_size(2 * 1024 * 1024).spawn(move || {
            while !worker_shared.registered.load(Ordering::Acquire) { std::thread::yield_now(); }
            while !worker_gate.load(Ordering::Acquire) { std::thread::sleep(Duration::from_millis(1)); }
            CoordinatorDone { output: Err(error("CANCELLED", "Finite ordinary-thread ownership fixture")), pumps: [PumpReceipt::not_created(RelayInputId(0)), PumpReceipt::not_created(RelayInputId(1))], child_spawned: false, child_reaped: false, outputs_joined: false, namespace_cleaned: true }
        }).unwrap();
        entries.slots[index].as_mut().unwrap().coordinator = Some(handle);
        shared.registered.store(true, Ordering::Release);
        drop(entries);
        if owners.is_empty() { uncertain(shared.id); }
        owners.push((Client { shared, armed: true }, charged, Gate(gate)));
    }
    assert_eq!(status().admitted(), 2);
    assert_eq!(status().charged_bytes(), 50 * 1024 * 1024);
    let rejected_charge = Arc::new(AtomicUsize::new(1));
    let rejected = admit(inputs(), DirectoryStamp::capture_resolved(&parent_path).unwrap(), SessionPolicy::byte_fixture(), Box::new(TestCharge(rejected_charge.clone())), CancellationToken::new());
    assert!(matches!(rejected, Err(SessionFailure { cause, settlement: Settlement::Closed(_) }) if cause.code == "RESOURCE_LIMIT"));
    assert_eq!(rejected_charge.load(Ordering::Acquire), 0);
    assert_eq!(status().admitted(), 2);
    let (first, first_charge, first_gate) = owners.remove(0);
    let first_cancel = first.shared.cancel.clone();
    uncertain(first.shared.id);
    uncertain(first.shared.id);
    drop(first);
    assert!(first_cancel.is_cancelled());
    assert_eq!(reconcile().unwrap(), 0);
    assert_eq!(first_charge.load(Ordering::Acquire), 2);
    assert_eq!(status().coordinator_threads(), 2);
    // Release all finite gates before any assertion capable of stranding a
    // later group; actual joins, rather than gate opening, return capacity.
    first_gate.0.store(true, Ordering::Release);
    let (_, second_charge, second_gate) = &owners[0];
    let stop = Instant::now() + Duration::from_secs(2);
    while first_charge.load(Ordering::Acquire) != 0 && Instant::now() < stop { reconcile().unwrap(); std::thread::sleep(Duration::from_millis(1)); }
    let second_still_charged = second_charge.load(Ordering::Acquire) == 1;
    second_gate.0.store(true, Ordering::Release);
    let first_closed = first_charge.load(Ordering::Acquire) == 0;
    while second_charge.load(Ordering::Acquire) != 0 && Instant::now() < stop { reconcile().unwrap(); std::thread::sleep(Duration::from_millis(1)); }
    assert!(first_closed);
    assert!(second_still_charged);
    assert_eq!(second_charge.load(Ordering::Acquire), 0);
    assert_eq!(status().admitted(), 0);
    callback_cases();
}


#[cfg(test)]
fn callback_cases() {
    use std::sync::atomic::AtomicUsize;
    struct Counts { marks: AtomicUsize, releases: AtomicUsize, drops: AtomicUsize }
    struct Charge { counts: Arc<Counts>, mark_panics: bool, release_panics: bool }
    impl Drop for Charge { fn drop(&mut self) { self.counts.drops.fetch_add(1, Ordering::AcqRel); } }
    impl SessionCharge for Charge {
        fn mark_uncertain(&mut self) {
            self.counts.marks.fetch_add(1, Ordering::AcqRel);
            assert!(!self.mark_panics, "finite charge mark unwind");
        }
        fn release(self: Box<Self>, closed: &ClosedSession) {
            assert!(closed.coordinator_joined() || (!closed.child_spawned() && closed.pumps().iter().all(|pump| !pump.endpoint_created())));
            self.counts.releases.fetch_add(1, Ordering::AcqRel);
            assert!(!self.release_panics, "finite consuming release unwind");
        }
    }
    struct Gate(Arc<AtomicBool>);
    impl Drop for Gate { fn drop(&mut self) { self.0.store(true, Ordering::Release); } }
    let root = tempfile::tempdir().unwrap();
    let path = std::fs::canonicalize(root.path()).unwrap();
    let parent = crate::security::private_directory(&path.join("callbacks")).unwrap();
    let inputs = || std::array::from_fn(|id| {
        let source = path.join(format!("callback-{id}"));
        std::fs::write(&source, [id as u8]).unwrap();
        ByteInput::new(RelayInputId(id as u8), SourceStamp::capture_resolved(&source).unwrap(), 1, Sha256::digest([id as u8]).into()).unwrap()
    });
    let counts = || Arc::new(Counts { marks: AtomicUsize::new(0), releases: AtomicUsize::new(0), drops: AtomicUsize::new(0) });
    let rejected = counts();
    let failure = admit_started(inputs(), DirectoryStamp::capture_resolved(&parent).unwrap(), SessionPolicy::byte_fixture(), Box::new(Charge { counts: rejected.clone(), mark_panics: false, release_panics: true }), CancellationToken::new(), |_| Err(std::io::Error::other("finite callback start rejection")));
    assert!(matches!(failure, Err(SessionFailure { cause, settlement: Settlement::Closed(_) }) if cause.code == "SESSION_CHARGE_CALLBACK_FAILED"));
    assert_eq!(rejected.releases.load(Ordering::Acquire), 1);
    assert_eq!(rejected.drops.load(Ordering::Acquire), 1);
    assert_eq!(status().admitted(), 0);
    let mut owners = Vec::new();
    for panics in [true, false] {
        let count = counts();
        let gate = Gate(Arc::new(AtomicBool::new(false)));
        let worker_gate = gate.0.clone();
        let client = admit_started(inputs(), DirectoryStamp::capture_resolved(&parent).unwrap(), SessionPolicy::byte_fixture(), Box::new(Charge { counts: count.clone(), mark_panics: panics, release_panics: panics }), CancellationToken::new(), move |shared| {
            std::thread::Builder::new().stack_size(2 * 1024 * 1024).spawn(move || {
                while !shared.registered.load(Ordering::Acquire) { std::thread::yield_now(); }
                let stop = Instant::now() + Duration::from_secs(2);
                while !worker_gate.load(Ordering::Acquire) && Instant::now() < stop { std::thread::sleep(Duration::from_millis(1)); }
                drop(lock(&shared.mailbox).preparation.take());
                CoordinatorDone { output: Err(error("CANCELLED", "Finite callback ownership fixture")), pumps: [PumpReceipt::not_created(RelayInputId(0)), PumpReceipt::not_created(RelayInputId(1))], child_spawned: false, child_reaped: false, outputs_joined: false, namespace_cleaned: true }
            })
        }).unwrap_or_else(|failure| panic!("{}", failure.cause));
        owners.push((client, count, gate));
    }
    let (first, count, gate) = owners.remove(0);
    for _ in 0..2 {
        let failure = first.retained();
        assert_eq!(failure.cause.code, "SESSION_CHARGE_CALLBACK_FAILED");
        assert!(matches!(failure.settlement, Settlement::Retained(id) if id == first.shared.id));
    }
    assert_eq!(count.marks.load(Ordering::Acquire), 1);
    assert_eq!(count.drops.load(Ordering::Acquire), 0);
    assert_eq!(status().admitted(), 2);
    assert_eq!(reconcile().unwrap(), 0);
    gate.0.store(true, Ordering::Release);
    let stop = Instant::now() + Duration::from_secs(3);
    while count.drops.load(Ordering::Acquire) == 0 && Instant::now() < stop { reconcile().unwrap(); std::thread::sleep(Duration::from_millis(1)); }
    let first_result = lock(&first.shared.mailbox).finished.take();
    // Release the sibling gate before inspecting the first result; an assertion
    // failure cannot strand its finite actual thread or charge.
    let (second, sibling_count, sibling_gate) = owners.remove(0);
    let sibling_untouched = sibling_count.releases.load(Ordering::Acquire) == 0 && sibling_count.marks.load(Ordering::Acquire) == 0;
    sibling_gate.0.store(true, Ordering::Release);
    while sibling_count.drops.load(Ordering::Acquire) == 0 && Instant::now() < stop { reconcile().unwrap(); std::thread::sleep(Duration::from_millis(1)); }
    assert!(matches!(first_result, Some(Err(SessionFailure { cause, settlement: Settlement::Closed(_) })) if cause.code == "SESSION_CHARGE_CALLBACK_FAILED"));
    assert_eq!(count.releases.load(Ordering::Acquire), 1);
    assert_eq!(count.drops.load(Ordering::Acquire), 1);
    assert!(sibling_untouched);
    assert_eq!(sibling_count.releases.load(Ordering::Acquire), 1);
    assert_eq!(sibling_count.drops.load(Ordering::Acquire), 1);
    assert_eq!(status().admitted(), 0);
    drop((first, second));
}
