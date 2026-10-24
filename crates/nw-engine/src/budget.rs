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
}
impl Default for ProcessBudget {
    fn default() -> Self {
        Self {
            slots: Arc::new(Semaphore::new(LIMIT)),
            quarantined: Arc::new(AtomicUsize::new(0)),
        }
    }
}
pub(super) struct Ticket {
    permit: Option<OwnedSemaphorePermit>,
    budget: ProcessBudget,
    settled: bool,
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
        })
    }
    pub(super) fn status(&self) -> (usize, usize, usize) {
        (
            LIMIT,
            self.slots.available_permits(),
            self.quarantined.load(Ordering::Acquire),
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
            )
        });
        result
    }
}
impl Drop for Ticket {
    fn drop(&mut self) {
        if !self.settled
            && let Some(permit) = self.permit.take()
        {
            permit.forget();
            let count = self.budget.quarantined.fetch_add(1, Ordering::AcqRel) + 1;
            if count >= LIMIT {
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
    }
}
