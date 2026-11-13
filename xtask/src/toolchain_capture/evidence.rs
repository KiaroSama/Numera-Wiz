//! Owned evidence files and a chunked, hash-addressed transport manifest.
use super::{metadata, process};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

pub const RAW_LIMIT: u64 = 512 * 1024 * 1024;
pub const SMALL_LIMIT: u64 = 8 * 1024 * 1024;
pub const CHUNK_LIMIT: u64 = 8 * 1024 * 1024;

pub fn hash_file(path: &Path, limit: u64) -> Result<(u64, String)> {
    let entry = fs::symlink_metadata(path)?;
    ensure!(
        entry.is_file() && !entry.file_type().is_symlink() && entry.len() <= limit,
        "non-regular or oversized evidence file"
    );
    let mut file = File::open(path)?;
    let mut hash = Sha256::new();
    let mut size = 0;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        size = process::check_count(size, count as u64, limit)?;
        hash.update(&buffer[..count]);
    }
    ensure!(size == entry.len(), "evidence file changed during hashing");
    Ok((size, hex::encode(hash.finalize())))
}

pub fn write_new(path: &Path, bytes: &[u8], limit: u64) -> Result<()> {
    ensure!(bytes.len() as u64 <= limit, "evidence write exceeds limit");
    let mut file = OpenOptions::new().create_new(true).write(true).open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

// The caller reserves one physical slot and bytes BEFORE calling this helper.
// A successful create establishes ownership even if write/sync subsequently fails.
// Every created file is either hashed (including a short prefix), removed, or
// explicitly returned UNKNOWN with its full reservation still charged.
pub struct ChunkWrite {
    pub chunk: Option<Value>,
    pub error: Option<String>,
}

pub fn write_chunk_with(
    path: &Path,
    relative: &str,
    bytes: &[u8],
    writer: impl FnOnce(&mut File, &[u8]) -> std::io::Result<()>,
) -> ChunkWrite {
    let mut file = match OpenOptions::new().create_new(true).write(true).open(path) {
        Ok(file) => file,
        Err(error) => {
            return ChunkWrite {
                chunk: None,
                error: Some(format!("create owned chunk: {error}")),
            };
        }
    };
    let written = writer(&mut file, bytes).and_then(|()| file.sync_all());
    drop(file);
    match hash_file(path, CHUNK_LIMIT) {
        Ok((size, hash)) if size <= bytes.len() as u64 => ChunkWrite {
            chunk: Some(json!({"path":relative,"bytes":size,"sha256":hash})),
            error: written
                .err()
                .map(|error| format!("write owned chunk: {error}"))
                .or_else(|| {
                    (size != bytes.len() as u64).then(|| "short owned chunk write".to_string())
                }),
        },
        observed => {
            let reason =
                format!("owned chunk identity unavailable: {observed:?}; write={written:?}");
            match fs::remove_file(path) {
                Ok(()) => ChunkWrite {
                    chunk: None,
                    error: Some(reason),
                },
                Err(error) => ChunkWrite {
                    chunk: Some(
                        json!({"path":relative,"bytes":bytes.len(),"sha256":null,"integrity":"UNKNOWN","reserved_upper_bound":true}),
                    ),
                    error: Some(format!("{reason}; removal failed: {error}")),
                },
            }
        }
    }
}

pub struct Finalized {
    pub manifest: Value,
    pub errors: Vec<String>,
}

pub fn canonical(value: &Value) -> Result<Vec<u8>> {
    fn ordered(value: &Value) -> Value {
        match value {
            Value::Object(map) => {
                let sorted: std::collections::BTreeMap<_, _> =
                    map.iter().map(|(k, v)| (k.clone(), ordered(v))).collect();
                Value::Object(sorted.into_iter().collect())
            }
            Value::Array(rows) => Value::Array(rows.iter().map(ordered).collect()),
            other => other.clone(),
        }
    }
    let mut bytes = serde_json::to_vec(&ordered(value))?;
    bytes.push(b'\n');
    Ok(bytes)
}

pub fn parse_canonical(bytes: &[u8]) -> Result<Value> {
    ensure!(
        bytes.len() as u64 <= SMALL_LIMIT,
        "oversized canonical evidence JSON"
    );
    let value: Value = serde_json::from_slice(bytes)?;
    ensure!(
        canonical(&value)? == bytes,
        "noncanonical or duplicate-key evidence JSON refused"
    );
    Ok(value)
}

pub struct Raw {
    directory: PathBuf,
    entries: Vec<Value>,
    seen: BTreeSet<String>,
    total: u64,
    pending: BTreeMap<PathBuf, (String, u64)>,
    sequence: usize,
}

impl Raw {
    pub fn new(directory: &Path) -> Result<Self> {
        fs::create_dir(directory)?;
        Ok(Self {
            directory: directory.into(),
            entries: Vec::new(),
            seen: BTreeSet::new(),
            total: 0,
            pending: BTreeMap::new(),
            sequence: 0,
        })
    }
    pub fn reserve(&mut self, logical: &str, maximum: u64) -> Result<PathBuf> {
        metadata::safe_path(logical)?;
        ensure!(
            !self.seen.contains(&logical.to_ascii_lowercase()),
            "duplicate raw logical path"
        );
        ensure!(
            maximum <= process::FILE_LIMIT && self.entries.len() + self.pending.len() < 500,
            "raw pending file/count bound exceeded"
        );
        let next = process::check_count(self.total, maximum, RAW_LIMIT)?;
        let path = self.directory.join(format!("input-{:04}", self.sequence));
        self.sequence = self
            .sequence
            .checked_add(1)
            .context("raw sequence overflow")?;
        self.seen.insert(logical.to_ascii_lowercase());
        self.total = next;
        self.pending
            .insert(path.clone(), (logical.to_string(), maximum));
        Ok(path)
    }
    pub fn cancel(&mut self, file: &Path) -> Result<()> {
        let (logical, reserved) = self
            .pending
            .get(file)
            .context("cancel requires an owned pending file")?
            .clone();
        match fs::remove_file(file) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(error)
                    .context("pending file remains owned and reserved until container cleanup");
            }
        }
        self.pending.remove(file);
        self.seen.remove(&logical.to_ascii_lowercase());
        self.total = self
            .total
            .checked_sub(reserved)
            .context("pending reservation underflow")?;
        Ok(())
    }
    pub fn adopt(&mut self, logical: &str, file: &Path) -> Result<Value> {
        let result: Result<Value> = (|| {
            let (owner, reserved) = self
                .pending
                .get(file)
                .context("adoption requires a reserved pending file")?;
            ensure!(owner == logical, "pending logical identity mismatch");
            let (size, hash) = hash_file(file, *reserved)?;
            let entry = json!({"logical_path":logical,"bytes":size,"sha256":hash,"storage":file.file_name().context("raw filename")?.to_str().context("UTF8 raw filename")?});
            self.total = self
                .total
                .checked_sub(*reserved)
                .and_then(|n| n.checked_add(size))
                .context("raw reservation arithmetic")?;
            self.pending.remove(file);
            self.entries.push(entry.clone());
            Ok(entry)
        })();
        if result.is_err() && self.pending.contains_key(file) {
            if let Err(cleanup) = self.cancel(file) {
                return result.with_context(|| format!("pending cancellation failed: {cleanup:#}"));
            }
        }
        result
    }
    pub fn settle<T>(
        &mut self,
        logical: &str,
        file: &Path,
        result: Result<T>,
    ) -> Result<(T, Value)> {
        match result {
            Ok(value) => Ok((value, self.adopt(logical, file)?)),
            Err(error) => {
                let cleanup = self.cancel(file);
                Err(error).with_context(|| format!("pending decoder cleanup: {cleanup:?}"))
            }
        }
    }
    pub fn bytes(&mut self, logical: &str, bytes: &[u8]) -> Result<Value> {
        let path = self.reserve(logical, bytes.len() as u64)?;
        if let Err(error) = write_new(&path, bytes, process::FILE_LIMIT) {
            let cleanup = self.cancel(&path);
            return Err(error).with_context(|| format!("pending write cleanup: {cleanup:?}"));
        }
        self.adopt(logical, &path)
    }
    pub fn copy(&mut self, logical: &str, source: &Path) -> Result<Value> {
        let metadata = fs::symlink_metadata(source)?;
        ensure!(
            metadata.is_file()
                && !metadata.file_type().is_symlink()
                && metadata.len() <= process::FILE_LIMIT,
            "unsafe raw input file"
        );
        let path = self.reserve(logical, metadata.len())?;
        let copied = (|| -> Result<()> {
            let mut output = OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&path)?;
            let mut input = File::open(source)?;
            let size = std::io::copy(&mut (&mut input).take(metadata.len()), &mut output)?;
            let mut extra = [0_u8; 1];
            ensure!(
                size == metadata.len() && input.read(&mut extra)? == 0,
                "raw input changed while copying"
            );
            output.sync_all()?;
            Ok(())
        })();
        if let Err(error) = copied {
            let cleanup = self.cancel(&path);
            return Err(error).with_context(|| format!("pending copy cleanup: {cleanup:?}"));
        }
        self.adopt(logical, &path)
    }
    pub fn source(&self, entry: &Value) -> Result<PathBuf> {
        let name = entry["storage"].as_str().context("raw storage reference")?;
        metadata::safe_path(name)?;
        Ok(self.directory.join(name))
    }
    pub fn tree(&mut self, label: &str, path: &Path) -> Result<()> {
        let entry = match fs::symlink_metadata(path) {
            Ok(entry) => entry,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                self.bytes(&format!("{label}/absent"), b"absent\n")?;
                return Ok(());
            }
            Err(error) => return Err(error.into()),
        };
        ensure!(!entry.file_type().is_symlink(), "symlink in captured state");
        if entry.is_file() {
            self.copy(label, path)?;
        } else {
            ensure!(entry.is_dir(), "special input in captured state");
            let mut rows = fs::read_dir(path)?.collect::<std::io::Result<Vec<_>>>()?;
            rows.sort_by_key(|row| row.file_name());
            for row in rows {
                let name = row
                    .file_name()
                    .into_string()
                    .map_err(|_| anyhow::anyhow!("non-UTF8 captured state name"))?;
                metadata::safe_path(&name)?;
                self.tree(&format!("{label}/{name}"), &row.path())?;
            }
        }
        Ok(())
    }
    pub fn finish(self, output: &Path) -> Finalized {
        self.finish_with(output, |file, bytes| file.write_all(bytes))
    }
    pub fn finish_with(
        mut self,
        output: &Path,
        mut writer: impl FnMut(&mut File, &[u8]) -> std::io::Result<()>,
    ) -> Finalized {
        let mut errors = Vec::new();
        let mut files = Vec::new();
        let mut count = 0_usize;
        let mut total = 0_u64;
        let mut sequence = 0_usize;
        for path in self.pending.keys().cloned().collect::<Vec<_>>() {
            if let Err(error) = self.cancel(&path) {
                errors.push(format!("pending finalization: {error:#}"));
            } else {
                errors.push(
                    "unadopted pending input cancelled during failed-capture finalization".into(),
                );
            }
        }
        let unexported = self.pending.iter().map(|(path,(logical,reserved))| json!({"path":path.file_name(),"logical_path":logical,"reserved_bytes":reserved,"cleanup":"still_owned_until_private_container_removal"})).collect::<Vec<_>>();
        let chunk_directory = output.join("raw");
        let ready = match fs::create_dir(&chunk_directory) {
            Ok(()) => true,
            Err(error) => {
                errors.push(format!("create raw export directory: {error}"));
                false
            }
        };
        if ready {
            for entry in &self.entries {
                let mut chunks = Vec::new();
                let mut bytes_written = 0_u64;
                let mut hash = Sha256::new();
                let mut identity_known = true;
                let finished = (|| -> Result<()> {
                    let source = self.source(entry)?;
                    let mut input = File::open(&source)?;
                    loop {
                        let mut bytes = Vec::new();
                        (&mut input).take(CHUNK_LIMIT).read_to_end(&mut bytes)?;
                        if bytes.is_empty() {
                            break;
                        }
                        process::check_count(
                            bytes_written,
                            bytes.len() as u64,
                            entry["bytes"].as_u64().context("admitted raw size")?,
                        )?;
                        ensure!(count < 500, "raw transport file count exceeded");
                        total = process::check_count(total, bytes.len() as u64, RAW_LIMIT)?;
                        count += 1;
                        let name = format!("chunk-{sequence:04}");
                        sequence += 1;
                        let relative = format!("raw/{name}");
                        let outcome = write_chunk_with(
                            &chunk_directory.join(&name),
                            &relative,
                            &bytes,
                            &mut writer,
                        );
                        match outcome.chunk {
                            Some(chunk) => {
                                let actual = chunk["bytes"]
                                    .as_u64()
                                    .context("owned chunk byte observation")?;
                                total = total
                                    .checked_sub(bytes.len() as u64 - actual)
                                    .context("chunk reservation reconciliation")?;
                                bytes_written = process::check_count(
                                    bytes_written,
                                    actual,
                                    process::FILE_LIMIT,
                                )?;
                                if chunk["sha256"].as_str()
                                    == Some(metadata::digest(&bytes[..actual as usize]).as_str())
                                {
                                    hash.update(&bytes[..actual as usize]);
                                } else {
                                    identity_known = false;
                                }
                                chunks.push(chunk);
                            }
                            None => {
                                total = total
                                    .checked_sub(bytes.len() as u64)
                                    .context("absent chunk reservation")?;
                                count -= 1;
                            }
                        }
                        if let Some(error) = outcome.error {
                            anyhow::bail!("{error}");
                        }
                    }
                    ensure!(
                        identity_known
                            && entry["bytes"].as_u64() == Some(bytes_written)
                            && entry["sha256"].as_str()
                                == Some(hex::encode(hash.clone().finalize()).as_str()),
                        "raw data changed during finalization"
                    );
                    fs::remove_file(source).context("remove finalized scratch input")?;
                    Ok(())
                })();
                let mut available = entry.clone();
                available
                    .as_object_mut()
                    .expect("authored raw object")
                    .remove("storage");
                available["expected_bytes"] = entry["bytes"].clone();
                available["expected_sha256"] = entry["sha256"].clone();
                available["bytes"] = json!(bytes_written);
                available["sha256"] = if identity_known {
                    json!(hex::encode(hash.finalize()))
                } else {
                    Value::Null
                };
                available["chunks"] = json!(chunks);
                available["capture_status"] = json!(if finished.is_ok() {
                    "complete"
                } else {
                    "incomplete"
                });
                if let Err(error) = finished {
                    errors.push(format!("finalize {}: {error:#}", entry["logical_path"]));
                }
                files.push(available);
            }
        }
        if let Err(error) = fs::remove_dir(&self.directory) {
            errors.push(format!("remove raw scratch directory: {error}"));
        }
        let manifest = json!({"schema_version":1,"encoding":"ordered_concatenation_of_sha256_checked_chunks","complete":errors.is_empty(),"accounting":"UNKNOWN chunks retain their full reserved upper bound","raw_bytes":total,"raw_files":files.len(),"transport_files":count,"files":files,"unexported_pending_inputs":unexported});
        Finalized { manifest, errors }
    }
}

pub fn write_small(directory: &Path, report: &Value, manifest: &Value) -> Result<()> {
    let report_bytes = canonical(report)?;
    let raw_bytes = canonical(manifest)?;
    let report_hash = format!("{}  report.json\n", metadata::digest(&report_bytes));
    let raw_hash = format!("{}  raw-manifest.json\n", metadata::digest(&raw_bytes));
    let total = report_bytes
        .len()
        .checked_add(raw_bytes.len())
        .and_then(|n| n.checked_add(report_hash.len() + raw_hash.len()))
        .context("small report bytes overflow")?;
    ensure!(
        total as u64 <= SMALL_LIMIT,
        "combined small report exceeds 8MiB"
    );
    let outputs = [
        ("report.json", report_bytes.as_slice()),
        ("raw-manifest.json", raw_bytes.as_slice()),
        ("report.sha256", report_hash.as_bytes()),
        ("raw-manifest.sha256", raw_hash.as_bytes()),
    ];
    for (name, _) in outputs {
        match fs::symlink_metadata(directory.join(name)) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Ok(_) => anyhow::bail!("small output already exists"),
            Err(error) => return Err(error).context("inspect fresh small-output path"),
        }
    }
    let mut attempted = Vec::new();
    for (name, bytes) in outputs {
        attempted.push(name);
        if let Err(error) = write_new(&directory.join(name), bytes, SMALL_LIMIT) {
            let mut failures = Vec::new();
            for owned in attempted {
                if let Err(cleanup) = fs::remove_file(directory.join(owned)) {
                    if cleanup.kind() != std::io::ErrorKind::NotFound {
                        failures.push(cleanup.to_string());
                    }
                }
            }
            return Err(error)
                .with_context(|| format!("partial small-output cleanup: {failures:?}"));
        }
    }
    Ok(())
}

pub fn state(path: &Path) -> Result<String> {
    fn visit(path: &Path, label: &str, hash: &mut Sha256, count: &mut usize) -> Result<()> {
        *count += 1;
        ensure!(*count <= 500, "state file count exceeded");
        let entry = fs::symlink_metadata(path)?;
        ensure!(!entry.file_type().is_symlink(), "symlink in APT/dpkg state");
        hash.update((label.len() as u64).to_be_bytes());
        hash.update(label.as_bytes());
        if entry.is_file() {
            let (size, digest) = hash_file(path, process::FILE_LIMIT)?;
            hash.update(size.to_be_bytes());
            hash.update(digest.as_bytes());
        } else {
            ensure!(entry.is_dir(), "special file in APT/dpkg state");
            let mut entries = fs::read_dir(path)?.collect::<std::io::Result<Vec<_>>>()?;
            entries.sort_by_key(|r| r.file_name());
            for row in entries {
                let name = row
                    .file_name()
                    .into_string()
                    .map_err(|_| anyhow::anyhow!("non-UTF8 state path"))?;
                metadata::safe_path(&name)?;
                visit(&row.path(), &format!("{label}/{name}"), hash, count)?;
            }
        }
        Ok(())
    }
    let mut hash = Sha256::new();
    let mut count = 0;
    visit(path, "state", &mut hash, &mut count)?;
    Ok(hex::encode(hash.finalize()))
}
