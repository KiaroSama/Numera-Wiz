//! Exact candidate protocol. Hosted observation must precede immutable-pin qualification.
use crate::{PrivateResult, Result, ScanFailure, assets, private, process, require};
use serde_json::{Value, json};
use std::{collections::BTreeMap, env, fs, io::Write,
    os::unix::{fs::MetadataExt, process::CommandExt},
    path::{Path, PathBuf}, process::Command, time::{Duration, Instant, SystemTime, UNIX_EPOCH}};

pub const LOG_OPTIONS: &str = "--full-history --diff-filter=uxdb --no-ext-diff --no-textconv --ignore-submodules=all -m";
#[derive(Clone)]
pub enum Scope { All, PullRequest { base: String, head: String } }
impl Scope {
    pub fn revision(&self) -> String {
        match self { Self::All => "--all".into(), Self::PullRequest { base, head } => format!("{base}..{head}") }
    }
    pub fn validate(&self) -> Result<()> {
        match self { Self::All => Ok(()), Self::PullRequest { base, head } =>
            require(revision(base) && revision(head), "SECURITY_REVISION_INVALID") }
    }
    pub fn from_event() -> Result<Self> {
        match env::var("GITHUB_EVENT_NAME").private("SECURITY_EVENT_INVALID")?.as_str() {
            "push" | "schedule" | "workflow_dispatch" => Ok(Self::All),
            "pull_request" => {
                let path = env::var_os("GITHUB_EVENT_PATH").ok_or(ScanFailure("SECURITY_EVENT_INVALID"))?;
                let event: Value = serde_json::from_slice(&private::read(Path::new(&path), private::MIB)?)
                    .private("SECURITY_EVENT_INVALID")?;
                let scope = Self::PullRequest { base: string(&event["pull_request"]["base"], "sha")?.to_owned(),
                    head: string(&event["pull_request"]["head"], "sha")?.to_owned() };
                scope.validate()?; Ok(scope)
            }
            _ => Err(ScanFailure("SECURITY_EVENT_INVALID")),
        }
    }
}
pub fn revision(text: &str) -> bool { text.len() == 40 && text.bytes().all(|b| b.is_ascii_hexdigit()) }
pub fn string<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    value[key].as_str().ok_or(ScanFailure("SECURITY_CONTEXT_INVALID"))
}
pub fn write_json(path: &Path, value: &Value) -> Result<()> {
    let bytes = serde_json::to_vec(value).private("SECURITY_CONTEXT_INVALID")?;
    require(bytes.len() as u64 <= private::RECORD_LIMIT, "SECURITY_RECORD_OVERSIZE")?;
    private::write(path, &bytes)
}
pub fn read_json(path: &Path) -> Result<Value> {
    let bytes = private::read(path, private::RECORD_LIMIT)?;
    let value: Value = serde_json::from_slice(&bytes).private("SECURITY_CONTEXT_INVALID")?;
    // Authored protocol numbers are bounded decimal integers. The report's
    // arbitrary_precision feature must not make alternate numeric spellings
    // canonical here (for example 1e0 or -0 in an otherwise ignored field).
    require(canonical_integer_values(&value), "SECURITY_CONTEXT_INVALID")?;
    // Authored records are canonical JSON. Re-serialization rejects duplicate keys,
    // trailing values and alternate encodings rather than silently overwriting them.
    require(serde_json::to_vec(&value).private("SECURITY_CONTEXT_INVALID")? == bytes,
        "SECURITY_CONTEXT_INVALID")?;
    Ok(value)
}
fn canonical_integer_values(value: &Value) -> bool {
    match value {
        Value::Number(number) => {
            let canonical = number.as_i64().map(|value| value.to_string())
                .or_else(|| number.as_u64().map(|value| value.to_string()));
            canonical.is_some_and(|text| number.to_string() == text)
        }
        Value::Array(values) => values.iter().all(canonical_integer_values),
        Value::Object(values) => values.values().all(canonical_integer_values),
        _ => true,
    }
}
pub fn unix_millis() -> Result<u64> {
    SystemTime::now().duration_since(UNIX_EPOCH).private("SECURITY_CLOCK_INVALID")?
        .as_millis().try_into().private("SECURITY_CLOCK_INVALID")
}
pub fn deadline_from(value: &Value) -> Result<Instant> {
    let remaining = value["deadline_ms"].as_u64().ok_or(ScanFailure("SECURITY_CONTEXT_INVALID"))?
        .checked_sub(unix_millis()?).ok_or(ScanFailure("SECURITY_DEADLINE"))?;
    require(remaining <= 20 * 60 * 1000, "SECURITY_DEADLINE")?;
    Ok(Instant::now() + Duration::from_millis(remaining))
}
pub fn history_args(root: &str, scope: &str) -> Vec<String> {
    let mut args = ["-C", root, "log", "-p", "-U0"].map(str::to_owned).to_vec();
    args.extend(LOG_OPTIONS.split(' ').map(str::to_owned));
    args.push(scope.to_owned()); args
}
pub fn classify(args: &[String], root: &str, scope: &str) -> Result<&'static str> {
    require(args.len() <= 128 && args.iter().all(|s| s.len() <= 4096)
        && args.iter().map(String::len).sum::<usize>() <= 16 * 1024, "SECURITY_ARGV_OVERSIZE")?;
    if args == history_args(root, scope) { return Ok("history"); }
    if args == ["ls-remote", "--quiet", "--get-url"] { return Ok("metadata"); }
    if args.len() == 5 && args[0] == "-C" && args[1] == root && args[2] == "cat-file" && args[3] == "blob" {
        let item = &args[4];
        let (commit, path) = item.split_once(':').ok_or(ScanFailure("SECURITY_ARGV_INVALID"))?;
        require(revision(commit) && !path.is_empty() && !path.starts_with('/')
            && path.split('/').all(|p| !p.is_empty() && p != "." && p != ".."), "SECURITY_ARGV_INVALID")?;
        return Ok("blob");
    }
    Err(ScanFailure("SECURITY_ARGV_INVALID"))
}

pub struct Context { pub root: PathBuf, pub value: Value }
impl Context {
    pub fn prepare(root: &Path, repository: &Path, git: &Path, release: &assets::Release,
        scope: &Scope, deadline: Instant) -> Result<Self> {
        scope.validate()?;
        let current = env::current_exe().private("SECURITY_EXECUTABLE_INVALID")?;
        let helper_hash = private::copy_executable(&current, &root.join("git"))?;
        private::copy_executable(&current, &root.join("security-launch"))?;
        private::directory(&root.join("proof"))?;
        private::write(&root.join("scanner.toml"), b"[extend]\nuseDefault = true\n")?;
        let value = json!({"run": private::nonce()?, "token": private::nonce()?,
            "repository": private::text(repository)?, "repository_id": private::identity(repository)?,
            "git": private::text(git)?, "git_id": private::identity(git)?, "git_sha": private::executable_hash(git)?,
            "helper_sha": helper_hash, "scanner": private::text(release.executable())?,
            "scanner_sha": private::executable_hash(release.executable())?, "scope": scope.revision(),
            "parent": rustix::process::getpid().as_raw_pid(), "scratch_id": private::identity(root)?,
            "deadline_ms": unix_millis()? + u64::try_from(deadline.saturating_duration_since(Instant::now()).as_millis())
                .private("SECURITY_DEADLINE")?});
        write_json(&root.join("context.json"), &value)?;
        Ok(Self { root: root.to_owned(), value })
    }
    pub fn launch(&self) -> Result<Command> {
        let mut command = Command::new(self.root.join("security-launch"));
        private::controlled(&mut command, &self.root);
        command.current_dir(string(&self.value, "repository")?)
            .env("NW_SECURITY_TOKEN", string(&self.value, "token")?);
        Ok(command)
    }
    pub fn monitor(&self) -> Result<()> {
        let report = self.root.join("findings.json");
        match fs::symlink_metadata(&report) {
            Ok(m) => require(m.is_file() && !m.file_type().is_symlink() && m.len() <= private::REPORT_LIMIT,
                "SECURITY_REPORT_OVERSIZE")?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {},
            Err(_) => return Err(ScanFailure("SECURITY_REPORT_INVALID")),
        }
        proof_files(&self.root.join("proof")).map(|_| ())
    }
}
fn authenticated() -> Result<Context> {
    let executable = fs::canonicalize(env::current_exe().private("SECURITY_CONTEXT_INVALID")?)
        .private("SECURITY_CONTEXT_INVALID")?;
    let root = executable.parent().ok_or(ScanFailure("SECURITY_CONTEXT_INVALID"))?.to_owned();
    let meta = fs::symlink_metadata(&root).private("SECURITY_CONTEXT_INVALID")?;
    require(meta.is_dir() && meta.mode() & 0o777 == 0o700 && meta.uid() == rustix::process::getuid().as_raw(),
        "SECURITY_CONTEXT_INVALID")?;
    let value = read_json(&root.join("context.json"))?;
    require(string(&value, "token")? == env::var("NW_SECURITY_TOKEN").private("SECURITY_CONTEXT_INVALID")?
        && string(&value, "scratch_id")? == private::identity(&root)?
        && string(&value, "helper_sha")? == private::executable_hash(&executable)?, "SECURITY_CONTEXT_INVALID")?;
    let repository = Path::new(string(&value, "repository")?);
    let git = Path::new(string(&value, "git")?);
    require(fs::canonicalize(repository).private("SECURITY_CONTEXT_INVALID")? == repository
        && env::current_dir().private("SECURITY_CONTEXT_INVALID")? == repository
        && private::identity(repository)? == string(&value, "repository_id")?
        && fs::canonicalize(git).private("SECURITY_CONTEXT_INVALID")? == git
        && !git.starts_with(repository) && !git.starts_with(&root)
        && private::identity(git)? == string(&value, "git_id")?
        && private::executable_hash(git)? == string(&value, "git_sha")?, "SECURITY_CONTEXT_INVALID")?;
    deadline_from(&value)?;
    Ok(Context { root, value })
}
pub fn launch_worker() -> Result<()> {
    let context = authenticated()?;
    let v = &context.value;
    require(v["parent"].as_i64() == rustix::process::getppid().map(|p| i64::from(p.as_raw_pid()))
        && rustix::process::getpgid(None).private("SECURITY_GROUP_INVALID")? == rustix::process::getpid(),
        "SECURITY_GROUP_INVALID")?;
    let group = rustix::process::getpid().as_raw_pid();
    write_json(&context.root.join("group.json"), &json!({"run": v["run"], "group": group}))?;
    let executable = Path::new(string(v, "scanner")?);
    require(private::executable_hash(executable)? == string(v, "scanner_sha")?, "SECURITY_EXECUTABLE_INVALID")?;
    let mut command = Command::new(executable);
    private::controlled(&mut command, &context.root);
    command.current_dir(string(v, "repository")?).env("PATH", format!("{}:/usr/bin:/bin", private::text(&context.root)?))
        .env("NW_SECURITY_TOKEN", string(v, "token")?)
        .arg("git").arg(string(v, "repository")?).arg("--config").arg(context.root.join("scanner.toml"))
        .arg("--gitleaks-ignore-path").arg(&context.root)
        .args(["--ignore-gitleaks-allow", "--redact=100", "--no-banner", "--no-color", "--log-level=error", "--exit-code=2", "--report-format=json"])
        .arg("--report-path").arg(context.root.join("findings.json"))
        .arg(format!("--log-opts={LOG_OPTIONS} {}", string(v, "scope")?));
    let _error = command.exec();
    Err(ScanFailure("SECURITY_SCANNER_EXEC_FAILED"))
}
pub fn proxy_worker() -> Result<i32> {
    let context = authenticated()?;
    let v = &context.value;
    let group = read_json(&context.root.join("group.json"))?;
    require(group["run"] == v["run"] && group["group"].as_i64() == Some(i64::from(
        rustix::process::getpgid(None).private("SECURITY_GROUP_INVALID")?.as_raw_pid())), "SECURITY_GROUP_INVALID")?;
    let id = private::nonce()?;
    let proof = context.root.join("proof");
    // Every attempt leaves durable evidence before argv validation; rejected attempts
    // cannot disappear behind a later successful history child.
    let mut started = json!({"invocation":id, "run": v["run"], "context": private::hash(&private::read(&context.root.join("context.json"), private::RECORD_LIMIT)?),
        "group": group["group"], "kind": "rejected", "argv": []});
    write_json(&proof.join(format!("{id}.attempt")), &started)?;
    let mut args = Vec::new(); let mut bytes = 0_usize;
    for argument in env::args_os().skip(1) {
        require(args.len() < 128 && argument.len() <= 4096, "SECURITY_ARGV_OVERSIZE")?;
        bytes = bytes.checked_add(argument.len()).ok_or(ScanFailure("SECURITY_ARGV_OVERSIZE"))?;
        require(bytes <= 16 * 1024, "SECURITY_ARGV_OVERSIZE")?;
        args.push(argument.into_string().private("SECURITY_ARGV_INVALID")?);
    }
    let kind = classify(&args, string(v, "repository")?, string(v, "scope")?)?;
    started["kind"] = json!(kind); started["argv"] = json!(args);
    write_json(&proof.join(format!("{id}.start")), &started)?;
    let mut command = Command::new(string(v, "git")?);
    private::controlled(&mut command, &context.root);
    command.current_dir(string(v, "repository")?).args(&args);
    let result = process::run(&mut command, process::Options { budget: Duration::from_secs(if kind == "history" { 930 } else { 15 }),
        deadline: deadline_from(v)?, inherited_group: true, forward_stdout: true }, || context.monitor())?;
    let completed = result.completed();
    let no_remote = kind == "metadata" && completed && result.status == Some(128)
        && result.stderr.bytes == b"fatal: No remote configured to list refs from.\n";
    let accepted = completed
        && ((result.status == Some(0) && result.stderr.quiet()) || no_remote);
    write_json(&proof.join(format!("{id}.done")), &json!({"invocation":id, "start": private::hash(&serde_json::to_vec(&started).private("SECURITY_CONTEXT_INVALID")?),
        "completed":completed, "stderr_quiet":result.stderr.quiet(), "accepted": accepted, "status": result.status, "no_remote": no_remote}))?;
    require(accepted, "SECURITY_GIT_CHILD_FAILED")?;
    if no_remote {
        // Fixed private protocol text preserves pinned Gitleaks's no-remote case.
        std::io::stderr().write_all(b"No remote configured\n").private("SECURITY_PRIVATE_IO_FAILED")?;
        Ok(128)
    } else { Ok(0) }
}
pub fn proof_files(path: &Path) -> Result<BTreeMap<String, PathBuf>> {
    let mut result = BTreeMap::new(); let mut total = 0_u64;
    for entry in fs::read_dir(path).private("SECURITY_PROOF_INVALID")? {
        require(result.len() < private::MAX_RECORDS, "SECURITY_PROOF_OVERFLOW")?;
        let entry = entry.private("SECURITY_PROOF_INVALID")?;
        let metadata = fs::symlink_metadata(entry.path()).private("SECURITY_PROOF_INVALID")?;
        require(metadata.is_file() && !metadata.file_type().is_symlink() && metadata.len() <= private::RECORD_LIMIT,
            "SECURITY_PROOF_INVALID")?;
        total = total.checked_add(metadata.len()).ok_or(ScanFailure("SECURITY_PROOF_OVERFLOW"))?;
        require(total <= private::PROOF_LIMIT, "SECURITY_PROOF_OVERFLOW")?;
        let name = entry.file_name().into_string().private("SECURITY_PROOF_INVALID")?;
        require(result.insert(name, entry.path()).is_none(), "SECURITY_PROOF_INVALID")?;
    }
    Ok(result)
}
pub struct ProofCounts { pub history: usize, pub metadata: usize, pub blobs: usize }
pub fn verify_proofs(context: &Context, leader: i32) -> Result<ProofCounts> {
    let files = proof_files(&context.root.join("proof"))?;
    let group = read_json(&context.root.join("group.json"))?;
    require(group["run"] == context.value["run"] && group["group"].as_i64() == Some(i64::from(leader)), "SECURITY_PROOF_INVALID")?;
    let descriptor = private::read(&context.root.join("context.json"), private::RECORD_LIMIT)?;
    require(descriptor == serde_json::to_vec(&context.value).private("SECURITY_CONTEXT_INVALID")?,
        "SECURITY_PROOF_INVALID")?;
    let digest = private::hash(&descriptor);
    let mut counts = ProofCounts { history: 0, metadata: 0, blobs: 0 };
    let mut history_attempts = 0; let mut failed_history = false;
    require(!files.is_empty() && files.len().is_multiple_of(3), "SECURITY_PROOF_INCOMPLETE")?;
    for (name, path) in &files {
        let (id, suffix) = name.split_once('.').ok_or(ScanFailure("SECURITY_PROOF_INVALID"))?;
        require(id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit())
            && matches!(suffix, "attempt" | "start" | "done"), "SECURITY_PROOF_INVALID")?;
        require(["attempt", "start", "done"].iter().all(|suffix| files.contains_key(&format!("{id}.{suffix}"))),
            "SECURITY_PROOF_INCOMPLETE")?;
        if suffix != "attempt" { continue; }
        let attempt = read_json(path)?;
        let start_path = files.get(&format!("{id}.start")).ok_or(ScanFailure("SECURITY_PROOF_INCOMPLETE"))?;
        let done_path = files.get(&format!("{id}.done")).ok_or(ScanFailure("SECURITY_PROOF_INCOMPLETE"))?;
        let started = read_json(start_path)?; let done = read_json(done_path)?;
        require(attempt["invocation"] == id && started["invocation"] == id && done["invocation"] == id
            && attempt["kind"] == "rejected" && attempt["argv"] == json!([]) && attempt["run"] == context.value["run"]
            && attempt["context"] == digest && attempt["group"] == group["group"]
            && started["run"] == attempt["run"] && started["context"] == attempt["context"]
            && started["group"] == attempt["group"]
            && done["start"] == private::hash(&private::read(start_path, private::RECORD_LIMIT)?), "SECURITY_PROOF_INVALID")?;
        let args = started["argv"].as_array().ok_or(ScanFailure("SECURITY_PROOF_INVALID"))?.iter()
            .map(|v| v.as_str().map(str::to_owned).ok_or(ScanFailure("SECURITY_PROOF_INVALID")))
            .collect::<Result<Vec<_>>>()?;
        let kind = classify(&args, string(&context.value, "repository")?, string(&context.value, "scope")?)?;
        require(started["kind"] == kind, "SECURITY_PROOF_INVALID")?;
        let status = done["status"].as_i64().ok_or(ScanFailure("SECURITY_PROOF_INVALID"))?;
        let accepted = done["accepted"].as_bool().ok_or(ScanFailure("SECURITY_PROOF_INVALID"))?;
        let no_remote = done["no_remote"].as_bool().ok_or(ScanFailure("SECURITY_PROOF_INVALID"))?;
        let quiet = done["stderr_quiet"].as_bool().ok_or(ScanFailure("SECURITY_PROOF_INVALID"))?;
        require(done["completed"] == true && (0..=255).contains(&status), "SECURITY_PROOF_INVALID")?;
        if kind == "history" { history_attempts += 1; }
        if kind == "history" && !accepted && status != 0 && !no_remote {
            failed_history = true;
            continue;
        }
        require(accepted && ((status == 0 && !no_remote && quiet)
            || (kind == "metadata" && status == 128 && no_remote && !quiet)), "SECURITY_PROOF_INVALID")?;
        match kind { "history" => counts.history += 1, "metadata" => counts.metadata += 1, _ => counts.blobs += 1 }
    }
    require(history_attempts > 0 && history_attempts <= 256, "SECURITY_PROOF_INCOMPLETE")?;
    // Only a complete, identity-validated nonzero real history completion reaches
    // this category; malformed/incomplete/other failed records return above.
    require(!failed_history, "SECURITY_HISTORY_GIT_FAILED")?;
    require(counts.history > 0, "SECURITY_PROOF_INCOMPLETE")?;
    Ok(counts)
}
