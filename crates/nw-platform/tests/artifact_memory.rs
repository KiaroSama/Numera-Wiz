//! Hosted isolated reader memory evidence, not aggregate application RSS qualification.
#![cfg(target_os = "linux")]
use nw_domain::{artifacts::ARTIFACT_CHUNK_BYTES, records::FileIdentity};
use nw_platform::{artifacts::ArtifactReader, files};
use sha2::{Digest, Sha256};
use std::io::Write;
use tokio_util::sync::CancellationToken;
#[test]
#[ignore = "Hosted CI isolated transfer memory journey"]
fn artifact_larger_than_selected_reader_memory_budget_streams_exactly() {
    let root = tempfile::tempdir().unwrap();
    let output = root.path().join("large.bin");
    let proof = root.path().join("proof.bin");
    let mut file = std::fs::File::create(&output).unwrap();
    let block = (0..ARTIFACT_CHUNK_BYTES)
        .map(|n| (n % 251) as u8)
        .collect::<Vec<_>>();
    let mut oracle = Sha256::new();
    for _ in 0..2048 {
        file.write_all(&block).unwrap();
        oracle.update(&block);
    }
    file.write_all(&block[..17]).unwrap();
    oracle.update(&block[..17]);
    file.sync_all().unwrap();
    drop(file);
    let bytes = 256 * 1024 * 1024u64 + 17;
    let expected = hex::encode(oracle.finalize());
    std::fs::hard_link(&output, &proof).unwrap();
    let identity = FileIdentity {
        path: files::path_text(&std::fs::canonicalize(&output).unwrap()).unwrap(),
        bytes: bytes.to_string(),
        sha256: expected.clone(),
    };
    let mut reader = ArtifactReader::open(&identity, &proof, CancellationToken::new()).unwrap();
    let mut observed = Sha256::new();
    let mut offset = 0;
    while offset < bytes {
        let chunk = reader.read_range(offset, ARTIFACT_CHUNK_BYTES).unwrap();
        observed.update(&chunk);
        offset += chunk.len() as u64;
    }
    assert_eq!(offset, bytes);
    assert_eq!(hex::encode(observed.finalize()), expected);
    let status = std::fs::read_to_string("/proc/self/status").unwrap();
    let high_kib = status
        .lines()
        .find_map(|line| line.strip_prefix("VmHWM:"))
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .parse::<u64>()
        .unwrap();
    assert!(
        high_kib < 128 * 1024,
        "Isolated artifact reader exceeded 128 MiB RSS: {high_kib} KiB"
    );
    println!(
        "Artifact bytes={bytes}; isolated process VmHWM={high_kib} KiB; reader budget=131072 KiB; exact SHA-256={expected}"
    );
    drop(reader);
    assert_eq!(
        std::fs::read_dir(root.path()).unwrap().count(),
        2,
        "Temporary block index is removed after transfer"
    );
}
