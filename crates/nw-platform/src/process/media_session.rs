//! Owned byte transport primitive. No media or publication verification is implied.
mod owned_child;
mod registry;
pub use super::input_relay::{ByteInput, PumpReceipt, RelayInputId};
use super::{RunOutput, input_relay::OwnedInputRelaySet};
use crate::files::{DirectoryStamp, SourceStamp};
use nw_domain::{DomainError, Result};
use owned_child::OwnedChild;
use sha2::{Digest, Sha256};
use std::{
    future::Future,
    io::Read,
    mem::ManuallyDrop,
    path::PathBuf,
    pin::Pin,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio_util::sync::CancellationToken;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SessionId(u64);
impl SessionId {
    pub fn get(self) -> u64 {
        self.0
    }
}
pub enum SessionArg {
    Literal(String),
    Input(RelayInputId),
}
pub struct InvocationSpec {
    pub program: PathBuf,
    pub args: Vec<SessionArg>,
    pub executable_sha256: [u8; 32],
    pub profile_id: String,
}
impl InvocationSpec {
    fn validate(&self) -> Result<()> {
        if !self.program.is_absolute()
            || self.args.len() > 64
            || self.profile_id.is_empty()
            || self.profile_id.len() > 128
            || !self.profile_id.is_ascii()
            || self.profile_id.contains('\0')
        {
            return Err(error(
                "INVOCATION_DENIED",
                "Invalid executable, profile or argument bounds",
            ));
        }
        let mut bytes = 0usize;
        let mut seen = [false; 2];
        for arg in &self.args {
            match arg {
                SessionArg::Literal(value) => {
                    bytes = bytes
                        .checked_add(value.len())
                        .ok_or_else(|| error("RESOURCE_LIMIT", "Argument count overflow"))?;
                    if bytes > 16 * 1024 || value.contains('\0') {
                        return Err(error(
                            "INVOCATION_DENIED",
                            "Argument text exceeds its bound",
                        ));
                    }
                }
                SessionArg::Input(RelayInputId(id)) => {
                    if *id > 1 || seen[usize::from(*id)] {
                        return Err(error(
                            "INVOCATION_DENIED",
                            "Duplicate or unknown input placeholder",
                        ));
                    }
                    seen[usize::from(*id)] = true;
                }
            }
        }
        if seen != [true, true] {
            return Err(error(
                "INVOCATION_DENIED",
                "Exactly two input placeholders are required",
            ));
        }
        Ok(())
    }
}
#[derive(Clone, Copy)]
pub struct SessionPolicy {
    execution: Option<Duration>,
}
impl SessionPolicy {
    pub fn byte_fixture() -> Self {
        Self {
            execution: Some(Duration::from_secs(5)),
        }
    }
    pub fn with_stage_deadline(
        mut self,
        deadline: nw_domain::execution::DeadlinePolicy,
    ) -> Result<Self> {
        self.execution = deadline.duration()?;
        Ok(self)
    }
    pub fn execution_deadline(&self) -> Option<Duration> {
        self.execution
    }
    pub fn preparation_deadline(&self) -> Duration {
        Duration::from_secs(2)
    }
    pub fn cleanup_deadline(&self) -> Duration {
        Duration::from_secs(3)
    }
}
pub type SinkFuture<'a> = Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>>;
/// Methods and future polling must not block the coordinator's thread. Futures
/// must be cancellation-safe: no detached work or native ownership may outlive
/// a dropped write/finish future. Destructors obey the same nonblocking and
/// no-detached-native-ownership contract. Accepted bytes are not verified media.
pub trait AsyncChunkSink: Send + 'static {
    fn write<'a>(&'a mut self, bytes: &'a [u8]) -> SinkFuture<'a>;
    fn finish(&mut self) -> SinkFuture<'_>;
}
pub enum StdoutMode {
    Capture {
        limit: usize,
    },
    Binary {
        sink: Box<dyn AsyncChunkSink>,
        byte_limit: u64,
    },
}
/// Internal ownership callbacks must be nonblocking, nonreentrant and must not
/// panic. A consuming release is called only after actual resource settlement.
pub trait SessionCharge: Send + 'static {
    fn mark_uncertain(&mut self);
    fn release(self: Box<Self>, closed: &ClosedSession);
}
pub struct PreparedRelays {
    client: Client,
}
pub struct MediaProcessSession {
    client: Client,
}
pub struct ClosedSession {
    facts: Box<ClosedFacts>,
}
struct ClosedFacts {
    id: SessionId,
    pumps: [PumpReceipt; 2],
    child_spawned: bool,
    child_reaped: bool,
    outputs_joined: bool,
    namespace_cleaned: bool,
    coordinator_joined: bool,
    stdout_bytes: u64,
    diagnostic_bytes: u64,
}
impl ClosedSession {
    fn empty(id: SessionId) -> Self {
        Self {
            facts: Box::new(ClosedFacts {
                id,
                pumps: [
                    PumpReceipt::not_created(RelayInputId(0)),
                    PumpReceipt::not_created(RelayInputId(1)),
                ],
                child_spawned: false,
                child_reaped: false,
                outputs_joined: false,
                namespace_cleaned: true,
                coordinator_joined: false,
                stdout_bytes: 0,
                diagnostic_bytes: 0,
            }),
        }
    }
    pub fn id(&self) -> SessionId {
        self.facts.id
    }
    pub fn pumps(&self) -> &[PumpReceipt; 2] {
        &self.facts.pumps
    }
    pub fn child_spawned(&self) -> bool {
        self.facts.child_spawned
    }
    pub fn child_reaped(&self) -> bool {
        self.facts.child_reaped
    }
    pub fn outputs_joined(&self) -> bool {
        self.facts.outputs_joined
    }
    pub fn namespace_cleaned(&self) -> bool {
        self.facts.namespace_cleaned
    }
    pub fn coordinator_joined(&self) -> bool {
        self.facts.coordinator_joined
    }
    pub fn stdout_bytes(&self) -> u64 {
        self.facts.stdout_bytes
    }
    pub fn diagnostic_bytes(&self) -> u64 {
        self.facts.diagnostic_bytes
    }
    pub fn namespace_kind(&self) -> &'static str {
        if cfg!(windows) {
            "NativeSingleInstancePipes"
        } else {
            "HeldDirectoryFifos"
        }
    }
}
pub struct SessionOutput {
    pub output: RunOutput,
    pub closed: ClosedSession,
}
pub enum Settlement {
    Closed(ClosedSession),
    Retained(SessionId),
}
pub struct SessionFailure {
    pub cause: DomainError,
    pub settlement: Settlement,
}
pub type SessionResult<T> = std::result::Result<T, SessionFailure>;
#[derive(Default)]
pub struct RegistryStatus {
    admitted: usize,
    retained: usize,
    coordinator_threads: usize,
    input_threads: usize,
    charged_bytes: u64,
    closed_unjoined: usize,
}
impl RegistryStatus {
    pub fn admitted(&self) -> usize {
        self.admitted
    }
    pub fn retained(&self) -> usize {
        self.retained
    }
    pub fn coordinator_threads(&self) -> usize {
        self.coordinator_threads
    }
    pub fn input_threads(&self) -> usize {
        self.input_threads
    }
    pub fn charged_bytes(&self) -> u64 {
        self.charged_bytes
    }
    pub fn closed_unjoined(&self) -> usize {
        self.closed_unjoined
    }
}
pub fn reconcile_finished_sessions() -> Result<usize> {
    registry::reconcile()
}
pub fn session_registry_status() -> RegistryStatus {
    registry::status()
}
#[cfg(test)]
pub(crate) fn foundation_cases() {
    registry::foundation_cases();
    preparation_cases();
}
#[cfg(test)]
#[path = "../bin/nw-process-fixture/relay.rs"]
mod direct_fixture;
fn error(code: &str, message: &str) -> DomainError {
    DomainError::new(code, message)
}
fn lock<T>(value: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    value
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
#[cfg(test)]
enum PreparationControl {
    Pause(Arc<AtomicBool>),
    AfterFirst(bool),
}
struct Mailbox {
    preparation: Option<([ByteInput; 2], DirectoryStamp)>,
    invocation: Option<(InvocationSpec, StdoutMode)>,
    ready: bool,
    spawned: bool,
    cause: Option<DomainError>,
    finished: Option<SessionResult<SessionOutput>>,
    #[cfg(test)]
    direct: bool,
    #[cfg(test)]
    endpoints: Option<[String; 2]>,
    #[cfg(test)]
    preparation_control: Option<PreparationControl>,
}
struct Shared {
    id: SessionId,
    policy: SessionPolicy,
    cancel: CancellationToken,
    registered: AtomicBool,
    retained: AtomicBool,
    input_threads: Arc<AtomicUsize>,
    stdout_bytes: AtomicU64,
    diagnostic_bytes: AtomicU64,
    mailbox: Mutex<Mailbox>,
}
impl Shared {
    fn new(
        id: SessionId,
        policy: SessionPolicy,
        cancel: CancellationToken,
        inputs: [ByteInput; 2],
        parent: DirectoryStamp,
    ) -> Self {
        Self {
            id,
            policy,
            cancel: cancel.child_token(),
            registered: AtomicBool::new(false),
            retained: AtomicBool::new(false),
            input_threads: Arc::new(AtomicUsize::new(0)),
            stdout_bytes: AtomicU64::new(0),
            diagnostic_bytes: AtomicU64::new(0),
            mailbox: Mutex::new(Mailbox {
                preparation: Some((inputs, parent)),
                invocation: None,
                ready: false,
                spawned: false,
                cause: None,
                finished: None,
                #[cfg(test)]
                direct: false,
                #[cfg(test)]
                endpoints: None,
                #[cfg(test)]
                preparation_control: None,
            }),
        }
    }
    fn fail(&self, cause: DomainError) {
        let mut mailbox = lock(&self.mailbox);
        super::input_relay::record_failure(&mut mailbox.cause, &cause);
        self.cancel.cancel();
    }
    fn cause(&self) -> DomainError {
        lock(&self.mailbox)
            .cause
            .clone()
            .unwrap_or_else(|| error("CANCELLED", "Owned byte session cancelled"))
    }
}
struct Client {
    shared: Arc<Shared>,
    armed: bool,
}
impl Drop for Client {
    fn drop(&mut self) {
        if self.armed {
            self.shared.cancel.cancel();
        }
    }
}
impl Client {
    fn retained(&self) -> SessionFailure {
        self.shared.cancel.cancel();
        let callback_failed = registry::uncertain(self.shared.id);
        let cause = if callback_failed {
            error(
                "SESSION_CHARGE_CALLBACK_FAILED",
                &format!(
                    "Charge mark panicked; retained session cause: {}",
                    self.shared.cause().code
                ),
            )
        } else {
            self.shared.cause()
        };
        SessionFailure {
            cause,
            settlement: Settlement::Retained(self.shared.id),
        }
    }
    async fn phase(&self, spawned: bool) -> SessionResult<()> {
        let stop = Instant::now() + self.shared.policy.preparation_deadline();
        loop {
            let _ = registry::reconcile();
            {
                let mut mailbox = lock(&self.shared.mailbox);
                if mailbox
                    .finished
                    .as_ref()
                    .is_some_and(std::result::Result::is_err)
                    && let Some(Err(failure)) = mailbox.finished.take()
                {
                    return Err(failure);
                }
                if if spawned {
                    mailbox.spawned
                } else {
                    mailbox.ready
                } {
                    return Ok(());
                }
            }
            if Instant::now() >= stop {
                self.shared.fail(error(
                    "SESSION_PREPARATION_TIMEOUT",
                    "Preparation response deadline expired",
                ));
                return Err(self.retained());
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
    async fn finish(&mut self) -> SessionResult<SessionOutput> {
        let execution = self.shared.policy.execution;
        let started = Instant::now();
        let mut cleanup = None;
        loop {
            let _ = registry::reconcile();
            if let Some(result) = lock(&self.shared.mailbox).finished.take() {
                self.armed = false;
                return result;
            }
            if execution.is_some_and(|limit| started.elapsed() >= limit) {
                self.shared.fail(error(
                    "PROCESS_TIMEOUT",
                    "Owned byte execution deadline expired",
                ));
            }
            if self.shared.cancel.is_cancelled() {
                let stop = *cleanup
                    .get_or_insert_with(|| Instant::now() + self.shared.policy.cleanup_deadline());
                if Instant::now() >= stop {
                    return Err(self.retained());
                }
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
}
impl PreparedRelays {
    pub async fn prepare(
        inputs: [ByteInput; 2],
        parent: DirectoryStamp,
        policy: SessionPolicy,
        charge: Box<dyn SessionCharge>,
        cancel: CancellationToken,
    ) -> SessionResult<Self> {
        let client = registry::admit(inputs, parent, policy, charge, cancel)?;
        Self::from_client(client).await
    }
    async fn from_client(client: Client) -> SessionResult<Self> {
        client.phase(false).await?;
        Ok(Self { client })
    }
}
impl MediaProcessSession {
    pub async fn spawn(
        prepared: PreparedRelays,
        invocation: InvocationSpec,
        stdout: StdoutMode,
    ) -> SessionResult<Self> {
        let mut client = prepared.client;
        if let Err(cause) = invocation.validate() {
            client.shared.fail(cause);
            return match client.finish().await {
                Err(failure) => Err(failure),
                Ok(done) => Err(SessionFailure {
                    cause: error("INVOCATION_DENIED", "Rejected invocation settled"),
                    settlement: Settlement::Closed(done.closed),
                }),
            };
        }
        lock(&client.shared.mailbox).invocation = Some((invocation, stdout));
        client.phase(true).await?;
        Ok(Self { client })
    }
    pub async fn finish(mut self) -> SessionResult<SessionOutput> {
        self.client.finish().await
    }
    pub async fn cancel_and_finish(mut self) -> SessionResult<SessionOutput> {
        self.client.shared.cancel.cancel();
        self.client.finish().await
    }
    pub fn id(&self) -> SessionId {
        self.client.shared.id
    }
}

struct PreparedInvocation {
    program: PathBuf,
    args: Vec<String>,
    executable: SourceStamp,
}
impl PreparedInvocation {
    fn new(spec: InvocationSpec, endpoints: &[String; 2], shared: &Shared) -> Result<Self> {
        if !spec.program.is_absolute()
            || spec.args.len() > 64
            || spec.profile_id.is_empty()
            || spec.profile_id.len() > 128
            || !spec.profile_id.is_ascii()
            || spec.profile_id.contains('\0')
        {
            return Err(error(
                "INVOCATION_DENIED",
                "Invalid executable, profile or argument bounds",
            ));
        }
        let mut seen = [false; 2];
        let mut bytes = 0usize;
        let mut args = Vec::with_capacity(spec.args.len());
        for argument in spec.args {
            let value = match argument {
                SessionArg::Literal(value) => value,
                SessionArg::Input(RelayInputId(id)) => {
                    let index = usize::from(id);
                    if index > 1 || seen[index] {
                        return Err(error(
                            "INVOCATION_DENIED",
                            "Duplicate or unknown input placeholder",
                        ));
                    }
                    seen[index] = true;
                    endpoints[index].clone()
                }
            };
            bytes = bytes
                .checked_add(value.len())
                .ok_or_else(|| error("RESOURCE_LIMIT", "Argument count overflow"))?;
            if bytes > 16 * 1024 || value.contains('\0') {
                return Err(error(
                    "INVOCATION_DENIED",
                    "Argument text exceeds its bound",
                ));
            }
            args.push(value);
        }
        if seen != [true, true] {
            return Err(error(
                "INVOCATION_DENIED",
                "Exactly two input placeholders are required",
            ));
        }
        let stamp = SourceStamp::capture_resolved(&spec.program).map_err(|_| {
            error(
                "PROCESS_LAUNCH_FAILED",
                "Executable is not an admitted regular file",
            )
        })?;
        let mut reader = stamp.try_clone_file()?;
        let mut buffer = [0; 8192];
        let mut hash = Sha256::new();
        let stop = Instant::now() + shared.policy.preparation_deadline();
        loop {
            if shared.cancel.is_cancelled() || Instant::now() >= stop {
                return Err(error(
                    "CANCELLED",
                    "Executable validation cancelled or expired",
                ));
            }
            let count = reader.read(&mut buffer).map_err(crate::files::io_error)?;
            if count == 0 {
                break;
            }
            hash.update(&buffer[..count]);
        }
        stamp.revalidate()?;
        let actual: [u8; 32] = hash.finalize().into();
        if actual != spec.executable_sha256 {
            return Err(error("COMPONENT_UNAVAILABLE", "Executable hash changed"));
        }
        tracing::info!(program = %spec.program.display(), profile = %spec.profile_id, executable_sha256 = %hex::encode(actual), argv = "<frozen literals; input-0; input-1>", "stage.prepared.private");
        Ok(Self {
            program: spec.program,
            args,
            executable: stamp,
        })
    }
}

type OutputTask = tokio::task::JoinHandle<Result<(Vec<u8>, u64)>>;
struct CoordinatorState {
    relays: Option<OwnedInputRelaySet>,
    child: Option<OwnedChild>,
    stdout: Option<OutputTask>,
    stderr: Option<OutputTask>,
    out: Option<(Vec<u8>, u64)>,
    err: Option<(Vec<u8>, u64)>,
    exit: Option<std::process::ExitStatus>,
    spawned: bool,
    stdout_started: bool,
    stderr_started: bool,
}
struct CoordinatorDone {
    output: Result<RunOutput>,
    pumps: [PumpReceipt; 2],
    child_spawned: bool,
    child_reaped: bool,
    outputs_joined: bool,
    namespace_cleaned: bool,
}

fn coordinator(shared: Arc<Shared>) -> CoordinatorDone {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build();
    let mut state = ManuallyDrop::new(CoordinatorState {
        relays: None,
        child: None,
        stdout: None,
        stderr: None,
        out: None,
        err: None,
        exit: None,
        spawned: false,
        stdout_started: false,
        stderr_started: false,
    });
    let runtime = ManuallyDrop::new(match runtime {
        Ok(runtime) => runtime,
        Err(problem) => {
            drop(lock(&shared.mailbox).preparation.take());
            return CoordinatorDone {
                output: Err(error("SESSION_RUNTIME_FAILED", &problem.to_string())),
                pumps: [
                    PumpReceipt::not_created(RelayInputId(0)),
                    PumpReceipt::not_created(RelayInputId(1)),
                ],
                child_spawned: false,
                child_reaped: false,
                outputs_joined: false,
                namespace_cleaned: true,
            };
        }
    });
    let run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        runtime.block_on(drive(&shared, &mut state))
    }));
    // Launch can unwind after installing a suspended native process. Receipt
    // facts come from its retained owner, even when drive never reached spawn's
    // normal return or installed either output consumer.
    state.spawned = state.child.as_ref().is_some_and(OwnedChild::spawned);
    match run {
        Ok(Ok(())) => {}
        Ok(Err(cause)) => shared.fail(cause),
        Err(_) => shared.fail(error(
            "SESSION_COORDINATOR_PANIC",
            "Owned coordinator panicked",
        )),
    }
    let cleanup = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        runtime.block_on(settle(&shared, &mut state))
    }));
    if cleanup.is_err() {
        registry::uncertain(shared.id);
        // Neither runtime nor native owners are dropped on a failed recovery.
        loop {
            std::thread::park_timeout(Duration::from_secs(60));
        }
    }
    let mut state = ManuallyDrop::into_inner(state);
    let pumps = state
        .relays
        .as_mut()
        .map(OwnedInputRelaySet::take_receipts)
        .unwrap_or_else(|| {
            [
                PumpReceipt::not_created(RelayInputId(0)),
                PumpReceipt::not_created(RelayInputId(1)),
            ]
        });
    let output = match lock(&shared.mailbox).cause.take() {
        Some(cause) => Err(cause),
        None => {
            let (stdout, _) = state.out.take().unwrap_or_default();
            let (stderr, diagnostic_bytes) = state.err.take().unwrap_or_default();
            Ok(RunOutput {
                stdout,
                stderr_tail: String::from_utf8_lossy(&stderr).into_owned(),
                exit_code: state.exit.and_then(|status| status.code()),
                diagnostic_bytes,
            })
        }
    };
    let done = CoordinatorDone {
        output,
        pumps,
        child_spawned: state.spawned,
        child_reaped: state.exit.is_some(),
        outputs_joined: state.stdout_started
            && state.stderr_started
            && state.stdout.is_none()
            && state.stderr.is_none(),
        namespace_cleaned: true,
    };
    drop(state);
    drop(ManuallyDrop::into_inner(runtime));
    done
}

async fn drive(shared: &Arc<Shared>, state: &mut CoordinatorState) -> Result<()> {
    let (inputs, parent) = lock(&shared.mailbox)
        .preparation
        .take()
        .ok_or_else(|| error("SESSION_STATE_FAILED", "Preparation ownership missing"))?;
    state.relays = Some(OwnedInputRelaySet::prepare(
        inputs,
        parent,
        shared.cancel.clone(),
        shared.input_threads.clone(),
    )?);
    #[cfg(test)]
    {
        let control = lock(&shared.mailbox).preparation_control.take();
        match control {
            Some(PreparationControl::Pause(gate)) => {
                let stop = Instant::now() + Duration::from_secs(2);
                while !gate.load(Ordering::Acquire) && Instant::now() < stop {
                    tokio::time::sleep(Duration::from_millis(1)).await;
                }
            }
            Some(PreparationControl::AfterFirst(panics)) => state
                .relays
                .as_mut()
                .expect("Installed relay owner")
                .fail_after_first(panics),
            None => {}
        }
    }
    let stop = Instant::now() + shared.policy.preparation_deadline();
    loop {
        let relays = state
            .relays
            .as_mut()
            .ok_or_else(|| error("SESSION_STATE_FAILED", "Relay owner missing"))?;
        relays.observe()?;
        if relays.ready() {
            break;
        }
        if shared.cancel.is_cancelled() {
            return Err(shared.cause());
        }
        if Instant::now() >= stop {
            return Err(error(
                "SESSION_PREPARATION_TIMEOUT",
                "Endpoint preparation deadline expired",
            ));
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    #[cfg(test)]
    {
        lock(&shared.mailbox).endpoints = Some(
            state
                .relays
                .as_ref()
                .ok_or_else(|| error("SESSION_STATE_FAILED", "Relay owner missing"))?
                .endpoints()?,
        );
    }
    lock(&shared.mailbox).ready = true;
    let command = loop {
        if shared.cancel.is_cancelled() {
            return Err(shared.cause());
        }
        #[cfg(test)]
        if lock(&shared.mailbox).direct {
            return direct_drive(shared, state).await;
        }
        if let Some(command) = lock(&shared.mailbox).invocation.take() {
            break command;
        }
        if Instant::now() >= stop {
            return Err(error(
                "SESSION_PREPARATION_TIMEOUT",
                "Prepared session was not launched",
            ));
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    };
    let relays = state
        .relays
        .as_mut()
        .ok_or_else(|| error("SESSION_STATE_FAILED", "Relay owner missing"))?;
    let invocation = PreparedInvocation::new(command.0, &relays.endpoints()?, shared)?;
    match &command.1 {
        StdoutMode::Capture { limit } if *limit > 256 * 1024 => {
            return Err(error("RESOURCE_LIMIT", "Capture exceeds fixture partition"));
        }
        StdoutMode::Binary { byte_limit, .. } if *byte_limit > 256 * 1024 => {
            return Err(error(
                "RESOURCE_LIMIT",
                "Binary output exceeds fixture partition",
            ));
        }
        _ => {}
    }
    if shared.cancel.is_cancelled() {
        return Err(shared.cause());
    }
    invocation.executable.revalidate()?;
    // Install the complete native owner before launch can create a process,
    // pending output operation or partially assigned suspended child.
    state.child = Some(OwnedChild::new());
    let child = state
        .child
        .as_mut()
        .ok_or_else(|| error("SESSION_STATE_FAILED", "Child owner missing"))?;
    let launched = child.launch(&invocation.program, &invocation.args);
    state.spawned = child.spawned();
    let (stdout, stderr) = launched?;
    invocation.executable.revalidate()?;
    let out_shared = shared.clone();
    state.stdout = Some(tokio::spawn(async move {
        let result = output(stdout, command.1, &out_shared).await;
        if let Err(cause) = &result {
            out_shared.fail(cause.clone());
        }
        result
    }));
    state.stdout_started = true;
    state.stderr = Some(tokio::spawn(diagnostics(stderr, shared.clone())));
    state.stderr_started = true;
    relays.start()?;
    lock(&shared.mailbox).spawned = true;
    let started = Instant::now();
    loop {
        observe_tasks(shared, state).await;
        state
            .relays
            .as_mut()
            .ok_or_else(|| error("SESSION_STATE_FAILED", "Relay owner missing"))?
            .observe()?;
        observe_child(shared, state)?;
        if state.exit.is_some_and(|status| !status.success()) {
            return Err(error(
                "PROCESS_FAILED",
                "Owned byte child exited unsuccessfully",
            ));
        }
        if shared.cancel.is_cancelled() {
            return Err(shared.cause());
        }
        if shared
            .policy
            .execution
            .is_some_and(|limit| started.elapsed() >= limit)
        {
            return Err(error(
                "PROCESS_TIMEOUT",
                "Owned byte execution deadline expired",
            ));
        }
        if state.exit.is_some()
            && state.child.as_ref().is_none_or(OwnedChild::settled)
            && state.child.as_ref().is_none_or(OwnedChild::outputs_settled)
            && state.stdout.is_none()
            && state.stderr.is_none()
            && state
                .relays
                .as_ref()
                .is_some_and(OwnedInputRelaySet::joined)
        {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

#[cfg(test)]
async fn direct_drive(shared: &Shared, state: &mut CoordinatorState) -> Result<()> {
    let relays = state
        .relays
        .as_mut()
        .ok_or_else(|| error("SESSION_STATE_FAILED", "Direct relay owner missing"))?;
    relays.start()?;
    let stop = Instant::now() + Duration::from_secs(3);
    loop {
        relays.observe()?;
        if relays.joined() {
            return Ok(());
        }
        if shared.cancel.is_cancelled() {
            return Err(shared.cause());
        }
        if Instant::now() >= stop {
            return Err(error("PROCESS_TIMEOUT", "Finite direct relay case expired"));
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

#[cfg(test)]
pub(crate) fn direct_cases() {
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
    assert_eq!(registry::status().admitted(), 0);
    let root = tempfile::tempdir().unwrap();
    let root_path = std::fs::canonicalize(root.path()).unwrap();
    let parent = crate::security::private_directory(&root_path.join("relay")).unwrap();
    let lengths = [65_553usize, 98_335usize];
    let tails: [&[u8]; 2] = [b"INPUT0-TAIL-\x00\xffEND!", b"INPUT1-TAIL-\x00\xffEND!"];
    let mut expected = [[0u8; 32]; 2];
    let inputs = std::array::from_fn(|id| {
        let mut bytes = (0..lengths[id])
            .map(|n| ((n * 17 + id * 73) % 256) as u8)
            .collect::<Vec<_>>();
        assert_eq!(tails[id].len(), 18);
        bytes[lengths[id] - 18..].copy_from_slice(tails[id]);
        assert_eq!(bytes.len(), lengths[id]);
        expected[id] = Sha256::digest(&bytes).into();
        let path = root_path.join(format!("input-{id}.bytes"));
        std::fs::write(&path, bytes).unwrap();
        ByteInput::new(
            RelayInputId(id as u8),
            SourceStamp::capture_resolved(&path).unwrap(),
            lengths[id] as u64,
            expected[id],
        )
        .unwrap()
    });
    let charge = Arc::new(AtomicUsize::new(1));
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let mut prepared = PreparedRelays::prepare(
            inputs,
            DirectoryStamp::capture_resolved(&parent).unwrap(),
            SessionPolicy::byte_fixture(),
            Box::new(Charge(charge.clone())),
            CancellationToken::new(),
        )
        .await
        .unwrap_or_else(|failure| panic!("{}", failure.cause));
        let endpoints = lock(&prepared.client.shared.mailbox)
            .endpoints
            .clone()
            .expect("Direct endpoint readiness");
        lock(&prepared.client.shared.mailbox).direct = true;
        let args = vec![
            "direct-fixture".into(),
            "relay-two-input".into(),
            endpoints[0].clone(),
            endpoints[1].clone(),
        ];
        let result = direct_fixture::run(&args);
        let done = prepared
            .client
            .finish()
            .await
            .unwrap_or_else(|failure| panic!("{}", failure.cause));
        assert_eq!(result, std::process::ExitCode::SUCCESS);
        assert!(!done.closed.child_spawned());
        assert!(!done.closed.child_reaped());
        assert!(done.closed.coordinator_joined());
        assert!(done.closed.namespace_cleaned());
        for (id, receipt) in done.closed.pumps().iter().enumerate() {
            assert_eq!(receipt.bytes(), lengths[id] as u64);
            assert_eq!(receipt.sha256(), Some(expected[id]));
            assert!(receipt.worker_joined());
            assert!(receipt.endpoint_closed());
            assert!(receipt.endpoint_cleaned());
            assert!(receipt.transport_complete());
        }
    });
    assert_eq!(charge.load(Ordering::Acquire), 0);
    assert_eq!(registry::status().admitted(), 0);
}

fn observe_child(shared: &Shared, state: &mut CoordinatorState) -> Result<()> {
    if let Some(child) = state.child.as_mut() {
        // Native output pumps and process observation both progress even when
        // the application sink is pending or one native branch reports error.
        let output = child.pump_outputs(shared.cancel.is_cancelled());
        let process = child.observe(shared.cancel.is_cancelled());
        if let Ok(status) = &process {
            state.exit = *status;
        }
        output?;
        process?;
    }
    Ok(())
}
async fn observe_tasks(shared: &Shared, state: &mut CoordinatorState) {
    for (task, output) in [
        (&mut state.stdout, &mut state.out),
        (&mut state.stderr, &mut state.err),
    ] {
        if task
            .as_ref()
            .is_some_and(tokio::task::JoinHandle::is_finished)
            && let Some(join) = task.take()
        {
            match join.await {
                Ok(Ok(result)) => *output = Some(result),
                Ok(Err(cause)) => shared.fail(cause),
                Err(problem) if problem.is_panic() => {
                    std::panic::resume_unwind(problem.into_panic())
                }
                Err(_) => shared.fail(error(
                    "SESSION_OUTPUT_FAILED",
                    "Output task was unexpectedly cancelled",
                )),
            }
        }
    }
}
async fn settle(shared: &Shared, state: &mut CoordinatorState) {
    let stop = Instant::now() + shared.policy.cleanup_deadline();
    loop {
        if let Err(cause) = observe_child(shared, state) {
            shared.fail(cause);
            registry::uncertain(shared.id);
        }
        observe_tasks(shared, state).await;
        if let Some(relays) = state.relays.as_mut()
            && let Err(cause) = relays.observe()
        {
            shared.fail(cause);
        }
        let workers = state.relays.as_ref().is_none_or(OwnedInputRelaySet::joined);
        let child = state.child.as_ref().is_none_or(OwnedChild::settled);
        let native_outputs = state.child.as_ref().is_none_or(OwnedChild::outputs_settled);
        // Native output owners settle their exact operations before task teardown.
        // A primary exit alone never permits dropping pending read storage.
        if Instant::now() >= stop {
            registry::uncertain(shared.id);
            if child && native_outputs {
                for task in [&mut state.stdout, &mut state.stderr] {
                    if let Some(join) = task.take() {
                        join.abort();
                        match join.await {
                            Err(problem) if problem.is_panic() => {
                                std::panic::resume_unwind(problem.into_panic())
                            }
                            Ok(Err(cause)) => shared.fail(cause),
                            Err(problem) if !problem.is_cancelled() => {
                                shared.fail(error("SESSION_OUTPUT_FAILED", &problem.to_string()))
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
        if workers && child && native_outputs && state.stdout.is_none() && state.stderr.is_none() {
            if let Some(relays) = state.relays.as_mut()
                && let Err(cause) = relays.cleanup()
            {
                shared.fail(cause);
                registry::uncertain(shared.id);
                tokio::time::sleep(Duration::from_millis(10)).await;
                continue;
            }
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

async fn output<R: AsyncRead + Unpin>(
    mut reader: R,
    mut mode: StdoutMode,
    shared: &Shared,
) -> Result<(Vec<u8>, u64)> {
    let cancel = &shared.cancel;
    let mut buffer = [0; 8192];
    let mut total = 0u64;
    let mut captured = Vec::new();
    loop {
        let count = tokio::select! { biased; _ = cancel.cancelled() => return Err(error("CANCELLED", "Output wait cancelled")), count = reader.read(&mut buffer) => count.map_err(crate::files::io_error)? };
        if count == 0 {
            break;
        }
        total = total
            .checked_add(count as u64)
            .ok_or_else(|| error("RESOURCE_LIMIT", "Output count overflow"))?;
        shared.stdout_bytes.store(total, Ordering::Release);
        match &mut mode {
            StdoutMode::Capture { limit } => {
                if total > *limit as u64 {
                    return Err(error("RESOURCE_LIMIT", "Capture budget exceeded"));
                }
                captured.extend_from_slice(&buffer[..count]);
            }
            StdoutMode::Binary { sink, byte_limit } => {
                if total > *byte_limit {
                    return Err(error("RESOURCE_LIMIT", "Binary output budget exceeded"));
                }
                tokio::select! { biased; _ = cancel.cancelled() => return Err(error("CANCELLED", "Sink wait cancelled")), result = sink.write(&buffer[..count]) => result? }
            }
        }
    }
    if let StdoutMode::Binary { sink, .. } = &mut mode {
        tokio::select! { biased; _ = cancel.cancelled() => return Err(error("CANCELLED", "Sink finish cancelled")), result = sink.finish() => result? }
    }
    Ok((captured, total))
}
async fn diagnostics<R: AsyncRead + Unpin>(
    mut reader: R,
    shared: Arc<Shared>,
) -> Result<(Vec<u8>, u64)> {
    let mut buffer = [0; 8192];
    let mut tail = std::collections::VecDeque::with_capacity(4096);
    let mut total = 0u64;
    loop {
        let count = reader
            .read(&mut buffer)
            .await
            .map_err(crate::files::io_error)?;
        if count == 0 {
            return Ok((tail.into_iter().collect(), total));
        }
        total = total
            .checked_add(count as u64)
            .ok_or_else(|| error("RESOURCE_LIMIT", "Diagnostic count overflow"))?;
        shared.diagnostic_bytes.store(total, Ordering::Release);
        for byte in &buffer[..count] {
            if tail.len() == 4096 {
                tail.pop_front();
            }
            tail.push_back(*byte);
        }
    }
}

#[cfg(test)]
fn preparation_cases() {
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
    struct Gate(Arc<AtomicBool>);
    impl Drop for Gate {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Release);
        }
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    for case in 0..3 {
        assert_eq!(registry::status().admitted(), 0);
        let root = tempfile::tempdir().unwrap();
        let path = std::fs::canonicalize(root.path()).unwrap();
        let parent = crate::security::private_directory(&path.join("preparation")).unwrap();
        let inputs = std::array::from_fn(|id| {
            let source = path.join(format!("source-{id}"));
            std::fs::write(&source, [id as u8]).unwrap();
            ByteInput::new(
                RelayInputId(id as u8),
                SourceStamp::capture_resolved(&source).unwrap(),
                1,
                Sha256::digest([id as u8]).into(),
            )
            .unwrap()
        });
        let charge = Arc::new(AtomicUsize::new(1));
        let gate = Gate(Arc::new(AtomicBool::new(false)));
        let control = if case == 0 {
            PreparationControl::Pause(gate.0.clone())
        } else {
            PreparationControl::AfterFirst(case == 2)
        };
        let client = registry::admit_started(
            inputs,
            DirectoryStamp::capture_resolved(&parent).unwrap(),
            SessionPolicy::byte_fixture(),
            Box::new(Charge(charge.clone())),
            CancellationToken::new(),
            move |shared| {
                lock(&shared.mailbox).preparation_control = Some(control);
                registry::start_coordinator(shared)
            },
        )
        .unwrap_or_else(|failure| panic!("{}", failure.cause));
        runtime.block_on(async {
            if case == 0 {
                let mut future = Box::pin(PreparedRelays::from_client(client));
                let mut context = std::task::Context::from_waker(std::task::Waker::noop());
                assert!(future.as_mut().poll(&mut context).is_pending());
                drop(future);
                assert_eq!(charge.load(Ordering::Acquire), 1);
                assert_eq!(registry::status().admitted(), 1);
                gate.0.store(true, Ordering::Release);
            } else {
                let failure = match PreparedRelays::from_client(client).await {
                    Ok(_) => panic!("injected preparation failure succeeded"),
                    Err(failure) => failure,
                };
                assert_eq!(
                    failure.cause.code,
                    if case == 1 {
                        "RELAY_PREPARATION_FAILED"
                    } else {
                        "SESSION_COORDINATOR_PANIC"
                    }
                );
                match failure.settlement {
                    Settlement::Closed(closed) => {
                        assert!(!closed.child_spawned());
                        assert!(closed.coordinator_joined());
                        assert!(closed.namespace_cleaned());
                        assert!(closed.pumps()[0].worker_joined());
                        assert!(closed.pumps()[0].endpoint_closed());
                        assert!(closed.pumps()[0].endpoint_cleaned());
                        assert!(!closed.pumps()[1].endpoint_created());
                    }
                    Settlement::Retained(_) => {
                        panic!("preparation fixture failed finite actual settlement")
                    }
                }
            }
            let stop = Instant::now() + Duration::from_secs(4);
            while charge.load(Ordering::Acquire) != 0 && Instant::now() < stop {
                registry::reconcile().unwrap();
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            assert_eq!(charge.load(Ordering::Acquire), 0);
            assert_eq!(registry::status().admitted(), 0);
            assert_eq!(std::fs::read_dir(&parent).unwrap().count(), 0);
        });
    }
}
