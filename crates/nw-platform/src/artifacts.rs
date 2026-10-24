//! Bounded verified reads from a retained publication object, never a caller path.
use crate::files::{self, SourceStamp};
use nw_domain::{DomainError, Result, artifacts::ARTIFACT_CHUNK_BYTES, records::FileIdentity};
use sha2::{Digest, Sha256};
use std::{fs::File, io::{Read, Seek, SeekFrom, Write}, path::Path, time::{Duration, Instant}};
use tokio_util::sync::CancellationToken;

fn changed() -> DomainError { DomainError::new("ARTIFACT_CHANGED", "Published artifact physical identity or verified bytes changed") }
/// The temporary fixed-size hash index is built while checking the complete digest.
/// It uses at most 64 MiB of disk for the 256 GiB admitted file bound, not RAM.
/// Every returned block must match that index; a changed file never supplies new bytes.
pub struct ArtifactReader {
    public: SourceStamp,
    proof: SourceStamp,
    file: File,
    index: tempfile::NamedTempFile,
    bytes: u64,
    cancel: CancellationToken,
    deadline: Instant,
}
impl ArtifactReader {
    pub fn open(identity: &FileIdentity, proof_path: &Path, cancel: CancellationToken) -> Result<Self> {
        let bytes = nw_domain::artifacts::decimal(&identity.bytes).map_err(|_| changed())?;
        if bytes > 256 * 1024 * 1024 * 1024 { return Err(DomainError::new("RESOURCE_LIMIT", "Artifact exceeds the 256 GiB admitted file bound")); }
        let public = SourceStamp::capture_resolved(Path::new(&identity.path)).map_err(|_| changed())?;
        let proof = SourceStamp::capture_resolved(proof_path).map_err(|_| changed())?;
        if !public.same_as(&proof) { return Err(changed()); }
        let file = proof.held_file().try_clone().map_err(files::io_error)?;
        if file.metadata().map_err(files::io_error)?.len() != bytes { return Err(changed()); }
        let parent = proof_path.parent().ok_or_else(changed)?;
        let index = tempfile::NamedTempFile::new_in(parent).map_err(files::io_error)?;
        let mut reader = Self { public, proof, file, index, bytes, cancel, deadline: Instant::now() + Duration::from_secs(120) };
        let mut buffer = vec![0; ARTIFACT_CHUNK_BYTES];
        let mut full = Sha256::new();
        let mut offset = 0u64;
        while offset < bytes {
            reader.check()?;
            let n = ((bytes - offset).min(ARTIFACT_CHUNK_BYTES as u64)) as usize;
            reader.file.read_exact(&mut buffer[..n]).map_err(|_| changed())?;
            full.update(&buffer[..n]);
            reader.index.write_all(&Sha256::digest(&buffer[..n])).map_err(files::io_error)?;
            offset += n as u64;
        }
        let mut extra = [0u8; 1];
        if reader.file.read(&mut extra).map_err(files::io_error)? != 0 || hex::encode(full.finalize()) != identity.sha256 { return Err(changed()); }
        reader.index.flush().map_err(files::io_error)?;
        reader.check()?;
        Ok(reader)
    }
    pub fn bytes(&self) -> u64 { self.bytes }
    pub fn check(&self) -> Result<()> {
        if self.cancel.is_cancelled() { return Err(DomainError::new("CANCELLED", "Artifact transfer cancelled")); }
        if Instant::now() >= self.deadline { return Err(DomainError::new("RESOURCE_LIMIT", "Artifact transfer exceeded its 120 second deadline")); }
        self.public.revalidate().map_err(|_| changed())?;
        self.proof.revalidate().map_err(|_| changed())?;
        Ok(())
    }
    pub fn read_range(&mut self, offset: u64, length: usize) -> Result<Vec<u8>> {
        if length == 0 || length > ARTIFACT_CHUNK_BYTES || offset > self.bytes || offset.checked_add(length as u64).is_none() {
            return Err(DomainError::new("INVALID_RANGE", "Requested artifact range is outside its bounded byte contract"));
        }
        self.check()?;
        let end = self.bytes.min(offset + length as u64);
        let mut position = offset;
        let mut result = Vec::with_capacity((end - offset) as usize);
        let mut buffer = vec![0; ARTIFACT_CHUNK_BYTES];
        while position < end {
            self.check()?;
            let block = position / ARTIFACT_CHUNK_BYTES as u64;
            let start = block * ARTIFACT_CHUNK_BYTES as u64;
            let n = ((self.bytes - start).min(ARTIFACT_CHUNK_BYTES as u64)) as usize;
            let mut digest = [0u8; 32];
            self.index.as_file_mut().seek(SeekFrom::Start(block * 32)).map_err(files::io_error)?;
            self.index.as_file_mut().read_exact(&mut digest).map_err(files::io_error)?;
            self.file.seek(SeekFrom::Start(start)).map_err(files::io_error)?;
            self.file.read_exact(&mut buffer[..n]).map_err(|_| changed())?;
            let actual: [u8; 32] = Sha256::digest(&buffer[..n]).into();
            if actual != digest { return Err(changed()); }
            let from = (position - start) as usize;
            let to = ((end - start).min(n as u64)) as usize;
            result.extend_from_slice(&buffer[from..to]);
            position = start + to as u64;
        }
        self.check()?;
        Ok(result)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (tempfile::TempDir, FileIdentity, std::path::PathBuf, Vec<u8>) {
        let root = tempfile::tempdir().unwrap();
        let output = root.path().join("published.bin");
        let proof = root.path().join("proof.bin");
        let bytes = (0..ARTIFACT_CHUNK_BYTES * 3 + 19).map(|n| (n % 251) as u8).collect::<Vec<_>>();
        std::fs::write(&output, &bytes).unwrap();
        std::fs::hard_link(&output, &proof).unwrap();
        let identity = files::identity(&output, &CancellationToken::new()).unwrap();
        let proof = files::source_path(&proof).unwrap();
        (root, identity, proof, bytes)
    }
    #[test]
    fn exact_cross_block_final_and_rejected_ranges_use_bounded_buffers() {
        let (_root, identity, proof, expected) = fixture();
        let mut reader = ArtifactReader::open(&identity, &proof, CancellationToken::new()).unwrap();
        for (offset, length) in [(0, 1), (ARTIFACT_CHUNK_BYTES - 7, 31), (expected.len() - 7, 30), (expected.len(), 1)] {
            assert_eq!(reader.read_range(offset as u64, length).unwrap(), expected[offset..expected.len().min(offset + length)]);
        }
        assert_eq!(reader.read_range(expected.len() as u64 + 1, 1).unwrap_err().code, "INVALID_RANGE");
        assert_eq!(reader.read_range(0, ARTIFACT_CHUNK_BYTES + 1).unwrap_err().code, "INVALID_RANGE");
    }
    #[test]
    fn substituted_identical_content_and_in_place_mutation_do_not_return_bytes() {
        let (root, identity, proof, expected) = fixture();
        let mut reader = ArtifactReader::open(&identity, &proof, CancellationToken::new()).unwrap();
        std::fs::rename(&identity.path, root.path().join("original.bin")).unwrap();
        std::fs::write(&identity.path, &expected).unwrap();
        assert_eq!(reader.read_range(0, 1).unwrap_err().code, "ARTIFACT_CHANGED");
        assert!(ArtifactReader::open(&identity, &proof, CancellationToken::new()).is_err());
        let (_root, identity, proof, mut expected) = fixture();
        let mut reader = ArtifactReader::open(&identity, &proof, CancellationToken::new()).unwrap();
        expected[0] ^= 0xff;
        std::fs::write(&identity.path, expected).unwrap();
        assert_eq!(reader.read_range(0, 1).unwrap_err().code, "ARTIFACT_CHANGED");
    }
    #[test]
    fn wrong_digest_and_cancellation_fail_without_byte_delivery() {
        let (_root, mut identity, proof, _) = fixture();
        identity.sha256 = "0".repeat(64);
        assert!(ArtifactReader::open(&identity, &proof, CancellationToken::new()).is_err());
        let token = CancellationToken::new(); token.cancel();
        assert_eq!(ArtifactReader::open(&identity, &proof, token).err().unwrap().code, "CANCELLED");
    }
}
