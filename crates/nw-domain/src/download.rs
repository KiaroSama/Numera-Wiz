//! Bounded client-side verification and cancellation for an authorized artifact copy.
use crate::{
    DomainError, Result,
    artifacts::{ARTIFACT_CHUNK_BYTES, ArtifactManifest, decimal},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    future::{Future, poll_fn},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU8, AtomicU64, Ordering},
    },
    task::{Poll, Waker},
};

const COPYING: u8 = 0;
const CANCELLING: u8 = 1;
const COMMITTING: u8 = 2;
const FINISHED: u8 = 3;

fn cancelled() -> DomainError {
    DomainError::new(
        "CANCELLED",
        "Artifact copy cancelled before client publication",
    )
}

/// One active save, with a cancellation/publication fence independent of its producer job.
#[derive(Default)]
pub struct SaveControl {
    state: AtomicU8,
    copied: AtomicU64,
    wake: Mutex<Option<Waker>>,
}
impl SaveControl {
    pub fn cancel(&self) -> bool {
        let accepted = self
            .state
            .compare_exchange(COPYING, CANCELLING, Ordering::AcqRel, Ordering::Acquire)
            .is_ok();
        if accepted
            && let Ok(mut wake) = self.wake.lock()
            && let Some(wake) = wake.take()
        {
            wake.wake();
        }
        accepted
    }
    pub fn check(&self) -> Result<()> {
        if self.state.load(Ordering::Acquire) == CANCELLING {
            Err(cancelled())
        } else {
            Ok(())
        }
    }
    pub fn begin_commit(&self) -> Result<()> {
        self.state
            .compare_exchange(COPYING, COMMITTING, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| ())
            .map_err(|state| {
                if state == CANCELLING {
                    cancelled()
                } else {
                    DomainError::new(
                        "COPY_STATE_CONFLICT",
                        "Client publication was already decided",
                    )
                }
            })
    }
    pub fn copied(&self) -> u64 {
        self.copied.load(Ordering::Acquire)
    }
    pub fn set_copied(&self, bytes: u64) {
        self.copied.store(bytes, Ordering::Release);
    }
    pub fn is_cancelling(&self) -> bool {
        self.state.load(Ordering::Acquire) == CANCELLING
    }
    pub fn is_finished(&self) -> bool {
        self.state.load(Ordering::Acquire) == FINISHED
    }
    /// Only network waits are interruptible. An admitted filesystem write must drain.
    pub async fn interruptible<F: Future>(&self, future: F) -> Result<F::Output> {
        let mut future = std::pin::pin!(future);
        poll_fn(|cx| {
            self.check()?;
            if let Ok(mut wake) = self.wake.lock() {
                *wake = Some(cx.waker().clone());
            }
            self.check()?;
            match future.as_mut().poll(cx) {
                Poll::Ready(value) => {
                    self.check()?;
                    Poll::Ready(Ok(value))
                }
                Poll::Pending => Poll::Pending,
            }
        })
        .await
    }
}

/// Drop only after outstanding writes and abort/close have been observed.
pub struct SaveSession(pub Arc<SaveControl>);
impl Drop for SaveSession {
    fn drop(&mut self) {
        self.0.state.store(FINISHED, Ordering::Release);
        if let Ok(mut wake) = self.0.wake.lock() {
            *wake = None;
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaveReceipt {
    pub artifact_id: String,
    pub encoded_byte_count: String,
    pub sha256: String,
    pub destination: String,
    pub warnings: Vec<String>,
}

pub struct DownloadVerifier {
    artifact_id: String,
    expected: u64,
    expected_sha256: String,
    received: u64,
    hash: Sha256,
}
impl DownloadVerifier {
    pub fn new(manifest: &ArtifactManifest) -> Result<Self> {
        let id = &manifest.artifact_id;
        let route = format!("/api/v1/artifacts/{id}/bytes");
        if manifest.schema_version != 1
            || id.is_empty()
            || id.len() > 128
            || !id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
            || manifest.availability != "retained_published_object"
            || manifest.bytes_route.as_deref() != Some(route.as_str())
            || !manifest
                .retrieval_methods
                .iter()
                .any(|m| m == "authenticated_http_when_enabled")
            || manifest.sha256.len() != 64
            || !manifest
                .sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(DomainError::new(
                "ARTIFACT_UNAVAILABLE",
                "The selected manifest does not admit a bounded authenticated byte copy",
            ));
        }
        Ok(Self {
            artifact_id: id.clone(),
            expected: decimal(&manifest.encoded_byte_count)?,
            expected_sha256: manifest.sha256.clone(),
            received: 0,
            hash: Sha256::new(),
        })
    }
    pub fn expected_bytes(&self) -> u64 {
        self.expected
    }
    pub fn received_bytes(&self) -> u64 {
        self.received
    }
    pub fn push(&mut self, bytes: &[u8]) -> Result<()> {
        if bytes.len() > ARTIFACT_CHUNK_BYTES
            || bytes.len() as u64 > self.expected.saturating_sub(self.received)
        {
            return Err(DomainError::new(
                "ARTIFACT_CHANGED",
                "Artifact copy exceeded its admitted frame or exact byte length",
            ));
        }
        self.hash.update(bytes);
        self.received += bytes.len() as u64;
        Ok(())
    }
    pub fn finish(self, destination: String) -> Result<SaveReceipt> {
        let actual = hex::encode(self.hash.finalize());
        if self.received != self.expected || actual != self.expected_sha256 {
            return Err(DomainError::new(
                "ARTIFACT_CHANGED",
                "Artifact copy did not match the selected byte length and SHA-256",
            ));
        }
        Ok(SaveReceipt {
            artifact_id: self.artifact_id,
            encoded_byte_count: self.received.to_string(),
            sha256: actual,
            destination,
            warnings: vec![],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn manifest() -> ArtifactManifest {
        ArtifactManifest {
            schema_version: 1,
            artifact_id: "job-example".into(),
            manifest_uri: "numera://artifact/job-example/manifest".into(),
            job_id: "example".into(),
            plan_id: "plan".into(),
            mime_type: "application/octet-stream".into(),
            encoded_byte_count: "3".into(),
            sha256: "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad".into(),
            availability: "retained_published_object".into(),
            expires_at: None,
            retrieval_methods: vec!["authenticated_http_when_enabled".into()],
            bytes_route: Some("/api/v1/artifacts/job-example/bytes".into()),
            max_chunk_bytes: ARTIFACT_CHUNK_BYTES as u32,
        }
    }
    #[test]
    fn selected_identity_rejects_changed_truncated_excess_and_foreign_route_bytes() {
        let m = manifest();
        let mut good = DownloadVerifier::new(&m).unwrap();
        good.push(b"a").unwrap();
        good.push(b"bc").unwrap();
        assert_eq!(good.finish("chosen".into()).unwrap().sha256, m.sha256);
        let mut truncated = DownloadVerifier::new(&m).unwrap();
        truncated.push(b"ab").unwrap();
        assert!(truncated.finish("chosen".into()).is_err());
        let mut changed = DownloadVerifier::new(&m).unwrap();
        changed.push(b"abd").unwrap();
        assert!(changed.finish("chosen".into()).is_err());
        assert!(DownloadVerifier::new(&m).unwrap().push(b"abcd").is_err());
        let mut foreign = m.clone();
        foreign.bytes_route = Some("https://foreign.invalid/bytes".into());
        assert!(DownloadVerifier::new(&foreign).is_err());
        foreign = m;
        foreign.artifact_id = "../other".into();
        assert!(DownloadVerifier::new(&foreign).is_err());
    }
    #[test]
    fn cancelling_a_pending_network_wait_wakes_it_and_returns_a_typed_stop() {
        use std::{sync::atomic::AtomicUsize, task::{Context, Wake, Waker}};
        struct Counter(AtomicUsize);
        impl Wake for Counter {
            fn wake(self: Arc<Self>) { self.0.fetch_add(1, Ordering::Relaxed); }
        }
        let counter = Arc::new(Counter(AtomicUsize::new(0)));
        let waker = Waker::from(counter.clone());
        let mut cx = Context::from_waker(&waker);
        let control = SaveControl::default();
        let mut pending = std::pin::pin!(control.interruptible(std::future::pending::<()>()));
        assert!(pending.as_mut().poll(&mut cx).is_pending());
        assert!(control.cancel());
        assert_eq!(counter.0.load(Ordering::Relaxed), 1);
        match pending.as_mut().poll(&mut cx) {
            Poll::Ready(Err(error)) => assert_eq!(error.code, "CANCELLED"),
            _ => panic!("Cancelled network wait remained live"),
        }
    }
    #[test]
    fn cancellation_and_client_publication_have_one_winner() {
        let cancelled = SaveControl::default();
        assert!(cancelled.cancel());
        assert!(!cancelled.cancel());
        assert!(cancelled.begin_commit().is_err());
        let published = SaveControl::default();
        published.begin_commit().unwrap();
        assert!(!published.cancel());
        assert!(published.begin_commit().is_err());
        let control = Arc::new(cancelled);
        let session = SaveSession(control.clone());
        assert!(!control.is_finished());
        drop(session);
        assert!(control.is_finished());
    }
}
