//! Complete bounded compiler reads share the existing authoritative MVCC transaction.
use super::ProjectReadSnapshot;
use nw_domain::{
    DomainError, Result,
    media::RenderRead,
    projects::{EntityEnvelope, EntityId, Sequence, Track, edit::ProjectRead},
};
use std::collections::BTreeSet;

impl RenderRead for ProjectReadSnapshot {
    fn render_header_raw(&self) -> &str {
        self.header_raw()
    }

    fn tracks_for_render(&self, sequence: &EntityId, limit: usize) -> Result<Vec<EntityEnvelope>> {
        if !(1..=16).contains(&limit) {
            return Err(DomainError::new("RESOURCE_LIMIT", "Invalid render track budget"));
        }
        let owner: Sequence = self.entity(sequence)?.payload("sequence")?;
        if owner.track_count > limit {
            return Err(DomainError::new("RESOURCE_LIMIT", "Render exceeds 16 tracks"));
        }
        let page = self.track_page(sequence, None, limit, 240 * 1024)?;
        if page.next_key.is_some()
            || page.items.len() != owner.track_count
            || page.total_count != owner.track_count
        {
            return Err(DomainError::new(
                "PROJECT_CORRUPT",
                "Render requires the complete declared track index",
            ));
        }
        let mut seen = BTreeSet::new();
        let mut tracks = Vec::with_capacity(page.items.len());
        for summary in page.items {
            let id = EntityId::parse(summary["id"].as_str().ok_or_else(|| {
                DomainError::new("PROJECT_CORRUPT", "Render track index lacks identity")
            })?)?;
            if !seen.insert(id.clone()) {
                return Err(DomainError::new("PROJECT_CORRUPT", "Render track index repeats identity"));
            }
            let entity = self.entity(&id)?;
            let track: Track = entity.payload("track")?;
            if track.sequence_id != *sequence || entity.owner_id.as_ref() != Some(sequence) {
                return Err(DomainError::new(
                    "PROJECT_CORRUPT",
                    "Render track identity or ownership differs from its index",
                ));
            }
            tracks.push(entity);
        }
        Ok(tracks)
    }
}
