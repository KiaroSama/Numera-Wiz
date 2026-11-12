//! Qualification only: actual scanner cases and explicitly fabricated evaluator rows.
use super::*;
use nw_security_scan::{
    classifier::{self, Counts, DOCUMENTS, Disposition, Plan},
    classifier_git::{self, Session},
    report,
};
use serde_json::json;

fn reviewed(result: &VerifiedScan, total: usize) -> Result<()> {
    require(
        result.finding_count == total
            && result.reviewed_public_data_count == total
            && result.unreviewed_count == 0
            && result.disposition == Disposition::ReviewedPublicData
            && result.scanner_status == 2
            && result.successful_history_invocations > 0,
        "SECURITY_REVIEWED_POLICY_POSITIVE_FAILED",
    )
}
fn row(commit: &str, path: usize) -> serde_json::Value {
    json!({"RuleID": if path == 3 { "sourcegraph-access-token" } else { "generic-api-key" },
        "File": DOCUMENTS[path].path, "StartLine": ([111,254,301,2][path]), "Commit": commit})
}
fn mock_plan(bytes: &[u8], deadline: Instant) -> Result<Plan> {
    let parsed = report::parse(bytes)?;
    Plan::from_report(bytes, parsed.count, deadline)
}
fn pure(deadline: Instant) -> Result<()> {
    let commit = "1".repeat(40);
    let mut rows = vec![row(&commit, 0); 201];
    rows[200]["RuleID"] = json!("unapproved-rule");
    let bytes = serde_json::to_vec(&rows).private("SECURITY_FIXTURE_IO")?;
    let plan = mock_plan(&bytes, deadline)?;
    require(
        plan.total() == 201 && plan.unknown() == 1 && plan.resolutions() == 1,
        "SECURITY_MOCK_ALL_ROW_CLASSIFICATION_FAILED",
    )?;
    rows[200]["Unknown"] = json!({"nested": ["valid Unicode \u{1f642}"]});
    let mut invalid = serde_json::to_vec(&rows).private("SECURITY_FIXTURE_IO")?;
    invalid.pop();
    require(
        failed_at(&mock_plan(&invalid, deadline), "SECURITY_REPORT_INVALID"),
        "SECURITY_MOCK_CLASSIFIER_EOF_FAILED",
    )?;
    for (field, value) in [
        ("Commit", "not-a-commit"),
        ("File", "elsewhere"),
        ("RuleID", "unknown"),
    ] {
        let mut unknown = row(&commit, 0);
        unknown[field] = json!(value);
        let bytes = serde_json::to_vec(&[unknown]).private("SECURITY_FIXTURE_IO")?;
        require(
            mock_plan(&bytes, deadline)?.unknown() == 1,
            "SECURITY_MOCK_LOCATION_FAILED",
        )?;
    }
    let many: Vec<_> = (0..257).map(|n| row(&format!("{n:040x}"), 0)).collect();
    require(
        failed_at(
            &mock_plan(
                &serde_json::to_vec(&many).private("SECURITY_FIXTURE_IO")?,
                deadline,
            ),
            "SECURITY_CLASSIFIER_RESOLUTION_LIMIT",
        ),
        "SECURITY_MOCK_RESOLUTION_BOUND_FAILED",
    )?;
    require(
        failed_at(
            &classifier::positive_cache_room(4),
            "SECURITY_CLASSIFIER_CACHE_LIMIT",
        ) && classifier::positive_cache_room(3).is_ok(),
        "SECURITY_MOCK_CACHE_BOUND_FAILED",
    )?;
    require(
        failed_at(&mock_plan(&bytes, Instant::now()), "SECURITY_DEADLINE"),
        "SECURITY_MOCK_CLASSIFIER_DEADLINE_FAILED",
    )?;
    require(
        failed_at(&classifier::active(deadline, true), "SECURITY_DEADLINE"),
        "SECURITY_MOCK_CLASSIFIER_CANCEL_FAILED",
    )?;
    let parsed = report::parse(&bytes)?;
    for (status, history, quiet) in [
        (Some(0), 1, true),
        (Some(2), 0, true),
        (Some(2), 1, false),
        (Some(1), 1, true),
    ] {
        require(
            failed_at(
                &report::agreement(status, &parsed, history, quiet),
                "SECURITY_RESULT_DISAGREEMENT",
            ),
            "SECURITY_MOCK_CLASSIFIER_AGREEMENT_FAILED",
        )?;
    }
    require(
        Counts::checked(5, 5, 0)?.disposition == Disposition::ReviewedPublicData
            && Counts::checked(6, 5, 1)?.disposition == Disposition::UnreviewedFindings
            && Counts::checked(0, 0, 0)?.disposition == Disposition::Clean
            && Counts::checked(5, 5, 1).is_err(),
        "SECURITY_MOCK_CLASSIFIER_COUNTS_FAILED",
    )?;
    let text = report::classified_summary(&parsed, false, &Counts::checked(201, 200, 1)?)?;
    require(
        text.contains("Total: **201**")
            && text.contains("Reviewed public data: **200**")
            && text.contains("Unreviewed: **1**")
            && text.contains("Showing 200 of 201"),
        "SECURITY_MOCK_CLASSIFIER_SUMMARY_FAILED",
    )?;
    diagnostic(
        "INFO",
        "SECURITY_MOCK_CLASSIFIER_ALL_ROWS_COUNTS_LIMITS_PASS",
    );
    Ok(())
}

fn cli(fixture: &Fixture, repository: &Path, reviewed_only: bool) -> Result<()> {
    let current = fs::canonicalize(env::current_exe().private("SECURITY_FIXTURE_IO")?)
        .private("SECURITY_FIXTURE_IO")?;
    let parent = current.parent().ok_or(ScanFailure("SECURITY_FIXTURE_IO"))?;
    let executable = parent.join("nw-security-scan");
    require(
        fs::canonicalize(&executable).private("SECURITY_FIXTURE_CLI_IDENTITY")? == executable,
        "SECURITY_FIXTURE_CLI_IDENTITY",
    )?;
    let identity = private::executable_hash(&executable)?;
    let summary = fixture.root.join(if reviewed_only {
        "cli-reviewed.md"
    } else {
        "cli-unreviewed.md"
    });
    let end = protocol::unix_millis()?
        .checked_add(
            u64::try_from(
                fixture
                    .deadline
                    .saturating_duration_since(Instant::now())
                    .as_millis(),
            )
            .private("SECURITY_DEADLINE")?,
        )
        .ok_or(ScanFailure("SECURITY_DEADLINE"))?;
    let mut command = Command::new(&executable);
    private::controlled(&mut command, &fixture.root);
    command
        .current_dir(&fixture.root)
        .args(["scan"])
        .arg(repository)
        .env("GITHUB_ACTIONS", "true")
        .env("GITHUB_EVENT_NAME", "workflow_dispatch")
        .env("RUNNER_TEMP", &fixture.root)
        .env("GITHUB_STEP_SUMMARY", &summary)
        .env("NW_SECURITY_DEADLINE_UNIX_MS", end.to_string());
    let outcome = process::run(
        &mut command,
        Options::command(Duration::from_secs(930), fixture.deadline),
        || Ok(()),
    )?;
    require(
        private::executable_hash(&executable)? == identity
            && outcome.completed()
            && outcome.status == Some(if reviewed_only { 0 } else { 1 })
            && outcome.stdout.quiet()
            && contains(
                &outcome.stderr.bytes,
                if reviewed_only {
                    b"SECURITY_SCAN_REVIEWED_PUBLIC_DATA"
                } else {
                    b"SECURITY_SCAN_UNREVIEWED_FINDINGS"
                },
            )
            && !contains(&outcome.stderr.bytes, b"SECURITY_SCAN_CLEAN"),
        "SECURITY_ACTUAL_CLASSIFIER_CLI_FAILED",
    )?;
    let bytes = private::read(&summary, private::MIB)?;
    require(
        contains(&bytes, b"Reviewed public data: **5**")
            && contains(
                &bytes,
                if reviewed_only {
                    b"Unreviewed: **0**"
                } else {
                    b"Unreviewed findings require review"
                },
            ),
        "SECURITY_ACTUAL_CLASSIFIER_CLI_SUMMARY_FAILED",
    )?;
    diagnostic(
        "INFO",
        if reviewed_only {
            "SECURITY_ACTUAL_REVIEWED_CLI_PASS"
        } else {
            "SECURITY_ACTUAL_UNREVIEWED_CLI_PASS"
        },
    );
    Ok(())
}

fn evaluator(
    fixture: &Fixture,
    repository: &Path,
    name: &str,
    action: impl FnOnce(&mut Session<'_>) -> Result<()>,
) -> Result<()> {
    let root = fixture.root.join(name);
    private::directory(&root)?;
    let context = protocol::Context::prepare(
        &root,
        &repository.join(".git"),
        &fixture.git,
        fixture
            .release
            .as_ref()
            .ok_or(ScanFailure("SECURITY_FIXTURE_ASSETS"))?,
        &Scope::All,
        fixture.deadline,
    )?;
    let mut session = Session::admit(&context, fixture.deadline)?;
    session.bind_report(&private::hash(b"explicitly fabricated evaluator input"))?;
    action(&mut session)?;
    session.verify()
}

fn variants(fixture: &Fixture) -> Result<()> {
    let changed = fixture.public_repository("public-changed", "Temporary verified introduction")?;
    fixture.git(&changed, &["reset", "--soft", "HEAD~1"])?;
    let path = changed.join(DOCUMENTS[0].path);
    let mut bytes = private::read(&path, private::MIB)?;
    let original = private::hash(&bytes);
    let position = bytes
        .iter()
        .position(|b| *b == b' ')
        .ok_or(ScanFailure("SECURITY_FIXTURE_IO"))?;
    bytes[position] = b'.';
    require(
        bytes.len() as u64 == DOCUMENTS[0].length && private::hash(&bytes) != original,
        "SECURITY_ACTUAL_CHANGED_SETUP_FAILED",
    )?;
    fs::write(&path, &bytes).private("SECURITY_FIXTURE_IO")?;
    private::write(
        &changed.join("runtime.txt"),
        format!("token={}\n", synthetic_secret()?).as_bytes(),
    )?;
    fixture.git(&changed, &["add", "."])?;
    fixture.git(
        &changed,
        &[
            "commit",
            "--quiet",
            "-m",
            "Changed full bytes with same approved location",
        ],
    )?;
    let result = fixture.scan(
        &changed,
        &Scope::All,
        &fixture.root.join("changed-summary.md"),
    )?;
    require(
        result.reviewed_public_data_count == 4
            && result.unreviewed_count >= 2
            && result.disposition == Disposition::UnreviewedFindings,
        "SECURITY_ACTUAL_CHANGED_FULL_HASH_FAILED",
    )?;
    diagnostic(
        "INFO",
        "SECURITY_ACTUAL_CHANGED_SAME_LENGTH_AND_SECRET_PASS",
    );
    let moved = fixture.public_repository("public-moved", "Temporary verified locations")?;
    fixture.git(&moved, &["reset", "--soft", "HEAD~1"])?;
    fs::rename(moved.join(DOCUMENTS[0].path), moved.join("moved-public.md"))
        .private("SECURITY_FIXTURE_IO")?;
    let path = moved.join(DOCUMENTS[1].path);
    let mut shifted = vec![b'\n'];
    shifted.extend(private::read(&path, private::MIB)?);
    fs::write(path, shifted).private("SECURITY_FIXTURE_IO")?;
    fixture.git(&moved, &["add", "-A"])?;
    fixture.git(
        &moved,
        &["commit", "--quiet", "-m", "Moved path and shifted line"],
    )?;
    let result = fixture.scan(&moved, &Scope::All, &fixture.root.join("moved-summary.md"))?;
    require(
        result.finding_count == 5
            && result.reviewed_public_data_count == 3
            && result.unreviewed_count == 2,
        "SECURITY_ACTUAL_MOVED_LOCATIONS_FAILED",
    )?;
    diagnostic("INFO", "SECURITY_ACTUAL_MOVED_PATH_LINE_PASS");
    let repeated = fixture.public_repository("public-repeated", "Left public introduction")?;
    let base = fixture.revision(&repeated, "HEAD~1")?;
    fixture.git(&repeated, &["checkout", "--quiet", "-b", "right", &base])?;
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for document in DOCUMENTS {
        let bytes = private::read(&source.join(document.path), private::MIB)?;
        require(
            bytes.len() as u64 == document.length && private::hash(&bytes) == document.sha256,
            "SECURITY_PUBLIC_DOCUMENT_IDENTITY_FAILED",
        )?;
        let target = repeated.join(document.path);
        fs::create_dir_all(target.parent().ok_or(ScanFailure("SECURITY_FIXTURE_IO"))?)
            .private("SECURITY_FIXTURE_IO")?;
        private::write(&target, &bytes)?;
    }
    private::write(
        &repeated.join("right.txt"),
        b"Right side of a real merge.\n",
    )?;
    fixture.git(&repeated, &["add", "."])?;
    fixture.git(
        &repeated,
        &["commit", "--quiet", "-m", "Right public introduction"],
    )?;
    fixture.git(&repeated, &["checkout", "--quiet", "main"])?;
    fixture.git(
        &repeated,
        &["merge", "--quiet", "--no-ff", "--no-edit", "right"],
    )?;
    require(
        fixture
            .git(&repeated, &["rev-list", "--parents", "-n", "1", "HEAD"])?
            .split_whitespace()
            .count()
            == 3,
        "SECURITY_ACTUAL_REPEATED_MERGE_SETUP_FAILED",
    )?;
    let result = fixture.scan(
        &repeated,
        &Scope::All,
        &fixture.root.join("repeat-summary.md"),
    )?;
    reviewed(&result, result.finding_count)?;
    require(
        result.finding_count >= 10
            && result.classifier_resolutions >= 8
            && result.classifier_blob_reads == 4
            && result.classifier_cache_hits >= 4,
        "SECURITY_ACTUAL_REPEATED_MERGE_CACHE_FAILED",
    )?;
    diagnostic("INFO", "SECURITY_ACTUAL_REPEATED_MERGE_CACHE_PASS");
    Ok(())
}

fn real_evaluator(fixture: &Fixture, repository: &Path) -> Result<()> {
    // These rows are evaluator inputs, never claimed as scanner or history evidence.
    let commit = fixture.revision(repository, "HEAD")?;
    let tree = fixture.revision(repository, "HEAD^{tree}")?;
    fixture.git(
        repository,
        &["tag", "-a", "-m", "Evaluator tag", "evaluator-tag"],
    )?;
    let tag = fixture.revision(repository, "refs/tags/evaluator-tag")?;
    let report_root = fixture.root.join("evaluator-all-rows");
    private::directory(&report_root)?;
    let context = protocol::Context::prepare(
        &report_root,
        &repository.join(".git"),
        &fixture.git,
        fixture
            .release
            .as_ref()
            .ok_or(ScanFailure("SECURITY_FIXTURE_ASSETS"))?,
        &Scope::All,
        fixture.deadline,
    )?;
    let mut session = Session::admit(&context, fixture.deadline)?;
    let mut rows = vec![row(&commit, 0); 201];
    rows[200]["RuleID"] = json!("unreviewed-after-display-limit");
    let bytes = serde_json::to_vec(&rows).private("SECURITY_FIXTURE_IO")?;
    let counts = mock_plan(&bytes, fixture.deadline)?.evaluate(&mut session)?;
    require(
        counts.total == 201
            && counts.reviewed == 200
            && counts.unreviewed == 1
            && counts.disposition == Disposition::UnreviewedFindings
            && session.stats().resolutions == 1
            && session.stats().blob_reads == 1,
        "SECURITY_MOCK_ROW201_EVALUATION_FAILED",
    )?;
    diagnostic("INFO", "SECURITY_MOCK_ROW201_ACTUAL_GIT_EVALUATION_PASS");
    evaluator(fixture, repository, "evaluator-positive", |session| {
        require(
            session.resolve(&commit, 0)?
                && session.resolve(&commit, 1)?
                && !session.resolve(&tree, 0)?
                && !session.resolve(&tag, 0)?,
            "SECURITY_MOCK_REAL_GIT_TYPE_FAILED",
        )
    })?;
    evaluator(fixture, repository, "evaluator-proof-faults", |session| {
        require(
            session.resolve(&commit, 0)?,
            "SECURITY_MOCK_PROOF_CONTROL_FAILED",
        )?;
        session.verify()?;
        let directory = fixture.root.join("evaluator-proof-faults/review-proof");
        let files = protocol::proof_files(&directory)?;
        let stem = |name: &str| -> Result<u128> {
            let (stem, _) = name
                .split_once('.')
                .ok_or(ScanFailure("SECURITY_MOCK_RENAME_ORDER_SETUP_FAILED"))?;
            u128::from_str_radix(stem, 16).private("SECURITY_MOCK_RENAME_ORDER_SETUP_FAILED")
        };
        let first_stem = stem(
            files
                .first_key_value()
                .ok_or(ScanFailure("SECURITY_MOCK_RENAME_ORDER_SETUP_FAILED"))?
                .0,
        )?;
        let last_stem = stem(
            files
                .last_key_value()
                .ok_or(ScanFailure("SECURITY_MOCK_RENAME_ORDER_SETUP_FAILED"))?
                .0,
        )?;
        let before = format!(
            "{:032x}.done",
            first_stem
                .checked_sub(1)
                .ok_or(ScanFailure("SECURITY_MOCK_RENAME_ORDER_SETUP_FAILED"))?
        );
        let after = format!(
            "{:032x}.done",
            last_stem
                .checked_add(1)
                .ok_or(ScanFailure("SECURITY_MOCK_RENAME_ORDER_SETUP_FAILED"))?
        );
        require(
            files
                .keys()
                .all(|name| before.as_str() < name.as_str() && name.as_str() < after.as_str()),
            "SECURITY_MOCK_RENAME_ORDER_SETUP_FAILED",
        )?;
        let mut done = files
            .into_iter()
            .filter(|(name, _)| name.ends_with(".done"));
        let (name, first) = done.next().ok_or(ScanFailure("SECURITY_FIXTURE_IO"))?;
        let (_, second) = done.next().ok_or(ScanFailure("SECURITY_FIXTURE_IO"))?;
        let original = private::read(&first, private::RECORD_LIMIT)?;
        let other = private::read(&second, private::RECORD_LIMIT)?;
        // A later unexpected name previously allowed a missing old completion to
        // reach the private-read error first. Exercise both lexical orders.
        for name in [&before, &after] {
            session.verify()?;
            let renamed = directory.join(name);
            fs::rename(&first, &renamed).private("SECURITY_FIXTURE_IO")?;
            require(
                failed_at(&session.verify(), "SECURITY_CLASSIFIER_PROOF_INVALID"),
                "SECURITY_MOCK_RENAMED_CLASSIFIER_PROOF_FAILED",
            )?;
            fs::rename(&renamed, &first).private("SECURITY_FIXTURE_IO")?;
            session.verify()?;
        }
        diagnostic("INFO", "SECURITY_MOCK_CLASSIFIER_RENAME_ORDER_PASS");
        let renamed = directory.join(&after);
        private::write(&renamed, &original)?;
        require(
            failed_at(&session.verify(), "SECURITY_CLASSIFIER_PROOF_INVALID"),
            "SECURITY_MOCK_COPIED_CLASSIFIER_PROOF_FAILED",
        )?;
        fs::remove_file(&renamed).private("SECURITY_FIXTURE_IO")?;
        session.verify()?;
        fs::write(&first, &other).private("SECURITY_FIXTURE_IO")?;
        require(
            failed_at(&session.verify(), "SECURITY_CLASSIFIER_PROOF_INVALID"),
            "SECURITY_MOCK_MIXED_CLASSIFIER_PROOF_FAILED",
        )?;
        fs::write(&first, &original).private("SECURITY_FIXTURE_IO")?;
        session.verify()?;
        fs::remove_file(directory.join(&name)).private("SECURITY_FIXTURE_IO")?;
        require(
            failed_at(&session.verify(), "SECURITY_CLASSIFIER_PROOF_INVALID"),
            "SECURITY_MOCK_INCOMPLETE_CLASSIFIER_PROOF_FAILED",
        )?;
        private::write(&first, &original)?;
        session.verify()?;
        // Unrelated missing-context I/O must keep its own category, never count
        // as a successful filename-set rejection. Restore before continuing.
        let context = fixture.root.join("evaluator-proof-faults/context.json");
        let held_context = fixture
            .root
            .join("evaluator-proof-faults/held-context.json");
        fs::rename(&context, &held_context).private("SECURITY_FIXTURE_IO")?;
        require(
            failed_at(&session.verify(), "SECURITY_PRIVATE_IO_FAILED"),
            "SECURITY_MOCK_CLASSIFIER_UNRELATED_IO_STAGE_FAILED",
        )?;
        fs::rename(&held_context, &context).private("SECURITY_FIXTURE_IO")?;
        session.verify()
    })?;
    diagnostic("INFO", "SECURITY_MOCK_CLASSIFIER_NONCE_PROOF_FAULTS_PASS");
    let clean = fixture.repository("evaluator-types")?;
    let clean_commit = fixture.revision(&clean, "HEAD")?;
    evaluator(fixture, &clean, "evaluator-missing", |session| {
        require(
            !session.resolve(&clean_commit, 0)?,
            "SECURITY_MOCK_REAL_GIT_MISSING_PATH_FAILED",
        )
    })?;
    let path = clean.join(DOCUMENTS[0].path);
    fs::create_dir_all(path.parent().ok_or(ScanFailure("SECURITY_FIXTURE_IO"))?)
        .private("SECURITY_FIXTURE_IO")?;
    std::os::unix::fs::symlink("untrusted-target", &path).private("SECURITY_FIXTURE_IO")?;
    fixture.git(&clean, &["add", "."])?;
    fixture.git(&clean, &["commit", "--quiet", "-m", "Symlink evaluator"])?;
    let link_commit = fixture.revision(&clean, "HEAD")?;
    evaluator(fixture, &clean, "evaluator-link", |session| {
        require(
            !session.resolve(&link_commit, 0)?,
            "SECURITY_MOCK_SYMLINK_FAILED",
        )
    })?;
    fixture.git(&clean, &["rm", "--quiet", "--", DOCUMENTS[0].path])?;
    fixture.git(
        &clean,
        &[
            "update-index",
            "--add",
            "--cacheinfo",
            "160000",
            &clean_commit,
            DOCUMENTS[0].path,
        ],
    )?;
    fixture.git(&clean, &["commit", "--quiet", "-m", "Gitlink evaluator"])?;
    let gitlink = fixture.revision(&clean, "HEAD")?;
    evaluator(fixture, &clean, "evaluator-gitlink", |session| {
        require(
            !session.resolve(&gitlink, 0)?,
            "SECURITY_MOCK_GITLINK_FAILED",
        )
    })?;
    fixture.git(
        &clean,
        &["rm", "--cached", "--quiet", "--", DOCUMENTS[0].path],
    )?;
    fs::create_dir_all(&path).private("SECURITY_FIXTURE_IO")?;
    private::write(&path.join("child.txt"), b"Tree entry\n")?;
    fixture.git(&clean, &["add", "."])?;
    fixture.git(&clean, &["commit", "--quiet", "-m", "Tree evaluator"])?;
    let directory = fixture.revision(&clean, "HEAD")?;
    evaluator(fixture, &clean, "evaluator-tree", |session| {
        require(
            !session.resolve(&directory, 0)?,
            "SECURITY_MOCK_TREE_FAILED",
        )
    })?;
    let missing = "0".repeat(40);
    require(
        fixture
            .git_result(&clean, &["cat-file", "-t", &missing])?
            .completed_nonzero(),
        "SECURITY_MOCK_MISSING_OBJECT_SETUP_FAILED",
    )?;
    let result = evaluator(fixture, &clean, "evaluator-missing-object", |session| {
        session.resolve(&missing, 0).map(|_| ())
    });
    require(
        failed_at(&result, "SECURITY_CLASSIFIER_COMMIT_READ_FAILED"),
        "SECURITY_MOCK_MISSING_OBJECT_STAGE_FAILED",
    )?;
    // Prove a real loose blob exists, remove it, then require the exact read stage.
    let damaged = fixture.public_repository("evaluator-damaged", "Evaluator damaged object")?;
    let revision = fixture.revision(&damaged, "HEAD")?;
    let oid = fixture.revision(&damaged, &format!("HEAD:{}", DOCUMENTS[0].path))?;
    require(
        fixture.git(&damaged, &["cat-file", "-t", &oid])? == "blob\n",
        "SECURITY_MOCK_REMOVED_OBJECT_SETUP_FAILED",
    )?;
    fs::remove_file(damaged.join(".git/objects").join(&oid[..2]).join(&oid[2..]))
        .private("SECURITY_FIXTURE_IO")?;
    require(
        fixture
            .git_result(&damaged, &["cat-file", "-t", &oid])?
            .completed_nonzero(),
        "SECURITY_MOCK_REMOVED_OBJECT_FAILURE_NOT_PROVEN",
    )?;
    let result = evaluator(fixture, &damaged, "evaluator-removed-object", |session| {
        session.resolve(&revision, 0).map(|_| ())
    });
    require(
        failed_at(&result, "SECURITY_CLASSIFIER_BLOB_TYPE_READ_FAILED"),
        "SECURITY_MOCK_REMOVED_OBJECT_STAGE_FAILED",
    )?;
    for bytes in [
        b"bad".as_slice(),
        b"100644 blob x\twrong\0",
        b"100644 blob 0000000000000000000000000000000000000000\twrong\0",
    ] {
        require(
            failed_at(
                &classifier_git::tree_entry(bytes, 0),
                "SECURITY_CLASSIFIER_TREE_INVALID",
            ),
            "SECURITY_MOCK_TREE_GRAMMAR_FAILED",
        )?;
    }
    diagnostic("INFO", "SECURITY_MOCK_METADATA_ACTUAL_GIT_RESOLUTION_PASS");
    Ok(())
}

#[derive(Debug)]
enum AdmissionCase {
    Shallow,
    Grafts,
    Alternates,
    HttpAlternates,
    CommonDirectory,
    LooseReplacement,
    Promisor,
    IncludedConfig,
    PackedReplacement,
    SymlinkDirectory,
    SharedLimits,
}
impl AdmissionCase {
    fn enter(&self) {
        let _ = writeln!(std::io::stderr(), "{} [INFO] SECURITY_MOCK_ADMISSION_CASE case={self:?}", protocol::unix_millis().unwrap_or(0));
    }
}
fn admission(fixture: &Fixture) -> Result<()> {
    for (index, (case, forbidden)) in [
        (AdmissionCase::Shallow, "shallow"),
        (AdmissionCase::Grafts, "info/grafts"),
        (AdmissionCase::Alternates, "objects/info/alternates"),
        (AdmissionCase::HttpAlternates, "objects/info/http-alternates"),
        (AdmissionCase::CommonDirectory, "commondir"),
        (AdmissionCase::LooseReplacement, "refs/replace"),
    ]
    .iter()
    .enumerate()
    {
        case.enter();
        let repo = fixture.repository(&format!("admission-forbidden-{index}"))?;
        let target = repo.join(".git").join(forbidden);
        fs::create_dir_all(target.parent().ok_or(ScanFailure("SECURITY_FIXTURE_IO"))?)
            .private("SECURITY_FIXTURE_IO")?;
        // Empty and dangling entries are still rejected; never interpreted as absence.
        if index == 5 {
            std::os::unix::fs::symlink("absent", &target).private("SECURITY_FIXTURE_IO")?;
        } else {
            private::write(&target, b"")?;
        }
        require(
            failed_at(
                &evaluator(fixture, &repo, &format!("admission-proof-{index}"), |_| {
                    Ok(())
                }),
                "SECURITY_CLASSIFIER_REPOSITORY_UNSAFE",
            ),
            "SECURITY_MOCK_ADMISSION_FORBIDDEN_FAILED",
        )?;
    }
    AdmissionCase::Promisor.enter();
    let repo = fixture.repository("admission-promisor")?;
    private::write(&repo.join(".git/objects/pack/marker.promisor"), b"")?;
    require(
        failed_at(
            &evaluator(fixture, &repo, "admission-promisor-proof", |_| Ok(())),
            "SECURITY_CLASSIFIER_REPOSITORY_UNSAFE",
        ),
        "SECURITY_MOCK_PROMISOR_FAILED",
    )?;
    AdmissionCase::IncludedConfig.enter();
    let repo = fixture.repository("admission-include")?;
    private::write(
        &repo.join(".git/included-config"),
        b"[remote \"unused\"]\n promisor = false\n",
    )?;
    fixture.git(&repo, &["config", "include.path", "included-config"])?;
    require(
        failed_at(
            &evaluator(fixture, &repo, "admission-include-proof", |_| Ok(())),
            "SECURITY_CLASSIFIER_CONFIG_UNSAFE",
        ),
        "SECURITY_MOCK_INCLUDE_FAILED",
    )?;
    AdmissionCase::PackedReplacement.enter();
    let repo = fixture.repository("admission-replace")?;
    let revision = fixture.revision(&repo, "HEAD")?;
    fixture.git(
        &repo,
        &["update-ref", &format!("refs/replace/{revision}"), &revision],
    )?;
    let packed = fixture.git_success(
        &repo,
        &["--no-replace-objects", "pack-refs", "--all", "--prune"],
    )?;
    require(
        packed.completed() && packed.stdout.bytes.is_empty() && packed.stderr.bytes.is_empty(),
        "SECURITY_MOCK_PACKED_REPLACE_SETUP_FAILED",
    )?;
    let loose = repo.join(".git/refs/replace");
    if loose.exists() {
        fs::remove_dir(&loose).private("SECURITY_FIXTURE_IO")?;
    }
    require(
        matches!(fs::symlink_metadata(&loose), Err(error) if error.kind() == std::io::ErrorKind::NotFound),
        "SECURITY_MOCK_PACKED_REPLACE_SETUP_FAILED",
    )?;
    // One additional owned fixture child proves the real packed ref and value;
    // its private output is never published or substituted for admission proof.
    let present = fixture.git_success(
        &repo,
        &[
            "--no-replace-objects",
            "for-each-ref",
            "--format=%(objectname) %(refname)",
            "refs/replace/",
        ],
    )?;
    require(
        present.completed()
            && present.stderr.bytes.is_empty()
            && present.stdout.bytes == format!("{revision} refs/replace/{revision}\n").as_bytes(),
        "SECURITY_MOCK_PACKED_REPLACE_SETUP_FAILED",
    )?;
    diagnostic("INFO", "SECURITY_MOCK_PACKED_REPLACE_SETUP_PASS");
    require(
        failed_at(
            &evaluator(fixture, &repo, "admission-replace-proof", |_| Ok(())),
            "SECURITY_CLASSIFIER_REFS_UNSAFE",
        ),
        "SECURITY_MOCK_PACKED_REPLACE_FAILED",
    )?;
    AdmissionCase::SymlinkDirectory.enter();
    let repo = fixture.repository("admission-symlink")?;
    fs::rename(
        repo.join(".git/objects/info"),
        repo.join(".git/objects/real-info"),
    )
    .private("SECURITY_FIXTURE_IO")?;
    std::os::unix::fs::symlink("real-info", repo.join(".git/objects/info"))
        .private("SECURITY_FIXTURE_IO")?;
    require(
        failed_at(
            &evaluator(fixture, &repo, "admission-symlink-proof", |_| Ok(())),
            "SECURITY_CLASSIFIER_REPOSITORY_UNSAFE",
        ),
        "SECURITY_MOCK_ADMISSION_SYMLINK_FAILED",
    )?;
    // Shared aggregate boundary is exercised without running any child.
    AdmissionCase::SharedLimits.enter();
    let root = fixture.root.join("classifier-shared-cap");
    private::directory(&root)?;
    private::directory(&root.join("proof"))?;
    private::directory(&root.join("review-proof"))?;
    let payload = vec![b' '; private::RECORD_LIMIT as usize];
    for n in 0usize..256 {
        private::write(
            &root
                .join(if n.is_multiple_of(2) {
                    "proof"
                } else {
                    "review-proof"
                })
                .join(format!("{n}")),
            &payload,
        )?;
    }
    protocol::check_proof_budget(&root, 0, 0)?;
    require(
        failed_at(
            &protocol::check_proof_budget(&root, 1, 1),
            "SECURITY_PROOF_OVERFLOW",
        ),
        "SECURITY_MOCK_SHARED_PROOF_BYTES_FAILED",
    )?;
    protocol::check_proof_budget(&root, private::MAX_RECORDS - 256, 0)?;
    require(
        failed_at(
            &protocol::check_proof_budget(&root, private::MAX_RECORDS - 255, 0),
            "SECURITY_PROOF_OVERFLOW",
        ),
        "SECURITY_MOCK_SHARED_PROOF_FILES_FAILED",
    )?;
    private::write(
        &root.join("review-proof/oversize"),
        &vec![b' '; private::RECORD_LIMIT as usize + 1],
    )?;
    require(
        failed_at(
            &protocol::check_proof_budget(&root, 0, 0),
            "SECURITY_PROOF_INVALID",
        ),
        "SECURITY_MOCK_SHARED_RECORD_LIMIT_FAILED",
    )?;
    diagnostic(
        "INFO",
        "SECURITY_MOCK_ACTUAL_GIT_ADMISSION_AND_SHARED_LIMIT_PASS",
    );
    Ok(())
}

pub(super) fn qualify(fixture: &Fixture, repository: &Path, initial: &VerifiedScan) -> Result<()> {
    reviewed(initial, 5)?;
    require(
        initial.classifier_commands == 20 && initial.classifier_blob_reads == 4,
        "SECURITY_ACTUAL_CLASSIFIER_SHARED_READS_FAILED",
    )?;
    let summary = private::read(
        &fixture.root.join("public-documents-summary.md"),
        private::MIB,
    )?;
    require(
        contains(&summary, b"Total: **5**")
            && contains(&summary, b"Reviewed public data: **5**")
            && contains(&summary, b"Unreviewed: **0**")
            && !contains(&summary, b"No matching secrets"),
        "SECURITY_ACTUAL_REVIEWED_SUMMARY_FAILED",
    )?;
    diagnostic("INFO", "SECURITY_ACTUAL_REVIEWED_PUBLIC_DATA_PASS");
    pure(fixture.deadline)?;
    cli(fixture, repository, true)?;
    let other = fixture.public_repository(
        "public-different-commit",
        "Different introduced commit with identical public bytes",
    )?;
    require(
        fixture.revision(repository, "HEAD")? != fixture.revision(&other, "HEAD")?,
        "SECURITY_ACTUAL_NEW_COMMIT_NOT_DISTINCT",
    )?;
    reviewed(
        &fixture.scan(&other, &Scope::All, &fixture.root.join("other-summary.md"))?,
        5,
    )?;
    diagnostic("INFO", "SECURITY_ACTUAL_IDENTICAL_BLOB_NEW_COMMIT_PASS");
    let secret = synthetic_secret()?;
    private::write(
        &repository.join("runtime-secret.txt"),
        format!("token={secret}\n").as_bytes(),
    )?;
    fixture.git(repository, &["add", "."])?;
    fixture.git(
        repository,
        &["commit", "--quiet", "-m", "Private mixed fixture"],
    )?;
    let mixed = fixture.scan(
        repository,
        &Scope::All,
        &fixture.root.join("mixed-summary.md"),
    )?;
    require(
        mixed.reviewed_public_data_count == 5
            && mixed.unreviewed_count > 0
            && mixed.finding_count == mixed.reviewed_public_data_count + mixed.unreviewed_count
            && mixed.disposition == Disposition::UnreviewedFindings,
        "SECURITY_ACTUAL_MIXED_POLICY_FAILED",
    )?;
    require(
        !contains(
            &private::read(&fixture.root.join("mixed-summary.md"), private::MIB)?,
            secret.as_bytes(),
        ),
        "SECURITY_ACTUAL_MIXED_PRIVACY_FAILED",
    )?;
    cli(fixture, repository, false)?;
    diagnostic("INFO", "SECURITY_ACTUAL_MIXED_POLICY_PASS");
    variants(fixture)?;
    real_evaluator(fixture, &other)?;
    admission(fixture)?;
    diagnostic("INFO", "SECURITY_CLASSIFIER_QUALIFICATION_PASS");
    Ok(())
}
