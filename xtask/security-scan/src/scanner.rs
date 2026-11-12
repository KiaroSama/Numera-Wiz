//! Scan orchestration. Repository data never selects Cargo, executables or rules.
use crate::bootstrap::Deadline;
use crate::{
    PrivateResult, Result, ScanFailure,
    assets::Release,
    classifier::{Disposition, Plan},
    classifier_git::Session,
    private::{self, Scratch},
    process,
    protocol::{self, Context, Scope},
    report, require,
};
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

pub struct ScanRequest<'a> {
    pub repository: &'a Path,
    pub summary: Option<&'a Path>,
    pub scope: &'a Scope,
    pub release: &'a Release,
    pub deadline: Deadline,
}
pub struct VerifiedScan {
    pub scanner_version: String,
    pub scanner_status: i32,
    pub finding_count: usize,
    pub successful_history_invocations: usize,
    pub metadata_invocations: usize,
    pub blob_invocations: usize,
    pub reviewed_public_data_count: usize,
    pub unreviewed_count: usize,
    pub disposition: Disposition,
    pub classifier_commands: usize,
    pub classifier_resolutions: usize,
    pub classifier_blob_reads: usize,
    pub classifier_cache_hits: usize,
    /// Only the original bounded public location view, never private report bytes.
    pub summary: String,
}
pub fn guard() -> Result<()> {
    require(
        env::var("RUNNER_DEBUG").as_deref() != Ok("1"),
        "SECURITY_DEBUG_REJECTED",
    )?;
    require(
        env::var("GITHUB_ACTIONS").as_deref() == Ok("true"),
        "SECURITY_HOST_UNSUPPORTED",
    )
}
pub fn temporary() -> Result<PathBuf> {
    let path = env::var_os("RUNNER_TEMP").ok_or(ScanFailure("SECURITY_TEMP_INVALID"))?;
    fs::canonicalize(path).private("SECURITY_TEMP_INVALID")
}
pub fn real_git() -> Result<PathBuf> {
    fs::canonicalize("/usr/bin/git").private("SECURITY_GIT_UNAVAILABLE")
}
pub fn scan(request: &ScanRequest<'_>) -> Result<VerifiedScan> {
    request.deadline.check()?;
    guard()?;
    process::initialize()?;
    process::barrier()?;
    request.scope.validate()?;
    request.release.validate(request.deadline)?;
    let repository = fs::canonicalize(request.repository).private("SECURITY_REPOSITORY_INVALID")?;
    let git_directory = repository.join(".git");
    require(
        fs::symlink_metadata(&git_directory)
            .is_ok_and(|m| m.is_dir() && !m.file_type().is_symlink()),
        "SECURITY_REPOSITORY_INVALID",
    )?;
    let git_directory = fs::canonicalize(git_directory).private("SECURITY_REPOSITORY_INVALID")?;
    require(
        !git_directory
            .join(".gitleaksignore")
            .try_exists()
            .private("SECURITY_REPOSITORY_INVALID")?,
        "SECURITY_METADATA_SUPPRESSION_REJECTED",
    )?;
    let git = real_git()?;
    require(!git.starts_with(&repository), "SECURITY_EXECUTABLE_INVALID")?;
    let scratch = Scratch::create(&temporary()?)?;
    let result =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<VerifiedScan> {
            require(
                !scratch.path().starts_with(&repository) && !repository.starts_with(scratch.path()),
                "SECURITY_REPOSITORY_INVALID",
            )?;
            let context = Context::prepare(
                scratch.path(),
                &git_directory,
                &git,
                request.release,
                request.scope,
                request.deadline,
            )?;
            let mut classifier = Session::admit(&context, request.deadline)?;
            if let Scope::PullRequest { base, head } = request.scope {
                for revision in [base, head] {
                    let mut command = Command::new(&git);
                    private::controlled(&mut command, scratch.path());
                    command
                        .current_dir(&git_directory)
                        .arg("-C")
                        .arg(&git_directory)
                        .args(["cat-file", "-e"])
                        .arg(format!("{revision}^{{commit}}"));
                    let checked = process::run(
                        &mut command,
                        process::Options::command(Duration::from_secs(15), request.deadline),
                        || Ok(()),
                    )?;
                    if checked.completed_nonzero() {
                        return Err(ScanFailure("SECURITY_REVISION_COMMIT_CHECK_FAILED"));
                    }
                    checked.successful()?;
                    require(
                        checked.completed() && checked.stdout.quiet() && checked.stderr.quiet(),
                        "SECURITY_REVISION_INVALID",
                    )?;
                }
            }
            let outcome = process::run(
                &mut context.launch()?,
                process::Options::command(Duration::from_secs(930), request.deadline),
                || context.monitor(),
            )?;
            if let Some(failure) = outcome.failure {
                return Err(failure);
            }
            require(outcome.adopted == 0, "SECURITY_UNEXPECTED_DESCENDANT")?;
            let proofs = protocol::verify_proofs(&context, outcome.leader)?;
            let report_bytes = private::read_bounded(
                &context.root.join("findings.json"),
                private::REPORT_LIMIT,
                request.deadline,
            )?;
            let parsed = report::parse(&report_bytes, request.deadline)?;
            request.deadline.check()?;
            require(!process::cancelled(), "SECURITY_DEADLINE")?;
            report::agreement(
                outcome.status,
                &parsed,
                proofs.history,
                outcome.stdout.quiet() && outcome.stderr.quiet(),
            )?;
            let counts = Plan::from_report(&report_bytes, parsed.count, request.deadline)?
                .evaluate(&mut classifier)?;
            // Recheck scanner proofs and aggregate accounting after the final internal
            // command; classifier transactions never alter the history counter.
            protocol::verify_proofs(&context, outcome.leader)?;
            let summary = report::classified_summary(
                &parsed,
                matches!(request.scope, Scope::PullRequest { .. }),
                &counts,
                request.deadline,
            )?;
            request.deadline.check()?;
            require(!process::cancelled(), "SECURITY_DEADLINE")?;
            Ok(VerifiedScan {
                scanner_version: crate::assets::VERSION.into(),
                finding_count: parsed.count,
                scanner_status: outcome
                    .status
                    .ok_or(ScanFailure("SECURITY_RESULT_DISAGREEMENT"))?,
                successful_history_invocations: proofs.history,
                metadata_invocations: proofs.metadata,
                blob_invocations: proofs.blobs,
                reviewed_public_data_count: counts.reviewed,
                unreviewed_count: counts.unreviewed,
                disposition: counts.disposition,
                classifier_commands: classifier.stats().commands,
                classifier_resolutions: classifier.stats().resolutions,
                classifier_blob_reads: classifier.stats().blob_reads,
                classifier_cache_hits: classifier.stats().cache_hits,
                summary,
            })
        }))
        .unwrap_or(Err(ScanFailure("SECURITY_SCAN_PANIC")));
    match process::barrier() {
        Ok(barrier) => {
            scratch.remove(&barrier, request.deadline)?;
            let verified = result?;
            request.deadline.check()?;
            require(!process::cancelled(), "SECURITY_DEADLINE")?;
            report::publish(request.summary, &verified.summary, request.deadline)?;
            Ok(verified)
        }
        Err(_) => Err(ScanFailure("SECURITY_CLEANUP_UNVERIFIED")),
    }
}
/// Must be called before normal logging in every binary copied into private workers.
pub fn internal_dispatch() -> Option<std::process::ExitCode> {
    crate::install_private_panic_hook();
    let executable = env::current_exe().ok()?;
    let name = executable.file_name()?.to_str()?;
    let result = match name {
        "git" => protocol::proxy_worker(),
        "security-launch" => protocol::launch_worker().map(|_| 0),
        "security-download" => crate::assets::downloader_worker().map(|_| 0),
        _ => return None,
    };
    Some(match result {
        Ok(code) => std::process::ExitCode::from(u8::try_from(code).unwrap_or(1)),
        Err(error) => {
            crate::diagnostic("ERROR", error.0);
            std::process::ExitCode::FAILURE
        }
    })
}
pub fn provenance(deadline: Deadline) -> Result<()> {
    let helper = private::executable_hash_bounded(
        &env::current_exe().private("SECURITY_EXECUTABLE_INVALID")?,
        deadline,
    )?;
    let manifest = private::hash_bounded(include_bytes!("../Cargo.toml"), deadline)?;
    let lock = private::hash_bounded(include_bytes!("../Cargo.lock"), deadline)?;
    // Only fixed labels and SHA-256 digests; compiler/source commit are verified by
    // the later trusted workflow bootstrap, not inferred from Cargo package metadata.
    eprintln!(
        "{} [INFO] SECURITY_PROVENANCE helper_sha256={helper} manifest_sha256={manifest} lock_sha256={lock}",
        protocol::unix_millis()?
    );
    Ok(())
}
