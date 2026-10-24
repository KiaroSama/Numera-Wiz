//! One database attachment for the existing transaction kernel.
//! A maintenance switch is admitted only after all transaction leases drain.
use nw_domain::{DomainError, Result};
use redb::{
    Database, ReadTransaction, ReadableDatabase, StorageError, TransactionError, WriteTransaction,
};
use std::{
    ops::{Deref, DerefMut},
    path::{Path, PathBuf},
    sync::{
        Arc, RwLock,
        atomic::{AtomicUsize, Ordering},
    },
};
struct Lease(Arc<AtomicUsize>);
impl Drop for Lease {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}
pub(super) struct Read {
    inner: ReadTransaction,
    _lease: Lease,
}
impl Deref for Read {
    type Target = ReadTransaction;
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}
pub(super) struct Write {
    inner: Option<WriteTransaction>,
    _lease: Lease,
}
impl Deref for Write {
    type Target = WriteTransaction;
    fn deref(&self) -> &Self::Target {
        self.inner
            .as_ref()
            .expect("A live transaction is present until consumed")
    }
}
impl DerefMut for Write {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.inner
            .as_mut()
            .expect("A live transaction is present until consumed")
    }
}
impl Write {
    pub(super) fn commit(mut self) -> std::result::Result<(), redb::CommitError> {
        self.inner
            .take()
            .expect("A transaction commits once")
            .commit()
    }
}
struct Attachment {
    database: Option<Database>,
    _guards: Vec<nw_platform::files::RootLock>,
    path: PathBuf,
}
pub(super) struct DatabaseSlot {
    database: RwLock<Attachment>,
    active: Arc<AtomicUsize>,
}
impl DatabaseSlot {
    pub(super) fn new(
        database: Database,
        path: PathBuf,
        guard: nw_platform::files::RootLock,
    ) -> Self {
        Self {
            database: RwLock::new(Attachment {
                database: Some(database),
                path,
                _guards: vec![guard],
            }),
            active: Arc::new(AtomicUsize::new(0)),
        }
    }
    pub(super) fn path(&self) -> Result<PathBuf> {
        Ok(self
            .database
            .read()
            .map_err(|_| {
                DomainError::new("STORAGE_ERROR", "Database attachment lock is unavailable")
            })?
            .path
            .clone())
    }
    fn lease(&self) -> Lease {
        self.active.fetch_add(1, Ordering::AcqRel);
        Lease(self.active.clone())
    }
    pub(super) fn begin_read(&self) -> std::result::Result<Read, TransactionError> {
        let state = self.database.read().map_err(StorageError::from)?;
        let database = state
            .database
            .as_ref()
            .ok_or(StorageError::DatabaseClosed)?;
        let lease = self.lease();
        let inner = database.begin_read()?;
        Ok(Read {
            inner,
            _lease: lease,
        })
    }
    pub(super) fn begin_write(&self) -> std::result::Result<Write, TransactionError> {
        // Read guards permit readers and concurrent writer admission to progress.
        // The redb writer remains the sole transaction serialization authority.
        let state = self.database.read().map_err(StorageError::from)?;
        let database = state
            .database
            .as_ref()
            .ok_or(StorageError::DatabaseClosed)?;
        let lease = self.lease();
        let inner = database.begin_write()?;
        Ok(Write {
            inner: Some(inner),
            _lease: lease,
        })
    }
    pub(super) fn close_drained(&self) -> Result<()> {
        let mut state = self.database.try_write().map_err(|error| match error {
            std::sync::TryLockError::WouldBlock => {
                DomainError::new("OWNER_BUSY", "Database admission is still active")
            }
            std::sync::TryLockError::Poisoned(_) => {
                DomainError::new("STORAGE_ERROR", "Database attachment lock is unavailable")
            }
        })?;
        if self.active.load(Ordering::Acquire) != 0 {
            return Err(DomainError::new(
                "OWNER_BUSY",
                "Database transactions must drain before a generation switch",
            ));
        }
        // Dropping redb flushes and closes its writer. No live transaction can defer close.
        drop(state.database.take());
        Ok(())
    }
    pub(super) fn reopen(&self, database: Database) -> Result<()> {
        let mut state = self.database.write().map_err(|_| {
            DomainError::new("STORAGE_ERROR", "Database attachment lock is unavailable")
        })?;
        if state.database.is_some() || self.active.load(Ordering::Acquire) != 0 {
            return Err(DomainError::new(
                "OWNER_BUSY",
                "A generation cannot replace an attached database",
            ));
        }
        state.database = Some(database);
        Ok(())
    }
    pub(super) fn attach_generation(
        &self,
        database: Database,
        path: &Path,
        guard: nw_platform::files::RootLock,
    ) -> Result<()> {
        let mut state = self.database.write().map_err(|_| {
            DomainError::new("STORAGE_ERROR", "Database attachment lock is unavailable")
        })?;
        if state.database.is_some() || self.active.load(Ordering::Acquire) != 0 {
            return Err(DomainError::new(
                "OWNER_BUSY",
                "A generation cannot replace an attached database",
            ));
        }
        // Historical owners lock generation1 rather than the state directory. Keep
        // every prior generation lease until this Store is dropped, including during
        // a closed cutover; selecting another generation must not release that fence.
        state._guards.push(guard);
        state.database = Some(database);
        state.path = path.to_owned();
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    const TABLE: redb::TableDefinition<&str, &[u8]> = redb::TableDefinition::new("slot-unit");
    #[test]
    fn switch_refuses_live_read_or_write_and_preserves_the_original_commits() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("store.redb");
        let slot = DatabaseSlot::new(
            Database::create(&path).unwrap(),
            temporary.path().to_owned(),
            nw_platform::files::RootLock::acquire(temporary.path()).unwrap(),
        );
        let write = slot.begin_write().unwrap();
        {
            let mut table = write.open_table(TABLE).unwrap();
            table.insert("identity", b"retained".as_slice()).unwrap();
        }
        assert_eq!(slot.close_drained().unwrap_err().code, "OWNER_BUSY");
        write.commit().unwrap();
        let read = slot.begin_read().unwrap();
        assert_eq!(slot.close_drained().unwrap_err().code, "OWNER_BUSY");
        drop(read);
        slot.close_drained().unwrap();
        assert!(
            slot.begin_read().is_err(),
            "Closed attachment must fail rather than create an empty store"
        );
        let closed = redb::ReadOnlyDatabase::open(&path).unwrap();
        let transaction = closed.begin_read().unwrap();
        assert_eq!(
            transaction
                .open_table(TABLE)
                .unwrap()
                .get("identity")
                .unwrap()
                .unwrap()
                .value(),
            b"retained"
        );
        drop(transaction);
        drop(closed);
        slot.reopen(Database::open(&path).unwrap()).unwrap();
        let read = slot.begin_read().unwrap();
        assert_eq!(
            read.open_table(TABLE)
                .unwrap()
                .get("identity")
                .unwrap()
                .unwrap()
                .value(),
            b"retained"
        );
    }
}
