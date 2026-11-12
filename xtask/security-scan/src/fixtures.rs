//! Private synthetic repositories and independently classified negative oracles.
use nw_security_scan::bootstrap::{Deadline, Stage};
use nw_security_scan::{
    PrivateResult, Result, ScanFailure,
    assets::{self, Release},
    diagnostic,
    private::{self, Scratch},
    process::{self, Options},
    protocol::{self, Scope},
    require,
    scanner::{self, ScanRequest, VerifiedScan},
};
use std::{
    env, fs,
    io::Write,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

#[path = "fixture_bootstrap.rs"]
mod bootstrap_cases;
#[path = "fixture_children.rs"]
mod children;
#[path = "fixture_classifier.rs"]
mod classifier_cases;
#[path = "fixture_policy.rs"]
mod policy;
pub use children::dispatch as child_dispatch;
const EXPECTED_SUMMARY: &str = concat!(
    "## Gitleaks\n\nScanner: **8.30.1**\n\n",
    "Scope: **All fetched branches and tags, including full history and merge diffs**\n\n",
    "Rules: Pinned built-in Gitleaks rules with standard exclusions. ",
    "Full history describes revision coverage, not universal byte or credential coverage.\n\n",
    "No matching secrets were found in the scanned Git history.\n",
);
struct Fixture {
    root: PathBuf,
    git: PathBuf,
    deadline: Deadline,
    release: Option<Release>,
    stage: Stage,
}
#[derive(Debug)]
enum FixtureGitOperation {
    Init,
    Add,
    Commit,
    Revision,
    Config,
    UpdateRef,
    PackRefs,
    ForEachRef,
    Other,
}
impl FixtureGitOperation {
    fn from_args(args: &[&str]) -> Self {
        let operation = if args.first() == Some(&"--no-replace-objects") {
            args.get(1)
        } else {
            args.first()
        };
        match operation.copied() {
            Some("init") => Self::Init,
            Some("add") => Self::Add,
            Some("commit") => Self::Commit,
            Some("rev-parse") => Self::Revision,
            Some("config") => Self::Config,
            Some("update-ref") => Self::UpdateRef,
            Some("pack-refs") => Self::PackRefs,
            Some("for-each-ref") => Self::ForEachRef,
            _ => Self::Other,
        }
    }
}
#[derive(Debug)]
enum FixtureGitFailure {
    Deadline,
    Timeout,
    Cancelled,
    ChildStatus,
    Other,
}
pub fn qualify() -> Result<()> {
    scanner::guard()?;
    process::initialize()?;
    process::barrier()?;
    let stage = Stage::admit()?;
    let deadline = stage.deadline.cap(Duration::from_secs(300))?;
    diagnostic("INFO", "SECURITY_ACTUAL_BOOTSTRAP_ENV_CONSUMED_PASS");
    scanner::provenance(deadline)?;
    let git = scanner::real_git()?;
    let scratch = Scratch::create(&scanner::temporary()?)?;
    let mut fixture = Fixture {
        root: scratch.path().to_owned(),
        git,
        deadline,
        release: None,
        stage,
    };
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| fixture.all()))
        .unwrap_or(Err(ScanFailure("SECURITY_QUALIFICATION_PANIC")));
    match process::barrier() {
        Ok(barrier) => {
            scratch.remove(&barrier, deadline)?;
            result
        }
        Err(_) => Err(ScanFailure("SECURITY_CLEANUP_UNVERIFIED")),
    }
}
impl Fixture {
    fn all(&mut self) -> Result<()> {
        self.real_clean()?;
        policy::qualify(&self.root, self.deadline)?;
        self.seed_suppression_attributes()?;
        self.merge_scope()?;
        self.all_refs_scope()?;
        self.real_git_failure()?;
        self.untrusted_build()?;
        self.process_cases()?;
        self.known_public_data_policy()?;
        bootstrap_cases::cases(self)?;
        bootstrap_cases::real_consumer(self)
    }
    fn git_result(&self, repository: &Path, args: &[&str]) -> Result<process::Outcome> {
        let mut command = Command::new(&self.git);
        private::controlled(&mut command, &self.root);
        command
            .current_dir(repository)
            .env("GIT_TEMPLATE_DIR", self.root.join("empty-template"))
            .env("GIT_AUTHOR_NAME", "Security qualification")
            .env("GIT_AUTHOR_EMAIL", "qualification@example.invalid")
            .env("GIT_COMMITTER_NAME", "Security qualification")
            .env("GIT_COMMITTER_EMAIL", "qualification@example.invalid")
            .args(["--no-pager", "-c", "commit.gpgSign=false"])
            .args(args);
        let result = process::run(
            &mut command,
            Options::command(Duration::from_secs(15), self.deadline),
            || Ok(()),
        );
        if let Err(error) = &result {
            self.git_failure(args, None, *error);
        }
        result
    }
    fn git_failure(&self, args: &[&str], outcome: Option<&process::Outcome>, error: ScanFailure) {
        let class = match error.0 {
            "SECURITY_DEADLINE" => FixtureGitFailure::Deadline,
            "SECURITY_CHILD_TIMEOUT" => FixtureGitFailure::Timeout,
            "SECURITY_CANCELLED" => FixtureGitFailure::Cancelled,
            "SECURITY_CHILD_FAILED" => FixtureGitFailure::ChildStatus,
            _ => FixtureGitFailure::Other,
        };
        // Only finite trusted enums and booleans. Diagnostic I/O cannot replace
        // the original failure; captures and actual arguments stay private.
        let _ = writeln!(
            std::io::stderr(),
            "{} [ERROR] SECURITY_FIXTURE_GIT_FAILURE op={:?} class={class:?} outcome_available={} completed={} completed_nonzero={} failure_present={} deadline_elapsed={} cancellation={} capture_failed={} capture_overflow={}",
            protocol::unix_millis().unwrap_or(0),
            FixtureGitOperation::from_args(args),
            outcome.is_some(),
            outcome.is_some_and(process::Outcome::completed),
            outcome.is_some_and(process::Outcome::completed_nonzero),
            outcome.is_some_and(|value| value.failure.is_some()),
            self.deadline.check().is_err(),
            process::cancelled(),
            outcome.is_some_and(|value| value.stdout.failed || value.stderr.failed),
            outcome.is_some_and(|value| value.stdout.overflow || value.stderr.overflow),
        );
    }
    fn git_success(&self, repository: &Path, args: &[&str]) -> Result<process::Outcome> {
        let output = self.git_result(repository, args)?;
        if let Err(error) = output.successful() {
            self.git_failure(args, Some(&output), error);
            return Err(error);
        }
        Ok(output)
    }
    fn git(&self, repository: &Path, args: &[&str]) -> Result<String> {
        let output = self.git_success(repository, args)?;
        String::from_utf8(output.stdout.bytes).private("SECURITY_FIXTURE_GIT_ENCODING")
    }
    fn revision(&self, repository: &Path, revision: &str) -> Result<String> {
        let result = self.git(repository, &["rev-parse", "--verify", revision])?;
        let result = result.trim().to_owned();
        require(protocol::revision(&result), "SECURITY_FIXTURE_REVISION")?;
        Ok(result)
    }
    fn repository(&self, name: &str) -> Result<PathBuf> {
        let path = self.root.join(name);
        private::directory(&path)?;
        self.git(&path, &["init", "--quiet", "--initial-branch=main"])?;
        fs::write(
            path.join("README.txt"),
            b"A clean synthetic repository for security qualification.\n",
        )
        .private("SECURITY_FIXTURE_IO")?;
        self.git(&path, &["add", "--", "README.txt"])?;
        self.git(
            &path,
            &["commit", "--quiet", "--no-gpg-sign", "-m", "Clean fixture"],
        )?;
        Ok(path)
    }
    fn scan(&self, repository: &Path, scope: &Scope, summary: &Path) -> Result<VerifiedScan> {
        let result = scanner::scan(&ScanRequest {
            repository,
            summary: Some(summary),
            scope,
            release: self
                .release
                .as_ref()
                .ok_or(ScanFailure("SECURITY_FIXTURE_ASSETS"))?,
            deadline: self.deadline,
        })?;
        eprintln!(
            "{} [INFO] SECURITY_SYNTHETIC_PROTOCOL history={} metadata={} blobs={}",
            protocol::unix_millis()?,
            result.successful_history_invocations,
            result.metadata_invocations,
            result.blob_invocations
        );
        Ok(result)
    }
    fn real_clean(&mut self) -> Result<()> {
        private::directory(&self.root.join("empty-template"))?;
        let repository = self.repository("repository")?;
        self.git(&repository, &["cat-file", "-e", "HEAD^{commit}"])?;
        self.git(
            &repository,
            &[
                "diff",
                "--quiet",
                "--no-ext-diff",
                "--no-textconv",
                "--ignore-submodules=all",
                "HEAD",
                "--",
            ],
        )?;
        diagnostic("INFO", "SECURITY_REAL_CLEAN_SETUP_PASS");
        let version_output = self.git(&repository, &["--version"])?;
        let version = version_output
            .trim()
            .strip_prefix("git version ")
            .ok_or(ScanFailure("SECURITY_GIT_VERSION_INVALID"))?;
        require(
            !version.is_empty()
                && version.len() <= 64
                && version.bytes().all(|b| b.is_ascii_digit() || b == b'.'),
            "SECURITY_GIT_VERSION_INVALID",
        )?;
        eprintln!(
            "{} [INFO] SECURITY_TRUSTED_GIT version={version} sha256={}",
            protocol::unix_millis()?,
            private::executable_hash_bounded(&self.git, self.deadline)?
        );
        let assets = self.root.join("assets");
        private::directory(&assets)?;
        self.release = Some(Release::prepare(&assets, self.deadline)?);
        let summary = self.root.join("summary.md");
        let result = self.scan(&repository, &Scope::All, &summary)?;
        require(
            result.scanner_version == "8.30.1"
                && result.finding_count == 0
                && result.reviewed_public_data_count == 0
                && result.unreviewed_count == 0
                && result.disposition == nw_security_scan::classifier::Disposition::Clean
                && result.successful_history_invocations > 0,
            "SECURITY_REAL_CLEAN_CONTRACT_FAILED",
        )?;
        require(
            private::read_bounded(&summary, private::MIB, self.deadline)? == EXPECTED_SUMMARY.as_bytes(),
            "SECURITY_REAL_CLEAN_SUMMARY_FAILED",
        )?;
        // Preserve the original no-remote clean fixture; no fake origin hides metadata behavior.
        require(
            result.metadata_invocations > 0,
            "SECURITY_METADATA_NOT_OBSERVED",
        )?;
        diagnostic("INFO", "SECURITY_REAL_CLEAN_PASS");
        Ok(())
    }
    fn seed_suppression_attributes(&self) -> Result<()> {
        let repository = self.repository("seeded")?;
        let secret = synthetic_secret()?;
        let canary = format!("private-message-{}", private::nonce()?);
        let source = format!("token = \"{secret}\" # gitleaks:allow\n");
        fs::write(repository.join("credential.txt"), &source).private("SECURITY_FIXTURE_IO")?;
        fs::write(
            repository.join(".gitleaks.toml"),
            "[[allowlists]]\nregexes = ['.*']\n",
        )
        .private("SECURITY_FIXTURE_IO")?;
        self.git(&repository, &["add", "."])?;
        self.git(&repository, &["commit", "--quiet", "-m", &canary])?;
        let seeded = self.revision(&repository, "HEAD")?;
        fs::write(
            repository.join(".gitleaksignore"),
            format!("{seeded}:credential.txt:github-pat:1\n"),
        )
        .private("SECURITY_FIXTURE_IO")?;
        self.git(&repository, &["add", "."])?;
        self.git(
            &repository,
            &["commit", "--quiet", "-m", "Suppression attempt"],
        )?;
        let summary = self.root.join("seeded-summary.md");
        let result = self.scan(&repository, &Scope::All, &summary)?;
        require(result.finding_count > 0, "SECURITY_SEEDED_SECRET_MISSED")?;
        require(
            result.unreviewed_count == result.finding_count
                && result.reviewed_public_data_count == 0
                && result.disposition
                    == nw_security_scan::classifier::Disposition::UnreviewedFindings,
            "SECURITY_SEEDED_POLICY_FAILED",
        )?;
        let public = private::read_bounded(&summary, private::MIB, self.deadline)?;
        require(
            !contains(&public, secret.as_bytes())
                && !contains(&public, canary.as_bytes())
                && !contains(&public, source.as_bytes()),
            "SECURITY_PRIVATE_DATA_PUBLISHED",
        )?;
        diagnostic("INFO", "SECURITY_REAL_SEEDED_SUPPRESSION_PRIVACY_PASS");
        let sentinel = self.root.join("fixture-sentinel");
        private::copy_executable_bounded(
            &env::current_exe().private("SECURITY_FIXTURE_IO")?,
            &sentinel,
         self.deadline)?;
        self.git(
            &repository,
            &["config", "diff.sentinel.command", private::text(&sentinel)?],
        )?;
        self.git(
            &repository,
            &[
                "config",
                "diff.sentinel.textconv",
                private::text(&sentinel)?,
            ],
        )?;
        fs::write(repository.join(".gitattributes"), "*.txt diff=sentinel\n")
            .private("SECURITY_FIXTURE_IO")?;
        self.git(
            &repository,
            &[
                "update-index",
                "--add",
                "--cacheinfo",
                &format!("160000,{seeded},submodule"),
            ],
        )?;
        self.git(&repository, &["add", ".gitattributes"])?;
        self.git(
            &repository,
            &["commit", "--quiet", "-m", "Attributes and gitlink"],
        )?;
        let next = self.revision(&repository, "HEAD")?;
        self.git(
            &repository,
            &[
                "update-index",
                "--cacheinfo",
                &format!("160000,{next},submodule"),
            ],
        )?;
        // A benign runtime-generated binary archive triggers the source-predicted cat-file path.
        let mut gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        gzip.write_all(&[0_u8; 512])
            .private("SECURITY_FIXTURE_IO")?;
        fs::write(
            repository.join("benign.gz"),
            gzip.finish().private("SECURITY_FIXTURE_IO")?,
        )
        .private("SECURITY_FIXTURE_IO")?;
        self.git(&repository, &["add", "benign.gz"])?;
        self.git(
            &repository,
            &["commit", "--quiet", "-m", "Gitlink transition and archive"],
        )?;
        let result = self.scan(
            &repository,
            &Scope::All,
            &self.root.join("attributes-summary.md"),
        )?;
        require(
            result.finding_count > 0
                && result.blob_invocations > 0
                && !self.root.join("sentinel-hit").exists(),
            "SECURITY_ATTRIBUTE_BLOB_CONTRACT_FAILED",
        )?;
        diagnostic("INFO", "SECURITY_REAL_ATTRIBUTES_GITLINK_BLOB_PASS");
        Ok(())
    }
    fn merge_scope(&self) -> Result<()> {
        let repository = self.repository("merge")?;
        let base = self.revision(&repository, "HEAD")?;
        self.git(&repository, &["checkout", "--quiet", "-b", "left"])?;
        fs::write(repository.join("merge.txt"), "left\n").private("SECURITY_FIXTURE_IO")?;
        self.git(&repository, &["add", "."])?;
        self.git(&repository, &["commit", "--quiet", "-m", "Left"])?;
        let left = self.revision(&repository, "HEAD")?;
        self.git(&repository, &["checkout", "--quiet", "-b", "right", &base])?;
        fs::write(repository.join("merge.txt"), "right\n").private("SECURITY_FIXTURE_IO")?;
        self.git(&repository, &["add", "."])?;
        self.git(&repository, &["commit", "--quiet", "-m", "Right"])?;
        let right = self.revision(&repository, "HEAD")?;
        let conflict =
            self.git_result(&repository, &["merge", "--no-commit", "--no-ff", "left"])?;
        require(
            conflict.status == Some(1) && conflict.failure.is_none(),
            "SECURITY_MERGE_SETUP_FAILED",
        )?;
        let secret = synthetic_secret()?;
        fs::write(
            repository.join("merge.txt"),
            format!("token = \"{secret}\"\n"),
        )
        .private("SECURITY_FIXTURE_IO")?;
        self.git(&repository, &["add", "merge.txt"])?;
        self.git(&repository, &["commit", "--quiet", "-m", "Resolution"])?;
        let head = self.revision(&repository, "HEAD")?;
        require(
            self.git(&repository, &["rev-list", "--parents", "-n", "1", "HEAD"])?
                .split_whitespace()
                .count()
                == 3,
            "SECURITY_MERGE_SETUP_FAILED",
        )?;
        for parent in [&left, &right] {
            let text = self.git(&repository, &["show", &format!("{parent}:merge.txt")])?;
            require(!text.contains(&secret), "SECURITY_MERGE_ORACLE_FAILED")?;
        }
        let result = self.scan(
            &repository,
            &Scope::PullRequest { base: right, head },
            &self.root.join("merge-summary.md"),
        )?;
        require(result.finding_count > 0, "SECURITY_MERGE_SECRET_MISSED")?;
        diagnostic("INFO", "SECURITY_REAL_MERGE_ONLY_PR_PASS");
        Ok(())
    }
    fn all_refs_scope(&self) -> Result<()> {
        let repository = self.repository("scope")?;
        let base = self.revision(&repository, "HEAD")?;
        self.git(&repository, &["checkout", "--quiet", "-b", "hidden"])?;
        fs::write(
            repository.join("hidden.txt"),
            format!("token = \"{}\"\n", synthetic_secret()?),
        )
        .private("SECURITY_FIXTURE_IO")?;
        self.git(&repository, &["add", "."])?;
        self.git(&repository, &["commit", "--quiet", "-m", "Hidden"])?;
        self.git(&repository, &["tag", "retained-secret"])?;
        self.git(&repository, &["checkout", "--quiet", "main"])?;
        self.git(&repository, &["branch", "-D", "hidden"])?;
        fs::write(repository.join("second.txt"), "clean second commit\n")
            .private("SECURITY_FIXTURE_IO")?;
        self.git(&repository, &["add", "."])?;
        self.git(&repository, &["commit", "--quiet", "-m", "Second clean"])?;
        let head = self.revision(&repository, "HEAD")?;
        require(
            self.scan(&repository, &Scope::All, &self.root.join("all-refs.md"))?
                .finding_count
                > 0,
            "SECURITY_ALL_REFS_MISSED",
        )?;
        require(
            self.scan(
                &repository,
                &Scope::PullRequest {
                    base: base.clone(),
                    head,
                },
                &self.root.join("pr-scope.md"),
            )?
            .finding_count
                == 0,
            "SECURITY_PR_SCOPE_INVALID",
        )?;
        let missing = "0".repeat(40);
        let independent = self.git_result(
            &repository,
            &["cat-file", "-e", &format!("{missing}^{{commit}}")],
        )?;
        require(
            independent.completed_nonzero(),
            "SECURITY_MISSING_REVISION_NOT_ESTABLISHED",
        )?;
        independent.barrier.recheck()?;
        require(
            failed_at(
                &self.scan(
                    &repository,
                    &Scope::PullRequest {
                        base,
                        head: missing,
                    },
                    &self.root.join("missing-head.md"),
                ),
                "SECURITY_REVISION_COMMIT_CHECK_FAILED",
            ),
            "SECURITY_MISSING_REVISION_ACCEPTED",
        )?;
        diagnostic("INFO", "SECURITY_REAL_MISSING_REVISION_COMMIT_CHECK_PASS");
        diagnostic("INFO", "SECURITY_REAL_ALL_REFS_PR_SCOPE_PASS");
        Ok(())
    }
    fn real_git_failure(&self) -> Result<()> {
        let repository = self.repository("broken")?;
        let blob = self.revision(&repository, "HEAD:README.txt")?;
        let original = self.git_result(&repository, &["cat-file", "-t", &blob])?;
        require(
            original.completed()
                && original.status == Some(0)
                && original.stdout.bytes == b"blob\n"
                && original.stderr.quiet(),
            "SECURITY_REAL_GIT_OBJECT_SETUP_FAILED",
        )?;
        let object = repository
            .join(".git/objects")
            .join(&blob[..2])
            .join(&blob[2..]);
        require(
            fs::symlink_metadata(&object).is_ok_and(|m| m.is_file() && !m.file_type().is_symlink()),
            "SECURITY_REAL_GIT_OBJECT_SETUP_FAILED",
        )?;
        fs::remove_file(&object).private("SECURITY_FIXTURE_IO")?;
        require(
            !object.try_exists().private("SECURITY_FIXTURE_IO")?,
            "SECURITY_REAL_GIT_OBJECT_STILL_PRESENT",
        )?;
        let absent = self.git_result(&repository, &["cat-file", "-e", &blob])?;
        require(
            absent.completed_nonzero(),
            "SECURITY_REAL_GIT_MISSING_OBJECT_NOT_ESTABLISHED",
        )?;
        absent.barrier.recheck()?;
        let args = protocol::history_args(private::text(&repository.join(".git"))?, "--all");
        let refs = args.iter().map(String::as_str).collect::<Vec<_>>();
        let fault = self.git_result(&repository, &refs)?;
        require(
            fault.completed_nonzero(),
            "SECURITY_REAL_GIT_FAULT_NOT_ESTABLISHED",
        )?;
        fault.barrier.recheck()?;
        require(
            failed_at(
                &self.scan(
                    &repository,
                    &Scope::All,
                    &self.root.join("broken-summary.md"),
                ),
                "SECURITY_HISTORY_GIT_FAILED",
            ),
            "SECURITY_REAL_GIT_FAILURE_ACCEPTED",
        )?;
        diagnostic("INFO", "SECURITY_REAL_HISTORY_COMPLETED_NONZERO_PASS");
        diagnostic("INFO", "SECURITY_REAL_GIT_FAILURE_PASS");
        Ok(())
    }
    fn untrusted_build(&self) -> Result<()> {
        let repository = self.repository("untrusted-build")?;
        private::directory(&repository.join(".cargo"))?;
        fs::write(
            repository.join("Cargo.toml"),
            "[package]\nname='untrusted-sentinel'\nversion='0.0.0'\n",
        )
        .private("SECURITY_FIXTURE_IO")?;
        fs::write(
            repository.join("build.rs"),
            "fn main() { std::fs::write(\"BUILD_EXECUTED\", b\"bad\").unwrap(); }\n",
        )
        .private("SECURITY_FIXTURE_IO")?;
        fs::write(
            repository.join(".cargo/config.toml"),
            format!(
                "[build]\nrustc-wrapper = {:?}\n",
                self.root.join("fixture-sentinel")
            ),
        )
        .private("SECURITY_FIXTURE_IO")?;
        private::copy_executable_bounded(
            &env::current_exe().private("SECURITY_FIXTURE_IO")?,
            &repository.join("git"),
         self.deadline)?;
        self.git(&repository, &["add", "."])?;
        self.git(
            &repository,
            &["commit", "--quiet", "-m", "Untrusted build inputs"],
        )?;
        require(
            self.scan(
                &repository,
                &Scope::All,
                &self.root.join("untrusted-summary.md"),
            )?
            .finding_count
                == 0
                && !repository.join("BUILD_EXECUTED").exists()
                && !self.root.join("sentinel-hit").exists(),
            "SECURITY_BUILD_BOUNDARY_FAILED",
        )?;
        diagnostic("INFO", "SECURITY_REAL_UNTRUSTED_BUILD_PASS");
        Ok(())
    }
    fn child(&self, mode: &str) -> Result<Command> {
        let mut command = Command::new(env::current_exe().private("SECURITY_FIXTURE_IO")?);
        private::controlled(&mut command, &self.root);
        command.env("NW_SECURITY_FIXTURE_DEADLINE", serde_json::to_string(&self.deadline.value()).private("SECURITY_FIXTURE_IO")?);
        command
            .current_dir(&self.root)
            .args(["--fixture-child", mode])
            .arg(&self.root);
        Ok(command)
    }
    fn process_cases(&self) -> Result<()> {
        for mode in ["tree-exit", "tree-detach", "nested", "race", "proxy-dies"] {
            let root = self.root.join(mode);
            private::directory(&root)?;
            let mut command = Command::new(env::current_exe().private("SECURITY_FIXTURE_IO")?);
            private::controlled(&mut command, &root);
        command.env("NW_SECURITY_FIXTURE_DEADLINE", serde_json::to_string(&self.deadline.value()).private("SECURITY_FIXTURE_IO")?);
            command.args(["--fixture-child", mode]).arg(&root);
            let result = process::run(
                &mut command,
                Options::command(Duration::from_secs(3), self.deadline),
                || {
                    if mode != "tree-exit"
                        && root
                            .join("tree-ready")
                            .try_exists()
                            .private("SECURITY_FIXTURE_IO")?
                    {
                        Err(ScanFailure("SECURITY_FIXTURE_TERMINATE_READY_TREE"))
                    } else {
                        Ok(())
                    }
                },
            )?;
            require(
                result.adopted > 0
                    && result.reaped >= 2
                    && result.failure.is_some()
                    && result.cleanup_elapsed <= process::GRACE,
                "SECURITY_ADOPTION_NOT_PROVEN",
            )?;
            result.barrier.recheck()?;
            if mode == "proxy-dies" {
                require(
                    result.status.is_none()
                        && root.join("proxy-started").is_file()
                        && !root.join("proxy-completed").exists(),
                    "SECURITY_SYNTHETIC_PROXY_DEATH_NOT_PROVEN",
                )?;
            }
        }
        diagnostic("INFO", "SECURITY_ACTUAL_DESCENDANT_ADOPTION_PASS");
        diagnostic("INFO", "SECURITY_SYNTHETIC_PROXY_DEATH_INCOMPLETE_PASS");
        let pressure = process::run(
            &mut self.child("pressure")?,
            Options::command(Duration::from_secs(3), self.deadline),
            || Ok(()),
        )?;
        require(
            pressure.failure.is_some()
                && (pressure.stdout.overflow || pressure.stderr.overflow)
                && pressure.stdout.bytes.len() <= private::CAPTURE_LIMIT
                && pressure.stderr.bytes.len() <= private::CAPTURE_LIMIT,
            "SECURITY_CAPTURE_PRESSURE_NOT_PROVEN",
        )?;
        let panic_payload = synthetic_secret()?;
        let panic = process::run(
            &mut self.child("sleep")?,
            Options::command(Duration::from_secs(2), self.deadline),
            || {
                std::panic::panic_any(panic_payload.clone());
            },
        )?;
        require(
            panic.failure == Some(ScanFailure("SECURITY_CHILD_PANIC")) && panic.reaped > 0,
            "SECURITY_PANIC_CLEANUP_NOT_PROVEN",
        )?;
        for mode in ["cancel", "cancel-int", "unrelated"] {
            let result = process::run(
                &mut self.child(mode)?,
                Options::command(Duration::from_secs(8), self.deadline),
                || Ok(()),
            )?;
            result.successful()?;
            require(
                result.stdout.bytes == b"FIXTURE_CONTROL_PASS\n",
                "SECURITY_PROCESS_ORACLE_FAILED",
            )?;
        }
        diagnostic(
            "INFO",
            "SECURITY_ACTUAL_CANCEL_PANIC_PRESSURE_UNRELATED_PASS",
        );
        self.fault_cases()?;
        let utility =
            fs::canonicalize("/usr/bin/env").private("SECURITY_FIXTURE_UTILITY_INVALID")?;
        let mut version = Command::new(&utility);
        private::controlled(&mut version, &self.root);
        version.env("NW_SECURITY_FIXTURE_DEADLINE", serde_json::to_string(&self.deadline.value()).private("SECURITY_FIXTURE_IO")?);
        version.arg("--version");
        let result = process::run(
            &mut version,
            Options::command(Duration::from_secs(15), self.deadline),
            || Ok(()),
        )?;
        result.successful()?;
        let output = std::str::from_utf8(&result.stdout.bytes)
            .private("SECURITY_FIXTURE_UTILITY_INVALID")?;
        let version = output
            .lines()
            .next()
            .and_then(|line| line.strip_prefix("env (GNU coreutils) "))
            .ok_or(ScanFailure("SECURITY_FIXTURE_UTILITY_INVALID"))?;
        require(
            !version.is_empty()
                && version.len() <= 64
                && version.bytes().all(|b| b.is_ascii_digit() || b == b'.'),
            "SECURITY_FIXTURE_UTILITY_INVALID",
        )?;
        eprintln!(
            "{} [INFO] SECURITY_FIXTURE_GNU_ENV version={version} sha256={}",
            protocol::unix_millis()?,
            private::executable_hash_bounded(&utility, self.deadline)?
        );
        for (option, mode) in [
            ("--block-signal=TERM", "blocked"),
            ("--ignore-signal=CHLD", "ignored"),
        ] {
            let mut command = Command::new(&utility);
            private::controlled(&mut command, &self.root);
        command.env("NW_SECURITY_FIXTURE_DEADLINE", serde_json::to_string(&self.deadline.value()).private("SECURITY_FIXTURE_IO")?);
            command
                .arg(option)
                .arg(env::current_exe().private("SECURITY_FIXTURE_IO")?)
                .args(["--fixture-child", mode])
                .arg(&self.root);
            let result = process::run(
                &mut command,
                Options::command(Duration::from_secs(2), self.deadline),
                || Ok(()),
            )?;
            result.successful()?;
            require(
                result.stdout.bytes == b"FIXTURE_STATE_OBSERVED_REJECTED\n",
                "SECURITY_SIGNAL_NEGATIVE_NOT_PROVEN",
            )?;
        }
        let mut debug = self.child("debug")?;
        debug.env("GITHUB_ACTIONS", "true").env("RUNNER_DEBUG", "1");
        process::run(
            &mut debug,
            Options::command(Duration::from_secs(2), self.deadline),
            || Ok(()),
        )?
        .successful()?;
        diagnostic("INFO", "SECURITY_ACTUAL_SIGNAL_STATE_DEBUG_PASS");
        Ok(())
    }
    fn fault_cases(&self) -> Result<()> {
        let mut absent = Command::new(self.root.join("absent-program"));
        require(
            process::run(
                &mut absent,
                Options::command(Duration::from_secs(1), self.deadline),
                || Ok(()),
            )
            .is_err(),
            "SECURITY_SPAWN_FAILURE_ACCEPTED",
        )?;
        process::barrier()?;
        for mode in [
            "fault-subreaper",
            "fault-registration",
            "fault-discovery",
            "fault-probe",
            "fault-cleanup",
        ] {
            let root = self.root.join(mode);
            private::directory(&root)?;
            let mut command = Command::new(env::current_exe().private("SECURITY_FIXTURE_IO")?);
            private::controlled(&mut command, &root);
        command.env("NW_SECURITY_FIXTURE_DEADLINE", serde_json::to_string(&self.deadline.value()).private("SECURITY_FIXTURE_IO")?);
            command.args(["--fixture-child", mode]).arg(&root);
            let result = process::run(
                &mut command,
                Options::command(Duration::from_secs(8), self.deadline),
                || Ok(()),
            )?;
            require(
                result.status == Some(0)
                    && result.stdout.bytes == b"FIXTURE_INJECTED_FAILURE_RETAINED\n",
                "SECURITY_INJECTED_FAULT_ORACLE_FAILED",
            )?;
            let retained = private::read_bounded(&root.join("retained-path"), 4096, self.deadline)?;
            let retained =
                Path::new(std::str::from_utf8(&retained).private("SECURITY_FIXTURE_IO")?);
            require(
                retained.starts_with(&root) && retained.is_dir(),
                "SECURITY_PREMATURE_SCRATCH_REMOVAL",
            )?;
            result.barrier.recheck()?;
            fs::remove_dir_all(&root).private("SECURITY_FIXTURE_IO")?;
        }
        diagnostic("INFO", "SECURITY_MOCK_OWNERSHIP_FAULT_RETENTION_PASS");
        let root = self.root.join("report-growth");
        private::directory(&root)?;
        private::directory(&root.join("proof"))?;
        let context = protocol::Context {
            root: root.clone(),
            deadline: self.deadline,
            value: serde_json::json!({}),
        };
        let mut writer = Command::new(env::current_exe().private("SECURITY_FIXTURE_IO")?);
        private::controlled(&mut writer, &root);
        writer.env("NW_SECURITY_FIXTURE_DEADLINE", serde_json::to_string(&self.deadline.value()).private("SECURITY_FIXTURE_IO")?);
        writer.args(["--fixture-child", "report-growth"]).arg(&root);
        let outcome = process::run(
            &mut writer,
            Options::command(Duration::from_secs(3), self.deadline),
            || context.monitor(),
        )?;
        require(
            outcome.failure == Some(ScanFailure("SECURITY_REPORT_OVERSIZE"))
                && outcome.reaped == 1
                && fs::metadata(root.join("findings.json"))
                    .private("SECURITY_FIXTURE_IO")?
                    .len()
                    > private::REPORT_LIMIT,
            "SECURITY_ACTUAL_REPORT_LIMIT_NOT_PROVEN",
        )?;
        diagnostic("INFO", "SECURITY_ACTUAL_REPORT_GROWTH_REAP_PASS");
        Ok(())
    }
    fn public_repository(&self, name: &str, message: &str) -> Result<PathBuf> {
        // TEST ONLY: immutable byte identities approved by the separate preflight.
        // Independent frozen source identities precede copying into synthetic Git.
        const DOCUMENTS: [(&str, u64, &str); 4] = [
            (
                "docs/research-pack/handoff/02-2-functional-scope-and-migration.md",
                27483,
                "02a9a4eb5a09d04d089dfbf25728af963772af8016a6cc62495c5358ec1d4bed",
            ),
            (
                "docs/research-pack/handoff/05-5-domain-model-and-interface-contracts.md",
                96791,
                "01c780f92cebe1e2f50c2b880a76cfd658c6bc1613804533f0884adf912b00f4",
            ),
            (
                "docs/research-pack/Numera-Wiz-Implementation-Mandate-EN.md",
                772543,
                "126c8aa8aa7f874802a09600040a1986cff650ad70b70f58f30a441d87fbef06",
            ),
            (
                "docs/research-pack/research_v7/filmcraft-source-commands.json",
                344827,
                "dc79337d44c786b037dabe5c18ecfac9cfeb3ea6912f761d3dc0d189590f1ed5",
            ),
        ];
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mut verified = Vec::with_capacity(4);
        for (path, length, digest) in DOCUMENTS {
            let bytes = private::read_bounded(&source.join(path), private::MIB, self.deadline)?;
            require(
                bytes.len() as u64 == length && private::hash_bounded(&bytes, self.deadline)? == digest,
                "SECURITY_PUBLIC_DOCUMENT_IDENTITY_FAILED",
            )?;
            verified.push((path, bytes));
        }
        let repository = self.repository(name)?;
        for (path, bytes) in verified {
            let target = repository.join(path);
            fs::create_dir_all(target.parent().ok_or(ScanFailure("SECURITY_FIXTURE_IO"))?)
                .private("SECURITY_FIXTURE_IO")?;
            private::write_bounded(&target, &bytes, self.deadline)?;
        }
        self.git(&repository, &["add", "."])?;
        self.git(&repository, &["commit", "--quiet", "-m", message])?;
        Ok(repository)
    }
    fn known_public_data_policy(&self) -> Result<()> {
        let repository =
            self.public_repository("known-public-data", "Verified public documentation fixture")?;
        let result = self.scan(
            &repository,
            &Scope::All,
            &self.root.join("public-documents-summary.md"),
        )?;
        require(
            result.finding_count == 5
                && result.scanner_status == 2
                && result.successful_history_invocations > 0,
            "SECURITY_PUBLIC_DOCUMENT_SCANNER_CONTRACT_FAILED",
        )?;
        eprintln!(
            "{} [INFO] SECURITY_PUBLIC_DOCUMENT_SCANNER_OBSERVED total=5 status=2 history={}",
            protocol::unix_millis()?,
            result.successful_history_invocations
        );
        classifier_cases::qualify(self, &repository, &result)
    }
}
fn synthetic_secret() -> Result<String> {
    let prefix = String::from_utf8(vec![103, 104, 112, 95]).private("SECURITY_FIXTURE_IO")?;
    let entropy = format!("{}{}", private::nonce()?, private::nonce()?);
    Ok(format!("{prefix}{}", &entropy[..36]))
}
fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}
fn failed_at<T>(result: &Result<T>, category: &'static str) -> bool {
    matches!(result, Err(error) if error.0 == category)
}
