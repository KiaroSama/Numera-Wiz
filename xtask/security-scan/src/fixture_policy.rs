//! Labeled evaluator fixtures; these do not substitute for actual Git/scanner cases.
use super::*;
use nw_security_scan::report;
use serde_json::{Value, json};
use std::io::Cursor;

pub fn qualify(root: &Path, deadline: Instant) -> Result<()> {
    reports(root)?;
    unknown_unicode()?;
    archives(deadline)?;
    proofs(root)?;
    failure_stage_controls(root, deadline)?;
    diagnostic("INFO", "SECURITY_MOCK_POLICY_ARCHIVE_REPORT_PROOF_PASS");
    Ok(())
}
fn unknown_unicode() -> Result<()> {
    // These unknown values used to take IgnoredAny's weaker string path.
    let malformed: &[&[u8]] = &[
        b"{\"Secret\":\"\xff\"}",
        b"{\"Extra\":{\"\xff\":\"private\"}}",
        b"{\"Extra\":[{\"key\":\"\xff\"}]}",
        br#"{"Secret":"\uD800"}"#,
        br#"{"Match":"\uDC00"}"#,
        br#"{"Extra":{"\uD800":"private"}}"#,
        br#"{"Extra":[{"key":"\uDC00"}]}"#,
        br#"{"Secret":"\uD800\u0041"}"#,
    ];
    for row in malformed {
        for preceding in [0, 200] {
            let mut bytes = b"[".to_vec();
            for _ in 0..preceding {
                bytes.extend_from_slice(b"{},");
            }
            bytes.extend_from_slice(row);
            bytes.push(b']');
            require(
                matches!(
                    report::parse(&bytes),
                    Err(ScanFailure("SECURITY_REPORT_INVALID"))
                ),
                "SECURITY_MOCK_UNKNOWN_UNICODE_ACCEPTED",
            )?;
            require(matches!(report::visit_rows(&bytes, |_| Ok(())),Err(ScanFailure("SECURITY_REPORT_INVALID"))),
                "SECURITY_MOCK_SECOND_PASS_UNICODE_ACCEPTED")?;
        }
    }
    let valid = br#"{"Secret":"\uD83D\uDE00","Extra":{"\uD83D\uDE00":[null,true,false,-1,18446744073709551616,1e400,{"private":"\u0061"}]}}"#;
    for preceding in [0, 200] {
        let mut bytes = b"[".to_vec();
        for _ in 0..preceding {
            bytes.extend_from_slice(b"{},");
        }
        bytes.extend_from_slice(valid);
        bytes.push(b']');
        let parsed = report::parse(&bytes)?;
        require(report::visit_rows(&bytes, |_| Ok(()))? == parsed.count,
            "SECURITY_MOCK_SECOND_PASS_COUNT_FAILED")?;
        require(
            parsed.count == preceding + 1 && parsed.rows.len() == (preceding + 1).min(200),
            "SECURITY_MOCK_VALID_UNKNOWN_UNICODE_REJECTED",
        )?;
        let summary = report::summary(&parsed, false);
        require(
            !summary.contains('😀') && !summary.contains("private"),
            "SECURITY_MOCK_UNKNOWN_UNICODE_PUBLISHED",
        )?;
    }
    diagnostic("INFO", "SECURITY_MOCK_FULL_UNICODE_VALIDATION_PASS");
    Ok(())
}
fn reports(root: &Path) -> Result<()> {
    let empty = report::parse(b"[]")?;
    require(
        empty.count == 0 && report::summary(&empty, false) == EXPECTED_SUMMARY,
        "SECURITY_MOCK_EMPTY_SUMMARY",
    )?;
    for malformed in [
        "",
        "null",
        "{}",
        "[",
        "[{}",
        "[[]]",
        "[1]",
        "[null]",
        "[]false",
        "[{\"RuleID\":null}]",
        "[{\"File\":false}]",
        "[{\"Commit\":1}]",
        "[{\"StartLine\":\"1\"}]",
        "[{\"StartLine\":-1}]",
        "[{\"StartLine\":1.5}]",
        "[{\"StartLine\":1e0}]",
        "[{\"StartLine\":18446744073709551616}]",
        "[{\"StartLine\":1e400}]",
        "[{\"StartLine\":{\"$serde_json::private::Number\":\"1\"}}]",
        "[{\"RuleID\":\"a\",\"RuleID\":\"b\"}]",
    ] {
        require(
            report::parse(malformed.as_bytes()).is_err(),
            "SECURITY_MOCK_REPORT_ACCEPTED",
        )?;
    }
    let missing = report::parse(b"[{}]")?;
    require(
        missing.count == 1 && missing.rows[0].line.is_empty(),
        "SECURITY_MOCK_MISSING_FIELDS",
    )?;
    require(
        report::parse(b"[{\"StartLine\":18446744073709551615}]")?.rows[0].line
            == "18446744073709551615",
        "SECURITY_MOCK_INTEGER_PRECISION_LOST",
    )?;
    require(
        report::parse(b"[{\"RuleID\":\"\xff\"}]").is_err()
            && private::read(&root.join("absent-report.json"), private::REPORT_LIMIT).is_err(),
        "SECURITY_MOCK_REPORT_INVALID",
    )?;
    require(
        matches!(
            report::parse(&vec![b' '; private::REPORT_LIMIT as usize + 1]),
            Err(ScanFailure("SECURITY_REPORT_OVERSIZE"))
        ),
        "SECURITY_MOCK_REPORT_SIZE_ACCEPTED",
    )?;
    for (status, findings, history, quiet) in [
        (Some(2), &empty, 1, true),
        (Some(0), &missing, 1, true),
        (Some(1), &empty, 1, true),
        (None, &empty, 1, true),
        (Some(0), &empty, 0, true),
        (Some(0), &empty, 1, false),
    ] {
        require(
            report::agreement(status, findings, history, quiet).is_err(),
            "SECURITY_MOCK_AGREEMENT_ACCEPTED",
        )?;
    }
    report::agreement(Some(0), &empty, 1, true)?;
    report::agreement(Some(2), &missing, 1, true)?;
    let secret = super::synthetic_secret()?;
    let canary = private::nonce()?;
    let edge = "&<>\"'|\r\n日本語 فارسی";
    let escaped = "&amp;&lt;&gt;&quot;&#x27;&#124;  日本語 فارسی";
    require(report::escape(edge) == escaped, "SECURITY_MOCK_ESCAPE")?;
    let mut rows = Vec::new();
    for index in 0..201 {
        rows.push(
            json!({"RuleID":edge, "File":edge, "StartLine":index, "Commit":edge,
            "Secret":secret, "Match":secret, "Message":canary}),
        );
    }
    let encoded = serde_json::to_vec(&rows).private("SECURITY_FIXTURE_IO")?;
    let parsed = report::parse(&encoded)?;
    let summary = report::summary(&parsed, true);
    require(
        parsed.count == 201
            && parsed.rows.len() == 200
            && parsed.rows[199].line == "199"
            && summary.contains("Showing 200 of 201 findings.")
            && summary.contains(escaped)
            && !summary.contains("<code>200</code>")
            && !summary.contains(&secret)
            && !summary.contains(&canary),
        "SECURITY_MOCK_SUMMARY_PRIVACY",
    )?;
    // Invalid row beyond the display limit must still be parsed and rejected.
    rows[200]["StartLine"] = json!(false);
    require(
        report::parse(&serde_json::to_vec(&rows).private("SECURITY_FIXTURE_IO")?).is_err(),
        "SECURITY_MOCK_REPORT_TAIL_SKIPPED",
    )?;
    report::write_summary(None, &empty, false)?;
    report::write_summary(Some(Path::new("")), &empty, false)?;
    require(
        report::write_summary(Some(root), &empty, false).is_err(),
        "SECURITY_MOCK_SUMMARY_WRITE_ACCEPTED",
    )?;
    let path = root.join("mock-summary.md");
    private::write(&path, b"prefix\n")?;
    report::write_summary(Some(&path), &empty, false)?;
    require(
        private::read(&path, private::MIB)? == format!("prefix\n{EXPECTED_SUMMARY}").as_bytes(),
        "SECURITY_MOCK_SUMMARY_APPEND",
    )?;
    let mut capture = process::Capture::default();
    capture.consume(&vec![b' '; private::CAPTURE_LIMIT]);
    capture.consume(b"x");
    capture.consume(b" \n");
    require(
        capture.nonwhite
            && capture.overflow
            && !capture.quiet()
            && capture.bytes.len() == private::CAPTURE_LIMIT,
        "SECURITY_MOCK_CAPTURE_STICKY",
    )?;
    let mut whitespace = process::Capture::default();
    whitespace.consume(&vec![b' '; private::CAPTURE_LIMIT + 1]);
    require(
        whitespace.overflow && !whitespace.nonwhite && !whitespace.quiet(),
        "SECURITY_MOCK_CAPTURE_OVERFLOW",
    )?;
    let failed = process::Capture {
        failed: true,
        ..process::Capture::default()
    };
    require(!failed.quiet(), "SECURITY_MOCK_CAPTURE_ERROR")?;
    Ok(())
}
fn archive(entries: &[(&str, u8, &[u8])]) -> Result<Vec<u8>> {
    let mut builder = tar::Builder::new(Vec::new());
    for (path, kind, bytes) in entries {
        let mut header = tar::Header::new_ustar();
        header.set_path(path).private("SECURITY_FIXTURE_ARCHIVE")?;
        header.set_entry_type(tar::EntryType::new(*kind));
        header.set_mode(0o700);
        header.set_size(bytes.len() as u64);
        header.set_cksum();
        builder
            .append(&header, Cursor::new(bytes))
            .private("SECURITY_FIXTURE_ARCHIVE")?;
    }
    let bytes = builder.into_inner().private("SECURITY_FIXTURE_ARCHIVE")?;
    gzip(&bytes)
}
fn gzip(bytes: &[u8]) -> Result<Vec<u8>> {
    let mut gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    gzip.write_all(bytes).private("SECURITY_FIXTURE_ARCHIVE")?;
    gzip.finish().private("SECURITY_FIXTURE_ARCHIVE")
}
fn archives(deadline: Instant) -> Result<()> {
    let good = archive(&[("gitleaks", b'0', b"synthetic executable bytes")])?;
    require(
        assets::inspect_archive(&good, deadline)? == b"synthetic executable bytes"
            && assets::verify_pinned(&good, deadline).is_err(),
        "SECURITY_MOCK_ARCHIVE_PIN",
    )?;
    for entries in [
        vec![("other", b'0', b"missing".as_slice())],
        vec![("gitleaks", b'0', b"".as_slice())],
        vec![
            ("gitleaks", b'0', b"a".as_slice()),
            ("gitleaks", b'0', b"b".as_slice()),
        ],
        vec![
            ("gitleaks", b'0', b"a".as_slice()),
            ("link", b'2', b"".as_slice()),
        ],
        vec![
            ("gitleaks", b'0', b"a".as_slice()),
            ("sparse", b'S', b"".as_slice()),
        ],
        vec![
            ("gitleaks", b'0', b"a".as_slice()),
            ("metadata", b'x', b"".as_slice()),
        ],
    ] {
        require(
            assets::inspect_archive(&archive(&entries)?, deadline).is_err(),
            "SECURITY_MOCK_ARCHIVE_ACCEPTED",
        )?;
    }
    // Mutate a library-built header, then recompute checksum through the library.
    for invalid in ["../escape", "/absolute", "a/./b", "a//b", "bad\\path", ""] {
        let mut header = tar::Header::new_ustar();
        header
            .set_path("gitleaks")
            .private("SECURITY_FIXTURE_ARCHIVE")?;
        header.set_entry_type(tar::EntryType::Regular);
        header.set_size(1);
        header.set_mode(0o700);
        let raw = header.as_mut_bytes();
        raw[..100].fill(0);
        raw[..invalid.len()].copy_from_slice(invalid.as_bytes());
        header.set_cksum();
        let mut raw = header.as_bytes().to_vec();
        raw.push(b'x');
        raw.resize(2048, 0);
        require(
            assets::inspect_archive(&gzip(&raw)?, deadline).is_err(),
            "SECURITY_MOCK_ARCHIVE_PATH_ACCEPTED",
        )?;
    }
    let names = (0..129).map(|i| format!("member{i}")).collect::<Vec<_>>();
    let mut many = names
        .iter()
        .map(|n| (n.as_str(), b'0', b"a".as_slice()))
        .collect::<Vec<_>>();
    many[0].0 = "gitleaks";
    require(
        assets::inspect_archive(&archive(&many)?, deadline)
            == Err(ScanFailure("SECURITY_ARCHIVE_MEMBER_LIMIT")),
        "SECURITY_MOCK_ARCHIVE_COUNT_ACCEPTED",
    )?;
    let mut bad_gzip = good.clone();
    bad_gzip.push(b'x');
    require(
        assets::inspect_archive(&bad_gzip, deadline).is_err(),
        "SECURITY_MOCK_ARCHIVE_TAIL_ACCEPTED",
    )?;
    for (size, kind, expected) in [
        (
            128 * private::MIB + 1,
            b'0',
            "SECURITY_ARCHIVE_EXECUTABLE_INVALID",
        ),
        (1, b'1', "SECURITY_ARCHIVE_TYPE_INVALID"),
        (1, b'6', "SECURITY_ARCHIVE_TYPE_INVALID"),
    ] {
        let mut header = tar::Header::new_ustar();
        header
            .set_path("gitleaks")
            .private("SECURITY_FIXTURE_ARCHIVE")?;
        header.set_entry_type(tar::EntryType::new(kind));
        header.set_size(size);
        header.set_mode(0o700);
        header.set_cksum();
        require(
            assets::inspect_archive(&gzip(header.as_bytes())?, deadline)
                == Err(ScanFailure(expected)),
            "SECURITY_MOCK_EXECUTABLE_SIZE_TYPE_ACCEPTED",
        )?;
    }
    let deep = "a/".repeat(16) + "gitleaks";
    require(
        assets::inspect_archive(&archive(&[(&deep, b'0', b"x")])?, deadline)
            == Err(ScanFailure("SECURITY_ARCHIVE_PATH_INVALID")),
        "SECURITY_MOCK_PATH_DEPTH_ACCEPTED",
    )?;
    require(
        assets::inspect_archive(&vec![0; (64 * private::MIB + 1) as usize], deadline)
            == Err(ScanFailure("SECURITY_ARCHIVE_OVERSIZE")),
        "SECURITY_MOCK_DOWNLOAD_SIZE_ACCEPTED",
    )?;
    let mut inflated = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    let block = [0; 64 * 1024];
    for _ in 0..4097 {
        require(Instant::now() < deadline, "SECURITY_DEADLINE")?;
        inflated
            .write_all(&block)
            .private("SECURITY_FIXTURE_ARCHIVE")?;
    }
    require(
        assets::inspect_archive(
            &inflated.finish().private("SECURITY_FIXTURE_ARCHIVE")?,
            deadline,
        ) == Err(ScanFailure("SECURITY_ARCHIVE_OVERSIZE")),
        "SECURITY_MOCK_DECODED_LIMIT_ACCEPTED",
    )?;
    assets::verify_version(Some(0), b"8.30.1\n", true)?;
    for (status, bytes, quiet) in [
        (Some(0), b"8.30.0\n".as_slice(), true),
        (Some(2), b"8.30.1".as_slice(), true),
        (Some(0), b"8.30.1".as_slice(), false),
        (Some(0), b"8.30.1\nextra".as_slice(), true),
    ] {
        require(
            assets::verify_version(status, bytes, quiet).is_err(),
            "SECURITY_MOCK_VERSION_ACCEPTED",
        )?;
    }
    Ok(())
}
fn proofs(root: &Path) -> Result<()> {
    let root = root.join("mock-proof");
    private::directory(&root)?;
    private::directory(&root.join("proof"))?;
    let context = protocol::Context {
        root: root.clone(),
        value: json!({"run":private::nonce()?,
        "repository":"/fixed-synthetic/.git", "scope":"--all"}),
    };
    protocol::write_json(&root.join("context.json"), &context.value)?;
    protocol::write_json(
        &root.join("group.json"),
        &json!({"run":context.value["run"],"group":123}),
    )?;
    let args = protocol::history_args("/fixed-synthetic/.git", "--all");
    let id = record(&context, &args, "history", 0, false)?;
    require(
        protocol::verify_proofs(&context, 123)?.history == 1,
        "SECURITY_MOCK_PROOF_VALID",
    )?;
    invocation_identity(&context, &args, &id)?;
    proof_integer_bounds(&context, &id)?;
    history_failure_proof(&context, &args, &id)?;
    for wrong in [
        vec![
            "ls-remote",
            "--quiet",
            "--get-url",
            "https://example.invalid",
        ],
        vec!["ls-remote", "--quiet"],
        vec!["fetch"],
        vec!["-C", "/other", "log"],
    ] {
        require(
            protocol::classify(
                &wrong.into_iter().map(str::to_owned).collect::<Vec<_>>(),
                "/fixed-synthetic/.git",
                "--all",
            )
            .is_err(),
            "SECURITY_MOCK_ARGV_ACCEPTED",
        )?;
    }
    for index in 0..args.len() {
        let mut changed = args.clone();
        changed.remove(index);
        require(
            protocol::classify(&changed, "/fixed-synthetic/.git", "--all").is_err(),
            "SECURITY_MOCK_ARGV_ACCEPTED",
        )?;
    }
    for oversize in [
        vec!["x".to_owned(); 129],
        vec!["x".repeat(4097)],
        vec!["x".repeat(4096); 5],
    ] {
        require(
            protocol::classify(&oversize, "/fixed-synthetic/.git", "--all").is_err(),
            "SECURITY_MOCK_ARGV_BOUND_ACCEPTED",
        )?;
    }
    let duplicate = root.join("duplicate.json");
    private::write(&duplicate, b"{\"run\":1,\"run\":2}")?;
    require(
        protocol::read_json(&duplicate).is_err(),
        "SECURITY_MOCK_DUPLICATE_KEYS_ACCEPTED",
    )?;
    let done = root.join("proof").join(format!("{id}.done"));
    let valid = protocol::read_json(&done)?;
    for (key, value) in [
        ("accepted", json!(false)),
        ("status", json!(1)),
        ("start", json!("stale")),
        ("no_remote", json!(true)),
    ] {
        let mut invalid = valid.clone();
        invalid[key] = value;
        replace(&done, &invalid)?;
        require(
            protocol::verify_proofs(&context, 123).is_err(),
            "SECURITY_MOCK_PROOF_ACCEPTED",
        )?;
    }
    replace(&done, &valid)?;
    require(
        protocol::verify_proofs(&context, 124).is_err(),
        "SECURITY_MOCK_STALE_GROUP_ACCEPTED",
    )?;
    fs::remove_file(&done).private("SECURITY_FIXTURE_IO")?;
    require(
        protocol::verify_proofs(&context, 123).is_err(),
        "SECURITY_MOCK_MISSING_PROOF_ACCEPTED",
    )?;
    protocol::write_json(&done, &valid)?;
    let metadata = ["ls-remote", "--quiet", "--get-url"].map(str::to_owned);
    let mid = record(&context, &metadata, "metadata", 128, true)?;
    require(
        protocol::verify_proofs(&context, 123)?.metadata == 1,
        "SECURITY_MOCK_METADATA_VALID",
    )?;
    let metadata_done = root.join("proof").join(format!("{mid}.done"));
    let valid_metadata = protocol::read_json(&metadata_done)?;
    let mut unexpected_metadata = valid_metadata.clone();
    unexpected_metadata["no_remote"] = json!(false);
    replace(&metadata_done, &unexpected_metadata)?;
    require(
        protocol::verify_proofs(&context, 123).is_err(),
        "SECURITY_MOCK_METADATA_FAILURE_HIDDEN",
    )?;
    replace(&metadata_done, &valid_metadata)?;
    for suffix in ["attempt", "start", "done"] {
        fs::remove_file(root.join("proof").join(format!("{id}.{suffix}")))
            .private("SECURITY_FIXTURE_IO")?;
    }
    require(
        protocol::verify_proofs(&context, 123).is_err(),
        "SECURITY_MOCK_METADATA_COUNTED_HISTORY",
    )?;
    for suffix in ["attempt", "start", "done"] {
        fs::remove_file(root.join("proof").join(format!("{mid}.{suffix}")))
            .private("SECURITY_FIXTURE_IO")?;
    }
    // Orphans must not pass a mere file-count multiple-of-three test.
    for _ in 0..3 {
        protocol::write_json(
            &root
                .join("proof")
                .join(format!("{}.start", private::nonce()?)),
            &json!({}),
        )?;
    }
    require(
        protocol::verify_proofs(&context, 123).is_err(),
        "SECURITY_MOCK_ORPHAN_PROOF_ACCEPTED",
    )?;
    for path in protocol::proof_files(&root.join("proof"))?.values() {
        fs::remove_file(path).private("SECURITY_FIXTURE_IO")?;
    }
    for _ in 0..256 {
        record(&context, &args, "history", 0, false)?;
    }
    require(
        protocol::verify_proofs(&context, 123)?.history == 256,
        "SECURITY_MOCK_HISTORY_BOUND",
    )?;
    record(&context, &args, "history", 0, false)?;
    require(
        protocol::verify_proofs(&context, 123).is_err(),
        "SECURITY_MOCK_HISTORY_OVERFLOW_ACCEPTED",
    )?;
    private::write(
        &root.join("proof").join("oversize"),
        &vec![0; private::RECORD_LIMIT as usize + 1],
    )?;
    require(
        protocol::proof_files(&root.join("proof")).is_err(),
        "SECURITY_MOCK_RECORD_OVERFLOW_ACCEPTED",
    )?;
    fs::remove_file(root.join("proof/oversize")).private("SECURITY_FIXTURE_IO")?;
    for path in protocol::proof_files(&root.join("proof"))?.values() {
        fs::remove_file(path).private("SECURITY_FIXTURE_IO")?;
    }
    let bytes = vec![b' '; private::RECORD_LIMIT as usize];
    for i in 0..257 {
        private::write(&root.join("proof").join(format!("aggregate-{i}")), &bytes)?;
    }
    require(
        protocol::proof_files(&root.join("proof")).is_err(),
        "SECURITY_MOCK_PROOF_AGGREGATE_ACCEPTED",
    )?;
    Ok(())
}
fn replace(path: &Path, value: &Value) -> Result<()> {
    fs::remove_file(path).private("SECURITY_FIXTURE_IO")?;
    protocol::write_json(path, value)
}
fn record(
    context: &protocol::Context,
    args: &[String],
    kind: &str,
    status: i32,
    no_remote: bool,
) -> Result<String> {
    let id = private::nonce()?;
    let attempt = json!({"invocation":id,"run":context.value["run"],"context":private::hash(&private::read(&context.root.join("context.json"), private::RECORD_LIMIT)?),
        "group":123,"kind":"rejected","argv":[]});
    let mut start = attempt.clone();
    start["kind"] = json!(kind);
    start["argv"] = json!(args);
    let proof = context.root.join("proof");
    protocol::write_json(&proof.join(format!("{id}.attempt")), &attempt)?;
    protocol::write_json(&proof.join(format!("{id}.start")), &start)?;
    protocol::write_json(
        &proof.join(format!("{id}.done")),
        &json!({"invocation":id,"accepted":true,"completed":true,"stderr_quiet":!no_remote,"status":status,"no_remote":no_remote,
        "start":private::hash(&private::read(&proof.join(format!("{id}.start")), private::RECORD_LIMIT)?)}),
    )?;
    Ok(id)
}
fn invocation_identity(context: &protocol::Context, args: &[String], first: &str) -> Result<()> {
    let proof = context.root.join("proof");
    let renamed = private::nonce()?;
    for suffix in ["attempt", "start", "done"] {
        fs::rename(
            proof.join(format!("{first}.{suffix}")),
            proof.join(format!("{renamed}.{suffix}")),
        )
        .private("SECURITY_FIXTURE_IO")?;
    }
    require(
        protocol::verify_proofs(context, 123).is_err(),
        "SECURITY_MOCK_RENAMED_INVOCATION_ACCEPTED",
    )?;
    for suffix in ["attempt", "start", "done"] {
        fs::rename(
            proof.join(format!("{renamed}.{suffix}")),
            proof.join(format!("{first}.{suffix}")),
        )
        .private("SECURITY_FIXTURE_IO")?;
    }
    for suffix in ["attempt", "start", "done"] {
        private::write(
            &proof.join(format!("{renamed}.{suffix}")),
            &private::read(
                &proof.join(format!("{first}.{suffix}")),
                private::RECORD_LIMIT,
            )?,
        )?;
    }
    require(
        protocol::verify_proofs(context, 123).is_err(),
        "SECURITY_MOCK_COPIED_INVOCATION_ACCEPTED",
    )?;
    for suffix in ["attempt", "start", "done"] {
        fs::remove_file(proof.join(format!("{renamed}.{suffix}")))
            .private("SECURITY_FIXTURE_IO")?;
    }
    let second = record(context, args, "history", 0, false)?;
    require(
        protocol::verify_proofs(context, 123)?.history == 2,
        "SECURITY_MOCK_IDENTICAL_ARGV_INVOCATIONS_REJECTED",
    )?;
    for suffix in ["attempt", "start", "done"] {
        let target = proof.join(format!("{first}.{suffix}"));
        let original = protocol::read_json(&target)?;
        replace(
            &target,
            &protocol::read_json(&proof.join(format!("{second}.{suffix}")))?,
        )?;
        require(
            protocol::verify_proofs(context, 123).is_err(),
            "SECURITY_MOCK_MIXED_INVOCATIONS_ACCEPTED",
        )?;
        replace(&target, &original)?;
    }
    for suffix in ["attempt", "start", "done"] {
        fs::remove_file(proof.join(format!("{second}.{suffix}"))).private("SECURITY_FIXTURE_IO")?;
    }
    require(
        protocol::verify_proofs(context, 123)?.history == 1,
        "SECURITY_MOCK_INVOCATION_RESTORE_FAILED",
    )?;
    diagnostic("INFO", "SECURITY_MOCK_INVOCATION_NONCE_CHAIN_PASS");
    Ok(())
}
fn failure_stage_controls(root: &Path, deadline: Instant) -> Result<()> {
    let empty = report::parse(b"[]")?;
    let controls = [
        (
            protocol::deadline_from(&json!({"deadline_ms":0})).map(|_| ()),
            "SECURITY_DEADLINE",
        ),
        (
            private::read(&root.join("absent-setup"), 16).map(|_| ()),
            "SECURITY_PRIVATE_IO_FAILED",
        ),
        (
            assets::verify_pinned(b"private invalid archive", deadline).map(|_| ()),
            "SECURITY_ARCHIVE_HASH_INVALID",
        ),
        (
            report::write_summary(Some(root), &empty, false),
            "SECURITY_SUMMARY_WRITE_FAILED",
        ),
    ];
    for (result, expected) in controls {
        require(
            failed_at(&result, expected)
                && !failed_at(&result, "SECURITY_HISTORY_GIT_FAILED")
                && !failed_at(&result, "SECURITY_REVISION_COMMIT_CHECK_FAILED"),
            "SECURITY_MOCK_INFRA_MATCHED_GIT_ORACLE",
        )?;
    }
    let mut outcome = process::Outcome {
        leader: 123,
        status: Some(128),
        stdout: process::Capture::default(),
        stderr: process::Capture::default(),
        failure: None,
        reaped: 1,
        adopted: 0,
        barrier: process::barrier()?,
        cleanup_elapsed: Duration::ZERO,
    };
    require(
        outcome.completed_nonzero(),
        "SECURITY_MOCK_NONZERO_COMPLETION_REJECTED",
    )?;
    for status in [None, Some(0), Some(-1), Some(256)] {
        outcome.status = status;
        require(
            !outcome.completed_nonzero(),
            "SECURITY_MOCK_INCOMPLETE_MATCHED_GIT_ORACLE",
        )?;
    }
    outcome.status = Some(128);
    outcome.failure = Some(ScanFailure("SECURITY_CHILD_TIMEOUT"));
    require(
        !outcome.completed_nonzero(),
        "SECURITY_MOCK_TIMEOUT_MATCHED_GIT_ORACLE",
    )?;
    outcome.failure = None;
    outcome.stderr.overflow = true;
    require(
        !outcome.completed_nonzero(),
        "SECURITY_MOCK_CAPTURE_MATCHED_GIT_ORACLE",
    )?;
    outcome.stderr.overflow = false;
    outcome.stdout.failed = true;
    require(
        !outcome.completed_nonzero(),
        "SECURITY_MOCK_CAPTURE_MATCHED_GIT_ORACLE",
    )?;
    outcome.stdout.failed = false;
    outcome.adopted = 1;
    require(
        !outcome.completed_nonzero(),
        "SECURITY_MOCK_ADOPTION_MATCHED_GIT_ORACLE",
    )?;
    outcome.adopted = 0;
    outcome.reaped = 0;
    require(
        !outcome.completed_nonzero(),
        "SECURITY_MOCK_UNWAITED_MATCHED_GIT_ORACLE",
    )?;
    diagnostic("INFO", "SECURITY_MOCK_FAILURE_STAGE_CONTROLS_PASS");
    Ok(())
}
fn proof_integer_bounds(context: &protocol::Context, id: &str) -> Result<()> {
    let canonical_path = context.root.join("number-spelling.json");
    for text in [
        "{\"extra\":1e0}",
        "{\"extra\":1.0}",
        "{\"extra\":-0}",
        "{\"extra\":1e400}",
        "{\"extra\":18446744073709551616}",
    ] {
        private::write(&canonical_path, text.as_bytes())?;
        require(
            protocol::read_json(&canonical_path).is_err(),
            "SECURITY_MOCK_NONCANONICAL_NUMBER_ACCEPTED",
        )?;
        fs::remove_file(&canonical_path).private("SECURITY_FIXTURE_IO")?;
    }
    protocol::write_json(&canonical_path, &json!({"extra":u64::MAX}))?;
    require(
        protocol::read_json(&canonical_path)?["extra"].as_u64() == Some(u64::MAX),
        "SECURITY_MOCK_CANONICAL_INTEGER_REJECTED",
    )?;
    let done_path = context.root.join("proof").join(format!("{id}.done"));
    let done = protocol::read_json(&done_path)?;
    for number in ["0.0", "0e0", "1e400", "-1", "256", "true", "\"0\""] {
        let mut invalid = done.clone();
        invalid["status"] = serde_json::from_str(number).private("SECURITY_FIXTURE_IO")?;
        replace(&done_path, &invalid)?;
        require(
            protocol::verify_proofs(context, 123).is_err(),
            "SECURITY_MOCK_UNBOUNDED_STATUS_ACCEPTED",
        )?;
    }
    replace(&done_path, &done)?;
    let group_path = context.root.join("group.json");
    let group = protocol::read_json(&group_path)?;
    for number in ["123.0", "123e0", "1e400", "-1", "\"123\""] {
        let mut invalid = group.clone();
        invalid["group"] = serde_json::from_str(number).private("SECURITY_FIXTURE_IO")?;
        replace(&group_path, &invalid)?;
        require(
            protocol::verify_proofs(context, 123).is_err(),
            "SECURITY_MOCK_UNBOUNDED_GROUP_ACCEPTED",
        )?;
    }
    replace(&group_path, &group)?;
    for number in ["1e400", "18446744073709551616", "-1", "true"] {
        let value: Value = serde_json::from_str(number).private("SECURITY_FIXTURE_IO")?;
        require(
            protocol::deadline_from(&json!({"deadline_ms":value})).is_err(),
            "SECURITY_MOCK_UNBOUNDED_DEADLINE_ACCEPTED",
        )?;
    }
    require(
        protocol::verify_proofs(context, 123)?.history == 1,
        "SECURITY_MOCK_INTEGER_PROOF_RESTORE_FAILED",
    )?;
    diagnostic("INFO", "SECURITY_MOCK_PROOF_INTEGER_BOUNDS_PASS");
    Ok(())
}
fn history_failure_proof(context: &protocol::Context, args: &[String], id: &str) -> Result<()> {
    let proof = context.root.join("proof");
    let path = proof.join(format!("{id}.done"));
    let valid = protocol::read_json(&path)?;
    let mut failed = valid.clone();
    failed["accepted"] = json!(false);
    failed["status"] = json!(128);
    failed["stderr_quiet"] = json!(false);
    replace(&path, &failed)?;
    require(
        failed_at(
            &protocol::verify_proofs(context, 123),
            "SECURITY_HISTORY_GIT_FAILED",
        ),
        "SECURITY_MOCK_HISTORY_FAILURE_NOT_REACHED",
    )?;
    let successful = record(context, args, "history", 0, false)?;
    require(
        failed_at(
            &protocol::verify_proofs(context, 123),
            "SECURITY_HISTORY_GIT_FAILED",
        ),
        "SECURITY_MOCK_LATER_SUCCESS_HID_FAILURE",
    )?;
    for (key, value) in [
        ("completed", json!(false)),
        ("status", Value::Null),
        ("status", json!(0)),
        ("status", json!(-1)),
        ("status", json!(256)),
        ("no_remote", json!(true)),
    ] {
        let mut invalid = failed.clone();
        invalid[key] = value;
        replace(&path, &invalid)?;
        let result = protocol::verify_proofs(context, 123);
        require(
            result.is_err() && !failed_at(&result, "SECURITY_HISTORY_GIT_FAILED"),
            "SECURITY_MOCK_INFRA_MISLABELLED_HISTORY",
        )?;
    }
    replace(&path, &failed)?;
    let orphan = proof.join(format!("{}.attempt", private::nonce()?));
    protocol::write_json(&orphan, &json!({}))?;
    let incomplete = protocol::verify_proofs(context, 123);
    require(
        incomplete.is_err() && !failed_at(&incomplete, "SECURITY_HISTORY_GIT_FAILED"),
        "SECURITY_MOCK_INCOMPLETE_MISLABELLED_HISTORY",
    )?;
    fs::remove_file(orphan).private("SECURITY_FIXTURE_IO")?;
    replace(&path, &valid)?;
    for suffix in ["attempt", "start", "done"] {
        fs::remove_file(proof.join(format!("{successful}.{suffix}")))
            .private("SECURITY_FIXTURE_IO")?;
    }
    require(
        protocol::verify_proofs(context, 123)?.history == 1,
        "SECURITY_MOCK_HISTORY_PROOF_RESTORE_FAILED",
    )?;
    diagnostic("INFO", "SECURITY_MOCK_COMPLETED_HISTORY_FAILURE_PASS");
    Ok(())
}
