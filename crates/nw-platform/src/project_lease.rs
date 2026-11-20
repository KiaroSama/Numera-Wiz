//! Project UUID writer ownership is user-scoped and independent of portable data roots.
use nw_domain::{DomainError, Result, projects::ProjectId};
use std::fs::File;
pub struct ProjectWriterLease {
    file: File,
}
impl ProjectWriterLease {
    pub fn acquire(id: &ProjectId) -> Result<Self> {
        let root = crate::claims::namespace()?;
        let file = crate::claims::open_lock(&root.join(format!("project-{id}.lock")))?;
        fs2::FileExt::try_lock_exclusive(&file).map_err(|_| {
            DomainError::new(
                "PROJECT_WRITER_BUSY",
                "Another owner holds this project UUID; reads remain available",
            )
        })?;
        Ok(Self { file })
    }
}
impl Drop for ProjectWriterLease {
    fn drop(&mut self) {
        let _ = fs2::FileExt::unlock(&self.file);
    }
}
