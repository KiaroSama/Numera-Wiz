//! Qualification-only inputs. No environment/CLI switch selects a clock or origin.
use super::*;
use nw_security_scan::bootstrap::{self, FileFacts};
use std::os::unix::fs::{PermissionsExt, symlink};

fn expect<T>(result: Result<T>, category: &'static str) -> Result<()> {
    require(
        matches!(result, Err(error) if error.0 == category),
        "SECURITY_BOOTSTRAP_ORACLE_FAILED",
    )
}

pub fn cases(fixture: &Fixture) -> Result<()> {
    let deadline = fixture.deadline;
    let root = fixture.root.join("bootstrap-negative");
    private::directory(&root)?;
    let good = b"123.45 678.90\n";
    require(
        bootstrap::parse_origin(good)? == 123_450_000_000,
        "SECURITY_BOOTSTRAP_ORACLE_FAILED",
    )?;
    for bytes in [
        b"".as_slice(),
        b"1 2\n",
        b"1.2 3.45\n",
        b"-1.00 2.00\n",
        b"01.00 2.00\n",
        b"1.00 2.00",
        b"1.00 2.00\nextra",
        b"1.00  2.00\n",
        b"18446744073709551615.00 0.00\n",
    ] {
        expect(
            bootstrap::parse_origin(bytes),
            "SECURITY_BOOTSTRAP_MARKER_INVALID",
        )?;
    }
    expect(
        bootstrap::parse_origin(&[b'0'; 129]),
        "SECURITY_BOOTSTRAP_MARKER_INVALID",
    )?;
    let marker = root.join("marker");
    private::write(&marker, good)?;
    require(
        bootstrap::fixture_read_marker(&marker)? == good,
        "SECURITY_BOOTSTRAP_ORACLE_FAILED",
    )?;
    expect(
        bootstrap::fixture_read_marker(&root.join("missing")),
        "SECURITY_BOOTSTRAP_FILE_INVALID",
    )?;
    let link = root.join("symlink");
    symlink(&marker, &link).private("SECURITY_FIXTURE_IO")?;
    expect(
        bootstrap::fixture_read_marker(&link),
        "SECURITY_BOOTSTRAP_FILE_INVALID",
    )?;
    let fifo = root.join("fifo");
    rustix::fs::mknodat(
        rustix::fs::CWD,
        &fifo,
        rustix::fs::FileType::Fifo,
        rustix::fs::Mode::RUSR | rustix::fs::Mode::WUSR,
        0,
    )
    .private("SECURITY_FIXTURE_IO")?;
    expect(
        bootstrap::fixture_read_marker(&fifo),
        "SECURITY_BOOTSTRAP_FILE_INVALID",
    )?;
    fs::set_permissions(&marker, fs::Permissions::from_mode(0o644))
        .private("SECURITY_FIXTURE_IO")?;
    expect(
        bootstrap::fixture_read_marker(&marker),
        "SECURITY_BOOTSTRAP_FILE_INVALID",
    )?;
    fs::set_permissions(&marker, fs::Permissions::from_mode(0o600))
        .private("SECURITY_FIXTURE_IO")?;
    fs::hard_link(&marker, root.join("hardlink")).private("SECURITY_FIXTURE_IO")?;
    expect(
        bootstrap::fixture_read_marker(&marker),
        "SECURITY_BOOTSTRAP_FILE_INVALID",
    )?;
    fs::remove_file(root.join("hardlink")).private("SECURITY_FIXTURE_IO")?;
    let facts = FileFacts::from_metadata(&fs::metadata(&marker).private("SECURITY_FIXTURE_IO")?);
    let mut wrong_owner = facts;
    wrong_owner.uid = wrong_owner.uid.wrapping_add(1);
    expect(wrong_owner.marker(), "SECURITY_BOOTSTRAP_FILE_INVALID")?;
    let mut oversized = facts;
    oversized.len = 129;
    expect(oversized.marker(), "SECURITY_BOOTSTRAP_FILE_INVALID")?;
    let mut replaced = facts;
    replaced.ino = replaced.ino.wrapping_add(1);
    expect(facts.unchanged(replaced), "SECURITY_BOOTSTRAP_FILE_CHANGED")?;
    let held = fs::File::open(&marker).private("SECURITY_FIXTURE_IO")?;
    fs::rename(&marker, root.join("old-marker")).private("SECURITY_FIXTURE_IO")?;
    private::write(&marker, good)?;
    expect(
        FileFacts::from_metadata(&held.metadata().private("SECURITY_FIXTURE_IO")?).unchanged(
            FileFacts::from_metadata(&fs::metadata(&marker).private("SECURITY_FIXTURE_IO")?),
        ),
        "SECURITY_BOOTSTRAP_FILE_CHANGED",
    )?;
    let mut env_mode = facts;
    env_mode.mode = (env_mode.mode & !0o7777) | 0o644;
    env_mode.environment()?;
    env_mode.mode |= 0o020;
    expect(env_mode.environment(), "SECURITY_BOOTSTRAP_ENV_INVALID")?;
    let expected = serde_json::json!({"exported":1,"version":1});
    bootstrap::validate_receipt(br#"{"exported":1,"version":1}"#, &expected)?;
    for bytes in [
        b"".as_slice(),
        br#"{"exported":1,"exported":1,"version":1}"#,
        br#"{"exported":1e0,"version":1}"#,
        br#"{"exported":2,"version":1}"#,
        br#"{"exported":1,"version":1}[]"#,
    ] {
        expect(
            bootstrap::validate_receipt(bytes, &expected),
            "SECURITY_BOOTSTRAP_RECEIPT_INVALID",
        )?;
    }
    expect(
        bootstrap::validate_receipt(&vec![b' '; private::RECORD_LIMIT as usize + 1], &expected),
        "SECURITY_BOOTSTRAP_RECEIPT_INVALID",
    )?;
    for bytes in [
        b"bad".as_slice(),
        b"\xff\n",
        b"NW_SECURITY_DEADLINE_UNIX_MS=1\n",
    ] {
        expect(
            bootstrap::validate_env_bytes(bytes),
            "SECURITY_BOOTSTRAP_ENV_INVALID",
        )?;
    }
    require(
        bootstrap::validate_env_bytes(b"OTHER=value\n").is_ok()
            && bootstrap::validate_env_bytes(b"").is_ok(),
        "SECURITY_BOOTSTRAP_ORACLE_FAILED",
    )?;
    expect(
        bootstrap::validate_env_bytes(&vec![b'x'; private::MIB as usize + 1]),
        "SECURITY_BOOTSTRAP_ENV_INVALID",
    )?;
    deadline.check()?;
    diagnostic("INFO", "SECURITY_MOCK_BOOTSTRAP_FILE_BOUNDS_PASS");

    // Pure samples: frozen Instant, wall jumps, future/expired BOOT and checked overflow.
    let origin = 123_450_000_000;
    let end = bootstrap::stage_end(origin)?;
    require(
        bootstrap::evaluate_remaining(origin, end, origin, 9_000)? == 1_149_000,
        "SECURITY_BOOTSTRAP_ORACLE_FAILED",
    )?;
    expect(
        bootstrap::evaluate_remaining(origin, end, origin - 1, 9_000),
        "SECURITY_DEADLINE",
    )?;
    expect(
        bootstrap::evaluate_remaining(origin, end, end, 9_000),
        "SECURITY_DEADLINE",
    )?;
    expect(
        bootstrap::evaluate_remaining(origin, end, end + 1, 9_000),
        "SECURITY_DEADLINE",
    )?;
    expect(
        bootstrap::evaluate_remaining(origin, end, origin, u64::MAX),
        "SECURITY_DEADLINE",
    )?;
    expect(bootstrap::stage_end(u64::MAX), "SECURITY_DEADLINE")?;
    require(
        bootstrap::exact_export("1149000", 1_149_000).is_ok(),
        "SECURITY_BOOTSTRAP_ORACLE_FAILED",
    )?;
    for value in ["01149000", "+1149000", "1149000\n", "1149001", "1148999"] {
        expect(
            bootstrap::exact_export(value, 1_149_000),
            "SECURITY_BOOTSTRAP_ENV_INVALID",
        )?;
    }
    // Export equality is stored metadata: wall changes cannot change this comparison.
    require(
        bootstrap::evaluate_remaining(origin, end, origin, 1)?
            != bootstrap::evaluate_remaining(origin, end, origin, 9_000)?,
        "SECURITY_BOOTSTRAP_ORACLE_FAILED",
    )?;
    expect(
        bootstrap::evaluate_active(end, end, true),
        "SECURITY_DEADLINE",
    )?;
    expect(
        bootstrap::evaluate_active(end, origin, false),
        "SECURITY_DEADLINE",
    )?;
    let original = deadline.value();
    Deadline::from_value(&original)?.check()?;
    for field in ["boot", "namespace_dev", "namespace_ino"] {
        let mut changed = original.clone();
        changed["host"][field] = serde_json::json!("wrong identity");
        expect(
            Deadline::from_value(&changed),
            "SECURITY_BOOTSTRAP_HOST_INVALID",
        )?;
    }
    for field in [
        "end_ns",
        "origin",
        "stage_end",
        "run",
        "attempt",
        "receipt_dev",
        "receipt_ino",
        "exported",
    ] {
        for invalid in [
            serde_json::json!(-1),
            serde_json::json!("1"),
            serde_json::json!(true),
        ] {
            let mut changed = original.clone();
            changed[field] = invalid;
            expect(Deadline::from_value(&changed), "SECURITY_DEADLINE")?;
        }
    }
    for field in ["origin", "end_ns", "stage_end"] {
        let mut changed = original.clone();
        changed[field] = serde_json::json!(u64::MAX);
        expect(Deadline::from_value(&changed), "SECURITY_DEADLINE")?;
    }
    let mut expired = original.clone();
    expired["end_ns"] = serde_json::json!(0);
    expect(Deadline::from_value(&expired), "SECURITY_DEADLINE")?;
    diagnostic("INFO", "SECURITY_MOCK_BOOTSTRAP_CLOCK_BOUNDARIES_PASS");
    require(
        bootstrap::cleanup_end(end - 2_000_000_000, end)? == end
            && bootstrap::cleanup_end(end, end)? == end
            && bootstrap::cleanup_end(end + 1, end)? == end,
        "SECURITY_BOOTSTRAP_ORACLE_FAILED",
    )?;
    let retained = Scratch::create(&root)?;
    let retained_path = retained.path().to_owned();
    let barrier = process::barrier()?;
    expect(
        retained.remove(&barrier, deadline.cap_window(Duration::ZERO)?),
        "SECURITY_DEADLINE",
    )?;
    require(
        retained_path.is_dir(),
        "SECURITY_BOOTSTRAP_RETENTION_FAILED",
    )?;
    expect(bootstrap::cleanup_end(u64::MAX, end), "SECURITY_DEADLINE")?;
    diagnostic("INFO", "SECURITY_MOCK_BOOTSTRAP_CLEANUP_CAP_PASS");
    deadline.check()
}

pub fn copy_export(fixture: &Fixture, executable: &Path, label: &str) -> Result<(PathBuf, String)> {
    // Exactly two fixed callers: original reviewed-0 and unreviewed-1 CLI oracles.
    require(
        matches!(label, "reviewed" | "unreviewed"),
        "SECURITY_BOOTSTRAP_ORACLE_FAILED",
    )?;
    let root = fixture.root.join(format!("bootstrap-copy-{label}"));
    private::directory(&root)?;
    fixture.stage.fixture_copy(&root)?;
    let env_file = root.join("private-env");
    private::write(&env_file, b"")?;
    let mut command = Command::new(executable);
    private::controlled(&mut command, &root);
    command
        .current_dir(&root)
        .arg("budget-from-marker")
        .env("GITHUB_ACTIONS", "true")
        .env("GITHUB_ENV", &env_file);
    fixture.stage.configure(&mut command, &root);
    let result = process::run(
        &mut command,
        Options::command(Duration::from_secs(15), fixture.deadline),
        || Ok(()),
    )?;
    require(
        result.completed()
            && result.status == Some(0)
            && result.stdout.quiet()
            && contains(&result.stderr.bytes, b"SECURITY_BOOTSTRAP_BUDGET_EXPORTED")
            && !contains(&result.stderr.bytes, b"SECURITY_SCAN_CLEAN"),
        "SECURITY_BOOTSTRAP_COPY_EXPORT_FAILED",
    )?;
    let bytes = private::read_bounded(&env_file, 128, fixture.deadline)?;
    let text = std::str::from_utf8(&bytes).private("SECURITY_BOOTSTRAP_COPY_EXPORT_FAILED")?;
    let exported = text
        .strip_prefix("NW_SECURITY_DEADLINE_UNIX_MS=")
        .and_then(|s| s.strip_suffix('\n'))
        .ok_or(ScanFailure("SECURITY_BOOTSTRAP_COPY_EXPORT_FAILED"))?;
    let number: u64 = exported
        .parse()
        .private("SECURITY_BOOTSTRAP_COPY_EXPORT_FAILED")?;
    bootstrap::exact_export(exported, number)?;
    diagnostic("INFO", "SECURITY_FIXTURE_BOOTSTRAP_COPY_PASS");
    Ok((root, exported.to_owned()))
}

pub fn real_consumer(fixture: &Fixture) -> Result<()> {
    fixture.deadline.check()?;
    let current = fs::canonicalize(env::current_exe().private("SECURITY_FIXTURE_IO")?)
        .private("SECURITY_FIXTURE_IO")?;
    let executable = current
        .parent()
        .ok_or(ScanFailure("SECURITY_FIXTURE_IO"))?
        .join("nw-security-scan");
    require(
        fs::canonicalize(&executable).private("SECURITY_FIXTURE_CLI_IDENTITY")? == executable,
        "SECURITY_FIXTURE_CLI_IDENTITY",
    )?;
    let identity = private::executable_hash_bounded(&executable, fixture.deadline)?;
    let summary = fixture.root.join("bootstrap-real-summary.md");
    let mut command = Command::new(&executable);
    private::controlled(&mut command, &fixture.root);
    command
        .current_dir(&fixture.root)
        .arg("scan")
        .arg(fixture.root.join("repository"))
        .env("GITHUB_ACTIONS", "true")
        .env("GITHUB_EVENT_NAME", "workflow_dispatch")
        .env("GITHUB_STEP_SUMMARY", &summary);
    fixture.stage.configure_original(&mut command);
    let result = process::run(
        &mut command,
        Options::command(Duration::from_secs(930), fixture.deadline),
        || Ok(()),
    )?;
    require(
        result.completed()
            && result.status == Some(0)
            && result.stdout.quiet()
            && contains(&result.stderr.bytes, b"SECURITY_SCAN_CLEAN")
            && private::executable_hash_bounded(&executable, fixture.deadline)? == identity
            && private::read_bounded(&summary, private::MIB, fixture.deadline)?
                == EXPECTED_SUMMARY.as_bytes(),
        "SECURITY_BOOTSTRAP_REAL_CONSUMER_FAILED",
    )?;
    fixture.stage.verify_consumed()?;
    diagnostic("INFO", "SECURITY_ACTUAL_BOOTSTRAP_REAL_MARKER_SCAN_PASS");
    fixture.deadline.check()
}
