//! Owned evidence files and a chunked, hash-addressed transport manifest.
use super::{metadata, process};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, fs::{self, File, OpenOptions}, io::{Read, Write}, path::{Path, PathBuf}};

pub const RAW_LIMIT: u64 = 512 * 1024 * 1024;
pub const SMALL_LIMIT: u64 = 8 * 1024 * 1024;
pub const CHUNK_LIMIT: u64 = 8 * 1024 * 1024;

pub fn hash_file(path: &Path, limit: u64) -> Result<(u64, String)> {
    let entry = fs::symlink_metadata(path)?;
    ensure!(entry.is_file() && !entry.file_type().is_symlink() && entry.len() <= limit, "non-regular or oversized evidence file");
    let mut file = File::open(path)?;
    let mut hash = Sha256::new(); let mut size = 0;
    let mut buffer = [0_u8; 64 * 1024];
    loop { let count = file.read(&mut buffer)?; if count == 0 { break; } size = process::check_count(size, count as u64, limit)?; hash.update(&buffer[..count]); }
    ensure!(size == entry.len(), "evidence file changed during hashing");
    Ok((size, hex::encode(hash.finalize())))
}

pub fn write_new(path: &Path, bytes: &[u8], limit: u64) -> Result<()> {
    ensure!(bytes.len() as u64 <= limit, "evidence write exceeds limit");
    let mut file = OpenOptions::new().create_new(true).write(true).open(path)?;
    file.write_all(bytes)?; file.sync_all()?; Ok(())
}

pub fn canonical(value: &Value) -> Result<Vec<u8>> {
    fn ordered(value: &Value) -> Value {
        match value {
            Value::Object(map) => {
                let sorted: std::collections::BTreeMap<_, _> = map.iter().map(|(k,v)| (k.clone(), ordered(v))).collect();
                Value::Object(sorted.into_iter().collect())
            },
            Value::Array(rows) => Value::Array(rows.iter().map(ordered).collect()),
            other => other.clone(),
        }
    }
    let mut bytes = serde_json::to_vec(&ordered(value))?; bytes.push(b'\n'); Ok(bytes)
}

pub fn parse_canonical(bytes: &[u8]) -> Result<Value> {
    ensure!(bytes.len() as u64 <= SMALL_LIMIT, "oversized canonical evidence JSON");
    let value: Value = serde_json::from_slice(bytes)?;
    ensure!(canonical(&value)? == bytes, "noncanonical or duplicate-key evidence JSON refused");
    Ok(value)
}

pub struct Raw {
    directory: PathBuf,
    entries: Vec<Value>,
    seen: BTreeSet<String>,
    total: u64,
}

impl Raw {
    pub fn new(directory: &Path) -> Result<Self> {
        fs::create_dir(directory)?;
        Ok(Self { directory: directory.into(), entries: Vec::new(), seen: BTreeSet::new(), total: 0 })
    }
    pub fn fresh(&self) -> PathBuf { self.directory.join(format!("input-{:04}", self.entries.len())) }
    pub fn adopt(&mut self, logical: &str, file: &Path) -> Result<Value> {
        metadata::unique_path(&mut self.seen, logical)?;
        ensure!(self.entries.len() < 500 && file == self.fresh(), "raw file count/path exceeded");
        let (size, hash) = hash_file(file, process::FILE_LIMIT)?;
        self.total = process::check_count(self.total, size, RAW_LIMIT)?;
        let entry = json!({"logical_path":logical,"bytes":size,"sha256":hash,"storage":file.file_name().context("raw filename")?.to_str().context("UTF8 raw filename")?});
        self.entries.push(entry.clone()); Ok(entry)
    }
    pub fn bytes(&mut self, logical: &str, bytes: &[u8]) -> Result<Value> {
        let path = self.fresh(); write_new(&path, bytes, process::FILE_LIMIT)?; self.adopt(logical, &path)
    }
    pub fn copy(&mut self, logical: &str, source: &Path) -> Result<Value> {
        let metadata = fs::symlink_metadata(source)?;
        ensure!(metadata.is_file() && !metadata.file_type().is_symlink() && metadata.len() <= process::FILE_LIMIT, "unsafe raw input file");
        let path = self.fresh();
        let mut output = OpenOptions::new().create_new(true).write(true).open(&path)?;
        let mut input = File::open(source)?.take(process::FILE_LIMIT + 1);
        let size = std::io::copy(&mut input, &mut output)?;
        ensure!(size <= process::FILE_LIMIT && size == metadata.len(), "raw input grew or exceeded limit"); output.sync_all()?;
        self.adopt(logical, &path)
    }
    pub fn source(&self, entry: &Value) -> Result<PathBuf> {
        let name = entry["storage"].as_str().context("raw storage reference")?; metadata::safe_path(name)?; Ok(self.directory.join(name))
    }
    pub fn tree(&mut self, label: &str, path: &Path) -> Result<()> {
        let entry = match fs::symlink_metadata(path) {
            Ok(entry) => entry,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => { self.bytes(&format!("{label}/absent"), b"absent\n")?; return Ok(()); },
            Err(error) => return Err(error.into()),
        };
        ensure!(!entry.file_type().is_symlink(), "symlink in captured state");
        if entry.is_file() { self.copy(label, path)?; }
        else {
            ensure!(entry.is_dir(), "special input in captured state");
            let mut rows = fs::read_dir(path)?.collect::<std::io::Result<Vec<_>>>()?;
            rows.sort_by_key(|row| row.file_name());
            for row in rows {
                let name = row.file_name().into_string().map_err(|_| anyhow::anyhow!("non-UTF8 captured state name"))?;
                metadata::safe_path(&name)?;
                self.tree(&format!("{label}/{name}"), &row.path())?;
            }
        }
        Ok(())
    }
    pub fn finish(mut self, output: &Path) -> Result<Value> {
        let chunk_directory = output.join("raw"); fs::create_dir(&chunk_directory)?;
        let mut count = 0_usize;
        for entry in &mut self.entries {
            let source = self.directory.join(entry["storage"].as_str().context("raw storage")?);
            let mut input = File::open(&source)?; let mut chunks = Vec::new(); let mut total = 0_u64;
            loop {
                let mut bytes = Vec::new(); (&mut input).take(CHUNK_LIMIT).read_to_end(&mut bytes)?;
                if bytes.is_empty() { break; }
                ensure!(count < 500, "raw transport file count exceeded");
                let name = format!("chunk-{count:04}"); count += 1;
                write_new(&chunk_directory.join(&name), &bytes, CHUNK_LIMIT)?;
                total = process::check_count(total, bytes.len() as u64, process::FILE_LIMIT)?;
                chunks.push(json!({"path":format!("raw/{name}"),"bytes":bytes.len(),"sha256":metadata::digest(&bytes)}));
            }
            ensure!(entry["bytes"].as_u64() == Some(total), "raw data changed during chunking");
            entry.as_object_mut().context("raw entry object")?.remove("storage"); entry["chunks"] = json!(chunks);
            fs::remove_file(source)?;
        }
        fs::remove_dir(&self.directory)?;
        Ok(json!({"schema_version":1,"encoding":"ordered_concatenation_of_sha256_checked_chunks","raw_bytes":self.total,"raw_files":self.entries.len(),"transport_files":count,"files":self.entries}))
    }
}

pub fn write_small(directory: &Path, report: &Value, manifest: &Value) -> Result<()> {
    let report_bytes = canonical(report)?; let raw_bytes = canonical(manifest)?;
    let report_hash = format!("{}  report.json\n", metadata::digest(&report_bytes));
    let raw_hash = format!("{}  raw-manifest.json\n", metadata::digest(&raw_bytes));
    let total = report_bytes.len().checked_add(raw_bytes.len()).and_then(|n| n.checked_add(report_hash.len() + raw_hash.len())).context("small report bytes overflow")?;
    ensure!(total as u64 <= SMALL_LIMIT, "combined small report exceeds 8MiB");
    write_new(&directory.join("report.json"), &report_bytes, SMALL_LIMIT)?;
    write_new(&directory.join("raw-manifest.json"), &raw_bytes, SMALL_LIMIT)?;
    write_new(&directory.join("report.sha256"), report_hash.as_bytes(), 1024)?;
    write_new(&directory.join("raw-manifest.sha256"), raw_hash.as_bytes(), 1024)?;
    Ok(())
}

pub fn state(path: &Path) -> Result<String> {
    fn visit(path: &Path, label: &str, hash: &mut Sha256, count: &mut usize) -> Result<()> {
        *count += 1; ensure!(*count <= 500, "state file count exceeded");
        let entry = fs::symlink_metadata(path)?; ensure!(!entry.file_type().is_symlink(), "symlink in APT/dpkg state");
        hash.update((label.len() as u64).to_be_bytes()); hash.update(label.as_bytes());
        if entry.is_file() { let (size, digest) = hash_file(path, process::FILE_LIMIT)?; hash.update(size.to_be_bytes()); hash.update(digest.as_bytes()); }
        else {
            ensure!(entry.is_dir(), "special file in APT/dpkg state");
            let mut entries = fs::read_dir(path)?.collect::<std::io::Result<Vec<_>>>()?; entries.sort_by_key(|r| r.file_name());
            for row in entries { let name = row.file_name().into_string().map_err(|_| anyhow::anyhow!("non-UTF8 state path"))?; metadata::safe_path(&name)?; visit(&row.path(), &format!("{label}/{name}"), hash, count)?; }
        }
        Ok(())
    }
    let mut hash = Sha256::new(); let mut count = 0; visit(path, "state", &mut hash, &mut count)?; Ok(hex::encode(hash.finalize()))
}
