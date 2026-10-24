# Feature Specification: Numera Wiz

**Feature Branch**: `implementation/numera-wiz-v1`

**Created**: 2026-10-23

**Status**: Implementation in progress; no complete-release claim

**Input**: Build the attached revision 7 mandate in a public repository; keep all
implementation on one review branch. The complete binding requirements and
acceptance assertions are incorporated by reference from
[the mandate](../../docs/research-pack/Numera-Wiz-Implementation-Mandate-EN.md).

## User Scenarios & Testing

### User Story 1 - Review and execute media tools (Priority: P1)
A human or agent inspects registered media, resolves explicit stream selections,
reviews an immutable command plan, submits it once, observes progress and obtains
verified output without creating a timeline project.

**Why this priority**: Establishes the real engine and all-interface foundation.
**Independent Test**: PJ01, MX7-J01 through MX7-J12 and the E03-E06 parity gate.

**Acceptance Scenarios**:
1. Given mixed-language tracks, when selecting original indices including 0 and
   applying a partial order, then every retained/dropped stream is reported and
   metadata edits refer to the resolved output stream.
2. Given keep-everything intent, when a file copy is planned, then full byte
   identity is verified before no-replace publication.
3. Given zero child exit with no new staging, then the job fails and an older
   destination is untouched.
4. Given a lost submission response, then a scoped retry returns the same job.

### User Story 2 - Edit with the same desktop and browser UI (Priority: P1)
Use FilmCraft-shaped workspaces, exact timeline edits, history, preview, effects,
fonts/captions, audio routing and export from one authoritative local engine.

**Why this priority**: Shared UI and command parity are initial requirements.
**Independent Test**: PJ02-PJ14, P7-J01-P7-J05, MF7-J01-MF7-J05, B7-J01-B7-J03.
**Acceptance Scenarios**:
1. Native/browser/CLI/MCP inspect the same revision and interval results.
2. An edit racing a drag rejects the stale commit atomically.
3. Enabled unknown semantic nodes block affected renders without data loss.

### User Story 3 - Run complete utility and donor workflows (Priority: P1)
Complete F01-F15, the full MuxCls rewrite, CD01-CD34 and admitted effect variants
from the supplied source ledgers, including their explicit limits.

**Why this priority**: A small first slice is not the requested release.
**Independent Test**: Annex A/D, per-canonical donor fixtures and M6-J01-M6-J12.
**Acceptance Scenarios**:
1. Metadata-only changes remux; unrelated attachments remain independent.
2. Removed ranges transform dependent audio/captions/chapters exactly once.
3. Each effect has actual profile availability and independent output assertions.

### User Story 4 - Portable and qualified delivery (Priority: P1)
Download and run Windows/Linux/macOS packages without development prerequisites;
use CPU-only media mode or a qualified GPU plan. Run the matching owner harness.

**Why this priority**: Offline portability and evidence are release requirements.
**Independent Test**: D6-J01-D6-J08, AR7-J01-AR7-J03 and owner artifact matching.
**Acceptance Scenarios**:
1. First core task runs offline with bundled verified FFmpeg/ffprobe.
2. Owner GPU reports match the exact candidate payload hashes.
3. Missing gates block release promotion; no fabricated success or binary links.

### Edge Cases
Changed inputs/outputs, source aliases, Unicode/native paths, hostile secondary
opens, private credentials, absent media/devices, slow consumers, disk full,
crash/publication races, stale revisions, expired scopes and future project data
follow the precise failure contracts in Sections 4-8 and Annex D.

## Requirements

### Functional Requirements
- **FR-001**: Preserve R01-R22 and all original-request traceability rows unchanged.
- **FR-002**: Every completed domain capability MUST pass five-surface parity.
- **FR-003**: All media MUST execute through admitted local FFmpeg/ffprobe stages.
- **FR-004**: Required workflow, source and acceptance ledgers MUST remain explicit.
- **FR-005**: Ordinary builds/tests MUST execute in hosted GitHub CI.
- **FR-006**: No implementation or review work may merge into main automatically.

### Key Entities
Asset/Stream, Project/Revision, ExactTime/TimeMap, Command/Receipt/ReplayScope,
Profile/Principal/Grant, Plan/Stage, Job/Artifact, UI/PreviewSession and Evidence.
Detailed fields, guards and lifetimes are mandated in Sections 4-6.

## Success Criteria

### Measurable Outcomes
- **SC-001**: All admitted required workflow journeys have real output evidence.
- **SC-002**: All advertised operations have equivalent admitted five-surface results.
- **SC-003**: Every published artifact matches its component and qualification record.
- **SC-004**: All unfinished required obligations block the complete-release claim.

## Assumptions
- The supplied mandate already settles routine product and architecture decisions.
- Android remains later scope; current desktop target qualification is still required.
- Public visibility was explicitly reconfirmed by the owner in this session.
- The only initial main commit is GitHub's generated README bootstrap.
