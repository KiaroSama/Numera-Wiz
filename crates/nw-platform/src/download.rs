//! A user-selected artifact transfer is a client copy, never a new producer job.
use crate::{
    files::{DirectoryStamp, io_error, output_path, path_text},
    publication::RetainedStaging,
};
use nw_domain::{
    DomainError, Result,
    artifacts::ArtifactManifest,
    download::{DownloadVerifier, SaveControl, SaveReceipt},
};
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};

pub struct ClientCopy {
    output: PathBuf,
    parent: DirectoryStamp,
    workspace: PathBuf,
    staging: Option<RetainedStaging>,
    verifier: Option<DownloadVerifier>,
    copied: u64,
    cleanup_attempted: bool,
}
impl ClientCopy {
    pub fn create(output: &Path, manifest: &ArtifactManifest) -> Result<Self> {
        let verifier = DownloadVerifier::new(manifest)?;
        let output = output_path(output)?;
        let parent = DirectoryStamp::capture_resolved(output.parent().ok_or_else(|| {
            DomainError::new("PATH_DENIED", "A client destination parent is required")
        })?)?;
        let directory = tempfile::Builder::new()
            .prefix(".numera-wiz-save-")
            .tempdir_in(output.parent().expect("Parent was checked"))
            .map_err(io_error)?;
        // Never recursively remove this directory, even on cancellation.
        let workspace = directory.keep();
        let staging = match tempfile::Builder::new()
            .prefix("payload-")
            .tempfile_in(&workspace)
            .map_err(io_error)
            .and_then(RetainedStaging::new)
        {
            Ok(staging) => staging,
            Err(error) => {
                let _ = std::fs::remove_dir(&workspace);
                return Err(error);
            }
        };
        Ok(Self {
            output,
            parent,
            workspace,
            staging: Some(staging),
            verifier: Some(verifier),
            copied: 0,
            cleanup_attempted: false,
        })
    }
    pub fn write(&mut self, bytes: &[u8]) -> Result<()> {
        let verifier = self.verifier.as_mut().ok_or_else(|| {
            DomainError::new(
                "COPY_STATE_CONFLICT",
                "Client verification is already finished",
            )
        })?;
        verifier.push(bytes)?;
        self.staging
            .as_mut()
            .ok_or_else(|| {
                DomainError::new("COPY_STATE_CONFLICT", "Client staging is unavailable")
            })?
            .client_write_handle()
            .write_all(bytes)
            .map_err(io_error)?;
        self.copied = verifier.received_bytes();
        Ok(())
    }
    pub fn copied(&self) -> u64 {
        self.copied
    }
    pub fn finish(mut self, control: &SaveControl) -> Result<SaveReceipt> {
        control.check()?;
        let verifier = self.verifier.take().ok_or_else(|| {
            DomainError::new(
                "COPY_STATE_CONFLICT",
                "Client verification is already finished",
            )
        })?;
        let mut receipt = verifier.finish(path_text(&self.output)?)?;
        let staging = self.staging.as_mut().ok_or_else(|| {
            DomainError::new("COPY_STATE_CONFLICT", "Client staging is unavailable")
        })?;
        staging.client_write_handle().flush().map_err(io_error)?;
        staging.client_write_handle().sync_all().map_err(io_error)?;
        // Verify the flushed file independently of the network digest before publishing it.
        let file = staging.client_write_handle();
        file.seek(SeekFrom::Start(0)).map_err(io_error)?;
        let mut hash = Sha256::new();
        let mut total = 0u64;
        let mut buffer = vec![0; nw_domain::artifacts::ARTIFACT_CHUNK_BYTES];
        loop {
            control.check()?;
            let count = file.read(&mut buffer).map_err(io_error)?;
            if count == 0 {
                break;
            }
            hash.update(&buffer[..count]);
            total = total.checked_add(count as u64).ok_or_else(|| {
                DomainError::new("ARTIFACT_CHANGED", "Client staging byte count overflowed")
            })?;
        }
        if total.to_string() != receipt.encoded_byte_count
            || hex::encode(hash.finalize()) != receipt.sha256
        {
            return Err(DomainError::new(
                "ARTIFACT_CHANGED",
                "Flushed client staging did not match the selected artifact",
            ));
        }
        self.parent.revalidate()?;
        let destination =
            crate::claims::Destination::reviewed(&self.output, &self.parent.object_identity()?)?;
        // Client copies participate in the same cross-root writer namespace.
        // Contention is explicit and nonblocking; no held filesystem write is dropped.
        let claims = crate::claims::DestinationClaims::try_acquire(vec![destination])?;
        claims.revalidate()?;
        control.check()?;
        // Once this wins, cancellation reports that publication is already underway.
        control.begin_commit()?;
        self.staging
            .as_ref()
            .expect("Staging was checked")
            .publish_no_replace(&self.parent, &self.output)?;
        if self.cleanup().is_err() {
            receipt
                .warnings
                .push("CLIENT_STAGING_CLEANUP_PENDING".into());
        }
        Ok(receipt)
    }
    fn cleanup(&mut self) -> Result<()> {
        if self.cleanup_attempted { return Ok(()); }
        // A failed ownership check is final for this instance. Drop must not
        // retry an unbound pathname after the original payload handle is gone.
        self.cleanup_attempted = true;
        if let Some(staging) = self.staging.take() {
            staging.remove_owned_name()?;
        }
        self.parent.revalidate()?;
        let name = self.workspace.file_name().ok_or_else(|| {
            DomainError::new("PATH_DENIED", "Client workspace must have one owned basename")
        })?;
        #[cfg(unix)]
        let removed = rustix::fs::unlinkat(
            self.parent.held_file(), name, rustix::fs::AtFlags::REMOVEDIR,
        ).map_err(std::io::Error::from);
        #[cfg(not(unix))]
        let removed = {
            let _ = name;
            std::fs::remove_dir(&self.workspace)
        };
        match removed {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(io_error(error)),
        }
    }
}
impl Drop for ClientCopy {
    fn drop(&mut self) {
        if let Err(error) = self.cleanup() {
            tracing::warn!(code = %error.code, "Client copy retained its owned staging for cleanup");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nw_domain::artifacts::ARTIFACT_CHUNK_BYTES;
    fn manifest(bytes: &[u8]) -> ArtifactManifest {
        ArtifactManifest {
            schema_version: 1,
            suggested_name: Some("test-copy.mkv".into()),
            artifact_id: "job-copy-test".into(),
            manifest_uri: "numera://artifact/job-copy-test/manifest".into(),
            job_id: "copy-test".into(),
            plan_id: "plan".into(),
            mime_type: "application/octet-stream".into(),
            encoded_byte_count: bytes.len().to_string(),
            sha256: hex::encode(Sha256::digest(bytes)),
            availability: "retained_published_object".into(),
            expires_at: None,
            retrieval_methods: vec!["authenticated_http_when_enabled".into()],
            bytes_route: Some("/api/v1/artifacts/job-copy-test/bytes".into()),
            max_chunk_bytes: ARTIFACT_CHUNK_BYTES as u32,
        }
    }
    #[test]
    fn real_client_copy_verifies_flushed_bytes_and_preserves_a_late_collision() {
        let root = tempfile::tempdir().unwrap();
        let bytes = vec![37; ARTIFACT_CHUNK_BYTES * 2 + 17];
        let m = manifest(&bytes);
        let output = root.path().join("saved.bin");
        let mut copy = ClientCopy::create(&output, &m).unwrap();
        for chunk in bytes.chunks(ARTIFACT_CHUNK_BYTES) {
            copy.write(chunk).unwrap();
        }
        let receipt = copy.finish(&SaveControl::default()).unwrap();
        assert_eq!(receipt.sha256, m.sha256);
        assert!(receipt.warnings.is_empty());
        assert_eq!(std::fs::read(&output).unwrap(), bytes);
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
        let collision = root.path().join("collision.bin");
        let mut copy = ClientCopy::create(&collision, &m).unwrap();
        for chunk in bytes.chunks(ARTIFACT_CHUNK_BYTES) {
            copy.write(chunk).unwrap();
        }
        std::fs::write(&collision, b"original").unwrap();
        assert_eq!(
            copy.finish(&SaveControl::default()).unwrap_err().code,
            "OUTPUT_COLLISION"
        );
        assert_eq!(std::fs::read(&collision).unwrap(), b"original");
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 2);
    }
    #[test]
    fn truncated_changed_disk_and_cancelled_copies_never_publish() {
        let root = tempfile::tempdir().unwrap();
        let m = manifest(b"abc");
        let truncated = root.path().join("truncated");
        let mut copy = ClientCopy::create(&truncated, &m).unwrap();
        copy.write(b"ab").unwrap();
        assert_eq!(
            copy.finish(&SaveControl::default()).unwrap_err().code,
            "ARTIFACT_CHANGED"
        );
        assert!(!truncated.exists());
        let changed = root.path().join("changed");
        let mut copy = ClientCopy::create(&changed, &m).unwrap();
        copy.write(b"abc").unwrap();
        let file = copy.staging.as_mut().unwrap().client_write_handle();
        file.seek(SeekFrom::Start(0)).unwrap();
        file.write_all(b"abd").unwrap();
        assert_eq!(
            copy.finish(&SaveControl::default()).unwrap_err().code,
            "ARTIFACT_CHANGED"
        );
        assert!(!changed.exists());
        let cancelled = root.path().join("cancelled");
        let mut copy = ClientCopy::create(&cancelled, &m).unwrap();
        copy.write(b"abc").unwrap();
        let control = SaveControl::default();
        assert!(control.cancel());
        assert_eq!(copy.finish(&control).unwrap_err().code, "CANCELLED");
        assert!(!cancelled.exists());
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
    }
    #[cfg(unix)]
    #[test]
    fn failed_cleanup_is_not_retried_against_a_redirected_empty_directory() {
        let root = tempfile::tempdir().unwrap();
        let parent = root.path().join("selected");
        let moved = root.path().join("moved");
        let outside = root.path().join("outside");
        std::fs::create_dir(&parent).unwrap();
        std::fs::create_dir(&outside).unwrap();
        let mut copy = ClientCopy::create(&parent.join("saved"), &manifest(b"abc")).unwrap();
        copy.write(b"abc").unwrap();
        let name = copy.workspace.file_name().unwrap().to_owned();
        std::fs::rename(&parent, &moved).unwrap();
        std::os::unix::fs::symlink(&outside, &parent).unwrap();
        let unrelated = outside.join(&name);
        std::fs::create_dir(&unrelated).unwrap();
        assert_eq!(copy.cleanup().unwrap_err().code, "TARGET_CHANGED");
        drop(copy);
        assert!(unrelated.is_dir(), "Drop must preserve the unrelated empty replacement directory");
        assert_eq!(std::fs::read_dir(&unrelated).unwrap().count(), 0);
        assert_eq!(std::fs::read_dir(moved.join(&name)).unwrap().count(), 1);
    }
    #[test]
    fn cleanup_removes_only_the_owned_payload_and_preserves_unknown_entries() {
        let root = tempfile::tempdir().unwrap();
        let output = root.path().join("saved");
        let copy = ClientCopy::create(&output, &manifest(b"abc")).unwrap();
        let workspace = copy.workspace.clone();
        let unknown = workspace.join("unrelated");
        std::fs::write(&unknown, b"preserve").unwrap();
        drop(copy);
        assert!(!output.exists());
        assert_eq!(std::fs::read(&unknown).unwrap(), b"preserve");
        assert_eq!(std::fs::read_dir(&workspace).unwrap().count(), 1);
    }
}
