//! Bookend generation observation with short, current-state cache critical sections.
use super::*;

impl Service {
    pub(in crate::runtime) fn project_snapshot_pressure(
        &self,
        projected_write_bytes: usize,
    ) -> Result<()> {
        self.project_snapshot_pressure_observing(projected_write_bytes, || {
            self.store.project_generation_bytes()
        })
    }

    fn project_snapshot_pressure_observing(
        &self,
        projected_write_bytes: usize,
        observe_generation: impl FnOnce() -> Result<u64>,
    ) -> Result<()> {
        {
            let mut cache = self.snapshots.lock().map_err(|_| lock_error())?;
            cache.expire();
            if cache.projects.is_empty() {
                cache.project_write_estimate = 0;
                return Ok(());
            }
            // Keep concurrent write reservations atomic, even while another observation is pending.
            if projected_write_bytes > 0 {
                let reservation = projected_write_bytes
                    .saturating_mul(8)
                    .saturating_add(64 * 1024);
                cache.project_write_estimate =
                    cache.project_write_estimate.saturating_add(reservation);
            }
        }

        // Store/filesystem observation must not hold the inventory and project snapshot mutex.
        let observed = observe_generation();
        let mut cache = self.snapshots.lock().map_err(|_| lock_error())?;
        cache.expire();
        // Preserve observation failures and outstanding nonempty-cache accounting.
        let generation_bytes = observed?;
        let Some(baseline) = cache.projects.values().map(|p| p.generation_bytes).min() else {
            return Ok(());
        };
        // Captures and estimates may have changed during observation; only current state counts.
        // These conservative software estimates are not a hard instantaneous filesystem cap.
        let growth = generation_bytes.saturating_sub(baseline);
        if cache.project_write_estimate > 32 * 1024 * 1024 || growth > 32 * 1024 * 1024 {
            cache.clear_project_reads();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
pub(super) async fn exercise_pressure_scheduling() {
    tests::exercise_pressure_scheduling().await;
}
