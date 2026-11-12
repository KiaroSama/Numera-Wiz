//! Finite parent-owned reads. These records never count as scanner history proof.
use crate::{
    PrivateResult, Result, ScanFailure,
    classifier::{self, DOCUMENTS},
    private, process,
    protocol::{self, Context},
    require,
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::ErrorKind,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};

const UNSAFE: &str = "SECURITY_CLASSIFIER_REPOSITORY_UNSAFE";
const FORBIDDEN_CONFIG: &str =
    r"^(extensions\.|remote\..*\.(promisor|partialclonefilter)$|core\.alternaterefscommand$)";
#[derive(Clone, Copy, Default)]
pub struct Stats {
    pub commands: usize,
    pub resolutions: usize,
    pub blob_reads: usize,
    pub cache_hits: usize,
}
struct Transaction {
    id: String,
    start_hash: String,
    children: Vec<Value>,
}
struct CachedBlob {
    path: usize,
    proof: String,
}
pub struct Session<'a> {
    context: &'a Context,
    deadline: Instant,
    git: PathBuf,
    repository: PathBuf,
    descriptor: String,
    filesystem: String,
    ledger: BTreeMap<String, String>,
    transactions: usize,
    report: Option<String>,
    commits: BTreeMap<String, (bool, String)>,
    blobs: BTreeMap<String, CachedBlob>,
    resolved: BTreeSet<(String, usize)>,
    stats: Stats,
}
fn absent(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        _ => Err(ScanFailure(UNSAFE)),
    }
}
fn directory(path: &Path, required: bool) -> Result<Value> {
    let meta = match fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(error) if error.kind() == ErrorKind::NotFound && !required => {
            return Ok(json!({"absent":true}));
        }
        Err(_) => return Err(ScanFailure(UNSAFE)),
    };
    require(
        meta.is_dir()
            && !meta.file_type().is_symlink()
            && fs::canonicalize(path).private(UNSAFE)? == path,
        UNSAFE,
    )?;
    Ok(json!({"id":format!("{}:{}",meta.dev(),meta.ino())}))
}
fn filesystem(repository: &Path) -> Result<Value> {
    let mut manifest = BTreeMap::new();
    for (name, required) in [
        ("", true),
        ("objects", true),
        ("objects/info", false),
        ("objects/pack", false),
        ("info", false),
        ("refs", false),
    ] {
        manifest.insert(
            name.to_owned(),
            directory(&repository.join(name), required)?,
        );
    }
    for name in [
        "commondir",
        "shallow",
        "info/grafts",
        "objects/info/alternates",
        "objects/info/http-alternates",
        "refs/replace",
    ] {
        absent(&repository.join(name))?;
    }
    let config = repository.join("config");
    let meta = fs::symlink_metadata(&config).private(UNSAFE)?;
    require(
        meta.is_file() && !meta.file_type().is_symlink() && meta.len() <= private::MIB,
        UNSAFE,
    )?;
    let bytes = private::read(&config, private::MIB)?;
    manifest.insert("config".into(),json!({"id":private::identity(&config)?,"length":bytes.len(),"sha256":private::hash(&bytes)}));
    let pack = repository.join("objects/pack");
    let mut entries = BTreeMap::new();
    let mut name_bytes = 0usize;
    if manifest["objects/pack"]["absent"] != true {
        for entry in fs::read_dir(&pack).private(UNSAFE)? {
            require(entries.len() < 4096, UNSAFE)?;
            let entry = entry.private(UNSAFE)?;
            let name = entry.file_name().into_string().private(UNSAFE)?;
            name_bytes = name_bytes
                .checked_add(name.len())
                .ok_or(ScanFailure(UNSAFE))?;
            require(
                !name.is_empty()
                    && name.len() <= 255
                    && name_bytes as u64 <= private::MIB
                    && !name.ends_with(".promisor"),
                UNSAFE,
            )?;
            let meta = fs::symlink_metadata(entry.path()).private(UNSAFE)?;
            require(meta.is_file() && !meta.file_type().is_symlink(), UNSAFE)?;
            entries.insert(
                name,
                json!({"id":format!("{}:{}",meta.dev(),meta.ino()),"length":meta.len()}),
            );
        }
    }
    manifest.insert(
        "pack_entries".into(),
        json!({"count":entries.len(),"name_bytes":name_bytes,
        "sha256":private::hash(&serde_json::to_vec(&entries).private(UNSAFE)?)}),
    );
    Ok(json!(manifest))
}
fn canonical_oid(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn object_type(bytes: &[u8]) -> Result<&'static str> {
    match bytes {
        b"commit\n" => Ok("commit"),
        b"tree\n" => Ok("tree"),
        b"blob\n" => Ok("blob"),
        b"tag\n" => Ok("tag"),
        _ => Err(ScanFailure("SECURITY_CLASSIFIER_TYPE_INVALID")),
    }
}
pub fn tree_entry(bytes: &[u8], path: usize) -> Result<Option<String>> {
    let fail = "SECURITY_CLASSIFIER_TREE_INVALID";
    let document = DOCUMENTS.get(path).ok_or(ScanFailure(fail))?;
    if bytes.is_empty() {
        return Ok(None);
    }
    let text = std::str::from_utf8(bytes).private(fail)?;
    let record = text.strip_suffix('\0').ok_or(ScanFailure(fail))?;
    require(!record.contains('\0'), fail)?;
    let (metadata, name) = record.split_once('\t').ok_or(ScanFailure(fail))?;
    require(name == document.path, fail)?;
    let mut parts = metadata.split(' ');
    let mode = parts.next().ok_or(ScanFailure(fail))?;
    let kind = parts.next().ok_or(ScanFailure(fail))?;
    let oid = parts.next().ok_or(ScanFailure(fail))?;
    require(parts.next().is_none() && canonical_oid(oid), fail)?;
    match (mode, kind) {
        ("100644" | "100755", "blob") => Ok(Some(oid.to_owned())),
        ("040000", "tree") | ("120000", "blob") | ("160000", "commit") => Ok(None),
        _ => Err(ScanFailure(fail)),
    }
}
fn size(bytes: &[u8]) -> Result<u64> {
    let fail = "SECURITY_CLASSIFIER_SIZE_INVALID";
    let text = std::str::from_utf8(bytes)
        .private(fail)?
        .strip_suffix('\n')
        .ok_or(ScanFailure(fail))?;
    require(
        !text.is_empty()
            && text.len() <= 20
            && text.bytes().all(|b| b.is_ascii_digit())
            && (text == "0" || !text.starts_with('0')),
        fail,
    )?;
    text.parse().private(fail)
}
impl<'a> Session<'a> {
    pub fn admit(context: &'a Context, deadline: Instant) -> Result<Self> {
        classifier::active(deadline, process::cancelled())?;
        let repository = PathBuf::from(protocol::string(&context.value, "repository")?);
        let git = PathBuf::from(protocol::string(&context.value, "git")?);
        private::directory(&context.root.join("review-proof"))?;
        let descriptor =
            private::hash(&serde_json::to_vec(&context.value).private("SECURITY_CONTEXT_INVALID")?);
        let mut session = Self {
            context,
            deadline,
            git,
            repository,
            descriptor,
            filesystem: String::new(),
            ledger: BTreeMap::new(),
            transactions: 0,
            report: None,
            commits: BTreeMap::new(),
            blobs: BTreeMap::new(),
            resolved: BTreeSet::new(),
            stats: Stats::default(),
        };
        session.identity()?;
        let transaction = session.begin("filesystem", json!({}))?;
        let manifest = filesystem(&session.repository)?;
        session.filesystem = private::hash(&serde_json::to_vec(&manifest).private(UNSAFE)?);
        session.finish(
            transaction,
            json!({"manifest":manifest,"sha256":session.filesystem}),
        )?;
        let mut transaction = session.begin("config", json!({}))?;
        let output = session.child(&mut transaction, "config", None, None)?;
        require(
            output.0 == 1 && output.1.is_empty(),
            "SECURITY_CLASSIFIER_CONFIG_UNSAFE",
        )?;
        session.finish(transaction, json!({"absent":true}))?;
        let mut transaction = session.begin("replace_refs", json!({}))?;
        let output = session.child(&mut transaction, "refs", None, None)?;
        require(
            output.0 == 0 && output.1.is_empty(),
            "SECURITY_CLASSIFIER_REFS_UNSAFE",
        )?;
        session.finish(transaction, json!({"absent":true}))?;
        let mut transaction = session.begin("format", json!({}))?;
        let output = session.child(&mut transaction, "format", None, None)?;
        require(
            output.0 == 0 && output.1 == b"sha1\n",
            "SECURITY_CLASSIFIER_FORMAT_UNSUPPORTED",
        )?;
        session.finish(transaction, json!({"format":"sha1"}))?;
        session.verify()?;
        Ok(session)
    }
    pub fn stats(&self) -> Stats {
        self.stats
    }
    fn identity(&self) -> Result<()> {
        classifier::active(self.deadline, process::cancelled())?;
        require(
            fs::canonicalize(&self.repository).private(UNSAFE)? == self.repository
                && private::identity(&self.repository)?
                    == protocol::string(&self.context.value, "repository_id")?
                && fs::canonicalize(&self.git).private("SECURITY_EXECUTABLE_INVALID")? == self.git
                && private::identity(&self.git)?
                    == protocol::string(&self.context.value, "git_id")?
                && private::executable_hash(&self.git)?
                    == protocol::string(&self.context.value, "git_sha")?
                && private::identity(&self.context.root)?
                    == protocol::string(&self.context.value, "scratch_id")?,
            "SECURITY_CLASSIFIER_IDENTITY_INVALID",
        )?;
        let descriptor = private::read(
            &self.context.root.join("context.json"),
            private::RECORD_LIMIT,
        )?;
        require(
            private::hash(&descriptor) == self.descriptor,
            "SECURITY_CLASSIFIER_IDENTITY_INVALID",
        )
    }
    pub fn bind_report(&mut self, digest: &str) -> Result<()> {
        require(
            self.report.is_none()
                && digest.len() == 64
                && digest.bytes().all(|b| b.is_ascii_hexdigit()),
            "SECURITY_CLASSIFIER_REPORT_BINDING",
        )?;
        self.report = Some(digest.to_owned());
        Ok(())
    }
    fn record(&mut self, id: &str, suffix: &str, value: Value) -> Result<String> {
        let bytes = serde_json::to_vec(&value).private("SECURITY_PROOF_INVALID")?;
        require(
            bytes.len() as u64 <= private::RECORD_LIMIT,
            "SECURITY_RECORD_OVERSIZE",
        )?;
        protocol::check_proof_budget(&self.context.root, 1, bytes.len() as u64)?;
        let name = format!("{id}.{suffix}");
        require(!self.ledger.contains_key(&name), "SECURITY_PROOF_INVALID")?;
        private::write(&self.context.root.join("review-proof").join(&name), &bytes)?;
        let hash = private::hash(&bytes);
        self.ledger.insert(name, hash.clone());
        Ok(hash)
    }
    fn begin(&mut self, kind: &str, key: Value) -> Result<Transaction> {
        classifier::active(self.deadline, process::cancelled())?;
        require(
            self.transactions < 260,
            "SECURITY_CLASSIFIER_RESOLUTION_LIMIT",
        )?;
        self.transactions += 1;
        let id = private::nonce()?;
        let base = json!({"invocation":id,"run":self.context.value["run"],"context":self.descriptor,
            "kind":kind,"key":key,"report":self.report,"format":"sha1"});
        let attempt = self.record(&id, "attempt", base.clone())?;
        let mut start = base;
        start["attempt"] = json!(attempt);
        let start_hash = self.record(&id, "start", start)?;
        Ok(Transaction {
            id,
            start_hash,
            children: Vec::new(),
        })
    }
    fn finish(&mut self, transaction: Transaction, result: Value) -> Result<String> {
        require(transaction.children.len() <= 5, "SECURITY_PROOF_INVALID")?;
        self.record(&transaction.id,"done",json!({"invocation":transaction.id,"run":self.context.value["run"],
            "context":self.descriptor,"start":transaction.start_hash,"children":transaction.children,"result":result}))?;
        self.verify_transaction(&transaction.id)?;
        Ok(transaction.id)
    }
    fn verify_transaction(&self, id: &str) -> Result<()> {
        let mut values = Vec::with_capacity(3);
        for suffix in ["attempt", "start", "done"] {
            let name = format!("{id}.{suffix}");
            let path = self.context.root.join("review-proof").join(&name);
            let bytes = private::read(&path, private::RECORD_LIMIT)?;
            require(
                self.ledger.get(&name) == Some(&private::hash(&bytes)),
                "SECURITY_CLASSIFIER_PROOF_INVALID",
            )?;
            let value = protocol::read_json(&path)?;
            require(
                value["invocation"] == id
                    && value["run"] == self.context.value["run"]
                    && value["context"] == self.descriptor,
                "SECURITY_CLASSIFIER_PROOF_INVALID",
            )?;
            values.push(value);
        }
        require(
            values[1]["attempt"] == self.ledger[&format!("{id}.attempt")]
                && values[2]["start"] == self.ledger[&format!("{id}.start")],
            "SECURITY_CLASSIFIER_PROOF_INVALID",
        )?;
        let children = values[2]["children"]
            .as_array()
            .ok_or(ScanFailure("SECURITY_CLASSIFIER_PROOF_INVALID"))?;
        require(children.len() <= 5, "SECURITY_CLASSIFIER_PROOF_INVALID")?;
        for (ordinal, child) in children.iter().enumerate() {
            let kind = protocol::string(child, "kind")?;
            let item = if child["item"].is_null() {
                None
            } else {
                Some(protocol::string(child, "item")?)
            };
            let path = if child["path"].is_null() {
                None
            } else {
                Some(
                    usize::try_from(
                        child["path"]
                            .as_u64()
                            .ok_or(ScanFailure("SECURITY_CLASSIFIER_PROOF_INVALID"))?,
                    )
                    .private("SECURITY_CLASSIFIER_PROOF_INVALID")?,
                )
            };
            let args = self.arguments(kind, item, path)?;
            let status = child["status"]
                .as_i64()
                .ok_or(ScanFailure("SECURITY_CLASSIFIER_PROOF_INVALID"))?;
            require(
                child["ordinal"].as_u64() == Some(ordinal as u64)
                    && child["argv_sha256"]
                        == private::hash(
                            &serde_json::to_vec(&args)
                                .private("SECURITY_CLASSIFIER_PROOF_INVALID")?,
                        )
                    && (status == 0 || (kind == "config" && status == 1))
                    && child["completed"] == true
                    && child["barrier"] == true
                    && child["reaped"].as_u64().is_some_and(|n| n > 0)
                    && child["adopted"].as_u64() == Some(0)
                    && child["leader"]
                        .as_i64()
                        .is_some_and(|n| (1..=i64::from(i32::MAX)).contains(&n))
                    && child["stdout_length"]
                        .as_u64()
                        .is_some_and(|n| n <= private::MIB)
                    && child["stderr_length"].as_u64() == Some(0)
                    && child["stdout_overflow"] == false
                    && child["stdout_failed"] == false
                    && child["stderr_overflow"] == false
                    && child["stderr_failed"] == false
                    && child["stdout_nonwhite"].as_bool().is_some()
                    && child["stderr_nonwhite"] == false
                    && child["cleanup_ms"].as_u64().is_some(),
                "SECURITY_CLASSIFIER_PROOF_INVALID",
            )?;
        }
        Ok(())
    }
    pub fn verify(&self) -> Result<()> {
        self.identity()?;
        protocol::check_proof_budget(&self.context.root, 0, 0)?;
        let files = protocol::proof_files(&self.context.root.join("review-proof"))?;
        require(
            files.len() == self.ledger.len() && files.len() == self.transactions * 3,
            "SECURITY_CLASSIFIER_PROOF_INVALID",
        )?;
        for name in files.keys() {
            require(
                self.ledger.contains_key(name),
                "SECURITY_CLASSIFIER_PROOF_INVALID",
            )?;
            if let Some(id) = name.strip_suffix(".attempt") {
                self.verify_transaction(id)?;
            }
        }
        require(
            private::hash(&serde_json::to_vec(&filesystem(&self.repository)?).private(UNSAFE)?)
                == self.filesystem,
            "SECURITY_CLASSIFIER_IDENTITY_INVALID",
        )
    }
    fn arguments(
        &self,
        kind: &str,
        item: Option<&str>,
        path: Option<usize>,
    ) -> Result<Vec<String>> {
        let root = private::text(&self.repository)?;
        let mut args = ["--no-replace-objects", "--literal-pathspecs", "-C", root]
            .map(str::to_owned)
            .to_vec();
        match kind {
            "config" => args.extend(
                [
                    "config",
                    "--null",
                    "--includes",
                    "--get-regexp",
                    FORBIDDEN_CONFIG,
                ]
                .map(str::to_owned),
            ),
            "refs" => args.extend(
                ["for-each-ref", "--format=%(refname)", "refs/replace/"].map(str::to_owned),
            ),
            "format" => {
                args.extend(["rev-parse", "--show-object-format=storage"].map(str::to_owned))
            }
            "commit" | "blob_type" | "size" | "payload" | "tree" => {
                let item = item.ok_or(ScanFailure("SECURITY_CLASSIFIER_ARGV_INVALID"))?;
                require(canonical_oid(item), "SECURITY_CLASSIFIER_ARGV_INVALID")?;
                if kind == "tree" {
                    let document = DOCUMENTS
                        .get(path.ok_or(ScanFailure("SECURITY_CLASSIFIER_ARGV_INVALID"))?)
                        .ok_or(ScanFailure("SECURITY_CLASSIFIER_ARGV_INVALID"))?;
                    args.extend(
                        ["ls-tree", "-z", "--full-tree", item, "--", document.path]
                            .map(str::to_owned),
                    );
                } else {
                    args.extend(
                        [
                            "cat-file",
                            match kind {
                                "size" => "-s",
                                "payload" => "blob",
                                _ => "-t",
                            },
                            item,
                        ]
                        .map(str::to_owned),
                    );
                }
            }
            _ => return Err(ScanFailure("SECURITY_CLASSIFIER_ARGV_INVALID")),
        }
        require(
            args.len() <= 16
                && args.iter().all(|arg| arg.len() <= 4096)
                && args.iter().map(String::len).sum::<usize>() <= 16 * 1024,
            "SECURITY_ARGV_OVERSIZE",
        )?;
        Ok(args)
    }
    fn child(
        &mut self,
        transaction: &mut Transaction,
        kind: &str,
        item: Option<&str>,
        path: Option<usize>,
    ) -> Result<(i32, Vec<u8>)> {
        self.identity()?;
        require(
            transaction.children.len() < 5 && self.stats.commands < 1283,
            "SECURITY_CLASSIFIER_COMMAND_LIMIT",
        )?;
        // Reserve the maximum eventual completion record before starting work.
        protocol::check_proof_budget(&self.context.root, 1, private::RECORD_LIMIT)?;
        let args = self.arguments(kind, item, path)?;
        let mut command = Command::new(&self.git);
        private::controlled(&mut command, &self.context.root);
        command.current_dir(&self.repository).args(&args);
        let actual = command
            .get_args()
            .map(|arg| {
                arg.to_str()
                    .map(str::to_owned)
                    .ok_or(ScanFailure("SECURITY_CLASSIFIER_ARGV_INVALID"))
            })
            .collect::<Result<Vec<_>>>()?;
        require(
            actual == self.arguments(kind, item, path)?,
            "SECURITY_CLASSIFIER_ARGV_INVALID",
        )?;
        let argv_hash = private::hash(
            &serde_json::to_vec(&actual).private("SECURITY_CLASSIFIER_ARGV_INVALID")?,
        );
        self.stats.commands += 1;
        let outcome = process::run(
            &mut command,
            process::Options::command(Duration::from_secs(15), self.deadline),
            || self.context.monitor(),
        )?;
        if let Some(failure) = outcome.failure {
            return Err(failure);
        }
        require(outcome.completed(), "SECURITY_CLASSIFIER_CHILD_INCOMPLETE")?;
        let failed_stage = match kind {
            "commit" => "SECURITY_CLASSIFIER_COMMIT_READ_FAILED",
            "tree" => "SECURITY_CLASSIFIER_TREE_READ_FAILED",
            "blob_type" => "SECURITY_CLASSIFIER_BLOB_TYPE_READ_FAILED",
            "size" => "SECURITY_CLASSIFIER_BLOB_SIZE_READ_FAILED",
            "payload" => "SECURITY_CLASSIFIER_BLOB_READ_FAILED",
            "config" => "SECURITY_CLASSIFIER_CONFIG_READ_FAILED",
            "refs" => "SECURITY_CLASSIFIER_REFS_READ_FAILED",
            _ => "SECURITY_CLASSIFIER_FORMAT_READ_FAILED",
        };
        require(outcome.stderr.bytes.is_empty(), failed_stage)?;
        let status = outcome
            .status
            .ok_or(ScanFailure("SECURITY_CLASSIFIER_CHILD_INCOMPLETE"))?;
        require(
            status == 0 || (kind == "config" && status == 1),
            failed_stage,
        )?;
        outcome.barrier.recheck()?;
        let parsed = match kind {
            "commit" | "blob_type" => json!({"type":object_type(&outcome.stdout.bytes)?}),
            "tree" => {
                json!({"regular_blob":tree_entry(&outcome.stdout.bytes,path.ok_or(ScanFailure("SECURITY_CLASSIFIER_ARGV_INVALID"))?)?})
            }
            "size" => json!({"size":size(&outcome.stdout.bytes)?}),
            "config" | "refs" => json!({"empty":outcome.stdout.bytes.is_empty()}),
            "format" => json!({"sha1":outcome.stdout.bytes == b"sha1\n"}),
            "payload" => json!({"bytes":outcome.stdout.bytes.len()}),
            _ => return Err(ScanFailure("SECURITY_CLASSIFIER_ARGV_INVALID")),
        };
        transaction.children.push(json!({"ordinal":transaction.children.len(),"kind":kind,"item":item,"path":path,
            "argv_sha256":argv_hash,"status":status,"completed":true,"reaped":outcome.reaped,"adopted":outcome.adopted,
            "leader":outcome.leader,"stdout_length":outcome.stdout.bytes.len(),"stdout_sha256":private::hash(&outcome.stdout.bytes),
            "stdout_overflow":outcome.stdout.overflow,"stdout_failed":outcome.stdout.failed,
            "stderr_length":outcome.stderr.bytes.len(),"stderr_overflow":outcome.stderr.overflow,"stderr_failed":outcome.stderr.failed,
            "stdout_nonwhite":outcome.stdout.nonwhite,"stderr_nonwhite":outcome.stderr.nonwhite,"barrier":true,"parsed":parsed,
            "cleanup_ms":u64::try_from(outcome.cleanup_elapsed.as_millis()).private("SECURITY_CLASSIFIER_CHILD_INCOMPLETE")?}));
        Ok((status, outcome.stdout.bytes))
    }
    pub fn resolve(&mut self, commit: &str, path: usize) -> Result<bool> {
        require(self.report.is_some(), "SECURITY_CLASSIFIER_REPORT_BINDING")?;
        require(
            canonical_oid(commit) && path < DOCUMENTS.len(),
            "SECURITY_CLASSIFIER_ARGV_INVALID",
        )?;
        require(
            self.resolved.len() < 256,
            "SECURITY_CLASSIFIER_RESOLUTION_LIMIT",
        )?;
        require(
            self.resolved.insert((commit.to_owned(), path)),
            "SECURITY_CLASSIFIER_DUPLICATE_RESOLUTION",
        )?;
        self.stats.resolutions += 1;
        let mut transaction = self.begin("resolution", json!({"commit":commit,"path":path}))?;
        let (is_commit, commit_ref) = if let Some(cached) = self.commits.get(commit) {
            cached.clone()
        } else {
            let output = self.child(&mut transaction, "commit", Some(commit), None)?;
            (object_type(&output.1)? == "commit", transaction.id.clone())
        };
        let mut cache_insert = None;
        let mut cache_reference = None;
        let matched = if !is_commit {
            false
        } else {
            let output = self.child(&mut transaction, "tree", Some(commit), Some(path))?;
            if let Some(oid) = tree_entry(&output.1, path)? {
                if let Some(cached) = self.blobs.get(&oid) {
                    self.stats.cache_hits += 1;
                    cache_reference = Some(cached.proof.clone());
                    DOCUMENTS[cached.path].sha256 == DOCUMENTS[path].sha256
                        && DOCUMENTS[cached.path].length == DOCUMENTS[path].length
                } else {
                    let output = self.child(&mut transaction, "blob_type", Some(&oid), None)?;
                    if object_type(&output.1)? != "blob" {
                        false
                    } else {
                        let output = self.child(&mut transaction, "size", Some(&oid), None)?;
                        if size(&output.1)? != DOCUMENTS[path].length {
                            false
                        } else {
                            let output =
                                self.child(&mut transaction, "payload", Some(&oid), None)?;
                            self.stats.blob_reads += 1;
                            let matched = output.1.len() as u64 == DOCUMENTS[path].length
                                && private::hash(&output.1) == DOCUMENTS[path].sha256;
                            // This is the only payload; it is discarded before another child.
                            drop(output);
                            if matched {
                                classifier::positive_cache_room(self.blobs.len())?;
                                cache_insert = Some(oid);
                            }
                            matched
                        }
                    }
                }
            } else {
                false
            }
        };
        let proof = self.finish(transaction,json!({"matched":matched,"commit_type":is_commit,"commit_proof":commit_ref,
            "cached_blob_proof":cache_reference,"approved_sha256":if matched {Some(DOCUMENTS[path].sha256)} else {None},
            "approved_length":if matched {Some(DOCUMENTS[path].length)} else {None}}))?;
        self.commits
            .entry(commit.to_owned())
            .or_insert((is_commit, proof.clone()));
        if let Some(oid) = cache_insert {
            self.blobs.insert(oid, CachedBlob { path, proof });
        }
        Ok(matched)
    }
}
