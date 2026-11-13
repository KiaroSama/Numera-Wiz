//! Plain synthetic checks; these never authenticate an archive or launch a process.
use super::{evidence, metadata};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{
    fs,
    io::{Cursor, Write},
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

pub fn run() -> Result<()> {
    let release = "Suite: noble\nCodename: noble\nArchitectures: amd64\nComponents: main restricted universe multiverse\nDate: Sun, 01 Nov 2026 00:00:00 UTC\nSHA256:\n ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad 3 main/binary-amd64/Packages.xz\n";
    let fields = metadata::fields(release)?;
    let entry = metadata::release_index(&fields, "main/binary-amd64/Packages.xz")?;
    ensure!(
        entry.0 == 3
            && entry.1 == "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
        "fixed Release oracle"
    );
    metadata::check_bytes(b"abc", entry.0, &entry.1)?;
    ensure!(
        metadata::check_bytes(b"abd", entry.0, &entry.1).is_err(),
        "bad digest refused"
    );
    ensure!(
        metadata::check_bytes(b"abc", 4, &entry.1).is_err(),
        "bad size refused"
    );
    ensure!(
        metadata::fields("Package: a\nPackage: b\n").is_err(),
        "duplicate fields refused"
    );
    ensure!(
        metadata::fields("Package: a\npackage: b\n").is_err(),
        "case-aliased fields refused"
    );
    for path in ["../x", "/x", "x/../y", "x\\y", "x//y", "x/./y"] {
        ensure!(
            metadata::safe_path(path).is_err(),
            "path traversal refused: {path}"
        );
    }
    ensure!(
        metadata::decimal("18446744073709551616").is_err(),
        "integer overflow refused"
    );
    ensure!(metadata::decimal("-1").is_err(), "signed size refused");
    let inventory = metadata::inventory("libx\tamd64\t2:1.0-1\tinstall ok installed\n")?;
    ensure!(
        inventory.len() == 1 && inventory[0]["version"] == "2:1.0-1",
        "dpkg epoch preserved"
    );
    ensure!(
        metadata::inventory("x\tarm64\t1\tinstall ok installed\n").is_err(),
        "architecture mismatch refused"
    );
    ensure!(
        metadata::inventory("x\tamd64\t1\tinstall ok installed").is_err(),
        "truncated dpkg output refused"
    );
    let identity = metadata::fields("Package: libx\nArchitecture: amd64\nVersion: 2:1.0-1\n")?;
    ensure!(
        metadata::selected_identity(&identity, &inventory[0]),
        "exact epoch and architecture join"
    );
    ensure!(
        !metadata::selected_identity(
            &identity,
            &serde_json::json!({"name":"libx","architecture":"amd64","version":"1.0-1"})
        ),
        "epoch mismatch refused"
    );
    ensure!(
        !metadata::selected_identity(
            &identity,
            &serde_json::json!({"name":"libx","architecture":"all","version":"2:1.0-1"})
        ),
        "index architecture mismatch refused"
    );
    let plan = metadata::transaction(
        "Inst libx [1:1] (2:1.0-1 Ubuntu:24.04/noble [amd64])\nConf libx (2:1.0-1 Ubuntu:24.04/noble [amd64])\n",
    )?;
    ensure!(
        plan.len() == 1 && plan[0]["version"] == "2:1.0-1",
        "solver exact identity"
    );
    ensure!(
        metadata::transaction("Remv libx [1]\n").is_err(),
        "Remv refused"
    );
    ensure!(
        metadata::transaction("Inst libx (1 Ubuntu [arm64])\n").is_err(),
        "solver architecture refused"
    );
    ensure!(
        metadata::transaction("Inst libx (1 Ubuntu [amd64])").is_err(),
        "truncated solver output refused"
    );
    let uri = "'https://snapshot.ubuntu.com/ubuntu/20261101T000000Z/pool/main/x/x.deb' x.deb 3 MD5Sum:00000000000000000000000000000000\n";
    let uris = metadata::uris(uri)?;
    ensure!(
        uris.len() == 1 && uris[0]["size"] == 3 && uris[0].get("sha256").is_none(),
        "URI MD5 never promoted to SHA256"
    );
    ensure!(
        metadata::uris(&uri.replace("snapshot.ubuntu.com", "archive.ubuntu.com")).is_err(),
        "foreign URI refused"
    );
    let mut seen = std::collections::BTreeSet::new();
    metadata::unique_path(&mut seen, "raw/A")?;
    ensure!(
        metadata::unique_path(&mut seen, "raw/a").is_err(),
        "case alias refused"
    );
    let mut parsed = Vec::new();
    metadata::stanzas(Cursor::new(b"Package: x\nVersion: 1\n\n"), |s| {
        parsed.push(s);
        Ok(())
    })?;
    ensure!(parsed.len() == 1, "streamed stanza count");
    ensure!(
        metadata::stanzas(Cursor::new(b"Package: x"), |_| Ok(())).is_err(),
        "truncated index refused"
    );
    let oversized = format!("Package: x\nDescription: {}\n\n", "a".repeat(1024 * 1024));
    ensure!(
        metadata::stanzas(Cursor::new(oversized.as_bytes()), |_| Ok(())).is_err(),
        "oversized stanza refused"
    );
    ensure!(metadata::signature_status(false, "[GNUPG:] VALIDSIG 0123456789012345678901234567890123456789 2026-11-01 0 0 4 0 1 8 01 0123456789012345678901234567890123456789\n").is_err(), "bad signature exit refused even with status token");
    ensure!(
        metadata::signature_status(true, "[GNUPG:] BADSIG 0123456789012345 synthetic\n").is_err(),
        "bad signature status refused"
    );
    ensure!(
        metadata::same_state("a", "b").is_err(),
        "changed state refused"
    );
    ensure!(
        metadata::require_match(&[], "missing base version").is_err(),
        "base version gap refused"
    );
    ensure!(
        metadata::require_match(
            &[
                serde_json::json!({"sha256":"a"}),
                serde_json::json!({"sha256":"b"})
            ],
            "ambiguous"
        )
        .is_err(),
        "differing stanzas refused"
    );
    ensure!(
        metadata::dependency_terms("libx (>= 2:1.0-1) | virtual-x:any")?.len() == 2,
        "dependency alternatives retained"
    );
    ensure!(
        metadata::provider_set(&[]).is_err(),
        "unresolved provider refused"
    );
    ensure!(
        metadata::provider_set(&["mawk:amd64=1.3.4".into(), "gawk:amd64=5.2.1".into()])?.len() == 2,
        "two already selected satisfying providers retained without inventing solver choice"
    );
    ensure!(
        !metadata::provider_version_available(true, None),
        "unversioned Provides cannot satisfy versioned dependency"
    );
    ensure!(
        metadata::provider_version_available(false, None),
        "unversioned virtual dependency accepts unversioned Provides"
    );
    ensure!(
        metadata::qualifier_matches(Some("any"), Some("allowed"), "amd64")?,
        "any qualifier requires explicit allowed metadata"
    );
    ensure!(
        !metadata::qualifier_matches(Some("any"), Some("foreign"), "amd64")?,
        "foreign is not silently substituted for allowed"
    );
    ensure!(
        metadata::qualifier_matches(Some("unsupported"), None, "amd64").is_err(),
        "unproved qualifier semantics refused"
    );
    ensure!(
        super::evidence::parse_canonical(b"{\"x\":1,\"x\":2}\n").is_err(),
        "duplicate evidence JSON refused"
    );
    ensure!(
        metadata::single_tar(&tar_fixture(), "fixture", 3)? == b"abc",
        "fixed ustar file oracle"
    );
    ensure!(
        metadata::single_tar(&tar_fixture(), "fixture", 2).is_err(),
        "tar size bound refused"
    );
    ensure!(
        metadata::single_tar(&tar_fixture()[..1024], "fixture", 3).is_err(),
        "truncated tar refused"
    );
    ensure!(
        metadata::single_tar(&tar_fixture(), "../fixture", 3).is_err(),
        "tar traversal refused"
    );
    let mut bad_tar = tar_fixture();
    bad_tar[156] = b'2';
    ensure!(
        metadata::single_tar(&bad_tar, "fixture", 3).is_err(),
        "tar link refused"
    );
    let mut duplicate_tar = tar_fixture();
    duplicate_tar[1024] = b'x';
    ensure!(
        metadata::single_tar(&duplicate_tar, "fixture", 3).is_err(),
        "extra tar entry refused"
    );
    super::process::check_count(16 * 1024 * 1024, 1, 16 * 1024 * 1024)
        .expect_err("output overflow must fail");
    ensure!(
        super::process::signal_target(0).is_err()
            && super::process::signal_target(1).is_err()
            && super::process::signal_target(u32::MAX).is_err(),
        "unsafe group identifiers refused without signalling"
    );
    ensure!(
        super::process::signal_target(42)? == 42,
        "owned numeric group framing"
    );
    index_target_cases()?;
    filesystem_cases()?;
    tracing::info!(
        "Linux capture synthetic self-check passed; no APT, signature, container or runtime truth is inferred"
    );
    Ok(())
}

struct OwnedFixture {
    path: PathBuf,
    active: bool,
}
impl OwnedFixture {
    fn create() -> Result<Self> {
        ensure!(
            std::env::consts::OS == "linux",
            "owned filesystem capture fixtures require the Linux HOST"
        );
        let name = format!(
            "nw-capture-selfcheck-{}-{}",
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
        );
        let path = std::env::temp_dir().join(name);
        let mut builder = fs::DirBuilder::new();
        builder.recursive(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(&path)?;
        Ok(Self { path, active: true })
    }
    fn cleanup(&mut self) -> Result<()> {
        fs::remove_dir_all(&self.path)?;
        self.active = false;
        Ok(())
    }
}
impl Drop for OwnedFixture {
    fn drop(&mut self) {
        if self.active {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

fn fixture_hash(bytes: &[u8]) -> Result<&'static str> {
    match bytes {
        b"a" => Ok("ca978112ca1bbdcafac231b39a23dc4da786eff8147c4e72b9807785afee48bb"),
        b"b" => Ok("3e23e8160039594a33894f6564e1b1348bbd7a0088d42c4acb73eeaed59c009d"),
        b"c" => Ok("2e7d2c03a9507ae265ecf5b5356885a53393a2029d241394997265a1a25aefc6"),
        b"bc" => Ok("1e0bbd6c686ba050b8eb03ffeedc64fdc9d80947fce821abbe5d6dc8d252c5ac"),
        b"abc" => Ok("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"),
        _ => anyhow::bail!("unexpected fixture bytes"),
    }
}

// Filesystem inventory and fixed payload digests are independent of Host/Raw's
// counters and production hashing. Fixtures total tiny literal bytes, no tools.
fn audit_files(
    output: &Path,
    files: &[Value],
    expected_bytes: u64,
    expected_count: usize,
) -> Result<()> {
    let directory = output.join("raw");
    let mut observed_bytes = 0;
    let mut paths = std::collections::BTreeSet::new();
    for entry in fs::read_dir(&directory)? {
        let entry = entry?;
        ensure!(
            entry.file_type()?.is_file(),
            "fixture physical entry must be regular"
        );
        observed_bytes += entry.metadata()?.len();
        paths.insert(format!(
            "raw/{}",
            entry.file_name().to_str().context("fixture filename")?
        ));
    }
    ensure!(
        observed_bytes == expected_bytes && paths.len() == expected_count,
        "independent physical byte/file inventory"
    );
    let mut declared = std::collections::BTreeSet::new();
    let mut declared_bytes = 0;
    for file in files {
        for chunk in file["chunks"].as_array().context("fixture chunks")? {
            let path = chunk["path"].as_str().context("fixture chunk path")?;
            ensure!(
                declared.insert(path.to_string()),
                "fixture duplicate physical path"
            );
            let bytes = fs::read(output.join(path))?;
            ensure!(
                bytes.len() <= 16
                    && chunk["bytes"].as_u64() == Some(bytes.len() as u64)
                    && chunk["sha256"].as_str() == Some(fixture_hash(&bytes)?),
                "literal physical prefix/hash oracle"
            );
            declared_bytes += bytes.len() as u64;
        }
    }
    ensure!(
        declared == paths && declared_bytes == expected_bytes,
        "manifest covers every physical retained file exactly"
    );
    Ok(())
}

fn filesystem_cases() -> Result<()> {
    let mut owned = OwnedFixture::create()?;
    let deadline = Instant::now() + Duration::from_secs(5);
    let result = (|| -> Result<()> {
        let host_path = owned.path.join("host");
        fs::create_dir(&host_path)?;
        fs::create_dir(host_path.join("raw"))?;
        let mut host = super::Host {
            output: host_path.clone(),
            files: Vec::new(),
            total: 0,
            chunks: 0,
            deadline,
            diagnostic_failures: Vec::new(),
            sequence: 0,
        };
        let chunk = |path: &str, bytes: &[u8]| -> Result<Value> {
            Ok(json!({"path":path,"bytes":bytes.len(),"sha256":fixture_hash(bytes)?}))
        };
        let manifest = json!({"schema_version":1,"raw_files":2,"raw_bytes":3,"transport_files":3,"files":[
            {"logical_path":"first","bytes":1,"sha256":fixture_hash(b"a")?,"chunks":[chunk("raw/chunk-0000",b"a")?]},
            {"logical_path":"second","bytes":2,"sha256":fixture_hash(b"bc")?,"chunks":[chunk("raw/chunk-0001",b"b")?,chunk("raw/chunk-0002",b"c")?]}
        ]});
        let transfer = super::transfer_raw_with(&mut host, &manifest, |_, path| match path {
            "raw/chunk-0000" => Ok(b"a".to_vec()),
            "raw/chunk-0001" => Ok(b"b".to_vec()),
            _ => anyhow::bail!("synthetic later fetch failure"),
        });
        ensure!(
            transfer.is_err()
                && host.chunks == 2
                && host.total == 2
                && host.files[0]["capture_status"] == "complete"
                && host.files[1]["capture_status"] == "incomplete",
            "completed and partial transfers remain immediately charged"
        );
        host.record("later-diagnostic", b"c")?;
        ensure!(
            host.total == 3 && host.chunks == 3,
            "later diagnostic cannot forget transferred physical files"
        );
        audit_files(&host_path, &host.files, 3, 3)?;

        let partial_path = owned.path.join("partial");
        fs::create_dir(&partial_path)?;
        fs::create_dir(partial_path.join("raw"))?;
        let mut partial = super::Host {
            output: partial_path.clone(),
            files: Vec::new(),
            total: 0,
            chunks: 0,
            deadline,
            diagnostic_failures: Vec::new(),
            sequence: 0,
        };
        let failure = partial.record_with("short-write", b"abc", |file, bytes| {
            file.write_all(&bytes[..1])?;
            Err(std::io::Error::other("synthetic partial write"))
        });
        ensure!(
            failure.is_err()
                && partial.total == 1
                && partial.chunks == 1
                && partial.files[0]["sha256"] == fixture_hash(b"a")?,
            "partial Host write retains a hashed actual prefix"
        );
        partial.record("after-short-write", b"bc")?;
        audit_files(&partial_path, &partial.files, 3, 2)?;

        let mut pending = evidence::Raw::new(&owned.path.join("pending"))?;
        let mut reservations = Vec::new();
        for index in 0..4 {
            reservations
                .push(pending.reserve(&format!("reserved-{index}"), super::process::FILE_LIMIT)?);
        }
        ensure!(
            pending.bytes("rejected-before-write", b"a").is_err()
                && fs::read_dir(owned.path.join("pending"))?.count() == 0,
            "aggregate rejection creates no unowned pending file"
        );
        for path in reservations {
            pending.cancel(&path)?;
        }
        let path = pending.reserve("wrong-adoption", 1)?;
        evidence::write_new(&path, b"a", 1)?;
        ensure!(
            pending.adopt("different-logical", &path).is_err() && !path.try_exists()?,
            "rejected adoption cancels its actual owned pending file"
        );
        let path = pending.reserve("decoder", super::process::FILE_LIMIT)?;
        evidence::write_new(&path, b"a", 1)?;
        ensure!(
            pending
                .settle::<()>(
                    "decoder",
                    &path,
                    Err(anyhow::anyhow!("synthetic decoder failure"))
                )
                .is_err()
                && !path.try_exists()?,
            "actual decoder-error settlement removes the pending file"
        );
        pending.bytes("retained", b"abc")?;
        let pending_output = owned.path.join("pending-out");
        fs::create_dir(&pending_output)?;
        let finalized = pending.finish(&pending_output);
        ensure!(
            finalized.errors.is_empty()
                && finalized.manifest["raw_bytes"] == 3
                && finalized.manifest["transport_files"] == 1,
            "pending refusals preserve later finalization"
        );
        audit_files(
            &pending_output,
            finalized.manifest["files"]
                .as_array()
                .context("pending files")?,
            3,
            1,
        )?;

        let mut growing = evidence::Raw::new(&owned.path.join("growing"))?;
        let original = growing.bytes("changed-input", b"a")?;
        fs::write(growing.source(&original)?, b"abc")?;
        let grown_output = owned.path.join("growing-out");
        fs::create_dir(&grown_output)?;
        let finalized = growing.finish(&grown_output);
        ensure!(
            !finalized.errors.is_empty()
                && finalized.manifest["raw_bytes"] == 0
                && finalized.manifest["transport_files"] == 0,
            "grown input is refused before exporting bytes beyond its admitted logical size"
        );
        audit_files(
            &grown_output,
            finalized.manifest["files"]
                .as_array()
                .context("grown files")?,
            0,
            0,
        )?;

        let mut failing = evidence::Raw::new(&owned.path.join("finalizing"))?;
        failing.bytes("first", b"abc")?;
        failing.bytes("second", b"abc")?;
        let output = owned.path.join("failed-out");
        fs::create_dir(&output)?;
        let mut writes = 0;
        let finalized = failing.finish_with(&output, |file, bytes| {
            writes += 1;
            if writes == 2 {
                file.write_all(&bytes[..1])?;
                Err(std::io::Error::other(
                    "synthetic finalization write failure",
                ))
            } else {
                file.write_all(bytes)
            }
        });
        ensure!(
            !finalized.errors.is_empty()
                && finalized.manifest["complete"] == false
                && finalized.manifest["raw_bytes"] == 4
                && finalized.manifest["transport_files"] == 2,
            "finalization failure preserves earlier chunk and actual later prefix"
        );
        audit_files(
            &output,
            finalized.manifest["files"]
                .as_array()
                .context("failed files")?,
            4,
            2,
        )?;
        let mut report = json!({"schema_version":1,"initial_inventory":["synthetic-base-row"],"trust":{"fixture":"retained"},"unresolved_reasons":[]});
        ensure!(
            super::inner::finish_capture(
                &output,
                &mut report,
                Err(anyhow::anyhow!("synthetic original capture failure")),
                finalized
            )
            .is_err(),
            "failed capture remains nonzero after report publication"
        );
        let written = evidence::parse_canonical(&fs::read(output.join("report.json"))?)?;
        let reasons = written["unresolved_reasons"].to_string();
        ensure!(
            written["complete_snapshot_lock"] == false
                && written["initial_inventory"][0] == "synthetic-base-row"
                && reasons.contains("synthetic original capture failure")
                && reasons.contains("synthetic finalization write failure"),
            "actual failed report preserves original context and finalization error"
        );
        let available = evidence::parse_canonical(&fs::read(output.join("raw-manifest.json"))?)?;
        ensure!(
            available["raw_bytes"] == 4 && available["transport_files"] == 2,
            "published available manifest remains truthful after failure"
        );
        unknown_handoff_case(&owned.path, deadline)?;
        ensure!(
            Instant::now() < deadline,
            "tiny filesystem fixtures exceeded their HOST budget"
        );
        Ok(())
    })();
    let cleanup = owned.cleanup();
    result.with_context(|| format!("owned filesystem fixture cleanup: {cleanup:?}"))?;
    cleanup
}

fn unknown_handoff_case(owned: &Path, deadline: Instant) -> Result<()> {
    let source = owned.join("unknown-source");
    fs::create_dir(&source)?;
    fs::create_dir(source.join("raw"))?;
    for (path, bytes) in [
        ("raw/chunk-0000", b"a"),
        ("raw/chunk-0001", b"b"),
        ("raw/chunk-0002", b"b"),
        ("raw/chunk-0003", b"c"),
    ] {
        evidence::write_new(&source.join(path), bytes, 1)?;
    }
    let known = |path: &str, bytes: &[u8]| -> Result<Value> {
        Ok(json!({"path":path,"bytes":bytes.len(),"sha256":fixture_hash(bytes)?}))
    };
    let manifest = json!({"schema_version":1,"complete":false,"raw_files":3,"raw_bytes":4,"transport_files":4,"files":[
        {"logical_path":"before-unknown","bytes":1,"sha256":fixture_hash(b"a")?,"capture_status":"complete","chunks":[known("raw/chunk-0000",b"a")?]},
        {"logical_path":"with-unknown","bytes":2,"sha256":null,"expected_bytes":2,"expected_sha256":fixture_hash(b"bc")?,"capture_status":"incomplete","chunks":[known("raw/chunk-0001",b"b")?,{"path":"raw/chunk-0002","bytes":1,"sha256":null,"integrity":"UNKNOWN","reserved_upper_bound":true}]},
        {"logical_path":"after-unknown","bytes":1,"sha256":fixture_hash(b"c")?,"capture_status":"complete","chunks":[known("raw/chunk-0003",b"c")?]}
    ]});
    let mut child = json!({"schema_version":1,"initial_inventory":["handoff-base-row"],"unresolved_reasons":[]});
    let finalized = evidence::Finalized {
        manifest,
        errors: vec!["synthetic UNKNOWN finalization failure".into()],
    };
    ensure!(
        super::inner::finish_capture(
            &source,
            &mut child,
            Err(anyhow::anyhow!("synthetic handoff capture failure")),
            finalized
        )
        .is_err(),
        "UNKNOWN child report remains unsuccessful"
    );
    let inner_bytes = fs::read(source.join("report.json"))?;
    let manifest_bytes = fs::read(source.join("raw-manifest.json"))?;
    let destination = owned.join("unknown-host");
    fs::create_dir(&destination)?;
    fs::create_dir(destination.join("raw"))?;
    let mut host = super::Host {
        output: destination.clone(),
        files: Vec::new(),
        total: 0,
        chunks: 0,
        deadline,
        diagnostic_failures: Vec::new(),
        sequence: 0,
    };
    let mut report = json!({"complete_snapshot_lock":false});
    let mut fetched = Vec::new();
    let result = super::receive_raw_with(
        &mut host,
        &mut report,
        &inner_bytes,
        &manifest_bytes,
        |_, path| {
            ensure!(
                path != "raw/chunk-0002",
                "UNKNOWN bytes must not be fetched as verified evidence"
            );
            fetched.push(path.to_string());
            Ok(fs::read(source.join(path))?)
        },
    );
    ensure!(
        result.is_err()
            && report["complete_snapshot_lock"] == false
            && report["metadata_capture"]["complete_snapshot_lock"] == false,
        "UNKNOWN handoff cannot become a successful lock"
    );
    let reasons = report["metadata_capture"]["unresolved_reasons"].to_string();
    ensure!(
        reasons.contains("synthetic handoff capture failure")
            && reasons.contains("synthetic UNKNOWN finalization failure"),
        "handoff preserves child original and finalization errors"
    );
    ensure!(
        fetched == ["raw/chunk-0000", "raw/chunk-0001", "raw/chunk-0003"]
            && host.files[2]["capture_status"] == "incomplete"
            && host.files[3]["capture_status"] == "complete",
        "known prefix and valid later file are actually exported after UNKNOWN"
    );
    ensure!(
        host.files[2]["unexported_chunks"][0]["path"] == "raw/chunk-0002"
            && host.files[2]["unexported_chunks"][0]["sha256"].is_null()
            && host.files[2]["expected_sha256"].is_null(),
        "UNKNOWN source identity remains explicit and unexported"
    );
    // The first host row is the exact child manifest; audit payload rows with
    // independent literal hashes, then reconcile the complete physical set.
    let retained_manifest = &host.files[0]["chunks"][0];
    ensure!(
        fs::read(
            destination.join(
                retained_manifest["path"]
                    .as_str()
                    .context("saved child manifest")?
            )
        )? == manifest_bytes,
        "exact source manifest retained beside failed child report"
    );
    let mut expected_paths = std::collections::BTreeSet::new();
    let mut retained_bytes = 0_u64;
    for file in &host.files {
        for chunk in file["chunks"].as_array().context("handoff chunks")? {
            let path = chunk["path"].as_str().context("handoff path")?;
            expected_paths.insert(path.to_string());
            let bytes = fs::read(destination.join(path))?;
            retained_bytes += bytes.len() as u64;
            ensure!(
                chunk["bytes"].as_u64() == Some(bytes.len() as u64),
                "handoff physical size matches manifest"
            );
            if path
                != retained_manifest["path"]
                    .as_str()
                    .context("source manifest path")?
            {
                ensure!(
                    chunk["sha256"].as_str() == Some(fixture_hash(&bytes)?),
                    "handoff fixed payload digest"
                );
            }
        }
    }
    let physical = fs::read_dir(destination.join("raw"))?
        .map(|entry| -> Result<String> {
            let entry = entry?;
            ensure!(
                entry.file_type()?.is_file(),
                "handoff regular physical file"
            );
            Ok(format!(
                "raw/{}",
                entry.file_name().to_str().context("handoff filename")?
            ))
        })
        .collect::<Result<std::collections::BTreeSet<_>>>()?;
    ensure!(
        physical == expected_paths
            && host.chunks == 4
            && host.total == retained_bytes
            && retained_bytes == manifest_bytes.len() as u64 + 3,
        "all retained files and bytes reconciled after UNKNOWN handoff"
    );
    let report_path = destination.join("report");
    fs::create_dir(&report_path)?;
    let exported = json!({"schema_version":1,"encoding":"ordered_concatenation_of_sha256_checked_chunks","complete":false,"raw_bytes":host.total,"raw_files":host.files.len(),"transport_files":host.chunks,"files":host.files});
    let exported_bytes = evidence::canonical(&exported)?;
    report["raw_manifest"] = json!({"path":"raw-manifest.json","bytes":exported_bytes.len(),"sha256":metadata::digest(&exported_bytes)});
    evidence::write_small(&report_path, &report, &exported)?;
    let stored = evidence::parse_canonical(&fs::read(report_path.join("report.json"))?)?;
    ensure!(
        stored["metadata_capture"] == child && stored["complete_snapshot_lock"] == false,
        "stored outer failed report preserves the exact child report and its errors"
    );

    for mode in [
        "invalid-unknown",
        "unsafe-unknown",
        "bad-known-digest",
        "no-known-prefix",
        "known-chunks-unknown-logical",
    ] {
        let mut declared = evidence::parse_canonical(&manifest_bytes)?;
        match mode {
            "invalid-unknown" => {
                declared["files"][1]["chunks"][1]["reserved_upper_bound"] = json!(false)
            }
            "unsafe-unknown" => declared["files"][1]["chunks"][1]["path"] = json!("../escape"),
            "no-known-prefix" => {
                declared["files"][1]["chunks"]
                    .as_array_mut()
                    .context("unknown chunks")?
                    .remove(0);
                declared["files"][1]["bytes"] = json!(1);
                declared["raw_bytes"] = json!(3);
                declared["transport_files"] = json!(3);
            }
            "known-chunks-unknown-logical" => {
                declared["files"][1]["chunks"]
                    .as_array_mut()
                    .context("known chunks")?
                    .remove(1);
                declared["files"][1]["bytes"] = json!(1);
                declared["raw_bytes"] = json!(3);
                declared["transport_files"] = json!(3);
            }
            _ => {}
        }
        let directory = owned.join(mode);
        fs::create_dir(&directory)?;
        fs::create_dir(directory.join("raw"))?;
        let mut checked = super::Host {
            output: directory.clone(),
            files: Vec::new(),
            total: 0,
            chunks: 0,
            deadline,
            diagnostic_failures: Vec::new(),
            sequence: 0,
        };
        let mut later = false;
        let result = super::transfer_raw_with(&mut checked, &declared, |_, path| {
            ensure!(
                matches!(path, "raw/chunk-0000" | "raw/chunk-0001" | "raw/chunk-0003"),
                "unsafe or UNKNOWN path must never be fetched"
            );
            if path == "raw/chunk-0003" {
                later = true;
                if mode == "bad-known-digest" {
                    return Ok(b"a".to_vec());
                }
            }
            Ok(fs::read(source.join(path))?)
        });
        ensure!(
            result.is_err(),
            "failed or malformed source never produces successful transfer"
        );
        let supported = matches!(mode, "no-known-prefix" | "known-chunks-unknown-logical");
        ensure!(
            later == (supported || mode == "bad-known-digest"),
            "malformed UNKNOWN rejects; supported UNKNOWN continues to later data"
        );
        let retained = if mode == "known-chunks-unknown-logical" {
            3
        } else {
            2
        };
        ensure!(
            checked.total == retained && checked.chunks == retained as usize,
            "rejected data never enters physical accounting"
        );
        audit_files(&directory, &checked.files, retained, retained as usize)?;
    }
    Ok(())
}

fn index_target_cases() -> Result<()> {
    // Literal independent identities, not a generated successful signature fixture.
    let empty_hash = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
    let abc_hash = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
    let release = metadata::fields(&format!(
        "SHA256:\n {empty_hash} 0 restricted/binary-amd64/Packages\n 0040f94d11d0039505328a90b2ff48968db873e9e7967307631bf40ef5679275 32 restricted/binary-amd64/Packages.xz\n"
    ))?;
    let select = |fields: &metadata::Fields, targets: &[metadata::Fields]| {
        metadata::selected_index_cache(fields, targets, "noble-backports", "restricted")
            .map(|path| path.map(Path::to_path_buf))
    };
    ensure!(
        select(&release, &[])?.is_none(),
        "signed-empty selection permits observed target absence without inventing a path"
    );
    let target = metadata::fields(
        "MetaKey: restricted/binary-amd64/Packages\nCodename: noble\nSuite: noble-backports\nTrusted: yes\nFilename: /capture/lists/fixture_Packages.lz4\n",
    )?;
    ensure!(
        select(&release, std::slice::from_ref(&target))?
            == Some(PathBuf::from("/capture/lists/fixture_Packages.lz4")),
        "existing empty-index cache keeps its actual target path and normal validation"
    );
    ensure!(
        select(&release, &[target.clone(), target.clone()]).is_err(),
        "duplicate target refused even for an empty index"
    );
    let nonempty = metadata::fields(&format!(
        "SHA256:\n {abc_hash} 3 restricted/binary-amd64/Packages\n"
    ))?;
    ensure!(
        select(&nonempty, &[]).is_err(),
        "missing nonempty target refused"
    );
    ensure!(
        select(&nonempty, std::slice::from_ref(&target))?.is_some(),
        "existing nonempty target retains the normal cache path"
    );
    let contradictory = metadata::fields(&format!(
        "SHA256:\n {abc_hash} 0 restricted/binary-amd64/Packages\n"
    ))?;
    ensure!(
        select(&contradictory, &[]).is_err(),
        "false empty hash refused"
    );
    ensure!(
        select(&metadata::fields("SHA256:\n")?, &[]).is_err(),
        "missing plain Release checksum refused"
    );
    let duplicate = metadata::fields(&format!(
        "SHA256:\n {empty_hash} 0 restricted/binary-amd64/Packages\n {empty_hash} 0 restricted/binary-amd64/Packages\n"
    ))?;
    ensure!(
        select(&duplicate, &[]).is_err(),
        "duplicate plain Release entry refused"
    );
    for (key, value) in [
        ("Trusted", "no"),
        ("Filename", "/capture/lists/../outside"),
        ("Filename", "https://untrusted.invalid/Packages"),
    ] {
        let mut invalid = target.clone();
        invalid.insert(key.into(), value.into());
        ensure!(
            select(&release, &[invalid]).is_err(),
            "invalid existing target refused"
        );
    }
    ensure!(
        metadata::selected_index_cache(&release, &[], "../noble-backports", "restricted").is_err(),
        "unsafe source URL input refused"
    );
    // Exercise the independent byte validator and actual streaming parser with
    // fixed data: selection never turns nonempty/corrupt plain data into an index.
    metadata::check_bytes(b"", 0, empty_hash)?;
    ensure!(
        metadata::check_bytes(b"x", 0, empty_hash).is_err(),
        "nonempty decoded data refused"
    );
    ensure!(
        metadata::check_bytes(b"", 0, abc_hash).is_err(),
        "corrupt plain digest refused"
    );
    let mut packages = 0;
    metadata::stanzas(Cursor::new(b""), |_| {
        packages += 1;
        Ok(())
    })?;
    ensure!(
        packages == 0,
        "empty source contributes zero packages without a fake stanza"
    );
    Ok(())
}

fn tar_fixture() -> Vec<u8> {
    // Independent fixed ustar fixture. Its header checksum is the literal octal
    // 007337, computed once outside the implementation; no production hash or
    // checksum routine constructs this oracle.
    let mut bytes = vec![0_u8; 2048];
    for (offset, value) in [
        (0, b"fixture".as_slice()),
        (100, b"0000600\0"),
        (108, b"0000000\0"),
        (116, b"0000000\0"),
        (124, b"00000000003\0"),
        (136, b"00000000000\0"),
        (148, b"007337\0 "),
        (156, b"0"),
        (257, b"ustar\0"),
        (263, b"00"),
        (512, b"abc"),
    ] {
        bytes[offset..offset + value.len()].copy_from_slice(value);
    }
    bytes
}
