//! Resolve the default data directory without mutating the application bundle.
use nw_domain::{DomainError, Result};
use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
};

pub fn default_data_directory(
    executable: &Path,
    macos: bool,
    home_directory: impl FnOnce() -> Option<PathBuf>,
) -> Result<PathBuf> {
    let directory = executable
        .parent()
        .filter(|path| path.is_absolute())
        .ok_or_else(|| {
            DomainError::new(
                "DATA_ROOT_UNAVAILABLE",
                "The executable directory could not be identified",
            )
        })?;
    let is_bundle = macos
        && directory.ends_with("Contents/MacOS")
        && directory
            .parent()
            .and_then(Path::parent)
            .and_then(Path::extension)
            == Some(OsStr::new("app"));
    if is_bundle {
        let home = home_directory()
            .filter(|path| path.is_absolute())
            .ok_or_else(|| {
                DomainError::new(
                    "DATA_ROOT_UNAVAILABLE",
                    "The macOS home directory is unavailable. Select a writable --data-dir.",
                )
            })?;
        return Ok(home.join("Library/Application Support/Numera Wiz"));
    }
    Ok(directory.to_owned())
}
