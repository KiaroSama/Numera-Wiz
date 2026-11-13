//! Compiler-only identity reads admit index/raw bytes before allocation or decoding.
use super::{ProjectReadSnapshot, RECORDS, bounded_table_raw, decode, domain, entity_key, storage};
use nw_domain::{
    DomainError, Result,
    media::{MAX_CLIPS, MAX_RAW_BYTES, MAX_TRACKS, RenderRead},
    projects::{EntityId, Track, views::TrackSummary},
    time::TimeRange,
};
use std::collections::BTreeSet;

impl RenderRead for ProjectReadSnapshot {
    fn render_header_raw(&self) -> &str {
        self.header_raw()
    }

    fn render_raw_entity(
        &self,
        id: &EntityId,
        remaining_raw_bytes: usize,
    ) -> Result<Option<String>> {
        if remaining_raw_bytes > MAX_RAW_BYTES {
            return Err(domain::limit("Invalid compiler raw admission"));
        }
        let table = self.read.open_table(RECORDS).map_err(storage)?;
        // bounded_table_raw checks AccessGuard length before UTF-8 copying. A zero
        // remaining budget also rejects a present row before allocating its String.
        bounded_table_raw(
            &table,
            &entity_key(&self.header.id, id),
            remaining_raw_bytes.min(192 * 1024),
        )
    }

    fn track_ids_for_render(
        &self,
        sequence: &EntityId,
        declared_tracks: usize,
        limit: usize,
    ) -> Result<Vec<EntityId>> {
        if !(1..=MAX_TRACKS).contains(&limit) || declared_tracks > limit {
            return Err(domain::limit("Render exceeds bounded track admission"));
        }
        let table = self.read.open_table(RECORDS).map_err(storage)?;
        let prefix = format!("project-track/{}/{sequence}/", self.header.id);
        let mut ids = Vec::new();
        let mut seen = BTreeSet::new();
        let mut bytes = 0;
        for row in table.range(prefix.as_str()..).map_err(storage)? {
            let (key, data) = row.map_err(storage)?;
            if !key.value().starts_with(&prefix) {
                break;
            }
            if ids.len() == limit
                || data.value().len() > 1024
                || data.value().len() > (MAX_TRACKS * 1024usize).saturating_sub(bytes)
            {
                return Err(domain::limit(
                    "Compiler track index exceeds summary admission",
                ));
            }
            bytes += data.value().len();
            let summary: TrackSummary = decode(data.value())?;
            if key.value() != format!("{prefix}{:08x}/{}", summary.order, summary.id)
                || !seen.insert(summary.id.clone())
            {
                return Err(DomainError::new(
                    "PROJECT_CORRUPT",
                    "Compiler track index identity/order differs",
                ));
            }
            ids.push(summary.id);
        }
        if ids.len() != declared_tracks {
            return Err(DomainError::new(
                "PROJECT_CORRUPT",
                "Compiler requires the complete declared track index",
            ));
        }
        Ok(ids)
    }

    fn clip_ids_for_render(
        &self,
        sequence: &EntityId,
        track_id: &EntityId,
        track: &Track,
        range: TimeRange,
        limit: usize,
    ) -> Result<Vec<EntityId>> {
        if !(1..=MAX_CLIPS).contains(&limit) {
            return Err(domain::limit("Invalid compiler clip admission"));
        }
        let (rows, more) = self.clip_rows_with_track(
            sequence,
            track_id,
            track,
            range,
            (None, limit),
            Some(MAX_CLIPS * 1024),
        )?;
        if more {
            return Err(domain::limit("Compiler interval index exceeds128 clips"));
        }
        let prefix = format!("project-clip/{}/{sequence}/{track_id}/", self.header.id);
        let mut seen = BTreeSet::new();
        let mut ids = Vec::with_capacity(rows.len());
        for (key, summary) in rows {
            let tick = domain::grid_tick(summary.sequence_start, track.grid)?;
            if summary.track_id != *track_id
                || key != format!("{prefix}{tick:016x}/{}", summary.id)
                || !seen.insert(summary.id.clone())
            {
                return Err(DomainError::new(
                    "PROJECT_CORRUPT",
                    "Compiler interval index identity/track differs",
                ));
            }
            ids.push(summary.id);
        }
        Ok(ids)
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
pub(super) fn exercise_compiler_admission() {
    tests::run();
}
