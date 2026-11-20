# Product implementation contract: bounded reads, complete actions and observable media

This text integrates into the existing domain, shared-GUI and protocol work packages. It keeps their crates, native local FFmpeg engine, shared Rust `nw-ui`, command envelope, authorization, baseline and CI-only qualification rules. No implementation or runtime qualification is claimed by this document. Contract names below are selected design inputs; generated schemas and fixtures must be produced by the assigned implementation work.

## 1. Read the same bounded project from every surface

Use `projects.inspect` and `sequences.inspect` as the existing canonical read commands. `project.id` names the project; `params.revision` is a captured read revision, not an optimistic current-head precondition. Queries omit `project.expected_revision`. If the initial request omits a revision, the service captures one and returns it explicitly. A client cannot label a view “current” after merely reusing an old read revision.

Select these projections in the command schema:

| Command/projection | Required contents and limits |
|---|---|
| `projects.inspect / summary` | Project header/schema status, root-bin and sequence IDs/counts, compatibility warnings and current revision. Do not embed all assets, clips, effects or history. |
| `projects.inspect / bin_page` | A specified bin/query with a stable sort key and ID tiebreaker; bounded asset/bin/sequence summaries, availability and thumbnail handles. Full probe metadata, waveform samples and images are separate reads. |
| `projects.inspect / entity` | A named asset/bin/sequence header or a bounded named detail section. Large extension/probe data requires an explicit subprojection or artifact manifest. |
| `sequences.inspect / track_page` | Stable ordered track summaries, locks, targeting/routing metadata and counts. Page tracks independently of their clips. |
| `sequences.inspect / viewport` | Exact half-open time range, explicit track IDs, zoom/detail level and ordered intersecting clip/transition/marker/caption summaries. Include necessary boundary context as labeled records. Large overlap sets paginate; distant content is not loaded just because it exists. |
| `sequences.inspect / entity` | A named clip/effect/curve/marker/caption detail section for an inspector. Curve points, transcript cues and other large collections page separately. |

Each page has `schema_version`, `project_revision`, `snapshot_id`, `snapshot_expires_at`, normalized query/projection, stable ordered items, `next_cursor` and the snapshot's event cursor. An opaque cursor is bound to authorization, revision, normalized query, sort and snapshot; changing those inputs while reusing it is an error. Initial item target is 128, hard maximum 512; the existing 256 KiB serialized page ceiling wins first. An item that cannot fit must expose bounded detail sections or an explicit too-large result, not silently truncate semantic data. Counts must label exact versus estimated values.

Apply the shared access-profile object-action policy to every query, subsequent page, count and change notification. A snapshot/cursor is an object lookup, not retained permission after revocation. Filter object identity/detail disclosures under current rights; project read/edit permission does not grant filesystem or device access. This uses the selected common policy, not a separate read-model ACL system.

Use the existing 30-second snapshot lease and global retention cap. A viewport query obtains only its own projection's pages, not a hidden prerequisite of downloading the entire project. Expiry discards the incomplete projection and returns `SNAPSHOT_EXPIRED`. Do not combine old and new pages under one revision label. Cache keys include projection/query/revision and relevant media identity; stale display remains visibly stale until refreshed.

Project mutations publish a transaction/change-set identity, base/new revision and bounded changed/deleted ID sets or a projection-invalidation summary. The client applies all available patches for one transaction atomically. If any required page/patch is unavailable or too large, invalidate and requery affected projections; retain the prior coherent display meanwhile. Never use notification arrival order as an alternative project model. Unrelated changes need not force a full-project reload. The engine resolves edit closures; a limited viewport does not limit authoritative edit validation.

`nw-domain` owns projection/query/result types; `nw-service` produces them from committed state; `nw-ui` maintains a bounded presentation cache and transient gesture overlays. CLI/MCP use the same read commands. Do not copy the mutable donor session into the browser or maintain a second authoritative document in a widget.

**Illustrative contract shape, not a runnable quickstart:** every angle-bracket value below is captured from an earlier authorized response; no server-issued ID is fabricated.

```json
{
  "api_version": 1,
  "request_id": "<caller-generated-correlation-id>",
  "command": {"id": "sequences.inspect", "version": 1},
  "project": {"id": "<captured-project-id>"},
  "params": {
    "revision": "<captured-project-revision>",
    "sequence_id": "<captured-sequence-id>",
    "projection": "viewport",
    "track_ids": ["<captured-track-id>"],
    "range": {
      "start": {"domain": "sequence", "num": "5", "den": "1"},
      "end": {"domain": "sequence", "num": "10", "den": "1"}
    },
    "detail": "clip_summary",
    "limit": 128
  }
}
```

**P7-J01:** CI opens a project with a large bin/sequence, reads a narrow viewport and pages a small bin, then performs a remote ripple edit while pages are in flight. Assert bounded bytes and allocations, stable ordering, correct interval membership, atomic visible revision changes and explicit expiry recovery. Native, browser and headless responses must identify the same committed state.

## 2. Assemble FilmCraft's complete shortcut contract without starting its UI

The pinned reference assembles frontend commands inside `FilmcraftApp::new`; `Shortcuts::new` alone has no external descriptors. `menus::external_commands` adds UI command definitions, keyboard commands and generated panels. Default construction includes known-command filtering, ordered panel/default tables, conflict-free Premiere fallback and platform-specific Ctrl/Command handling. The focused-panel resolver wins before application bindings. These are inputs to extraction, not implementation details that can be discarded. [Registration](https://github.com/storytold/filmcraft/blob/d643a886650bfbf16f24762274054575aa1e8f20/crates/ui-egui/src/lib.rs#L538-L543) · [Descriptor assembly](https://github.com/storytold/filmcraft/blob/d643a886650bfbf16f24762274054575aa1e8f20/crates/ui-egui/src/menus.rs#L887-L898) · [Construction](https://github.com/storytold/filmcraft/blob/d643a886650bfbf16f24762274054575aa1e8f20/crates/engine/src/shortcuts.rs#L515-L627) · [Resolution](https://github.com/storytold/filmcraft/blob/d643a886650bfbf16f24762274054575aa1e8f20/crates/engine/src/shortcuts.rs#L943-L951)

Place pure action descriptors in the shared headless registry before command/keymap discovery. A descriptor may require a live client to execute a focus/fullscreen gesture, but listing its parameters/default keys must not initialize a frame server, window, GPU or audio device. Keep source action identity and context separate from the canonical handler: aliases sharing a handler may retain different fixed parameters or bindings.

Generate the full effective Default map for each supported platform from the assembled pinned definitions/resolver, then map each source action to its canonical Numera action and fixed arguments. Record source order, platform, context, chord, canonical action/arguments, host requirement, origin/default/fallback, source anchor and any unbound/collision reason. Apply source resolution before canonical alias consolidation. The existing full-map artifact must cover every bindable action, including those deliberately unbound. A static inventory is evidence of source coverage; only the assembled CI export establishes the reference effective map.

Keep browser-reserved chords as explicit host overrides with visible alternatives and the same canonical operation. Do not call a browser substitution the original FilmCraft chord. Persistent remapping uses its own saved-profile revision; a per-client focus/selection snapshot does not become an implicit global selection for an agent. The complete map is searchable and discoverable from every surface.

**P7-J02:** compare the assembled reference resolver with the generated Numera map for platform/context cases and key collisions. Assert full registry/map fingerprints across all clients; headless discovery starts no media or presentation subsystems. Preserve the already documented Default/Premiere `Shift+E` difference, panel precedence, aliases and intentionally unbound actions. This closes the export mechanism; it does not replace the existing full keyboard acceptance suite.

## 3. Preserve future project data without silently changing output

Version project schema and node-envelope schema explicitly. Reserve bounded, named extension locations for nonsemantic UI/annotation data. These fields round-trip through open/save and unrelated edits without semantic loss; preserving JSON whitespace or key order is unnecessary. Reject duplicate-key ambiguity and apply the declared project parsing/depth/string/collection limits. Unknown fields outside a documented optional-extension boundary must not automatically be classified as harmless.

Every semantic node envelope carries its stable node ID, kind/schema version, owner, enabled state and declared dependency/reference IDs separately from kind-specific payload. An unknown kind with a supported envelope is retained as opaque payload. Its inspector exposes identity, compatibility reason and available explicit recovery actions; it cannot invent controls for unknown semantics. Validate envelope references before allowing edits around it. If dependency meaning or the envelope itself is unsupported, use the read-only compatibility path.

An enabled unknown node blocks any preview/export/analysis plan whose resolved dependency closure includes it. The result names the node and missing capability/version. Unaffected branches may remain editable/renderable only when the declared envelope proves independence. Never silently bypass an unknown effect, drop an unknown required transform or substitute a thumbnail for final output. Explicit disable/remove/replace is a revision-guarded ordinary edit with a change/loss report and normal undo. A new reviewed plan reflects that choice.

An unsupported project schema major opens only through bounded read-only inspection/export of original bytes; it must not be rewritten as the older schema. A known schema with unsupported optional metadata is a different compatibility state. Save preserves opaque supported-envelope payloads and flags them in the document status. These rules apply to Numera's own format and admitted interchange conversion reports; they do not add donor native-project compatibility to the baseline.

**P7-J03:** fixtures combine a future UI annotation, an unknown enabled effect with valid declared references, a malformed reference and a newer major version. Prove semantic round-trip through safe unrelated edits, exact affected-plan rejection, one explicit replacement/undo and no accidental downgrade/write of unsupported schema data.

## 4. Make `sequences.update` settings behavior deterministic

Replace the ledger's unspecified `retime_policy` with generated typed policy fields. Default `time_policy` is `preserve_elapsed_time`: FPS changes preserve exact source and sequence times, clip durations, markers, captions and automation events. They change the output cadence and frame/timecode display. A duration-changing rate-stretch or retime remains its existing explicit edit; it is never inferred from changing sequence FPS. Revalidate drop-frame/timecode compatibility against the new rate.

Default `geometry_policy` is `preserve_authored_values`, matching the donor's `scaleMotion=false` default. Canvas dimensions change without silently changing stored transforms, positions or font sizes. The UI reframes the canvas separately. Optional `scale_supported_motion` reports every affected property and coordinate domain using registered geometric descriptors; it cannot transform an opaque/custom property by guessing from its numeric type. If a requested rescale includes an unsupported property, return an explicit incompatibility rather than claiming a complete rescale. [Pinned settings semantics](https://github.com/storytold/filmcraft/blob/d643a886650bfbf16f24762274054575aa1e8f20/crates/engine/src/commands.rs#L2183-L2251)

A sample-rate setting changes render sampling, not elapsed time or pitch. Channel-layout changes revalidate the existing explicit routing/downmix contract and reject unresolved channel relationships. Working-space changes keep each asset's source interpretation and authored parameter values; derive the new admitted color graph and report consequential display/output changes. Unsupported parameter-domain conversion requires an explicit conversion or rejection, not a claim of appearance preservation.

`sequences.update` commits atomically under `project.expected_revision`. Its result contains old/new settings, selected policies, changed IDs/property summaries, unchanged timing/geometry assertions where applicable, compatibility warnings and derived-artifact invalidations. A bounded change summary may refer to paged report details. Use the same domain computation for any existing edit-preflight presentation and the final commit; a preview is not an additional mandatory approval gate. One operation creates one undo item. Existing accepted jobs retain their captured graph/settings and external-source checks.

**P7-J04:** change a sequence from 24 to 30 fps, double its canvas, and change audio sampling from 48 kHz to 44.1 kHz. Exact authored times remain equal; default coordinate values remain equal; decoded duration/pitch assertions use the existing audio oracles. Explicit supported motion scaling reports only the intended properties. Invalid color/routing/timecode settings reject atomically; frozen accepted jobs remain unchanged.

## 5. Give headless agents the same identifiable media observations

Register `previews.observe` as a bounded read of a captured revision, not a UI-session gesture or an unrestricted file reader. It uses the same FFmpeg graph/lowering and source policy as the corresponding monitor. It needs no visible window, local audio output device or active GUI session. The ordinary request/work lease, authorization, deadline, cancellation and scheduler limits apply. If production cannot fit the bounded observation request, return a typed `RENDER_PLAN_REQUIRED` with the existing plan path; do not introduce a second background-job lifecycle.

The request requires project/sequence or explicit source target, captured revision where applicable, exact time or short half-open audio range, original/proxy policy, selected inspection/display profile and output encoding. A time request also selects the documented frame-selection rule, such as the frame whose presentation interval contains the requested sequence time. Never resolve an agent request through another client's current playhead. Default still inspection is SDR with maximum dimensions 1280 × 720; record the actual transform/size and letterbox/crop policy. Exact/high-bit-depth inspection is a separately declared artifact mode, not an undisclosed SDR conversion. Short audio observation declares range, sample rate, layout and any downmix explicitly.

Illustrative parameter shape inside the standard command envelope; placeholders must be filled from authorized query results:

```json
{
  "command": {"id": "previews.observe", "version": 1},
  "project": {"id": "<captured-project-id>"},
  "params": {
    "revision": "<captured-project-revision>",
    "sequence_id": "<captured-sequence-id>",
    "kind": "still",
    "at": {"domain": "sequence", "num": "5", "den": "2"},
    "frame_selection": "contains_time",
    "source_policy": "original_required",
    "display_profile": "inspection_sdr",
    "max_dimensions": {"width": 1280, "height": 720},
    "fit": "contain",
    "encoding": {"format": "jpeg", "quality": 85},
    "delivery": "inline_or_manifest"
  }
}
```

The result schema includes `observation_id`, `artifact_id`, `manifest_uri`, `project_revision`, target IDs, requested time/range, actual selected frame PTS/duration or audio range, source identities and proxy selection, render/graph identity, display/color/encoding profile, dimensions or audio format, `mime_type`, `encoded_byte_count`, digest, expiry and warnings. Distinguish requested time from the selected frame's actual presentation interval. Multi-source observations expose a bounded provenance manifest instead of embedding every source descriptor. Return missing originals, expired revisions and unsupported render branches as errors, not successful substitute pictures.

Use the build/protocol contract's selected media limits: aggregate encoded media bytes before base64 at most 512 KiB, structured metadata at most 64 KiB and final JSON-RPC result at most 768 KiB. These are an explicit observation exception to ordinary result-page limits. MCP may return its standard image/audio content plus matching structured metadata. Do not put media base64 in arbitrary text fields or invent a video content type. If the requested encoding exceeds the limit, return the same artifact's bounded manifest with `INLINE_BUDGET_EXCEEDED`; do not secretly reduce quality to force an inline result. Larger/exact artifacts use the authorized transfer contract. [Official result types](https://modelcontextprotocol.io/specification/2026-07-28/server/tools#tool-result)

CLI returns the same observation metadata and can save the artifact to an explicit granted destination. A tool-returned manifest/resource need not appear in global discovery. Every fetch revalidates current access; opaque handles are names, not permission grants. Expired content can be regenerated only against a still-valid captured input or a deliberate new request. Native/browser preview and agent observation agree on graph/time/source/display intent; distinct lossy encodings need not have identical decoded pixels.

Observation creation, discovery and retrieval follow the same object-action policy as other application objects. The engine assigns the effective creator principal; target/project rights and generated-artifact rights are checked through the common policy. Neither knowing a project/observation ID nor receiving a URI grants access to another principal's media, filesystem roots or device permissions.

**P7-J05:** a headless CLI and each MCP transport request a labeled frame and a bounded audio interval from the same frozen fixture used by the GUI. Validate requested versus actual timestamps, graph/source/profile identity, visible/audible content, bounded result size, explicit manifest fallback, expiry and denied access. No window/audio playback device initializes. The reference fixture must make wrong time, proxy, color transform and stale revision independently detectable.

## Integration artifacts and ownership

Extend the existing generated schema/reference-doc outputs with the projection queries/results, compatibility envelope, settings policies and observation DTOs. Extend the existing shortcut export with assembled headless registration. Put their fixtures beside the current domain/protocol/UI fixtures and link P7-J01 through P7-J05 in the same CI evidence matrix. Avoid a parallel hand-maintained schema or another application framework. Each descriptor, canonical request, UI action, CLI expansion and MCP wrapper must reference the same Rust types; file inventories and narrative promises alone are not completion evidence.
