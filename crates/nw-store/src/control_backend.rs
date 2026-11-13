//! Existing-file-only growth-bounded backend for accepted restoration control work.
use crate::{Store, generations::RootLease, storage, versions};
use nw_domain::{DomainError, Result, records::NativeObjectIdentity};
use redb::{BackendError, StorageBackend};
use std::{io, ops::Bound, path::Path};

#[derive(Debug)]
struct BoundedBackend {
    inner: redb::backends::FileBackend,
    maximum: u64,
}
impl BoundedBackend {
    fn range(&self, length: u64) -> io::Result<()> {
        if length == 0 || length > self.maximum {
            return Err(io::Error::other(
                "Existing generation exceeds its physical growth ceiling",
            ));
        }
        Ok(())
    }
}
impl StorageBackend for BoundedBackend {
    fn len(&self) -> io::Result<u64> {
        let length = self.inner.len()?;
        self.range(length)?;
        Ok(length)
    }
    fn read(&self, offset: u64, out: &mut [u8]) -> io::Result<()> {
        self.inner.read(offset, out)
    }
    fn set_len(&self, length: u64) -> io::Result<()> {
        self.range(length)?;
        self.inner.set_len(length)
    }
    fn sync_data(&self) -> io::Result<()> {
        self.inner.sync_data()
    }
    fn write(&self, offset: u64, data: &[u8]) -> io::Result<()> {
        let end = offset
            .checked_add(data.len() as u64)
            .ok_or_else(|| io::Error::other("Generation write offset overflow"))?;
        self.range(end)?;
        self.inner.write(offset, data)
    }
    fn close(&self) -> io::Result<()> {
        self.inner.close()
    }
    fn try_lock_range(
        &self,
        start: Bound<u64>,
        end: Bound<u64>,
    ) -> std::result::Result<bool, BackendError> {
        self.inner.try_lock_range(start, end)
    }
    fn try_lock_shared_range(
        &self,
        start: Bound<u64>,
        end: Bound<u64>,
    ) -> std::result::Result<bool, BackendError> {
        self.inner.try_lock_shared_range(start, end)
    }
    fn lock_range(
        &self,
        start: Bound<u64>,
        end: Bound<u64>,
    ) -> std::result::Result<(), BackendError> {
        self.inner.lock_range(start, end)
    }
    fn lock_shared_range(
        &self,
        start: Bound<u64>,
        end: Bound<u64>,
    ) -> std::result::Result<(), BackendError> {
        self.inner.lock_shared_range(start, end)
    }
    fn unlock_range(
        &self,
        start: Bound<u64>,
        end: Bound<u64>,
    ) -> std::result::Result<(), BackendError> {
        self.inner.unlock_range(start, end)
    }
    fn query_lock_range(
        &self,
        start: Bound<u64>,
        end: Bound<u64>,
    ) -> std::result::Result<bool, BackendError> {
        self.inner.query_lock_range(start, end)
    }
}
impl Store {
    pub(super) fn open_retained_control(
        &self,
        lease: &RootLease,
        path: &Path,
        expected: &NativeObjectIdentity,
    ) -> Result<redb::Database> {
        lease.revalidate()?;
        if !path.starts_with(lease.state())
            || !path.join("store.redb").try_exists().map_err(storage)?
        {
            return Err(DomainError::new(
                "STORE_RECOVERY_REQUIRED",
                "Accepted generation is absent or outside this owned root",
            ));
        }
        self.db.retain_existing_lock(path)?;
        open_validated_backend(path, expected, self.root_id(), |_| Ok(()))
    }
    pub(crate) fn open_restoration_control(
        &self,
        lease: &RootLease,
        path: &Path,
        expected: &NativeObjectIdentity,
        authority: &crate::restorations::Authority,
    ) -> Result<redb::Database> {
        lease.revalidate()?;
        let source = crate::generations::location_path(lease.root(), &authority.source_location)?;
        let overlay = lease
            .state()
            .join("generation-workspaces")
            .join(&authority.record.id)
            .join("restoration");
        if (path != source && path != overlay)
            || (path == source && *expected != authority.barrier.source_object)
        {
            return Err(DomainError::new(
                "RESTORATION_REFUSED",
                "Control attachment differs from the accepted source or overlay",
            ));
        }
        self.db.retain_existing_lock(path)?;
        open_validated_backend(path, expected, self.root_id(), |snapshot| {
            crate::restorations::validate_source_snapshot(lease, authority, snapshot)
        })
    }
}
fn open_backend(path: &Path, expected: &NativeObjectIdentity) -> Result<redb::Database> {
    let file = nw_platform::generation_restoration::open_existing_control_file(
        &path.join("store.redb"),
        expected,
    )?;
    let backend = BoundedBackend {
        inner: redb::backends::FileBackend::new(file).map_err(versions::database_error)?,
        maximum: versions::MAX_GENERATION_BYTES,
    };
    backend.len().map_err(storage)?;
    // redb4.4 exposes no open_with_backend. The verified existing handle and
    // nonzero-length backend make its allow-create empty-file branch unreachable.
    redb::Database::builder()
        .set_cache_size(8 * 1024 * 1024)
        .create_with_backend(backend)
        .map_err(versions::database_error)
}
fn open_validated_backend(
    path: &Path,
    expected: &NativeObjectIdentity,
    root_id: &str,
    validate: impl FnOnce(&Path) -> Result<()>,
) -> Result<redb::Database> {
    let stamp = nw_platform::files::SourceStamp::capture_resolved(&path.join("store.redb"))?;
    if nw_platform::files::source_object(&path.join("store.redb"))? != *expected {
        return Err(DomainError::new(
            "SOURCE_CHANGED",
            "Accepted source native identity changed",
        ));
    }
    let (facts, snapshot) = versions::existing_snapshot(path)?;
    if facts.root_id != root_id {
        return Err(DomainError::new(
            "OWNER_IDENTITY_CONFLICT",
            "Accepted generation has another root",
        ));
    }
    // A dirty file is repaired only in the independently copied probe above.
    // Full logical eligibility precedes any writable open of the original object.
    validate(&snapshot)?;
    stamp.revalidate()?;
    open_backend(path, expected)
}
pub(crate) fn repair_accepted_source(
    lease: &RootLease,
    path: &Path,
    authority: &crate::restorations::Authority,
) -> Result<()> {
    lease.retain_existing_generation(path)?;
    let database = open_validated_backend(
        path,
        &authority.barrier.source_object,
        &authority.record.plan.root_id,
        |snapshot| crate::restorations::validate_source_snapshot(lease, authority, snapshot),
    )?;
    drop(database);
    lease.revalidate()
}

#[cfg(test)]
pub(crate) fn exercise_bounded_existing_backend() {
    use std::io::{Read, Seek, SeekFrom, Write};
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("existing.bin");
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(&path)
        .unwrap();
    file.write_all(&[7u8; 16]).unwrap();
    let backend = BoundedBackend {
        inner: redb::backends::FileBackend::new(file.try_clone().unwrap()).unwrap(),
        maximum: 32,
    };
    assert_eq!(backend.len().unwrap(), 16);
    assert!(backend.set_len(0).is_err());
    assert!(backend.set_len(33).is_err());
    assert!(backend.write(31, &[1, 2]).is_err());
    assert!(backend.write(u64::MAX, &[1, 2]).is_err());
    assert_eq!(file.metadata().unwrap().len(), 16);
    let mut unchanged = [0u8; 16];
    file.seek(SeekFrom::Start(0)).unwrap();
    file.read_exact(&mut unchanged).unwrap();
    assert_eq!(
        unchanged, [7u8; 16],
        "Rejected operations must not delegate any bytes"
    );
    backend.write(4, &[1, 2]).unwrap();
    backend.sync_data().unwrap();
    let mut written = [0u8; 2];
    backend.read(4, &mut written).unwrap();
    assert_eq!(written, [1, 2]);
    assert!(
        backend
            .try_lock_range(Bound::Included(0), Bound::Included(0))
            .unwrap()
    );
    backend
        .unlock_range(Bound::Included(0), Bound::Included(0))
        .unwrap();
    drop(backend);
    let empty = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(directory.path().join("empty.bin"))
        .unwrap();
    let empty = BoundedBackend {
        inner: redb::backends::FileBackend::new(empty).unwrap(),
        maximum: 32,
    };
    assert!(
        empty.len().is_err(),
        "redb empty-file initialization must be unreachable"
    );
}
