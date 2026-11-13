//! Metadata-only APT work inside the exact disposable image. Never installs.
use super::{evidence::{self, Raw}, metadata::{self, Fields}, process};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{collections::{BTreeMap, BTreeSet}, fs, io::BufReader, path::Path, process::Command, time::{Duration, Instant}};

const WORK: &str = "/capture";
const KEYRING: &str = "/usr/share/keyrings/ubuntu-archive-keyring.gpg";

fn apt(program: &str, arguments: &[&str]) -> Command {
    let mut command = process::command(program);
    command.env("APT_CONFIG", "/capture/apt.conf").args(arguments);
    command
}

fn recorded(raw: &mut Raw, label: &str, command: &mut Command, deadline: Instant, seconds: u64) -> Result<process::Output> {
    let result = process::run(command, deadline, seconds, None)?;
    raw.bytes(&format!("commands/{label}.stdout"), &result.stdout)?;
    raw.bytes(&format!("commands/{label}.stderr"), &result.stderr)?;
    raw.bytes(&format!("commands/{label}.status"), format!("{}\n", result.status).as_bytes())?;
    Ok(result)
}

fn state() -> Result<String> {
    let mut state = BTreeMap::new();
    for path in ["/var/lib/dpkg/status", "/etc/apt", "/capture/apt.conf", "/capture/snapshot.sources", "/capture/lists", KEYRING] {
        state.insert(path, evidence::state(Path::new(path))?);
    }
    Ok(metadata::digest(&evidence::canonical(&json!(state))?))
}

pub fn worker(config: &Value, seconds: u64) -> Result<()> {
    let native_file_limit = process::begin_worker_limits()?;
    fs::create_dir(WORK)?;
    fs::create_dir("/capture/out")?;
    let mut raw = Raw::new(Path::new("/capture/input"))?;
    let mut report = json!({"schema_version":1,"complete_snapshot_lock":false,"abi_execution":"copied_host_executable_actually_started_in_selected_base","initial_inventory":[],"transaction":[],"final_inventory":[],"releases":[],"indices":[],"provider_and_dependency_records":[],"unresolved_reasons":[],"execution":{"install":false,"compiler":false,"ffmpeg":false,"source_bootstrap":false,"package_payloads":"not_downloaded"}});
    let deadline = Instant::now() + Duration::from_secs(seconds.saturating_sub(5));
    let result = capture(config, &mut raw, &mut report, deadline);
    let (scratch_bytes, scratch_files) = process::scratch_observation();
    report["resource_observations"] = json!({"worker_inherited_RLIMIT_FSIZE_bytes":native_file_limit,"native_scratch_max_observed_bytes":scratch_bytes,"native_scratch_max_observed_files":scratch_files,"native_scratch_peak":"UNKNOWN_between_samples","sampling_scope":"private_lists_cache_and_download_index_temporary_files_only","sampling_interval_ms":50,"sampling_walk_cap_ms":50,"hard_aggregate_container_disk_quota":false,"retained_raw_limits":"separate_512MiB_500_files_128MiB_per_file","scope_exclusions":"base_rootfs_copied_executable_and_temporary_representations_are_not_claimed_bounded_by_the_retention_aggregate"});
    let finalized = raw.finish(Path::new("/capture/out"));
    finish_capture(Path::new("/capture/out"), &mut report, result, finalized)
}

pub(super) fn finish_capture(output: &Path, report: &mut Value, result: Result<()>, finalized: evidence::Finalized) -> Result<()> {
    let complete = result.is_ok() && finalized.errors.is_empty() && finalized.manifest["complete"] == true;
    if let Err(error) = &result { report["unresolved_reasons"].as_array_mut().context("unresolved array")?.push(json!(format!("{error:#}"))); }
    for error in finalized.errors { report["unresolved_reasons"].as_array_mut().context("unresolved array")?.push(json!(format!("evidence finalization: {error}"))); }
    let manifest_bytes = evidence::canonical(&finalized.manifest)?;
    report["raw_manifest"] = json!({"path":"raw-manifest.json","bytes":manifest_bytes.len(),"sha256":metadata::digest(&manifest_bytes)});
    report["complete_snapshot_lock"] = json!(complete);
    if let Err(error) = evidence::write_small(output, report, &finalized.manifest) {
        let mut summary = json!({"schema_version":1,"complete_snapshot_lock":false,"initial_inventory":report["initial_inventory"],"trust":report["trust"],"raw_manifest":report["raw_manifest"],"unresolved_reasons":report["unresolved_reasons"],"full_report_sha256":metadata::digest(&evidence::canonical(report)?)});
        summary["unresolved_reasons"].as_array_mut().context("summary unresolved array")?.push(json!(format!("write full report: {error:#}")));
        if evidence::canonical(&summary)?.len().checked_add(manifest_bytes.len()).and_then(|n| n.checked_add(512)).context("failed summary size overflow")? as u64 > evidence::SMALL_LIMIT {
            summary["initial_inventory"] = json!({"status":"omitted_from_failed_summary_due_to_report_bound","raw_evidence":"commands/base-inventory.stdout","full_report_sha256":summary["full_report_sha256"]});
        }
        evidence::write_small(output, &summary, &finalized.manifest).context("failed to write full report or unsuccessful fallback; filesystem status UNKNOWN")?;
        *report = summary; anyhow::bail!("capture/report finalization unresolved; failed summary retained");
    }
    ensure!(complete, "capture/evidence finalization unresolved; original and finalization failures retained"); Ok(())
}

fn capture(config: &Value, raw: &mut Raw, report: &mut Value, deadline: Instant) -> Result<()> {
    raw.copy("base/dpkg-status", Path::new("/var/lib/dpkg/status"))?;
    raw.tree("base/etc-apt", Path::new("/etc/apt"))?;
    raw.tree("base/dpkg-architecture", Path::new("/var/lib/dpkg/arch"))?;
    raw.tree("base/extended-states", Path::new("/var/lib/apt/extended_states"))?;
    let anchor = raw.copy("trust/ubuntu-archive-keyring.gpg", Path::new(KEYRING))?;
    report["trust"] = json!({"archive_anchor":anchor,"archive_anchor_provenance":"exact_selected_OCI_base_before_APT_changes","archive_anchor_path":KEYRING,"signer_fingerprints":[],"fingerprint_coverage":"actual_verified_signers_and_their_primary_keys","tls":{"verify_peer":true,"verify_host":true},"snapshot_timestamp_binding":"HTTPS_service_provenance_not_a_signature_claim"});
    for (label, program, arguments) in [
        ("base-apt-version", "/usr/bin/apt-get", vec!["--version"]),
        ("base-dpkg-version", "/usr/bin/dpkg", vec!["--version"]),
        ("base-gpgv-version", "/usr/bin/gpgv", vec!["--version"]),
        ("base-apt-config", "/usr/bin/apt-config", vec!["dump"]),
        ("base-holds", "/usr/bin/apt-mark", vec!["showhold"]),
        ("base-foreign-architectures", "/usr/bin/dpkg", vec!["--print-foreign-architectures"]),
    ] {
        let output = recorded(raw, label, process::command(program).args(&arguments), deadline, 5)?; output.success()?;
        report["versions_and_base_configuration"][label] = json!(output.text()?);
    }
    let mut inventory_command = process::command("/usr/bin/dpkg-query");
    inventory_command.args(["--show", "--showformat=${Package}\t${Architecture}\t${Version}\t${Status}\n"]);
    let inventory_output = recorded(raw, "base-inventory", &mut inventory_command, deadline, 5)?; inventory_output.success()?;
    let initial = metadata::inventory(inventory_output.text()?)?;
    report["initial_inventory"] = json!(initial);
    let base_status_hash = evidence::hash_file(Path::new("/var/lib/dpkg/status"), process::FILE_LIMIT)?.1;
    report["base_status_sha256"] = json!(base_status_hash);

    for directory in ["empty.d", "preferences.d", "lists", "lists/partial", "cache", "cache/archives", "cache/archives/partial", "gnupg"] { fs::create_dir(Path::new(WORK).join(directory))?; }
    #[cfg(unix)] {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions("/capture/gnupg", fs::Permissions::from_mode(0o700))?;
    }
    fs::write("/capture/empty", b"")?;
    fs::write("/capture/preferences", b"")?;
    let sources = format!("Types: deb\nURIs: {}\nSuites: noble noble-updates noble-security noble-backports\nComponents: main restricted universe multiverse\nArchitectures: amd64\nSigned-By: {KEYRING}\nCheck-Valid-Until: yes\n", metadata::SNAPSHOT);
    fs::write("/capture/snapshot.sources", &sources)?;
    let mut policy = String::from("Dir::Etc::main \"/capture/empty\";\nDir::Etc::parts \"/capture/empty.d\";\nDir::Etc::sourcelist \"/capture/snapshot.sources\";\nDir::Etc::sourceparts \"/capture/empty.d\";\nDir::Etc::preferences \"/capture/preferences\";\nDir::Etc::preferencesparts \"/capture/preferences.d\";\nDir::Etc::trusted \"/capture/absent-keyring\";\nDir::Etc::trustedparts \"/capture/empty.d\";\nDir::State::status \"/var/lib/dpkg/status\";\nDir::State::lists \"/capture/lists\";\nDir::State::extended_states \"/capture/extended_states\";\nDir::Cache \"/capture/cache\";\nDir::Cache::pkgcache \"\";\nDir::Cache::srcpkgcache \"\";\nAPT::Architecture \"amd64\";\n#clear APT::Architectures;\nAPT::Architectures { \"amd64\"; };\nAPT::Install-Recommends \"false\";\nAPT::Install-Suggests \"false\";\nAPT::Get::AllowUnauthenticated \"false\";\nAPT::Update::Error-Mode \"any\";\nAcquire::AllowInsecureRepositories \"false\";\nAcquire::AllowDowngradeToInsecureRepositories \"false\";\nAcquire::Check-Date \"true\";\nAcquire::Check-Valid-Until \"true\";\nAcquire::Languages \"none\";\nAcquire::Retries \"0\";\nAcquire::http::AllowRedirect \"false\";\nAcquire::https::AllowRedirect \"false\";\nAcquire::https::Verify-Peer \"true\";\nAcquire::https::Verify-Host \"true\";\nAcquire::https::Timeout \"20\";\nAcquire::http::Timeout \"20\";\nAPT::Sandbox::User \"root\";\nDebug::Acquire::gpgv \"true\";\n");
    policy.push_str("Acquire::GzipIndexes \"true\";\n");
    let base_ca = Path::new("/etc/ssl/certs/ca-certificates.crt");
    if base_ca.is_file() {
        let trust = raw.copy("trust/base-tls-ca.crt", base_ca)?;
        report["trust"]["tls"]["input"] = json!({"provenance":"selected_base","identity":trust});
    } else {
        let trust = raw.copy("trust/host-transport-ca.crt", Path::new("/capture-host-ca.crt"))?;
        policy.push_str("Acquire::https::CaInfo \"/capture-host-ca.crt\";\n");
        report["trust"]["tls"]["input"] = json!({"provenance":"host_actual_/etc/ssl/certs/ca-certificates.crt_transport_only_not_archive_signing_trust","identity":trust});
    }
    fs::write("/capture/apt.conf", &policy)?;
    raw.bytes("policy/apt.conf", policy.as_bytes())?; raw.bytes("policy/snapshot.sources", sources.as_bytes())?;
    let effective = recorded(raw, "isolated-apt-config", &mut apt("/usr/bin/apt-config", &["dump"]), deadline, 5)?; effective.success()?;
    report["policy"] = json!({"selected":config["policy"],"effective_apt_config":effective.text()?,"ambient_sources_preferences_architectures":"explicitly_excluded","list_cache":"fresh_owned_empty_directory","sandbox_user":"root_only_inside_capability_dropped_private_container"});
    let update = recorded(raw, "strict-update", &mut apt("/usr/bin/apt-get", &["update", "--error-on=any"]), deadline, 100)?;
    report["apt_update_status"] = json!(update.status.to_string());
    report["apt_update_supervision_failure"] = json!(update.failure);
    if !update.status.success() || update.failure.is_some() { raw.tree("failed-update/lists", Path::new("/capture/lists"))?; }
    update.success().context("strict APT update failed; no stale, expiry, insecure or live-source fallback")?;
    ensure!(evidence::hash_file(Path::new(KEYRING), process::FILE_LIMIT)?.1 == report["trust"]["archive_anchor"]["sha256"].as_str().context("initial anchor hash")?, "base archive trust anchor changed during update");
    let targets_output = recorded(raw, "indextargets", &mut apt("/usr/bin/apt-get", &["indextargets"]), deadline, 10)?; targets_output.success()?;
    let mut targets = Vec::new();
    metadata::stanzas(BufReader::new(targets_output.stdout.as_slice()), |fields| { targets.push(fields); Ok(()) })?;
    report["index_targets"] = json!(targets);
    let policy_output = recorded(raw, "apt-policy", &mut apt("/usr/bin/apt-cache", &["policy"]), deadline, 10)?; policy_output.success()?;
    report["source_policy"] = json!(policy_output.text()?);

    // APT has authenticated its own update. Independently retain and verify every
    // clearsigned pocket against the unchanged base anchor, then its exact .xz.
    let mut index_sources = Vec::new(); let mut all_signers = BTreeSet::new();
    for pocket in ["noble", "noble-updates", "noble-security", "noble-backports"] {
        let suffix = format!("_dists_{pocket}_InRelease");
        let mut candidates = fs::read_dir("/capture/lists")?.collect::<std::io::Result<Vec<_>>>()?;
        candidates.retain(|entry| entry.file_name().to_string_lossy().ends_with(&suffix));
        ensure!(candidates.len() == 1, "missing or ambiguous InRelease for {pocket}");
        let signed = raw.copy(&format!("metadata/{pocket}/InRelease"), &candidates[0].path())?;
        let release_path = Path::new(WORK).join(format!("{pocket}.Release"));
        let mut verifier = process::command("/usr/bin/gpgv");
        verifier.args(["--homedir", "/capture/gnupg", "--keyring", KEYRING, "--status-fd", "1", "--output"]).arg(&release_path).arg(candidates[0].path());
        let verified = recorded(raw, &format!("verify-{pocket}"), &mut verifier, deadline, 10)?;
        verified.success()?;
        let signers = metadata::signature_status(verified.status.success(), verified.text()?)?;
        for signer in &signers { all_signers.insert(signer.clone()); }
        let release_raw = raw.copy(&format!("metadata/{pocket}/Release"), &release_path)?;
        ensure!(fs::metadata(&release_path)?.len() <= 1024 * 1024, "Release exceeded stanza limit");
        let release_bytes = fs::read(&release_path)?;
        let fields = metadata::fields(std::str::from_utf8(&release_bytes)?)?; metadata::release(&fields, pocket)?;
        report["releases"].as_array_mut().context("releases array")?.push(json!({"pocket":pocket,"requested_url":format!("{}/dists/{pocket}/InRelease",metadata::SNAPSHOT),"final_url":format!("{}/dists/{pocket}/InRelease",metadata::SNAPSHOT),"redirects":"refused_by_policy","signed_raw":signed,"release_raw":release_raw,"signature_verified":true,"signer_fingerprints":signers,"fields":fields}));
        for component in ["main", "restricted", "universe", "multiverse"] {
            let meta_key = format!("{component}/binary-amd64/Packages");
            let selected: Vec<_> = targets.iter().filter(|target| target.get("MetaKey") == Some(&meta_key) && target.get("Codename").map(String::as_str) == Some("noble") && target.get("Suite").map(String::as_str) == Some(pocket)).collect();
            ensure!(selected.len() == 1, "missing/ambiguous selected APT index target {pocket}/{component}");
            let target = selected[0];
            ensure!(target.get("Trusted").map(String::as_str) == Some("yes"), "APT target is not authenticated");
            let cached = Path::new(target.get("Filename").context("index target filename")?);
            ensure!(cached.parent() == Some(Path::new("/capture/lists")), "index target escaped owned lists");
            let cached_ref = raw.copy(&format!("metadata/{pocket}/{component}/apt-cache"), cached)?;
            let index_path = format!("{meta_key}.xz"); let (size, hash) = metadata::release_index(&fields, &index_path)?;
            ensure!(size <= process::FILE_LIMIT, "compressed index exceeds bound");
            let url = format!("{}/dists/{pocket}/{index_path}", metadata::SNAPSHOT);
            let destination = Path::new(WORK).join(format!("download-{pocket}-{component}.xz"));
            let mut download = apt("/usr/lib/apt/apt-helper", &["download-file", &url]);
            download.arg(&destination).arg(format!("SHA256:{hash}"));
            let output = recorded_download(&mut download, deadline)?;
            let compressed = if destination.exists() { Some(raw.copy(&format!("metadata/{pocket}/{component}/Packages.xz"), &destination)?) } else { None };
            raw.bytes(&format!("commands/download-{pocket}-{component}.stdout"), &output.stdout)?;
            raw.bytes(&format!("commands/download-{pocket}-{component}.stderr"), &output.stderr)?;
            output.success()?;
            let compressed = compressed.context("successful helper download produced no raw index")?;
            ensure!(compressed["bytes"].as_u64() == Some(size) && compressed["sha256"].as_str() == Some(hash.as_str()), "authenticated compressed index size/hash mismatch");
            let decoded_logical = format!("metadata/{pocket}/{component}/Packages");
            let decoded = raw.reserve(&decoded_logical, process::FILE_LIMIT)?;
            let mut decode = apt("/usr/lib/apt/apt-helper", &["cat-file"]); decode.arg(&destination);
            let decoded_result = process::run(&mut decode, deadline, 20, Some(&decoded));
            let (decoded_result, decoded_ref) = raw.settle(&decoded_logical, &decoded, decoded_result)?;
            fs::remove_file(&destination)?;
            raw.bytes(&format!("commands/decode-{pocket}-{component}.stderr"), &decoded_result.stderr)?;
            decoded_result.success()?;
            let (uncompressed_size, uncompressed_hash) = metadata::release_index(&fields, &meta_key)?;
            ensure!(decoded_ref["bytes"].as_u64() == Some(uncompressed_size) && decoded_ref["sha256"].as_str() == Some(uncompressed_hash.as_str()), "decoded index differs from authenticated uncompressed Release entry");
            // Also bind APT's recompressed cache to the very same plain index.
            let cached_plain = Path::new(WORK).join("cached-plain");
            let mut cat = apt("/usr/lib/apt/apt-helper", &["cat-file"]); cat.arg(cached);
            let cached_result = process::run(&mut cat, deadline, 20, Some(&cached_plain))?; cached_result.success()?;
            let cached_hash = evidence::hash_file(&cached_plain, process::FILE_LIMIT)?;
            ensure!(cached_hash == (uncompressed_size, uncompressed_hash.clone()), "APT cache representation does not match selected authenticated index"); fs::remove_file(cached_plain)?;
            let index_ref = format!("{pocket}/{component}/amd64");
            report["indices"].as_array_mut().context("indices array")?.push(json!({"id":index_ref,"pocket":pocket,"component":component,"architecture":"amd64","requested_url":url,"final_url":url,"redirects":"refused_by_policy","release_path":index_path,"published_bytes":size,"published_sha256":hash,"metadata_hash_match":true,"signature_verified":true,"compressed_raw":compressed,"decoded_raw":decoded_ref,"apt_cache_raw":cached_ref,"apt_cache_original_path":cached,"cache_matches_authenticated_plain_index":true}));
            index_sources.push((index_ref, raw.source(&decoded_ref)?));
        }
    }
    report["trust"]["signer_fingerprints"] = json!(all_signers);
    let before = state()?; report["state_before_solve_sha256"] = json!(before);
    let roots = config["roots"].as_array().context("capture roots")?;
    let roots = roots.iter().map(|root| root.as_str().context("root package string")).collect::<Result<Vec<_>>>()?;
    let mut simulate = apt("/usr/bin/apt-get", &["--simulate", "--no-install-recommends", "--no-remove", "install"]); simulate.args(&roots);
    let solved = recorded(raw, "apt-simulate", &mut simulate, deadline, 20)?; solved.success()?;
    let transaction = metadata::transaction(solved.text()?)?; report["transaction"] = json!(transaction);
    let mut print_uris = apt("/usr/bin/apt-get", &["--print-uris", "--download-only", "--assume-yes", "--no-install-recommends", "--no-remove", "install"]); print_uris.args(&roots);
    let downloads = recorded(raw, "apt-print-uris", &mut print_uris, deadline, 20)?; downloads.success()?;
    let downloads = metadata::uris(downloads.text()?)?; report["download_uris"] = json!(downloads);
    join_inventory(raw, report, &initial, &transaction, &downloads, &index_sources, deadline)?;
    for root in roots {
        ensure!(report["final_inventory"].as_array().context("final inventory")?.iter().any(|row| row["name"] == root && row["published_package_hash"] == true), "root package lacks an authenticated final row: {root}");
    }
    let after = state()?; report["state_after_capture_sha256"] = json!(after);
    metadata::same_state(&before, &after)?;
    ensure!(evidence::hash_file(Path::new("/var/lib/dpkg/status"), process::FILE_LIMIT)?.1 == base_status_hash, "base dpkg status changed");
    ensure!(evidence::hash_file(Path::new(KEYRING), process::FILE_LIMIT)?.1 == report["trust"]["archive_anchor"]["sha256"].as_str().context("initial anchor hash")?, "base archive trust anchor changed during capture");
    ensure!(report["unresolved_reasons"].as_array().context("unresolved array")?.is_empty(), "snapshot lock has required unresolved rows");
    Ok(())
}

fn recorded_download(command: &mut Command, deadline: Instant) -> Result<process::Output> { process::run(command, deadline, 20, None) }

fn package_row(fields: &Fields, index: &str) -> Result<Value> {
    let name = fields.get("Package").context("package name")?; metadata::package_name(name)?;
    let version = fields.get("Version").context("package version")?; metadata::version(version)?;
    let architecture = fields.get("Architecture").context("package architecture")?;
    ensure!(matches!(architecture.as_str(), "amd64" | "all"), "unexpected authenticated package architecture");
    let filename = fields.get("Filename").context("package Filename")?; metadata::safe_path(filename)?; ensure!(filename.starts_with("pool/") && !filename.contains(['%', '?', '#', '@']), "unsafe package Filename");
    let size = metadata::decimal(fields.get("Size").context("package Size")?)?; ensure!(size > 0, "empty Debian package payload identity");
    let hash = fields.get("SHA256").context("package SHA256")?; metadata::sha256(hash)?;
    Ok(json!({"name":name,"version":version,"architecture":architecture,"filename":filename,"size":size,"sha256":hash,"index_ref":index,"stanza_sha256":metadata::digest(&evidence::canonical(&json!(fields))?),"stanza_identity_encoding":"canonical_deb822_fields_JSON_v1","dependency_fields":fields,"metadata_hash_match":true,"signature_verified":true,"signature_scope":"authenticated_Release_to_index_to_package_hash_chain_not_an_independent_deb_signature","published_package_hash":true,"payload_sha256":"payload_not_downloaded"}))
}

fn join_inventory(raw: &mut Raw, report: &mut Value, initial: &[Value], transaction: &[Value], downloads: &[Value], sources: &[(String, std::path::PathBuf)], deadline: Instant) -> Result<()> {
    let mut expected = BTreeMap::new();
    for row in initial { expected.insert(row["name"].as_str().context("base name")?.to_string(), row.clone()); }
    for row in transaction {
        let name = row["name"].as_str().context("transaction name")?;
        let mut row = row.clone(); row["change"] = json!(if expected.contains_key(name) { "changed" } else { "new" });
        expected.insert(name.to_string(), row);
    }
    let mut matches = BTreeMap::<String, Vec<Value>>::new();
    let mut package_count = 0_u64;
    for (index, file) in sources {
        metadata::stanzas(BufReader::new(fs::File::open(file)?), |fields| {
            package_count = package_count.checked_add(1).context("aggregate package count overflow")?;
            ensure!(package_count <= 1_000_000, "aggregate package stanza count exceeded");
            ensure!(Instant::now() < deadline, "index parser deadline exceeded");
            if let Some(expected) = fields.get("Package").and_then(|name| expected.get(name)) {
                if metadata::selected_identity(&fields, expected) {
                    let name = fields.get("Package").context("selected package name")?.clone();
                    let rows = matches.entry(name).or_default();
                    ensure!(!rows.iter().any(|row| row["index_ref"] == index.as_str()), "duplicate exact package identity inside one index");
                    rows.push(package_row(&fields, index)?);
                }
            }
            Ok(())
        })?;
        report["parsed_package_stanzas"] = json!(package_count);
    }
    let mut final_rows = Vec::new(); let mut authenticated = BTreeMap::new();
    for (name, expected) in expected {
        match metadata::require_match(matches.get(&name).map_or(&[], Vec::as_slice), &format!("{name}:{}={}", expected["architecture"], expected["version"])) {
            Ok(mut row) => { row["origin"] = expected["origin"].clone(); row["change"] = expected["change"].clone(); row["base_inventory_row"] = initial.iter().find(|r| r["name"] == name).cloned().unwrap_or(Value::Null); authenticated.insert(name, row.clone()); final_rows.push(row); },
            Err(error) => {
                let mut row = expected; row["snapshot_status"] = json!("unresolved");
                row["metadata_hash_match"] = json!(false); row["signature_verified"] = json!(false); row["published_package_hash"] = json!(false); row["payload_sha256"] = json!("payload_not_downloaded"); row["unresolved_reason"] = json!(error.to_string());
                row["oci_provenance"] = json!(if row["origin"] == "unchanged_base" { "base_only_exact_OCI_inventory_no_fabricated_deb_hash" } else { "transaction_missing_authenticated_snapshot_stanza" });
                report["unresolved_reasons"].as_array_mut().context("unresolved array")?.push(json!(error.to_string())); final_rows.push(row);
            }
        }
    }
    report["final_inventory"] = json!(final_rows);
    for download in downloads {
        let selected: Vec<_> = authenticated.values().filter(|row| download["url"].as_str() == Some(&format!("{}/{}",metadata::SNAPSHOT,row["filename"].as_str().unwrap_or(""))) && row["size"] == download["size"]).collect();
        ensure!(selected.len() == 1, "URI does not uniquely join exact authenticated Filename/Size");
    }
    for row in transaction {
        let name = row["name"].as_str().context("transaction name")?;
        let package = authenticated.get(name).context("APT transaction package has no authenticated stanza")?;
        ensure!(downloads.iter().any(|download| download["url"].as_str() == Some(&format!("{}/{}",metadata::SNAPSHOT,package["filename"].as_str().unwrap_or(""))) && download["size"] == package["size"]), "APT Inst lacks a corresponding URI");
    }
    dependencies(raw, report, &authenticated, deadline)?;
    Ok(())
}

fn dependencies(raw: &mut Raw, report: &mut Value, packages: &BTreeMap<String, Value>, deadline: Instant) -> Result<()> {
    let mut providers = BTreeMap::<String, Vec<(String, Option<String>)>>::new();
    for (name, package) in packages {
        providers.entry(name.clone()).or_default().push((name.clone(), package["version"].as_str().map(str::to_string)));
        if let Some(provides) = package["dependency_fields"]["Provides"].as_str() {
            for group in provides.split(',') {
                let terms = metadata::dependency_terms(group)?; ensure!(terms.len() == 1, "invalid Provides alternatives");
                let term = &terms[0]; ensure!(term.qualifier.is_none() && term.constraint.as_ref().is_none_or(|(op,_)| op == "="), "invalid Provides relation or qualifier");
                providers.entry(term.name.clone()).or_default().push((name.clone(), term.constraint.as_ref().map(|(_,v)| v.clone())));
            }
        }
    }
    let mut comparison = 0_usize;
    for (name, package) in packages {
        for kind in ["Pre-Depends", "Depends"] {
            if let Some(dependencies) = package["dependency_fields"][kind].as_str() {
                for group in dependencies.split(',') {
                    let mut chosen = BTreeSet::new(); let mut clauses = Vec::new();
                    for term in metadata::dependency_terms(group)? {
                        for (candidate, version) in providers.get(&term.name).into_iter().flatten() {
                            let selected = packages.get(candidate).context("selected provider package")?;
                            if !metadata::qualifier_matches(term.qualifier.as_deref(), selected["dependency_fields"]["Multi-Arch"].as_str(), selected["architecture"].as_str().context("provider architecture")?)? { continue; }
                            if !metadata::provider_version_available(term.constraint.is_some(), version.as_deref()) { continue; }
                            let satisfies = if let Some((operator, required)) = &term.constraint {
                                if let Some(version) = version {
                                    comparison = comparison.checked_add(1).context("dependency comparisons overflow")?; ensure!(comparison <= 100_000, "dependency comparison bound exceeded");
                                    let mut compare = process::command("/usr/bin/dpkg"); compare.args(["--compare-versions", version, operator, required]);
                                    let output = process::run(&mut compare, deadline, 2, None)?;
                                    ensure!(output.failure.is_none(), "dpkg version comparison incomplete");
                                    ensure!(matches!(output.status.code(), Some(0 | 1)), "dpkg version comparison failed");
                                    output.status.success()
                                } else { false }
                            } else { true };
                            if satisfies {
                                let identity = format!("{candidate}:{}={}", selected["architecture"].as_str().context("provider architecture")?, selected["version"].as_str().context("provider version")?);
                                chosen.insert(identity.clone());
                                clauses.push(json!({"term":term.name,"qualifier":term.qualifier,"constraint":term.constraint,"satisfier":identity,"provided_version":version}));
                            }
                        }
                    }
                    let chosen: Vec<_> = chosen.into_iter().collect();
                    let providers = metadata::provider_set(&chosen).with_context(|| format!("{name} {kind}: {group}"))?;
                    report["provider_and_dependency_records"].as_array_mut().context("dependency records")?.push(json!({"package":name,"kind":kind,"constraint_and_alternatives":group,"satisfying_actual_final_inventory_members":providers,"satisfied_clauses":clauses,"solver_chosen_provider":"unobserved_in_APT_simulation_output","choice_evidence":"satisfier_set_of_actual_APT_expected_final_inventory_not_a_custom_solver_choice","version_comparator":"actual_base_dpkg_compare_versions"}));
                }
            }
        }
    }
    raw.bytes("solver/provider-and-dependency-records.json", &evidence::canonical(&report["provider_and_dependency_records"])?)?;
    Ok(())
}
