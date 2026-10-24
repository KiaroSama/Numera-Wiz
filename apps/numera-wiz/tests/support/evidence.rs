//! Retain bounded protected fixture diagnostics on success and failure.
pub struct RetainLogs {
    pub source: std::path::PathBuf,
    pub destination: std::path::PathBuf,
}
impl Drop for RetainLogs {
    fn drop(&mut self) {
        let Ok(entries) = std::fs::read_dir(&self.source) else {
            return;
        };
        if std::fs::create_dir_all(&self.destination).is_err() {
            return;
        }
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "jsonl")
                && let Ok(bytes) =
                    nw_platform::security::read_private(&path, 2 * 1024 * 1024 + 1024)
            {
                let _ = std::fs::write(self.destination.join(entry.file_name()), bytes);
            }
        }
    }
}
