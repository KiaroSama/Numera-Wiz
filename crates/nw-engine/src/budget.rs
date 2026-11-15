//! Per-owner leaf-process admission; uncertain cleanup never returns capacity to the pool.
use nw_domain::{DomainError, Result};
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio_util::sync::CancellationToken;

const LIMIT: usize = 2;
#[derive(Clone)]
pub(super) struct ProcessBudget {
    slots: Arc<Semaphore>,
    quarantined: Arc<AtomicUsize>,
    retained_relays: Arc<AtomicUsize>,
    forgotten: Arc<AtomicUsize>,
}
impl Default for ProcessBudget {
    fn default() -> Self {
        Self {
            slots: Arc::new(Semaphore::new(LIMIT)),
            quarantined: Arc::new(AtomicUsize::new(0)),
            retained_relays: Arc::new(AtomicUsize::new(0)),
            forgotten: Arc::new(AtomicUsize::new(0)),
        }
    }
}
pub(super) struct Ticket {
    permit: Option<OwnedSemaphorePermit>,
    budget: ProcessBudget,
    settled: bool,
    retained_relay: bool,
}
impl ProcessBudget {
    pub(super) async fn acquire(&self, cancel: &CancellationToken) -> Result<Ticket> {
        let permit = tokio::select! {
            biased;
            _ = cancel.cancelled() => return Err(DomainError::new("CANCELLED", "Media process admission cancelled before spawn")),
            result = tokio::time::timeout(Duration::from_secs(120), self.slots.clone().acquire_owned()) => match result {
                Ok(Ok(permit)) => permit,
                Ok(Err(_)) => return Err(DomainError::new("RESOURCE_LIMIT", "Media process capacity is quarantined pending owned-process reconciliation")),
                Err(_) => return Err(DomainError::new("RESOURCE_LIMIT", "Media process admission exceeded its finite wait budget")),
            },
        };
        Ok(Ticket {
            permit: Some(permit),
            budget: self.clone(),
            settled: false,
            retained_relay: false,
        })
    }
    pub(super) fn status(&self) -> (usize, usize, usize) {
        (
            LIMIT,
            self.slots.available_permits(),
            self.quarantined.load(Ordering::Acquire) + self.retained_relays.load(Ordering::Acquire),
        )
    }
}
impl Ticket {
    pub(super) fn finish<T>(mut self, result: Result<T>) -> Result<T> {
        self.settled = !result.as_ref().is_err_and(|error| {
            matches!(
                error.code.as_str(),
                "TREE_TERMINATION_UNCERTAIN"
                    | "PROCESS_WAIT_FAILED"
                    | "PROCESS_CAPTURE_FAILED"
                    | "PROCESS_LAUNCH_FAILED"
                    | "RELAY_SETTLEMENT_UNCERTAIN"
                    | "SESSION_SETTLEMENT_UNCERTAIN"
            )
        });
        result
    }
}
impl nw_platform::process::SessionCharge for Ticket {
    fn mark_uncertain(&mut self) {
        if !self.retained_relay {
            self.retained_relay = true;
            self.budget.retained_relays.fetch_add(1, Ordering::AcqRel);
        }
    }
    fn release(mut self: Box<Self>, _closed: &nw_platform::process::ClosedSession) {
        self.settled = true;
        if self.retained_relay {
            self.budget.retained_relays.fetch_sub(1, Ordering::AcqRel);
            self.retained_relay = false;
        }
        // The live permit is returned by its ordinary Drop, only after the
        // platform supplied its unforgeable actually-joined closed receipt.
    }
}
impl Drop for Ticket {
    fn drop(&mut self) {
        if !self.settled
            && let Some(permit) = self.permit.take()
        {
            permit.forget();
            if !self.retained_relay { self.budget.quarantined.fetch_add(1, Ordering::AcqRel); }
            let count = self.budget.quarantined.load(Ordering::Acquire) + self.budget.retained_relays.load(Ordering::Acquire);
            let forgotten = self.budget.forgotten.fetch_add(1, Ordering::AcqRel) + 1;
            if forgotten >= LIMIT {
                self.budget.slots.close();
            }
            tracing::error!(
                quarantined = count,
                "Uncertain media process lifetime quarantined its admission slot"
            );
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    static RELAY_CASE_GATE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    #[tokio::test]
    async fn pressure_and_cancel_never_spawn_or_release_another_tasks_capacity() {
        let budget = ProcessBudget::default();
        let token = CancellationToken::new();
        let a = budget.acquire(&token).await.unwrap();
        let b = budget.acquire(&token).await.unwrap();
        assert_eq!(budget.status(), (2, 0, 0));
        let waiting = token.child_token();
        waiting.cancel();
        assert!(matches!(budget.acquire(&waiting).await, Err(error) if error.code == "CANCELLED"));
        assert_eq!(budget.status(), (2, 0, 0));
        a.finish(Ok(())).unwrap();
        assert_eq!(budget.status(), (2, 1, 0));
        b.finish::<()>(Err(DomainError::new(
            "PROCESS_FAILED",
            "A reaped nonzero exit",
        )))
        .unwrap_err();
        assert_eq!(budget.status(), (2, 2, 0));
        let _relay_guard = RELAY_CASE_GATE.lock().await;
        transferred_relay_charge(false).await;
        rejected_relay_charge().await;
    }
    #[tokio::test]
    async fn uncertain_reap_and_dropped_supervisor_quarantine_capacity() {
        let budget = ProcessBudget::default();
        let token = CancellationToken::new();
        let ticket = budget.acquire(&token).await.unwrap();
        ticket
            .finish::<()>(Err(DomainError::new(
                "TREE_TERMINATION_UNCERTAIN",
                "Synthetic unconfirmed reap",
            )))
            .unwrap_err();
        assert_eq!(budget.status(), (2, 1, 1));
        drop(budget.acquire(&token).await.unwrap());
        assert_eq!(budget.status(), (2, 0, 2));
        assert!(
            matches!(budget.acquire(&token).await, Err(error) if error.code == "RESOURCE_LIMIT")
        );
        let _relay_guard = RELAY_CASE_GATE.lock().await;
        transferred_relay_charge(true).await;
    }

    async fn transferred_relay_charge(mark: bool) {
        use nw_platform::{files::{DirectoryStamp, SourceStamp}, process::{ByteInput, PreparedRelays, RelayInputId, SessionCharge, SessionPolicy, reconcile_finished_sessions, session_registry_status}};
        use sha2::{Digest, Sha256};
        reconcile_finished_sessions().unwrap();
        assert_eq!(session_registry_status().admitted(), 0);
        let root = tempfile::tempdir().unwrap();
        let parent = nw_platform::security::private_directory(&root.path().join("relay")).unwrap();
        let mut inputs = Vec::new();
        for id in 0..2 {
            let path = std::fs::canonicalize(root.path()).unwrap().join(format!("source-{id}"));
            std::fs::write(&path, [id]).unwrap();
            inputs.push(ByteInput::new(RelayInputId(id), SourceStamp::capture_resolved(&path).unwrap(), 1, Sha256::digest([id]).into()).unwrap());
        }
        let inputs: [ByteInput; 2] = inputs.try_into().unwrap_or_else(|_| panic!("two fixture inputs"));
        let budget = ProcessBudget::default();
        let token = CancellationToken::new();
        let mut transferred = budget.acquire(&token).await.unwrap();
        let running = budget.acquire(&token).await.unwrap();
        if mark { transferred.mark_uncertain(); transferred.mark_uncertain(); }
        assert_eq!(budget.status(), (2, 0, usize::from(mark)));
        let prepared = PreparedRelays::prepare(inputs, DirectoryStamp::capture_resolved(&parent).unwrap(), SessionPolicy::byte_fixture(), Box::new(transferred), token).await.unwrap_or_else(|failure| panic!("{}", failure.cause));
        assert_eq!(budget.status(), (2, 0, usize::from(mark)));
        drop(prepared);
        // A dropped facade requests cancellation; it cannot itself release.
        assert_eq!(budget.status(), (2, 0, usize::from(mark)));
        running.finish(Ok(())).unwrap();
        assert_eq!(budget.status(), (2, 1, usize::from(mark)));
        let stop = std::time::Instant::now() + Duration::from_secs(4);
        while budget.status().1 != 2 && std::time::Instant::now() < stop {
            reconcile_finished_sessions().unwrap();
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(budget.status(), (2, 2, 0));
        assert_eq!(session_registry_status().admitted(), 0);
    }

    async fn rejected_relay_charge() {
        use nw_platform::{files::{DirectoryStamp, SourceStamp}, process::{ByteInput, PreparedRelays, RelayInputId, SessionPolicy, Settlement, session_registry_status}};
        use sha2::{Digest, Sha256};
        for cancelled in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let path = std::fs::canonicalize(root.path()).unwrap();
        let parent = nw_platform::security::private_directory(&path.join("rejected")).unwrap();
        let source = path.join("one");
        std::fs::write(&source, b"x").unwrap();
        // Each input is individually valid; the pair is invalid before any
        // endpoint/worker allocation. No production fault switch is involved.
        let inputs = std::array::from_fn(|id| ByteInput::new(RelayInputId(if cancelled { id as u8 } else { 0 }), SourceStamp::capture_resolved(&source).unwrap(), 1, Sha256::digest(b"x").into()).unwrap());
        let budget = ProcessBudget::default();
        let token = CancellationToken::new();
        let ticket = budget.acquire(&token).await.unwrap();
        assert_eq!(budget.status(), (2, 1, 0));
        if cancelled { token.cancel(); }
        let failure = match PreparedRelays::prepare(inputs, DirectoryStamp::capture_resolved(&parent).unwrap(), SessionPolicy::byte_fixture(), Box::new(ticket), token).await {
            Ok(_) => panic!("duplicate byte inputs were admitted"),
            Err(failure) => failure,
        };
        assert_eq!(failure.cause.code, if cancelled { "CANCELLED" } else { "INPUT_DENIED" });
        match failure.settlement {
            Settlement::Closed(closed) => {
                assert!(closed.coordinator_joined());
                assert!(!closed.child_spawned());
                assert!(closed.pumps().iter().all(|pump| !pump.endpoint_created() && !pump.worker_joined()));
            },
            Settlement::Retained(_) => panic!("empty preparation must settle"),
        }
        assert_eq!(budget.status(), (2, 2, 0));
        assert_eq!(session_registry_status().admitted(), 0);
        assert_eq!(std::fs::read_dir(&parent).unwrap().count(), 0);
        }
    }

}
