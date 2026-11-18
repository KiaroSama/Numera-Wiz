//! Additive hosted relay cases; these helpers introduce no test identities.
use nw_domain::{DomainError, Result};
use nw_platform::{
    files::{DirectoryStamp, SourceStamp},
    process::*,
};
use sha2::{Digest, Sha256};
use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};
use tokio_util::sync::CancellationToken;

pub(super) static RELAY_CASE_GATE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

struct Charge(Arc<AtomicUsize>);
impl SessionCharge for Charge {
    fn mark_uncertain(&mut self) {
        self.0.store(2, Ordering::Release);
    }
    fn release(self: Box<Self>, closed: &ClosedSession) {
        assert!(closed.coordinator_joined());
        self.0.store(0, Ordering::Release);
    }
}

fn payload(id: usize) -> Vec<u8> {
    let lengths = [65_553, 98_335];
    let tails: [&[u8]; 2] = [b"INPUT0-TAIL-\x00\xffEND!", b"INPUT1-TAIL-\x00\xffEND!"];
    assert_eq!(tails[id].len(), 18);
    let mut bytes = (0..lengths[id])
        .map(|n| ((n * 17 + id * 73) % 256) as u8)
        .collect::<Vec<_>>();
    bytes[lengths[id] - 18..].copy_from_slice(tails[id]);
    assert_eq!(bytes.len(), lengths[id]);
    assert_eq!(&bytes[lengths[id] - 18..], tails[id]);
    bytes
}

fn clean() {
    reconcile_finished_sessions().unwrap();
    let status = session_registry_status();
    assert_eq!(status.admitted(), 0);
    assert_eq!(status.charged_bytes(), 0);
    assert_eq!(status.coordinator_threads(), 0);
    assert_eq!(status.input_threads(), 0);
}

async fn prepared(root: &Path, token: CancellationToken) -> (PreparedRelays, Arc<AtomicUsize>) {
    prepare_with_policy(root, token, SessionPolicy::byte_fixture()).await
}
async fn prepare_with_policy(
    root: &Path,
    token: CancellationToken,
    policy: SessionPolicy,
) -> (PreparedRelays, Arc<AtomicUsize>) {
    let parent = nw_platform::security::private_directory(&root.join("endpoints")).unwrap();
    let inputs = std::array::from_fn(|id| {
        let bytes = payload(id);
        let path = root.join(format!("source-{id}"));
        std::fs::write(&path, &bytes).unwrap();
        ByteInput::new(
            RelayInputId(id as u8),
            SourceStamp::capture_resolved(&path).unwrap(),
            bytes.len() as u64,
            Sha256::digest(&bytes).into(),
        )
        .unwrap()
    });
    let charge = Arc::new(AtomicUsize::new(1));
    let prepared = PreparedRelays::prepare(
        inputs,
        DirectoryStamp::capture_resolved(&parent).unwrap(),
        policy,
        Box::new(Charge(charge.clone())),
        token,
    )
    .await
    .unwrap_or_else(|failure| panic!("preparation: {}", failure.cause));
    (prepared, charge)
}

fn invocation(program: &Path, mode: &str) -> InvocationSpec {
    let program = nw_platform::files::source_path(program).unwrap();
    let executable_sha256 = Sha256::digest(std::fs::read(&program).unwrap()).into();
    InvocationSpec {
        program,
        args: vec![
            SessionArg::Literal(mode.into()),
            SessionArg::Input(RelayInputId(0)),
            SessionArg::Input(RelayInputId(1)),
        ],
        executable_sha256,
        profile_id: "owned-byte-fixture-v1".into(),
    }
}

pub(super) async fn positive(program: &Path) {
    clean();
    for mode in [
        "relay-two-input",
        "relay-two-input-interleaved",
        "relay-slow-tail",
        "relay-open-stall",
        "relay-descendant",
        "relay-roundtrip",
    ] {
        let root = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(root.path()).unwrap();
        let start = Instant::now();
        let (prepared, charge) = prepared(&root, CancellationToken::new()).await;
        let arguments = [
            "",
            "sp ace",
            "quote\"inside",
            "trailing\\",
            "slashes\\\\\"quote",
            "یونیکد λ 東京",
        ];
        let mut spec = invocation(program, mode);
        if mode == "relay-roundtrip" {
            spec.args.extend(
                arguments
                    .iter()
                    .map(|value| SessionArg::Literal((*value).into())),
            );
        }
        let session =
            MediaProcessSession::spawn(prepared, spec, StdoutMode::Capture { limit: 4096 })
                .await
                .unwrap_or_else(|failure| panic!("spawn: {}", failure.cause));
        let done = session
            .finish()
            .await
            .unwrap_or_else(|failure| panic!("finish: {}", failure.cause));
        assert!(start.elapsed() < Duration::from_secs(8));
        assert!(done.closed.coordinator_joined());
        assert!(done.closed.child_reaped());
        assert!(done.closed.outputs_joined());
        assert!(done.closed.namespace_cleaned());
        assert_eq!(charge.load(Ordering::Acquire), 0);
        let text = String::from_utf8(done.output.stdout).unwrap();
        if mode == "relay-roundtrip" {
            let actual = text
                .lines()
                .filter_map(|line| line.strip_prefix("ARG "))
                .collect::<Vec<_>>();
            let expected = arguments
                .iter()
                .map(|value| hex::encode(value.as_bytes()))
                .collect::<Vec<_>>();
            assert_eq!(actual, expected);
        }
        for (id, pump) in done.closed.pumps().iter().enumerate() {
            let expected = payload(id);
            let hash: [u8; 32] = Sha256::digest(&expected).into();
            assert_eq!(pump.input_id(), RelayInputId(id as u8));
            assert_eq!(pump.bytes(), expected.len() as u64);
            assert_eq!(pump.sha256(), Some(hash));
            assert!(pump.transport_complete());
            assert!(pump.worker_joined());
            assert!(pump.completed_writes() > 0);
            assert!(pump.max_buffer_bytes() <= 8192);
            assert!(text.contains(&format!(
                "RELAY1 {id} {} {} EOF TAIL18",
                expected.len(),
                hex::encode(hash)
            )));
        }
        #[cfg(windows)]
        if mode == "relay-open-stall" {
            assert!(
                done.closed
                    .pumps()
                    .iter()
                    .any(|pump| pump.connect_observation() == "Pending"),
                "real pending-connect branch must be observed"
            );
        }
        assert!(done.output.stderr_tail.len() <= 4096 * 3);
        assert_eq!(
            std::fs::read_dir(root.join("endpoints")).unwrap().count(),
            0
        );
        clean();
    }
}

struct Reject;
impl AsyncChunkSink for Reject {
    fn write<'a>(&'a mut self, _bytes: &'a [u8]) -> SinkFuture<'a> {
        Box::pin(async {
            Err(DomainError::new(
                "VERIFICATION_FAILED",
                "Fixture sink rejects",
            ))
        })
    }
    fn finish(&mut self) -> SinkFuture<'_> {
        Box::pin(async { Ok(()) })
    }
}
struct Pending;
impl AsyncChunkSink for Pending {
    fn write<'a>(&'a mut self, _bytes: &'a [u8]) -> SinkFuture<'a> {
        Box::pin(std::future::pending::<Result<()>>())
    }
    fn finish(&mut self) -> SinkFuture<'_> {
        Box::pin(std::future::pending::<Result<()>>())
    }
}
struct Panicking;
impl AsyncChunkSink for Panicking {
    fn write<'a>(&'a mut self, _bytes: &'a [u8]) -> SinkFuture<'a> {
        Box::pin(async { panic!("finite fixture sink unwind") })
    }
    fn finish(&mut self) -> SinkFuture<'_> {
        Box::pin(async { Ok(()) })
    }
}

pub(super) async fn sinks(program: &Path) {
    clean();
    for pending in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let path = std::fs::canonicalize(root.path()).unwrap();
        let token = CancellationToken::new();
        let (prepared, charge) = prepared(&path, token.clone()).await;
        let sink: Box<dyn AsyncChunkSink> = if pending {
            Box::new(Pending)
        } else {
            Box::new(Reject)
        };
        let session = MediaProcessSession::spawn(
            prepared,
            invocation(program, "relay-two-input"),
            StdoutMode::Binary {
                sink,
                byte_limit: 4096,
            },
        )
        .await
        .unwrap_or_else(|failure| panic!("spawn: {}", failure.cause));
        let trigger = token.clone();
        let cancel = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(250)).await;
            trigger.cancel();
        });
        let start = Instant::now();
        let failure = match session.finish().await {
            Ok(_) => panic!("fault sink succeeded"),
            Err(failure) => failure,
        };
        assert_eq!(
            failure.cause.code,
            if pending {
                "CANCELLED"
            } else {
                "VERIFICATION_FAILED"
            }
        );
        match &failure.settlement {
            Settlement::Closed(closed) => {
                assert!(closed.outputs_joined());
                assert!(closed.child_reaped());
                assert!(closed.diagnostic_bytes() >= 8192);
            }
            Settlement::Retained(_) => assert_eq!(charge.load(Ordering::Acquire), 2),
        }
        assert!(start.elapsed() < Duration::from_secs(8));
        cancel.await.unwrap();
        settle(charge).await;
        clean();
    }
}

async fn settle(charge: Arc<AtomicUsize>) {
    let stop = Instant::now() + Duration::from_secs(4);
    while charge.load(Ordering::Acquire) != 0 && Instant::now() < stop {
        reconcile_finished_sessions().unwrap();
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(
        charge.load(Ordering::Acquire),
        0,
        "real retention is a failed finite-settlement gate"
    );
}

pub(super) async fn dropped_prepared() {
    clean();
    let root = tempfile::tempdir().unwrap();
    let path = std::fs::canonicalize(root.path()).unwrap();
    let (prepared, charge) = prepared(&path, CancellationToken::new()).await;
    drop(prepared);
    settle(charge).await;
    clean();
}

pub(super) async fn lifecycle(program: &Path) {
    dropped_prepared().await;
    let root = tempfile::tempdir().unwrap();
    let path = std::fs::canonicalize(root.path()).unwrap();
    let executable = program.to_owned();
    let caller = std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let result = runtime.block_on(async {
            let (prepared, charge) = prepared(&path, CancellationToken::new()).await;
            let session = MediaProcessSession::spawn(
                prepared,
                invocation(&executable, "relay-two-input"),
                StdoutMode::Binary {
                    sink: Box::new(Pending),
                    byte_limit: 4096,
                },
            )
            .await
            .unwrap_or_else(|failure| panic!("{}", failure.cause));
            (session, charge)
        });
        drop(runtime);
        result
    });
    while !caller.is_finished() {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let (session, charge) = caller.join().unwrap();
    assert_eq!(session_registry_status().admitted(), 1);
    let waiter = tokio::spawn(session.finish());
    tokio::time::sleep(Duration::from_millis(30)).await;
    waiter.abort();
    match waiter.await {
        Err(problem) => assert!(problem.is_cancelled()),
        Ok(_) => panic!("dropped finish waiter unexpectedly completed"),
    }
    settle(charge).await;
    clean();

    let root = tempfile::tempdir().unwrap();
    let path = std::fs::canonicalize(root.path()).unwrap();
    let (prepared, charge) = prepared(&path, CancellationToken::new()).await;
    let session = MediaProcessSession::spawn(
        prepared,
        invocation(program, "relay-two-input"),
        StdoutMode::Binary {
            sink: Box::new(Panicking),
            byte_limit: 4096,
        },
    )
    .await
    .unwrap_or_else(|failure| panic!("{}", failure.cause));
    let failure = match session.finish().await {
        Ok(_) => panic!("panicking sink succeeded"),
        Err(failure) => failure,
    };
    assert_eq!(failure.cause.code, "SESSION_COORDINATOR_PANIC");
    settle(charge).await;
    clean();
    pending_transport(program).await;
}

pub(super) async fn failures(program: &Path) {
    clean();
    changed_sources(program).await;
    for missing in [true, false] {
        let root = tempfile::tempdir().unwrap();
        let path = std::fs::canonicalize(root.path()).unwrap();
        let (prepared, charge) = prepared(&path, CancellationToken::new()).await;
        let mut spec = invocation(program, "relay-early-exit");
        if missing {
            spec.program = program.with_file_name("missing-owned-relay-fixture");
        }
        let result =
            match MediaProcessSession::spawn(prepared, spec, StdoutMode::Capture { limit: 4096 })
                .await
            {
                Ok(session) => session.finish().await,
                Err(failure) => Err(failure),
            };
        let failure = match result {
            Ok(_) => panic!("failed fixture succeeded"),
            Err(failure) => failure,
        };
        assert_eq!(
            failure.cause.code,
            if missing {
                "PROCESS_LAUNCH_FAILED"
            } else {
                "PROCESS_FAILED"
            }
        );
        settle(charge).await;
        clean();
    }
}

pub(super) async fn deadlines(program: &Path) {
    clean();
    for disabled in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let path = std::fs::canonicalize(root.path()).unwrap();
        let token = CancellationToken::new();
        let deadline = if disabled {
            nw_domain::execution::DeadlinePolicy::Disabled
        } else {
            nw_domain::execution::DeadlinePolicy::Finite { seconds: 1 }
        };
        let policy = SessionPolicy::byte_fixture()
            .with_stage_deadline(deadline)
            .unwrap();
        let (prepared, charge) = prepare_with_policy(&path, token.clone(), policy).await;
        let session = MediaProcessSession::spawn(
            prepared,
            invocation(program, "relay-two-input"),
            StdoutMode::Binary {
                sink: Box::new(Pending),
                byte_limit: 4096,
            },
        )
        .await
        .unwrap_or_else(|failure| panic!("{}", failure.cause));
        let started = Instant::now();
        let trigger = if disabled {
            Some(tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(250)).await;
                token.cancel();
            }))
        } else {
            None
        };
        let failure = match session.finish().await {
            Ok(_) => panic!("pending sink succeeded"),
            Err(failure) => failure,
        };
        assert_eq!(
            failure.cause.code,
            if disabled {
                "CANCELLED"
            } else {
                "PROCESS_TIMEOUT"
            }
        );
        assert!(started.elapsed() < Duration::from_secs(8));
        if let Some(trigger) = trigger {
            trigger.await.unwrap();
        }
        settle(charge).await;
        clean();
    }
}

async fn pending_transport(program: &Path) {
    for stall in [false, true] {
        clean();
        let root = tempfile::tempdir().unwrap();
        let path = std::fs::canonicalize(root.path()).unwrap();
        let parent = nw_platform::security::private_directory(&path.join("pending")).unwrap();
        let inputs = std::array::from_fn(|id| {
            // Exceeds the native pipe allowance and user buffers; the stall
            // fixture deliberately does not invoke the positive byte oracle.
            let bytes = vec![id as u8; 2 * 1024 * 1024];
            let source = path.join(format!("stall-{id}"));
            std::fs::write(&source, &bytes).unwrap();
            ByteInput::new(
                RelayInputId(id as u8),
                SourceStamp::capture_resolved(&source).unwrap(),
                bytes.len() as u64,
                Sha256::digest(&bytes).into(),
            )
            .unwrap()
        });
        let charge = Arc::new(AtomicUsize::new(1));
        let token = CancellationToken::new();
        let prepared = PreparedRelays::prepare(
            inputs,
            DirectoryStamp::capture_resolved(&parent).unwrap(),
            SessionPolicy::byte_fixture(),
            Box::new(Charge(charge.clone())),
            token.clone(),
        )
        .await
        .unwrap_or_else(|failure| panic!("{}", failure.cause));
        let mode = if stall {
            "relay-read-stall"
        } else {
            "relay-open-stall"
        };
        let session = MediaProcessSession::spawn(
            prepared,
            invocation(program, mode),
            StdoutMode::Capture { limit: 4096 },
        )
        .await
        .unwrap_or_else(|failure| panic!("{}", failure.cause));
        let trigger = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(if stall { 300 } else { 50 })).await;
            token.cancel();
        });
        let started = Instant::now();
        let failure = match session.finish().await {
            Ok(_) => panic!("cancelled stalled transport succeeded"),
            Err(failure) => failure,
        };
        trigger.await.unwrap();
        assert!(started.elapsed() < Duration::from_secs(8));
        assert_eq!(failure.cause.code, "CANCELLED");
        match failure.settlement {
            Settlement::Closed(closed) => {
                assert!(closed.child_reaped());
                assert!(closed.outputs_joined());
                assert!(closed.coordinator_joined());
                assert!(closed.namespace_cleaned());
                assert!(closed.pumps().iter().all(|pump| pump.worker_joined()
                    && pump.endpoint_closed()
                    && pump.endpoint_cleaned()));
                if stall {
                    assert!(
                        closed
                            .pumps()
                            .iter()
                            .any(PumpReceipt::pending_write_observed),
                        "actual write backpressure must be observed"
                    );
                }
                #[cfg(windows)]
                if !stall {
                    assert!(
                        closed
                            .pumps()
                            .iter()
                            .any(|pump| pump.connect_observation() == "Pending"),
                        "actual pending-connect cancellation must be observed"
                    );
                }
            }
            Settlement::Retained(_) => {
                panic!("required cancellation path failed finite actual settlement")
            }
        }
        settle(charge).await;
        clean();
    }
}

async fn changed_sources(program: &Path) {
    use std::io::Write;
    for mutation in 0..3 {
        clean();
        let root = tempfile::tempdir().unwrap();
        let path = std::fs::canonicalize(root.path()).unwrap();
        let (prepared, charge) = prepared(&path, CancellationToken::new()).await;
        let source = path.join("source-0");
        match mutation {
            0 => std::fs::OpenOptions::new()
                .write(true)
                .open(&source)
                .unwrap()
                .set_len(1)
                .unwrap(),
            1 => std::fs::OpenOptions::new()
                .append(true)
                .open(&source)
                .unwrap()
                .write_all(b"unexpected suffix")
                .unwrap(),
            _ => {
                std::fs::rename(&source, path.join("original-source-0")).unwrap();
                std::fs::write(&source, b"replacement must remain untouched").unwrap();
            }
        }
        let started = Instant::now();
        let result = match MediaProcessSession::spawn(
            prepared,
            invocation(program, "relay-two-input"),
            StdoutMode::Capture { limit: 4096 },
        )
        .await
        {
            Ok(session) => session.finish().await,
            Err(failure) => Err(failure),
        };
        let failure = match result {
            Ok(_) => panic!("changed byte source succeeded"),
            Err(failure) => failure,
        };
        assert_eq!(failure.cause.code, "SOURCE_CHANGED");
        assert!(started.elapsed() < Duration::from_secs(8));
        match failure.settlement {
            Settlement::Closed(closed) => {
                assert!(closed.coordinator_joined());
                assert!(closed.child_reaped());
                assert!(closed.namespace_cleaned());
                assert!(!closed.pumps()[0].transport_complete());
                assert_eq!(closed.pumps()[0].bytes(), 0);
            }
            Settlement::Retained(_) => panic!("source fault failed finite actual settlement"),
        }
        if mutation == 2 {
            assert_eq!(
                std::fs::read(&source).unwrap(),
                b"replacement must remain untouched"
            );
            assert_eq!(
                std::fs::read(path.join("original-source-0")).unwrap(),
                payload(0)
            );
        }
        settle(charge).await;
        clean();
    }
}
