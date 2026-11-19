//! Short browser audition bytes are retained under the current source authority.
use super::*;
use nw_domain::{
    preview::{SOURCE_SEGMENT_SECONDS, SegmentBytes, SourceSegment, SourceSegmentInput},
    time::{Rational, TimeRange},
};
use std::ops::Range;

const COMMAND: &str = "previews.source_segment";
const MAX_SEGMENTS: usize = 4;
const TIME_TO_LIVE: u32 = 45;

pub(in crate::runtime) struct BrowserSegmentLease {
    principal: String,
    profile: Option<String>,
    expires: Instant,
    manifest: SourceSegment,
    segment: Arc<nw_engine::BrowserSegment>,
    source: Arc<files::SourceStamp>,
}

impl BrowserSegmentLease {
    fn owned_by(&self, context: &Context) -> bool {
        self.principal == context.principal && self.profile == context.profile
    }
}

fn extent(range: &Range<usize>) -> SegmentBytes {
    SegmentBytes {
        offset: range.start as u32,
        length: range.len() as u32,
    }
}

fn no_space() -> DomainError {
    DomainError::new("RESOURCE_LIMIT", "Browser preview capture capacity is full")
}

impl Service {
    pub(in crate::runtime) fn prepare_browser_source_segment(
        &self,
        context: &Context,
        selection: SourceSegmentInput,
        cancel: CancellationToken,
    ) -> Result<SourceSegment> {
        let _capacity = self
            .browser_captures
            .clone()
            .try_acquire_owned()
            .map_err(|_| no_space())?;
        {
            let mut segments = self.browser_segments.lock().map_err(|_| lock_error())?;
            let now = Instant::now();
            segments.retain(|_, entry| now < entry.expires);
            if segments.len() >= MAX_SEGMENTS
                && !segments.values().any(|entry| entry.owned_by(context))
            {
                return Err(no_space());
            }
        }
        let (asset, source) = self.analysis_source(context, &selection.asset_id)?;
        let (segment, source) =
            tokio::runtime::Handle::current().block_on(self.engine()?.browser_source_segment(
                &asset,
                source,
                selection.video_stream,
                selection.audio_stream,
                cancel.clone(),
            ))?;
        self.complete_analysis(context, &[&asset], COMMAND, &cancel)?;
        let manifest = SourceSegment {
            segment_id: Uuid::new_v4().to_string(),
            source_sha256: asset.identity.sha256,
            selection,
            sha256: nw_domain::projects::hash(&segment.bytes),
            bytes: segment.bytes.len() as u32,
            mime_type: segment.mime_type.clone(),
            initialization: extent(&segment.initialization),
            fragments: segment.fragments.iter().map(extent).collect(),
            expires_in_seconds: TIME_TO_LIVE,
            requested_range: TimeRange {
                start: Rational::ZERO,
                end: Rational::new(SOURCE_SEGMENT_SECONDS, 1)?,
            },
        };
        let lease = BrowserSegmentLease {
            principal: context.principal.clone(),
            profile: context.profile.clone(),
            expires: Instant::now() + Duration::from_secs(u64::from(TIME_TO_LIVE)),
            manifest: manifest.clone(),
            segment: Arc::new(segment),
            source: Arc::new(source),
        };
        let mut segments = self.browser_segments.lock().map_err(|_| lock_error())?;
        let now = Instant::now();
        segments.retain(|_, entry| now < entry.expires);
        if segments.len() >= MAX_SEGMENTS {
            let replace = segments
                .iter()
                .filter(|(_, entry)| entry.owned_by(context))
                .min_by_key(|(_, entry)| entry.expires)
                .map(|(id, _)| id.clone())
                .ok_or_else(no_space)?;
            segments.remove(&replace);
        }
        segments.insert(manifest.segment_id.clone(), lease);
        Ok(manifest)
    }

    pub fn browser_source_bytes(
        &self,
        context: &Context,
        id: &str,
    ) -> Result<Arc<nw_engine::BrowserSegment>> {
        let (manifest, segment, source) = {
            let mut segments = self.browser_segments.lock().map_err(|_| lock_error())?;
            let now = Instant::now();
            segments.retain(|_, entry| now < entry.expires);
            let entry = segments
                .get(id)
                .filter(|entry| entry.owned_by(context))
                .ok_or_else(DomainError::object_not_found)?;
            (
                entry.manifest.clone(),
                entry.segment.clone(),
                entry.source.clone(),
            )
        };
        let asset: Asset = self
            .store
            .get(&named("asset", &manifest.selection.asset_id)?)?
            .ok_or_else(DomainError::object_not_found)?;
        if asset.identity.sha256 != manifest.source_sha256 {
            return Err(DomainError::new(
                "SOURCE_CHANGED",
                "The registered preview source changed",
            ));
        }
        self.complete_analysis(context, &[&asset], COMMAND, &CancellationToken::new())?;
        source.revalidate()?;
        Ok(segment)
    }
}
