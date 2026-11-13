//! Plain synthetic checks; these never authenticate an archive or launch a process.
use super::metadata;
use anyhow::{Result, ensure};
use std::io::Cursor;

pub fn run() -> Result<()> {
    let release = "Suite: noble\nCodename: noble\nArchitectures: amd64\nComponents: main restricted universe multiverse\nDate: Sun, 01 Nov 2026 00:00:00 UTC\nSHA256:\n ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad 3 main/binary-amd64/Packages.xz\n";
    let fields = metadata::fields(release)?;
    let entry = metadata::release_index(&fields, "main/binary-amd64/Packages.xz")?;
    ensure!(entry.0 == 3 && entry.1 == "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad", "fixed Release oracle");
    metadata::check_bytes(b"abc", entry.0, &entry.1)?;
    ensure!(metadata::check_bytes(b"abd", entry.0, &entry.1).is_err(), "bad digest refused");
    ensure!(metadata::check_bytes(b"abc", 4, &entry.1).is_err(), "bad size refused");
    ensure!(metadata::fields("Package: a\nPackage: b\n").is_err(), "duplicate fields refused");
    ensure!(metadata::fields("Package: a\npackage: b\n").is_err(), "case-aliased fields refused");
    for path in ["../x", "/x", "x/../y", "x\\y", "x//y", "x/./y"] {
        ensure!(metadata::safe_path(path).is_err(), "path traversal refused: {path}");
    }
    ensure!(metadata::decimal("18446744073709551616").is_err(), "integer overflow refused");
    ensure!(metadata::decimal("-1").is_err(), "signed size refused");
    let inventory = metadata::inventory("libx\tamd64\t2:1.0-1\tinstall ok installed\n")?;
    ensure!(inventory.len() == 1 && inventory[0]["version"] == "2:1.0-1", "dpkg epoch preserved");
    ensure!(metadata::inventory("x\tarm64\t1\tinstall ok installed\n").is_err(), "architecture mismatch refused");
    ensure!(metadata::inventory("x\tamd64\t1\tinstall ok installed").is_err(), "truncated dpkg output refused");
    let identity = metadata::fields("Package: libx\nArchitecture: amd64\nVersion: 2:1.0-1\n")?;
    ensure!(metadata::selected_identity(&identity, &inventory[0]), "exact epoch and architecture join");
    ensure!(!metadata::selected_identity(&identity, &serde_json::json!({"name":"libx","architecture":"amd64","version":"1.0-1"})), "epoch mismatch refused");
    ensure!(!metadata::selected_identity(&identity, &serde_json::json!({"name":"libx","architecture":"all","version":"2:1.0-1"})), "index architecture mismatch refused");
    let plan = metadata::transaction("Inst libx [1:1] (2:1.0-1 Ubuntu:24.04/noble [amd64])\nConf libx (2:1.0-1 Ubuntu:24.04/noble [amd64])\n")?;
    ensure!(plan.len() == 1 && plan[0]["version"] == "2:1.0-1", "solver exact identity");
    ensure!(metadata::transaction("Remv libx [1]\n").is_err(), "Remv refused");
    ensure!(metadata::transaction("Inst libx (1 Ubuntu [arm64])\n").is_err(), "solver architecture refused");
    ensure!(metadata::transaction("Inst libx (1 Ubuntu [amd64])").is_err(), "truncated solver output refused");
    let uri = "'https://snapshot.ubuntu.com/ubuntu/20261101T000000Z/pool/main/x/x.deb' x.deb 3 MD5Sum:00000000000000000000000000000000\n";
    let uris = metadata::uris(uri)?;
    ensure!(uris.len() == 1 && uris[0]["size"] == 3 && uris[0].get("sha256").is_none(), "URI MD5 never promoted to SHA256");
    ensure!(metadata::uris(&uri.replace("snapshot.ubuntu.com", "archive.ubuntu.com")).is_err(), "foreign URI refused");
    let mut seen = std::collections::BTreeSet::new();
    metadata::unique_path(&mut seen, "raw/A")?;
    ensure!(metadata::unique_path(&mut seen, "raw/a").is_err(), "case alias refused");
    let mut parsed = Vec::new();
    metadata::stanzas(Cursor::new(b"Package: x\nVersion: 1\n\n"), |s| { parsed.push(s); Ok(()) })?;
    ensure!(parsed.len() == 1, "streamed stanza count");
    ensure!(metadata::stanzas(Cursor::new(b"Package: x"), |_| Ok(())).is_err(), "truncated index refused");
    let oversized = format!("Package: x\nDescription: {}\n\n", "a".repeat(1024 * 1024));
    ensure!(metadata::stanzas(Cursor::new(oversized.as_bytes()), |_| Ok(())).is_err(), "oversized stanza refused");
    ensure!(metadata::signature_status(false, "[GNUPG:] VALIDSIG 0123456789012345678901234567890123456789 2026-11-01 0 0 4 0 1 8 01 0123456789012345678901234567890123456789\n").is_err(), "bad signature exit refused even with status token");
    ensure!(metadata::signature_status(true, "[GNUPG:] BADSIG 0123456789012345 synthetic\n").is_err(), "bad signature status refused");
    ensure!(metadata::same_state("a", "b").is_err(), "changed state refused");
    ensure!(metadata::require_match(&[], "missing base version").is_err(), "base version gap refused");
    ensure!(metadata::require_match(&[serde_json::json!({"sha256":"a"}), serde_json::json!({"sha256":"b"})], "ambiguous").is_err(), "differing stanzas refused");
    ensure!(metadata::dependency_terms("libx (>= 2:1.0-1) | virtual-x:any")?.len() == 2, "dependency alternatives retained");
    ensure!(metadata::provider_set(&[]).is_err(), "unresolved provider refused");
    ensure!(metadata::provider_set(&["mawk:amd64=1.3.4".into(), "gawk:amd64=5.2.1".into()])?.len() == 2, "two already selected satisfying providers retained without inventing solver choice");
    ensure!(!metadata::provider_version_available(true, None), "unversioned Provides cannot satisfy versioned dependency");
    ensure!(metadata::provider_version_available(false, None), "unversioned virtual dependency accepts unversioned Provides");
    ensure!(metadata::qualifier_matches(Some("any"), Some("allowed"), "amd64")?, "any qualifier requires explicit allowed metadata");
    ensure!(!metadata::qualifier_matches(Some("any"), Some("foreign"), "amd64")?, "foreign is not silently substituted for allowed");
    ensure!(metadata::qualifier_matches(Some("unsupported"), None, "amd64").is_err(), "unproved qualifier semantics refused");
    ensure!(super::evidence::parse_canonical(b"{\"x\":1,\"x\":2}\n").is_err(), "duplicate evidence JSON refused");
    ensure!(metadata::single_tar(&tar_fixture(), "fixture", 3)? == b"abc", "fixed ustar file oracle");
    ensure!(metadata::single_tar(&tar_fixture(), "fixture", 2).is_err(), "tar size bound refused");
    ensure!(metadata::single_tar(&tar_fixture()[..1024], "fixture", 3).is_err(), "truncated tar refused");
    ensure!(metadata::single_tar(&tar_fixture(), "../fixture", 3).is_err(), "tar traversal refused");
    let mut bad_tar = tar_fixture(); bad_tar[156] = b'2';
    ensure!(metadata::single_tar(&bad_tar, "fixture", 3).is_err(), "tar link refused");
    let mut duplicate_tar = tar_fixture(); duplicate_tar[1024] = b'x';
    ensure!(metadata::single_tar(&duplicate_tar, "fixture", 3).is_err(), "extra tar entry refused");
    super::process::check_count(16 * 1024 * 1024, 1, 16 * 1024 * 1024).expect_err("output overflow must fail");
    ensure!(super::process::signal_target(0).is_err() && super::process::signal_target(1).is_err() && super::process::signal_target(u32::MAX).is_err(), "unsafe group identifiers refused without signalling");
    ensure!(super::process::signal_target(42)? == 42, "owned numeric group framing");
    tracing::info!("Linux capture synthetic self-check passed; no APT, signature, container or runtime truth is inferred");
    Ok(())
}

fn tar_fixture() -> Vec<u8> {
    // Independent fixed ustar fixture. Its header checksum is the literal octal
    // 007337, computed once outside the implementation; no production hash or
    // checksum routine constructs this oracle.
    let mut bytes = vec![0_u8; 2048];
    for (offset, value) in [(0, b"fixture".as_slice()), (100, b"0000600\0"), (108, b"0000000\0"), (116, b"0000000\0"), (124, b"00000000003\0"), (136, b"00000000000\0"), (148, b"007337\0 "), (156, b"0"), (257, b"ustar\0"), (263, b"00"), (512, b"abc")] {
        bytes[offset..offset + value.len()].copy_from_slice(value);
    }
    bytes
}
