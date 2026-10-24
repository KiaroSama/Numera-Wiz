> Generated view of [Numera-Wiz-Implementation-Mandate-EN.md](../Numera-Wiz-Implementation-Mandate-EN.md), revision 7.0. Main SHA-256: `126c8aa8aa7f874802a09600040a1986cff650ad70b70f58f30a441d87fbef06`. Normative text is unchanged; fragment links point to the complete directive.

## 5. Domain model and interface contracts

#### One authoritative model

Use stable opaque IDs for projects, assets, streams, sequences, tracks, clips, links, effects, keyframes, markers, captions, jobs and artifacts. IDs do not encode paths or array positions. Store the native project as versioned `.nwiz` JSON containing a project UUID, schema version, disk generation, exact-time data and explicit references. It is a portable checkpoint, not the only storage of accepted unsaved edits. Keep media external by default; provide a separately specified collect/package workflow with size/path checks and a relocation report.

Represent time as checked rational integers. Define positive denominators, normalized values, overflow errors and a rounding policy at every frame/sample boundary. Use half-open ranges `[start,end)`. Distinguish source presentation time, edited sequence time, output presentation time and transport clock time. VFR input has frame timestamps, not a fictitious constant frame period. A displayed SMPTE/drop-frame timecode is a presentation format with a qualified rate, not a different arithmetic clock. Float seconds may be a convenience input, converted once; they are not the stored edit model.

Store signed/unsigned 64-bit integers losslessly on the wire, using decimal strings where browser JSON number precision would be inadequate. Examples of rationals use `{"num":"30000","den":"1001"}`. Reject NaN, infinity, zero denominators and invalid coordinate sentinels; use tagged `auto`/`unset` variants instead.

The project includes asset identity and selected streams; source ranges and transforms; track targets/locks/sync locks; linked A/V; nesting/multicam; transitions and their partners/handles; ordered effects and masks; keyframes; routing/buses/automation; captions and graphic layers; markers/notes; color/output policy; and missing-component/unsupported-data records. UI view state is separate. A headless edit must supply stable targets and an expected project revision, never inherit another client's focus or selection.

Model asset provenance in layers: canonical native file identity and path, size/mtime plus a quick fingerprint for cheap change detection, probe revision, and a content hash where stronger identity is required. A quick fingerprint is not a cryptographic whole-file identity. Recheck input identities before dependent stages. Keep original media, proxy, waveform, thumbnail, analysis, intermediate and exported artifacts distinct. A proxy is never silently substituted into final-quality export.

Use source-range-to-timeline mappings for all dependent streams. Changing speed or removing a range must also resolve audio, captions, chapters, markers and side-data behavior. Unsupported retiming of a particular stream produces a clear decision before execution; it is not an accidental dropped track.

#### Read the same bounded project from every surface

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

#### Preserve future project data without silently changing output

Version project schema and node-envelope schema explicitly. Reserve bounded, named extension locations for nonsemantic UI/annotation data. These fields round-trip through open/save and unrelated edits without semantic loss; preserving JSON whitespace or key order is unnecessary. Reject duplicate-key ambiguity and apply the declared project parsing/depth/string/collection limits. Unknown fields outside a documented optional-extension boundary must not automatically be classified as harmless.

Every semantic node envelope carries its stable node ID, kind/schema version, owner, enabled state and declared dependency/reference IDs separately from kind-specific payload. An unknown kind with a supported envelope is retained as opaque payload. Its inspector exposes identity, compatibility reason and available explicit recovery actions; it cannot invent controls for unknown semantics. Validate envelope references before allowing edits around it. If dependency meaning or the envelope itself is unsupported, use the read-only compatibility path.

An enabled unknown node blocks any preview/export/analysis plan whose resolved dependency closure includes it. The result names the node and missing capability/version. Unaffected branches may remain editable/renderable only when the declared envelope proves independence. Never silently bypass an unknown effect, drop an unknown required transform or substitute a thumbnail for final output. Explicit disable/remove/replace is a revision-guarded ordinary edit with a change/loss report and normal undo. A new reviewed plan reflects that choice.

An unsupported project schema major opens only through bounded read-only inspection/export of original bytes; it must not be rewritten as the older schema. A known schema with unsupported optional metadata is a different compatibility state. Save preserves opaque supported-envelope payloads and flags them in the document status. These rules apply to Numera's own format and admitted interchange conversion reports; they do not add donor native-project compatibility to the baseline.

**P7-J03:** fixtures combine a future UI annotation, an unknown enabled effect with valid declared references, a malformed reference and a newer major version. Prove semantic round-trip through safe unrelated edits, exact affected-plan rejection, one explicit replacement/undo and no accidental downgrade/write of unsupported schema data.

#### Asset intake, ownership and retention

An imported asset records its origin as **referenced native media**, **managed imported bytes**, or **generated media**, together with stable asset/stream IDs and the existing provenance fields. Show this distinction in asset properties and collect/package reports. Referenced media remains at the user-selected path. Managed imported bytes and generated media have owned immutable locations; they are not ordinary evictable preview cache entries.

For a browser/clipboard upload, receive bytes into a private quota-limited ingest object with an opaque handle, expected size when known, rolling byte count, digest, cancellation and finite expiry. Stream data; do not buffer the whole file in browser WASM or worker RAM. Completion verifies the declared length/digest, closes the writer and promotes the bytes atomically into the managed asset store before a durable project reference can commit. A truncated, abandoned or rejected upload cannot become a ready asset. An unknown size is allowed only under the same hard byte quota. Do not trust the client filename as a storage path or content type.

An ingest handle is bound to its effective principal and data-root identity. The importing operation resolves it to a stable asset identity and actual selected streams. Repeating the same accepted import returns its recorded result; a repeated byte upload may be deduplicated by verified content identity, but it never silently grants another principal access. Directory import preserves an explicit relative-path manifest; reject traversal, duplicate/case-conflicting names and unauthorized directory escapes.

Import may be partially successful only where the operation explicitly returns a per-item result. The atomic `assets.import_and_place` macro has one documented commit boundary: every placed item has verified accessible media and the intended source/track mapping, or no timeline placement commits. Existing staged/managed bytes can remain as a reported reusable import result after a placement conflict; this does not count as a successful edit.

Managed source retention is reference-based: live projects/checkpoints, unsaved durable working state, retained undo/history, accepted job snapshots and explicit keep pins protect their assets. Preview-cache pruning must not delete them. Remove-from-bin is not implicit source-file deletion. An explicit purge operation reports eligible unreferenced managed objects, retention policy and size, validates that references have not changed, and never deletes referenced external originals. Deleting staging after expiry cannot remove a promoted managed asset. Project collect/package includes managed assets, original references and required non-media dependencies under its existing policy.

Managed media lives under `assets/<content-digest>/`, separate from rebuildable cache. The collection root set also includes every retained rollback/recovery store generation. A generation cannot be advertised as recoverable after its referenced managed media or required component profiles have been collected. Retiring a generation releases its references under the same serialized retention decision; collection revalidates references before deleting manifest-owned objects. External originals remain outside managed-object deletion.

Opening an uploaded `.nwiz` does not authorize external paths written inside it. Resolve accessible references through the stored profile/grants and present a relink report for the rest. Saving that project cannot conceal this distinction. A browser download is a transfer of a verified artifact to the browser; the selected browser download folder is not a filesystem path the native engine has obtained authority to write.

#### Proxy and relink workflow closure

A proxy relationship stores original identity/probe revision, proxy identity, selected video/audio stream map, dimensions/SAR/rotation/field interpretation, color interpretation, exact start/duration and an explicit source-time correspondence. VFR or altered-cadence proxies carry a validated correspondence/index; equal nominal duration/frame rate is insufficient. Creating a proxy is an ordinary immutable media plan/job. Attach it only after verification and a project-revision-checked edit; a proxy job that finishes after its asset was replaced returns a valid detached artifact plus a stale-attachment diagnostic.

Preview policy is a requested preference plus an actual selected source. Every monitor exposes original/proxy identity, quality and fallback reason. If an original is absent but its verified proxy is usable, editing/audition may continue visibly in proxy mode. If a proxy is absent, original fallback is permitted only under the user's preview/resource policy. Final-quality export resolves originals independently and rejects missing or changed originals; use-proxies export is a deliberate recorded output policy, never a silent rescue path. Proxy audio downmix or omitted channels cannot masquerade as final channel parity.

Relink candidate search is read-only and bounded to grants. Return exact, probable, ambiguous or rejected candidates with safe matching evidence, not just filename equality. A relink report binds candidate identities/probe revisions to the original project revision and enumerates affected clips, selected streams, proxies, source interpretation, plans and analysis. Applying it validates the report again and commits once. Replacing footage with a genuinely different file is a separate explicit operation, with a report for duration/stream/layout/color/timecode changes and clips that now exceed the usable source range. Never trim those clips silently or turn them into unreported black/silence.

After a successful change, invalidate only derived artifacts whose identity/semantic dependencies changed. Accepted jobs retain their frozen snapshot and source identity checks. The open UI immediately reflects the new source revision without retaining old thumbnails/waveforms as if they were fresh. Undo restores the prior relationship and metadata, but cannot promise to recreate an external file deleted outside Numera.

#### Dependency and configuration selection

Use a small coherent Rust dependency set: serde/serde_json plus generated JSON Schema, clap for the CLI, Tokio for bounded async I/O, Axum/Tower for the selected HTTP service, reqwest/rustls for native clients where needed, official rmcp, egui/eframe Glow, redb for the durable worker store, and narrowly scoped platform/audio APIs. Pin exact compatible versions and source hashes in Cargo.lock and the component lock; do not duplicate HTTP/TLS/runtime/render stacks without a concrete reason. The researched redb seed is 4.4.0, with an explicit 8 MiB cache. Keep native-only dependencies out of WASM, and server/store/process dependencies out of the shared UI model. Prefer existing audited crates over writing another general protocol or archive framework.

Version application configuration. Precedence is explicit CLI option, selected profile/config file, then documented default. Separate portable-data paths, processing profiles and UI preferences. Persist safe preferences; never persist an access token in a project or export preset. `doctor` reports the effective configuration, profile/capability fingerprints, writable roots, port status and qualified/unavailable paths with secrets redacted. Changes to a profile/configuration that affect semantics invalidate relevant plans and cache entries.

#### Registry as the feature contract

Define a versioned `CapabilityDescriptor` and `CommandDescriptor` in Rust. A descriptor must include:

| Field group | Required contents |
|---|---|
| Identity | Stable Numera ID, schema/command version, label, category, functional definition, source provenance and aliases that are actually supported. |
| Parameters | Types, units, default, useful bounds, upstream parser bounds, enum choices, dependent constraints, optional assets and a migration rule. |
| State contract | Query, bootstrap, durable mutation or volatile session; typed target/resource; preconditions and revision fields; acknowledgement/receipt, replay, retention and cancellation behavior. Declare this per typed variant where one command supports both live-session and saved-profile targets. |
| Media semantics | Input/output topology, channel map, supported pixel/sample formats, color/range/alpha domain, spatial bounds, timebase/cadence, latency, preroll and tail. |
| Automation | Per-parameter support for initialization, frame expressions, runtime commands and graph restart; interpolation and discontinuity policy. |
| Availability | FFmpeg build/profile features, external asset/license requirements, CPU route, GPU route/device/driver constraints and precise unavailable reason. |
| Lowering | Typed graph/argv compiler, explicit secondary file/network accesses, stage templates, output validation and error mapping. |
| Resources | Window/history/queue bounds, graph fan-out, scratch estimates, cost profile and limits at high resolution/sample rate. |
| Surfaces | GUI/editor view, browser behavior, human CLI mapping, machine command schema and MCP discovery/execution. |
| Evidence | Examples, independent fixtures/oracles, supported parameter-domain cases, CI results and owner GPU qualification when applicable. |

An effect is a parameterized processing descriptor; actions such as `effects.add`, `effects.update` and `effects.remove` operate on its instance. Do not create a different processing engine behind each action or expose every donor internal alias as a distinct product promise. Generate consistent advanced controls, help, machine schemas and reference docs from the registry; use designed task-specific panels for usability.

New features follow one path: source/manual check → functional contract → descriptor/schema → lowering/handler → resource/availability policy → shared UI controls → CLI/MCP exposure → fixtures/security cases → documentation → qualification. Adding an effect must not require separate command builders in five clients. No dynamic executable plugin system is required for the initial release. Declarative future preset packs may reference only registered capabilities and bounded validated assets.

#### Exact edits and history

An edit is an atomic transaction: validate every affected track/clip/source handle, compute the change and inverse, commit one revision and return changed IDs. Reject cycles in nested sequences and invalid/orphan transition references. Locked-track or sync-lock conflicts fail the whole edit. Razor, insert, overwrite, lift, extract, ripple, roll, slip and slide have distinct documented time invariants; test them with linked and unlinked tracks, captions, transitions, reverse and non-unit speed.

Use reversible patches or structural sharing with byte budgets. A pointer drag keeps a transient view and commits one logical history item on completion; do not deep-clone the entire project at every pointer event. Undo/redo is authoritative document behavior and therefore callable by CLI/MCP with project/revision targets. History eviction must not delete a snapshot retained by an accepted job. Within supported project/node envelopes, save/reopen preserves admitted opaque data under the explicit compatibility contract. Unsupported major/envelope versions remain read-only, and enabled unknown semantic nodes block affected renders.

#### Resolve an edit before mutating the timeline

All interactive and headless edits resolve to an **edit specification** containing the project/sequence revision, stable target IDs, exact requested time/delta, selected collision and link policy, affected track set, source-patch mapping, and marker/caption participation. A source-monitor drag snapshots asset, selected streams and marks at gesture start, as the reference UI does. A later source selection change cannot alter the in-flight drag. Native GUI and browser gestures resolve session selection/targeting into explicit command inputs before commit; CLI/MCP supplies the same inputs directly. [Source drag snapshot](https://github.com/storytold/filmcraft/blob/d643a886650bfbf16f24762274054575aa1e8f20/crates/ui-egui/src/panels/source_drag.rs#L1-L38)

Source patches map chosen source video/audio streams to compatible destination tracks. Destination targeting chooses which tracks a timeline action affects; it does not implicitly change source-stream selection. Linked selection expands the selected clip set according to the explicit link policy before lock, handle, collision or revision checks. Sync lock expands only operations that shift sequence time. A track that the operation must change but cannot change because it is locked causes an atomic rejection; an unrelated locked track does not invalidate the edit.

The engine computes the affected closure before mutation, including linked partners, transitions, captions/markers selected by policy and nested dependencies. It returns a concise change summary or preflight diagnostic using the same logic that commits the edit. A GUI may ask for a bounded drag preview, but the final request records the exact admitted delta and base revision. Machine requests default to rejecting a requested edit outside the valid range; any clamp-to-valid-range mode is explicit and returns requested and applied values. Group operations use one common admitted delta, never independently clamp each member and silently break relative timing.

| Operation | Required interval invariant and collision behavior |
|---|---|
| Razor / split | Split strictly inside the selected half-open clip interval. The union of the two pieces covers the same timeline interval and source presentation as before. Preserve linked-piece relationships, dependent curves and valid transitions. A boundary split is a reported no-change, not a zero-length clip. |
| Standard trim | Change one edge while keeping the opposite edge fixed; preserve retained material's mapping to source time. Admit only legal source handles and the declared neighboring-clip/transition policy. No downstream ripple is implied. |
| Ripple trim | Apply the same edge change, then shift the declared downstream affected closure by the exact duration difference. Captions/markers participate according to the captured ripple policy. Preserve unrelated tracks and reject required locked-track changes. |
| Roll | Move one shared cut between adjacent clips while preserving their combined outer interval. Both source-handle constraints apply. The request names the adjacency/cut; a gap or third intervening clip is not guessed away. |
| Slip | Preserve timeline start, duration and track placement while shifting the source-time window. The signed source delta is explicit. Source-anchored effect animation moves with the selected source content; sequence-anchored automation does not move. |
| Slide | Move the selected clip/group earlier or later, preserving its duration and selected source content; compensate the declared adjacent edges so the outer affected timeline extent stays fixed. Missing neighbors/handles require a precise rejection or an explicitly selected gap policy. |
| Move | Preserve clip duration, source map and relative offsets of the group. Select `reject`, `overwrite` or `insert` collision behavior explicitly. Overwrite trims/replaces only the destination closure; insert opens the declared time span on affected targets/sync-locked tracks. Never infer a destructive policy from an overlapping pixel drop alone. |
| Insert from source | Open an interval of the placed material's exact timeline duration on targeted and eligible sync-locked tracks, split crossing items coherently, then place the captured source streams. Independent audio/video marks produce an explicit J/L layout rather than an accidental shift. |
| Overwrite from source | Replace only the specified destination intervals/streams, splitting existing items at boundaries. It does not shorten the sequence or ripple other tracks. Warn/report affected linked partners according to the captured link policy. |
| Lift | Remove the selected content while preserving the remaining items' positions and leaving the resulting gap. |
| Extract / ripple delete | Remove the requested time spans and close them on the resolved affected closure. Normalize multiple spans once, apply the total piecewise time transform and transform participating markers/captions once; do not shift the same item repeatedly through a client-side loop. |
| Rate stretch | Keep the chosen edge fixed and preserve the selected source interval while changing duration and the time map. Pitch and interpolation policy are explicit. A frame hold is a separately typed operation, not division by a zero speed. |

Same-track overlap must be represented by an intentional operation/transition/composition policy. The planner cannot resolve accidental overlaps by whichever clip happens to appear first in a vector. Transition edits validate their partner IDs and required handles; deleting or splitting a partner applies the captured transition policy and never leaves an orphan. This table specifies Numera behavior using the donor's distinct operations as evidence; it does not require transplanting the donor's floating-point edit arithmetic. [Edit implementations](https://github.com/storytold/filmcraft/blob/d643a886650bfbf16f24762274054575aa1e8f20/crates/edit/src/lib.rs#L260-L365) · [Trim/roll/slip/slide](https://github.com/storytold/filmcraft/blob/d643a886650bfbf16f24762274054575aa1e8f20/crates/edit/src/lib.rs#L725-L1103)

#### Make `sequences.update` settings behavior deterministic

Use the generated typed sequence-setting policy fields recorded in the current ledger. Default `time_policy` is `preserve_elapsed_time`: FPS changes preserve exact source and sequence times, clip durations, markers, captions and automation events. They change the output cadence and frame/timecode display. A duration-changing rate-stretch or retime remains its existing explicit edit; it is never inferred from changing sequence FPS. Revalidate drop-frame/timecode compatibility against the new rate.

Default `geometry_policy` is `preserve_authored_values`, matching the donor's `scaleMotion=false` default. Canvas dimensions change without silently changing stored transforms, positions or font sizes. The UI reframes the canvas separately. Optional `scale_supported_motion` reports every affected property and coordinate domain using registered geometric descriptors; it cannot transform an opaque/custom property by guessing from its numeric type. If a requested rescale includes an unsupported property, return an explicit incompatibility rather than claiming a complete rescale. [Pinned settings semantics](https://github.com/storytold/filmcraft/blob/d643a886650bfbf16f24762274054575aa1e8f20/crates/engine/src/commands.rs#L2183-L2251)

A sample-rate setting changes render sampling, not elapsed time or pitch. Channel-layout changes revalidate the existing explicit routing/downmix contract and reject unresolved channel relationships. Working-space changes keep each asset's source interpretation and authored parameter values; derive the new admitted color graph and report consequential display/output changes. Unsupported parameter-domain conversion requires an explicit conversion or rejection, not a claim of appearance preservation.

`sequences.update` commits atomically under `project.expected_revision`. Its result contains old/new settings, selected policies, changed IDs/property summaries, unchanged timing/geometry assertions where applicable, compatibility warnings and derived-artifact invalidations. A bounded change summary may refer to paged report details. Use the same domain computation for any existing edit-preflight presentation and the final commit; a preview is not an additional mandatory approval gate. One operation creates one undo item. Existing accepted jobs retain their captured graph/settings and external-source checks.

**P7-J04:** change a sequence from 24 to 30 fps, double its canvas, and change audio sampling from 48 kHz to 44.1 kHz. Exact authored times remain equal; default coordinate values remain equal; decoded duration/pitch assertions use the existing audio oracles. Explicit supported motion scaling reports only the intended properties. Invalid color/routing/timecode settings reject atomically; frozen accepted jobs remain unchanged.

#### Animation clocks, edits and collisions

Every animated property has a declared clock anchor: `source`, `clip` or `sequence`, plus its owning asset/clip/sequence and rational key times. The descriptor fixes its supported/default anchor; changing an anchor is an explicit conversion operation with a reported mapping, not a hidden reinterpretation. Ordinary clip-media effect/intrinsic curves default to the observed source-media clock; generated graphic/title curves can declare clip-local time; track/bus automation uses sequence time. The media compiler evaluates these through its one checked time-map model. A private donor comment is not an authoritative clock definition. [Observed effect clock](https://github.com/storytold/filmcraft/blob/d643a886650bfbf16f24762274054575aa1e8f20/crates/project/src/lib.rs#L633-L664) · [Observed key movement](https://github.com/storytold/filmcraft/blob/d643a886650bfbf16f24762274054575aa1e8f20/crates/engine/src/keyframe_move.rs#L43-L108)

Moving a clip changes its placement without rewriting source- or clip-owned key values/times. Slipping changes which part of source-anchored animation is displayed. Rate/reverse changes alter evaluation through the time map rather than overwriting all curve times. Splitting preserves rendered continuity: source-anchored pieces retain source key coordinates; clip-local pieces receive an equivalent range/rebase so a value at the split is neither restarted nor duplicated incorrectly. Retain out-of-view keys needed for later retrimming instead of deleting them whenever a clip becomes shorter. Sequence automation remains attached to sequence time unless the explicit ripple/automation-edit policy transforms it.

A frame hold specifies both the held media instant and `freeze_effect_clock`. Select `freeze_effect_clock=false` by default, matching the pinned FilmCraft option default; show the selected value on the instance. When effect-clock freezing is disabled, source-picture sampling remains held while the effect clock advances according to the declared moving map; clip/sequence clocks remain independent. Tests must distinguish a frozen frame with an animated title/glow from a completely frozen processed result.

Keyframe identity is stable and independent of array position/time. A lane has one value at a given exact time. Explicit collision policies are `reject` and `replace`; a group move resolves all selected source and destination coordinates simultaneously before modifying the lane, so moving keys into each other's old positions does not lose them. One common timeline delta is translated through each selected lane's declared clock; non-invertible maps require an explicit per-key target time or a clear rejection. Report bounded/clamped results; never silently compress keys into one time.

Interpolation belongs to the segment leaving a key. Support the admitted numeric/vector/color/path curves with declared Linear, Hold and qualified Bezier/ease behavior; discrete enum/Boolean/text values hold. Define endpoint extrapolation as holding the first/last value unless the descriptor explicitly selects another policy. Reject incompatible path topology or use an explicitly named hold transition; do not interpolate arbitrary mismatched vertices. Parameters whose current FFmpeg route cannot honor animated values expose that exact limitation and a qualified restart/baked-control route where available, not an animation toggle with no rendered effect.

#### Mixer automation passes

Retain the source's useful modes with explicit Numera recording state:

| Mode | Playback and writing contract |
|---|---|
| Off | Read static values and ignore stored lanes; no automation is recorded. |
| Read | Read stored automation; auditioning a control does not write a persistent lane. |
| Touch | Write while touched, then return to the previously captured automation over the explicit automatch interval. |
| Latch | Begin writing at the first touch, then continue the last value after release until the pass ends. |
| Write | Write the declared armed controls from pass start through its end; it does not implicitly arm every project parameter. |

[Pinned mode definitions and sequence-time lanes](https://github.com/storytold/filmcraft/blob/d643a886650bfbf16f24762274054575aa1e8f20/crates/project/src/mixer.rs#L1-L18) · [Mode behavior](https://github.com/storytold/filmcraft/blob/d643a886650bfbf16f24762274054575aa1e8f20/crates/project/src/mixer.rs#L75-L119)

An automation pass records target lane IDs, base project revision, controller identity, sequence-time transport origin, mode, armed controls, automatch settings and an explicit end policy. One active recording owner controls a given lane; a second client receives a conflict instead of silently interleaving points. Capture samples with bounded rate/count/bytes and a declared curve reduction error bound. Use transport sequence time, not UI arrival or wall-clock time. A delayed value cannot be stamped at the current playhead as if it arrived on time.

Stop commits the reviewed pass as one durable revision/undo item. Cancel discards its transient collection. A seek, reverse, loop wrap or discontinuity closes the current monotonic segment; do not connect its endpoints across an unrelated interval. Continuing after it requires the pass's explicit segmentation policy. A client disconnect/lost recording lease stops collection and retains only a bounded in-memory draft until its declared expiry, with visible finish/discard status; it does not auto-commit half a curve or require durable storage per control event. Owner restart may discard an uncommitted draft and must report that no pass commit exists. Finishing a retained draft uses the ordinary revision/conflict rules.

Audio routing, mute/solo/send semantics, compensation and actual FFmpeg curve evaluation are defined by the shared media compiler. GUI meter/fader movement alone is not evidence that the automation reached preview or export. Check a small independently calculated gain envelope and a routed send/submix signal in the actual rendered samples.

#### Command state classes and acknowledgements

Durability follows the typed operation and target, never the transport, source module, `ui.*` prefix or source-ledger `session` disposition.

| Class | Required behavior |
|---|---|
| `query` | Read persistent state or create bounded expiring result handles, such as a plan or a session-control attachment. Require the applicable authentication/grants and an active request/work lease; do not imply durable edit/job acceptance or allocate a durable mutation receipt. |
| `bootstrap` | Only the explicitly listed identity/pairing/scope/credential operations below. Validate their proof/current authority, finite lifetime and quotas. They do not require a pre-existing replay scope; a retry may issue another bounded credential/scope, never repeat project edits or media work. |
| `durable_mutation` | Persist the accepted effect and receipt under the valid replay scope before acknowledgement. Use project, saved-profile, access-policy, job/control or other declared resource revisions/state guards as appropriate. |
| `volatile_session` | Control a named authorized live session/preview with generation and sequence acknowledgements. Bound retained state; do not write a durable receipt or increment a project revision merely for selection, viewport, playback or audition changes. |

For volatile control, authenticated `ui.sessions.attach` returns the target session generation, a controller-specific `control_stream_id` and current session revision. Each request includes these plus a monotonic decimal-string `control_sequence` in `params`; `session_id` remains the explicit target. Retain a high-water mark and a bounded recent acknowledgement/digest window per attached control stream until its session generation ends. Cap live sessions/streams and reject excess attachment instead of evicting protection for an active stream. A matching recent duplicate returns its acknowledgement; an older/forgotten sequence or stale generation never reapplies the action. A reconnect to a new generation resynchronizes current state and does not automatically replay old gestures. Preview render generations remain separate from this control ordering.

Resolve toggles and deltas to explicit desired state, or require the current session/layout revision for the read-modify-write action. Acknowledgements report the applied sequence and resulting session/resource revision; expired/too-old controls require a state refresh. A request that needs a live client's focus/fullscreen/clipboard still reports the actual host/gesture limitation. Session revisions do not purport to survive owner restart.

Saved settings, shortcuts, presets and workspaces are durable even when their donor row is labeled `session`. For example, `ui.workspaces.apply` changes a live session, while `ui.workspaces.save` persists one captured layout under its workspace revision. `settings.update` and `shortcuts.update` use their saved-profile revision; they do not advance an unrelated project head. An automation gesture may collect bounded transient samples, but its committed `automation.gestures.end`/`automation.record.stop` variant validates the expected project revision and writes one durable edit; `commit=false` discards only the transient gesture. Where a typed command supports session and saved-profile variants, its schema makes that target/discriminator explicit before validation or hashing. No client may select weaker durability for a project edit.

#### Immutable plans and deliberate execution

All processing requests produce an `ExecutionPlan` before execution. Commands such as `media.encode`, `media.cut`, `streams.replace` and `timeline.render` are processing intents that return a plan by default. `jobs.submit` executes a plan. A convenience CLI `--run` performs those same two steps under the chosen noninteractive/approval policy; it is not another command builder. Read-only queries and pure project edits do not spawn a media job merely to conform to this pattern. The plan records:

- Request/command/schema versions and resolved source/project revisions.
- Exact selected assets/streams, input identities and explicit read/write grants.
- A directed acyclic stage graph: probing/analysis, staging, transform/render, verification and publication.
- FFmpeg/ffprobe profile and executable hashes; relevant runtime feature/device fingerprint; model/font/LUT/IR hashes.
- Requested and resolved compute stages, encoder/rate-control settings, time/color/alpha policies and every stream outcome.
- Structured argv arrays, owned working directories, sidecar contents/hashes and all secondary opens.
- Resource/scratch estimates, finite limits, warnings, prerequisite failures and chosen alternatives.
- Intended output claims, overwrite policy, expected verification and recovery/checkpoint rules.
- A versioned canonical plan digest. Hash semantic data, not nondeterministic log timestamps or UI presentation labels.

Execute the reviewed plan by ID/hash. At submission, source change, component update, changed required grant assumption, stale expected project revision or incompatible device yields a precise stale/invalid-plan result. Do not silently regenerate a different command. Once accepted, a job retains its immutable project snapshot and purpose-limited durable execution authorization: subsequent editing or browser-session expiry does not change that render or invalidate acceptance. Revalidate external source identities and explicit execution-grant revocations before dependent stages, as defined in the lifecycle contract. If an allowed GPU fallback was preapproved, its semantic constraints and alternate stages are part of the plan; emit the resolved fallback command and reason before execution. Otherwise return a revised plan for the normal client approval policy.

Analysis-dependent stages, such as two-pass loudness, cannot know their final measurement arguments in advance. The plan contains a typed stage template and admissible measurements. After analysis, validate the values, freeze the concrete argv/sidecars and emit a durable `stage.prepared` event before spawning. Every interface can inspect this exact command. Never fabricate measurements or describe a template as the already-final second-pass command.

Show full escaped display text locally and retain the authoritative argv vector. The supervisor passes argv directly to the OS; it never executes the displayed shell text. Secret-bearing credentials are represented as redacted fields and transmitted through protected mechanisms. Showing the command does not mean asking for a separate confirmation for every automatic stage: the reviewed request and execution policy determine approval.

#### Wire request, response and compatibility

Use one application envelope over native/private HTTP, public HTTP and the MCP adapter. Public API major version is `v1`; individual commands have their own versions. A representative durable project-mutation request is:

```json
{
  "api_version": 1,
  "request_id": "9b63093a-37eb-4684-befc-66f1c49e82b7",
  "idempotency_key": "166384a9-b4a2-4be4-974b-6c73ecfd648a",
  "replay_scope": "<opaque-server-issued-scope>",
  "command": {"id": "clips.split", "version": 1},
  "project": {"id": "project_01", "expected_revision": "12"},
  "params": {
    "clip_ids": ["clip_01"],
    "at": {"domain": "sequence", "num": "5", "den": "2"}
  }
}
```

The result contains `request_id`, `ok`, and either typed `result` or `error`. A durable mutation also returns `receipt_id` and `resource_revisions`, a list of `{kind,id,revision}` for resources whose durable state it committed; creation uses an explicit absent/create precondition, and guarded job transitions need not invent a project revision. A save additionally reports its captured `saved_project_revision` and published `disk_generation`. Volatile controls return their generation/sequence acknowledgement and session revision without a durable receipt. Queries and bootstrap operations return their typed results and any stated expiry. Errors carry a stable code, concise message, field/target where safe, retryability, and bounded details. Generate these types and JSON schemas from the same Rust definitions. Do not serialize private Rust layout or make human log text the machine API.

The source ledger's `canonical_parameters` uses flattened design notation. Each registry descriptor supplies one explicit mapping to this envelope:

| Ledger field | Wire location |
|---|---|
| `project_id` | `project.id`; omit the project envelope for a command with no project target. |
| `expected_revision` for a project edit | `project.expected_revision`. |
| Unqualified `expected_revision` for a different owning resource | The descriptor's specific field under `params`, for example `params.expected_profile_revision` for `shortcuts.update`. The field list alone must not infer a project target. |
| Explicit `expected_layout_revision`, `expected_workspace_revision`, other resource preconditions | Their typed fields under `params`. Require a precondition where an update could otherwise lose a concurrent change; a named unconditional/state-guarded operation documents that exception. |
| Save's `revision` or another captured input revision | Its command-specific field under `params`; it is not silently converted into an optimistic current-head precondition. |
| `request_id`, `idempotency_key`, `replay_scope`, API and command identity | Their envelope locations. Durable-only fields are required only by the declared state class. |
| Remaining operation/session/control fields | `params`, including stable target IDs, session generation and control sequence. |

The wire schema accepts each field at one location only. Reject duplicate/unknown context copies such as `params.project_id` rather than accepting a conflicting second value. CLI/MCP adapters perform this declared flattening expansion before submitting the canonical request; the server does not guess from a source alias. Normalized field placement, typed target, fixed alias values and frozen defaults are identical on every surface.

Durable-mutation idempotency is scoped to the authenticated principal, a server-issued replay scope and the logical operation namespace. Hash the submitted canonical structural request with its command version, before consulting changing defaults or current target existence. Exclude transport correlation fields. Authenticate, validate the replay scope and verify permission to read the receipt; if a matching receipt exists, return its recorded result even if the edit advanced the revision or deleted the target, subject to the selected restricted-receipt result rule after access revocation. If the key exists with different request content, return `IDEMPOTENCY_CONFLICT`. Only a genuinely new request performs current revision/target/grant validation and executes.

Make retention finite without permitting a forgotten old key to execute again. Authenticated `access.replay_scope.create` is a `bootstrap` operation explicitly exempt from requiring an existing replay scope; it issues an opaque signed scope bound to the current principal, stable data-root identity and API major, with key ID, issued time and expiry; default validity is 7 days. A lost issuance response can be retried to obtain another bounded scope without executing domain work. This token is not an authentication substitute. All first-party clients obtain/retain a scope automatically for durable requests and reuse the identical scope and idempotency key for retries. Expired, retired or unknown scopes return `REPLAY_SCOPE_EXPIRED` or `REPLAY_SCOPE_INVALID` without execution; creating a fresh scope expresses deliberate new work, not an automatic retry. The caller can query retained durable receipt/job IDs under current read grants after expiry.

Keep accepted receipt identities, structural digests and recorded outcomes through scope expiry and for all active jobs. Preserve replay-only command-version/alias/digest metadata through unexpired scopes so retiring an executable command cannot hide an accepted result; unmatched new requests for that retired version return COMMAND_VERSION_RETIRED. Compact bulky terminal results to stable receipt/job IDs and a digest where the result is reconstructable. Start with a 30-day terminal-history target and explicit count/byte quotas; when unexpired receipts fill their quota, reject/backpressure new acceptance instead of evicting protection early. Garbage-collect eligible receipts only after their scope can no longer authorize acceptance. Preserve old signing-key verification for every unexpired scope, keep keys in the protected data root, and fail closed on unknown keys or uncertain expiry after a detected clock rollback. Scope issuance and key rotation are tested with crash/restart and expiry. Do not implement indefinite in-memory tombstones or silently retry old work under a new scope.

Minimum stable error families are validation/unsupported-version; missing or changed source; stale resource revision/plan/session generation; control sequence conflict/too old; unavailable capability/component/device; permission/path denial; output collision; resource/scratch exhaustion; process/codec failure; verification failure; cancellation too late; interrupted recovery; and expired event/snapshot cursor. A failed media process must preserve its actual exit/signal, stage identity and bounded diagnostics without exposing secrets.

#### Access profiles and bootstrap

Initialize a stable local-owner principal and a stable discovery-only `agent` principal/profile when the protected data root is first created. The latter has no filesystem/device/network or access-administration grants. Reading the protected runtime record is the native launcher's owner bootstrap; it is not a public HTTP operation. Subsequent owner GUI/CLI mutations acquire an owner replay scope through the authenticated issuer and follow the ordinary durable path.

Use the following narrow credential bootstrap contracts, each rate/count limited with expiring credentials and redacted diagnostics:

- `POST /api/v1/pairing/exchange` validates the one-use pairing proof, exact origin and existing principal/grant selection. It cannot accept caller-defined roots or grant escalation; it does not require a prior bearer or replay scope.
- `access.replay_scope.create` requires current principal authentication and issues only that principal's scope.
- `access.sessions.create` is owner-authorized launch/setup for a selected stored access profile. The stdio adapter obtains a scoped private-transport credential, then forwards every MCP command under that effective principal. Incoming MCP fields cannot select owner identity, create a broader profile or invoke the adapter's owner setup channel. Renewal preserves the profile's principal ID and observes revocation.
- `access.tokens.issue` is owner-authorized direct-HTTP credential issuance for a selected stored profile, with the same maximum 8-hour lifetime unless a stricter profile applies. A token is scoped to its selected instance/audience and cannot carry broader rights than the stored profile. Repeating issuance may create another bounded credential, not repeat a mutation. Token revocation and changes to stored grants are ordinary durable access-policy mutations.

Select `--access-profile <name>` for authorization, distinct from media-processing profiles. The local owner creates a named profile with `access profiles create --name <name> --read-root <path> --write-root <path>`; repeated root flags are explicit grants, and omitted device/network grants remain denied. Canonical operation ID is `access.profiles.create`. The result returns the stable principal/profile IDs and access-policy revision. An existing name is an explicit conflict: inspect it with `access profiles inspect --name <name>` and change it through `access profiles update --name <name> --expected-revision <revision> --grants-file <file>`, whose typed grant document replaces the declared policy. Creation uses an absent-name precondition; update uses the access-policy revision. Neither scoped CLI nor MCP path arguments expand grants. Native owner dispatch may authorize explicitly supplied local paths under the security contract.

The bridge selects `--access-profile <name>`; without it, use the initialized zero-grant `agent` profile, permit discovery, and return a precise permission error for ungranted work. It never silently inherits native-owner file access. Headless browser launch may select the same explicit profile with `serve --access-profile <name> --open-browser`; a native GUI launch may instead provision the previously selected project/assets through the same owner-controlled grant path. Stable principal identity survives credential rotation; expired/rotated transport credentials are not new replay identities. These controls do not claim isolation against a compromised process running as the same OS user.

For direct HTTP, `access tokens issue --for-access-profile <name> --out <token-file> --json` creates a user-only, no-overwrite UTF-8 token file and prints only token ID, principal, expiry and file location. The separate `--for-access-profile` flag names the credential's target; the shared `--access-profile` always selects the effective caller and never grants issuance authority. Do not put bearer values in stdout JSON, shell command arguments, project files or logs. A rejected file creation does not report a usable credential; revoke/discard an issued token that could not be safely delivered. The selected client's documented launch flow reads that file into its supported authorization-header facility; a token-file pathname is not a generic MCP JSON feature. The complete setup contract below selects a concrete client flow. Tokens can be revoked with the owner-authorized durable `access tokens revoke --token-id <id>` operation. Generated setup documentation must show creation, grants, scoped connection, discovery and the first plan; naming an undefined token command is insufficient.

#### Object identity, access and delegation


Path grants do not select which live projects, unsaved edits, plans, jobs or observations another principal may read. Add object-action checks to the existing access-profile policy; this is a local-owner and scoped-agent policy, not a cloud account or team-management system. An opaque ID, content digest, event cursor, plan hash, artifact URI or MCP session ID is a lookup value, never authority. OWASP requires authorization for each operation on an identified object, including when IDs are UUIDs. [Primary reference](https://api-security.owasp.org/editions/2023/en/0xa1-broken-object-level-authorization/)

The engine assigns every durable project, plan, job, ingest object and observation a data-root identity and stable creator principal. Related records also name their owning project/job. A request cannot supply a different creator or ownership field. The protected local owner can administer all objects in that root. A named operational profile can create and use its own objects only within its admitted command and filesystem/device/network policy. The built-in discovery-only `agent` profile cannot create work or acquire object rights. Document this distinction in profile inspection; possession of a transport credential is insufficient.

Select concrete profile fields: `mode` is `discovery` or `operate`; `owned_object_access` is a boolean; `allowed_command_ids` is a finite explicit set. The documented owner `access profiles create` with root flags creates `mode=operate`, `owned_object_access=true`, and resolves the current registry's ordinary non-owner project/media/query/preview/job actions plus the own-principal bootstrap `access.replay_scope.create` into that explicit set. Replay-scope creation cannot target another principal or expand permissions. It grants no access administration, maintenance, global credential operation or arbitrary shell command. Print the effective policy and revision. Future command additions do not silently enter an existing profile's allowlist; an owner update admits them. The initialized `agent` has `mode=discovery`, `owned_object_access=false` and only the documented discovery/schema commands. Setting `owned_object_access=false` withdraws inherited creator access; separately explicit object grants remain subject to the rest of the current policy. An operation must pass both command and object checks.

The selected grant document extends the current versioned profile schema with `object_grants`, an array of `{kind, id, actions}`. `kind` is `project`, `plan` or `job`; `id` is a specific engine-issued identifier, not a wildcard or path. The action sets are `project: read, edit`, `plan: read, submit`, and `job: read, control`. `edit`, `submit` and `control` require the corresponding `read` right. Creator access is derived from the engine record and current operational profile, not copied into a caller-controlled grant. Owner-managed policy may explicitly disable that profile's access to its own objects without changing historical creator IDs. Record the profile's access revision and effective rights in bounded inspection output.

The owner delegates or revokes these rights through the existing `access.profiles.update` / `access profiles update --expected-revision ... --grants-file ...` path. The document replaces the declared policy, so documentation must show preserving existing path/operation grants when adding an object grant. No separate invitation, sharing server or agent self-grant endpoint is introduced. Object grants do not add a filesystem root, a device, network access, access-administration authority or permission to run arbitrary commands.

| Resource/action | Required relationship |
|---|---|
| Project summary, graph, revision, bin/viewport pages and project events | Current project `read`, with authorized filtering before pagination/counting. |
| Project mutation and undo/redo | Current project `edit`, command permission, exact revision and the operation's independent source/output grants. |
| Plan details and sidecars | Current plan `read`; project-derived semantic details additionally require project `read`. Redact protected credentials and unauthorized native paths. |
| Plan submission | Current plan `submit` plus current access to its captured dependencies and outputs; submission cannot borrow its creator's grants. |
| Job metadata, progress, stage commands and log records | Current job `read`; redact paths or embedded project data outside the reader's current grants. A project grant alone does not expose every other principal's job. |
| Cancellation and retry | Current job `control`; retry is a new job and revalidates the requesting principal's current execution grants. |
| Managed asset, derived preview or artifact | Current right to its owning project/job or creator-scoped observation, plus the applicable scoped byte-read purpose. Verified deduplication does not merge access rights. |
| Download of an output created by an admitted job | Current owning-job read plus an exact artifact byte-read purpose bound to that output identity; it does not imply read access to the rest of its destination directory. |

For external source reads, observations and fresh render planning, revalidate current file/source access as well as the object right. An existing project read grant may expose the project's authorized structure but cannot open arbitrary referenced paths. Do not infer permission to inspect an already-open project's unsaved working state merely from read access to an older checkpoint file. Opening a granted checkpoint with no existing root-local project creates creator-scoped state under the existing UUID/writer-lease rules. Attaching to an existing live project requires current project rights; an existing owner-delegated grant already satisfies that authority without another confirmation. Without those rights, a readable older checkpoint permits bounded disk-snapshot inspection or an explicit clone with a new UUID, never access to unsaved live edits or a second writer for the original.

On successful publication, record the artifact's exact identity, digest and owning job, and establish its job-scoped byte-read purpose for current authorized job readers. This lets the documented read-input/write-output profile inspect its own produced output without gaining general read access to the export directory. Check the current artifact/job policy and stored publication identity on every retrieval; a substituted file at the same path is `ARTIFACT_CHANGED`, not the old artifact. Job-read revocation removes this retrieval right. Managed observations similarly retain an exact, expiring byte purpose tied to their creator and source/project authorization.

Before submitting another principal's plan, compare the required resolved capability, input/output and grant assumptions with the submitting principal's current rights. If authority-relevant assumptions differ, return the existing stale/invalid-plan result and require a newly reviewed plan under that principal. If the plan is transferable without semantic or authority changes, atomically accept a new job whose durable purpose-limited authorization is derived from the submitter. Never execute with stored creator credentials. Creator/principal metadata is not media semantics; all-surface parity compares the same admitted operation under equivalent rights, not identical credential IDs.

`ExecutionPlan.required_rights` stores normalized resource identities/actions, resolved paths and purpose constraints. Its semantic digest includes those requirements, not creator-specific credential values or access-policy revision numbers. Record creation principal/policy revision separately as audit metadata. Submission must validate the unchanged reviewed plan hash and prove the submitter satisfies those same requirements; it does not rewrite the plan to substitute another grant ID. A creator-bound ingest, private snapshot or other nontransferable assumption is explicitly marked and requires a new plan if the submitter cannot obtain the same authorized dependency. Irrelevant policy-revision changes alone do not change media semantics; revoked or changed required rights still reject submission.

Apply checks consistently to direct reads, guessed IDs, lists, searches, projection pages, counts, events, resources, observations, logs and downloads. Cursors bind the current principal and query policy; policy revocation invalidates affected pages/streams without leaking later changes. Unauthorized and absent IDs produce the same safe public lookup outcome when distinguishing them would reveal an object. The protected owner audit log may record the actual denial reason. This does not claim protection against a compromised process already running as the same OS user.

Receipt replay preserves the original principal/scope contract. A currently authenticated principal may inspect its own retained receipt identity/status under the receipt-read command even if the operation deleted its target; target existence is not revalidated to authorize replay. Return the exact stored result only while its protected result fields remain readable under current grants. Otherwise return `RECEIPT_RESULT_RESTRICTED` with only the caller's receipt identity, immutable completion state and safe digest; never rerun the operation or rewrite its stored result. Owner audit reads do not impersonate its principal. Accepted jobs still use the already selected durable execution authorization across bearer expiry; explicit grant revocation follows the cancellation/publication-fence contract. Reading a job after expiry requires a new valid transport credential and current object rights.

**A7-J01 — Cross-principal object boundaries (WP03):** create profiles A and B with the same media/export roots but separate projects, plans and jobs. B cannot discover A's unsaved state through IDs, counts, cursors, event streams, preview/resource links or universal command dispatch. The owner delegates one project's read right and one plan's read/submit rights; B can inspect only the authorized objects, and submission succeeds only under B's current source/output permissions. Remove one needed permission, revoke a project grant during paged/event reads, and attempt download with only a copied URI. Assert denial before protected bytes/domain work, preserved receipt semantics and no execution under A's credentials. Repeat through native/scoped CLI, stdio MCP and HTTP MCP.

#### Human CLI

All CLI commands run without GUI initialization. Offer designed verbs for everyday workflows plus a schema-driven universal route for complete coverage. Shared options include `--data-dir`, `--access-profile`, `--json`, `--non-interactive`, `--log-level` and a bounded request file. Mutating/processing commands distinguish planning, submission and waiting; unattended requests with missing required fields return validation instead of opening a window or reading an interactive prompt.

Required command families include `project`, `assets`, `media`, `audio`, `streams`, `metadata`, `chapters`, `cover`, `timeline`, `tracks`, `clips`, `effects`, `keyframes`, `captions`, `graphics`, `mixer`, `proxies`, `previews`, `artifacts`, `analysis`, `batch`, `capabilities`, `components`, `plans`, `jobs`, `events`, `access`, `config`, `doctor`, `serve` and `bridge`. The command-domain and source-row mapping tables add the remaining editing, template, transcript, capture, interchange, settings and session families. The human CLI spelling `project` is an alias for canonical `projects.*`; canonical API IDs use the mapping ledger. Generate command-specific argument validation and schema help from the registry; add `commands list`, `commands describe <id>` and `commands execute --request <file>` so no completed domain operation is hidden from automation.

Examples of the selected invocation structure follow. These illustrate command shape; the packaged human quickstart specified below must additionally ship a complete current-schema request and capture returned IDs rather than using these illustrative placeholders:

```powershell
function Write-NwSetupLog {
    param(
        [ValidateSet('INFO', 'WARNING', 'ERROR', 'DEBUG')][string]$Level,
        [string]$Message
    )
    if ($Level -eq 'DEBUG' -and -not $NwSetupDebug) { return }
    [Console]::Error.WriteLine(('[{0:yyyy-MM-dd HH:mm:ss zzz}] [{1}] {2}' -f (Get-Date), $Level, $Message))
}
$NwSetupDebug = $false
Write-NwSetupLog INFO 'Starting the documented Numera Wiz command sequence.'
$Exe = '.\numera-wiz.exe'
& $Exe # Asynchronous native GUI launch is intentional.
& $Exe capabilities inspect --json | ConvertFrom-Json
if ($LASTEXITCODE -ne 0) { Write-NwSetupLog ERROR "Capability query failed: $LASTEXITCODE"; throw "Capability query failed: $LASTEXITCODE" }
& $Exe media probe --input 'D:\Media\Example.mp4' --json | ConvertFrom-Json
if ($LASTEXITCODE -ne 0) { Write-NwSetupLog ERROR "Probe failed: $LASTEXITCODE"; throw "Probe failed: $LASTEXITCODE" }
& $Exe config import-ffmwiz --input '.\config.env' --out '.\job.json' | Out-Host
if ($LASTEXITCODE -ne 0) { Write-NwSetupLog ERROR "Configuration import failed: $LASTEXITCODE"; throw "Configuration import failed: $LASTEXITCODE" }
& $Exe plans create --request '.\job.json' --json | ConvertFrom-Json
if ($LASTEXITCODE -ne 0) { Write-NwSetupLog ERROR "Plan creation failed: $LASTEXITCODE"; throw "Plan creation failed: $LASTEXITCODE" }
& $Exe jobs submit --plan-id '<plan-id>' --plan-hash '<hash>' --wait --json | ConvertFrom-Json
if ($LASTEXITCODE -ne 0) { Write-NwSetupLog ERROR "Job failed: $LASTEXITCODE"; throw "Job failed: $LASTEXITCODE" }
& $Exe jobs list --json | ConvertFrom-Json
if ($LASTEXITCODE -ne 0) { Write-NwSetupLog ERROR "Job query failed: $LASTEXITCODE"; throw "Job query failed: $LASTEXITCODE" }
& $Exe jobs cancel --job-id '<job-id>' --json | ConvertFrom-Json
if ($LASTEXITCODE -ne 0) { Write-NwSetupLog ERROR "Cancellation request failed: $LASTEXITCODE"; throw "Cancellation request failed: $LASTEXITCODE" }
```

These are the command contracts to implement and validate in CI, not a claim that the binary already exists. The downstream PowerShell pipeline establishes synchronous CLI result capture for the selected GUI-subsystem EXE; verify `$LASTEXITCODE` after completion. Qualify UTF-8 inherited/redirected handles and other shell/process-spawn behavior under the delivery gate. MCP clients spawn the EXE directly with owned pipes/process handles and no `Out-Host`/JSON-parsing pipeline. Assign stable exit codes: `0` success/accepted as specified, `2` usage/validation, `3` availability, `4` conflict/stale state, `5` authorization, `6` execution/verification, `7` cancellation, `8` transport, `9` partial batch failure, `10` resource exhaustion. JSON output remains parseable; progress/human diagnostics use stderr or the selected event stream, never interleave arbitrary text with stdout JSON.

Concrete owner setup for a scoped agent, using an explicitly selected portable data root:

```powershell
function Write-NwSetupLog {
    param(
        [ValidateSet('INFO', 'WARNING', 'ERROR', 'DEBUG')][string]$Level,
        [string]$Message
    )
    if ($Level -eq 'DEBUG' -and -not $NwSetupDebug) { return }
    [Console]::Error.WriteLine(('[{0:yyyy-MM-dd HH:mm:ss zzz}] [{1}] {2}' -f (Get-Date), $Level, $Message))
}
$NwSetupDebug = $false
Write-NwSetupLog INFO 'Starting the documented Numera Wiz command sequence.'
$Exe = '.\numera-wiz.exe'
$DataDir = '.\data'
& $Exe access profiles create --name editing-agent --read-root 'D:\Media' --write-root 'D:\Exports' --data-dir $DataDir --json | ConvertFrom-Json
if ($LASTEXITCODE -ne 0) { Write-NwSetupLog ERROR "Access-profile creation failed: $LASTEXITCODE"; throw "Access-profile creation failed: $LASTEXITCODE" }
& $Exe capabilities inspect --access-profile editing-agent --data-dir $DataDir --json | ConvertFrom-Json
if ($LASTEXITCODE -ne 0) { Write-NwSetupLog ERROR "Scoped discovery failed: $LASTEXITCODE"; throw "Scoped discovery failed: $LASTEXITCODE" }
& $Exe plans create --request '.\job.json' --access-profile editing-agent --data-dir $DataDir --json | ConvertFrom-Json
if ($LASTEXITCODE -ne 0) { Write-NwSetupLog ERROR "Scoped plan creation failed: $LASTEXITCODE"; throw "Scoped plan creation failed: $LASTEXITCODE" }
# Only for direct HTTP: the file contains the secret; stdout contains metadata.
& $Exe access tokens issue --for-access-profile editing-agent --out '.\editing-agent.token' --data-dir $DataDir --json | ConvertFrom-Json
if ($LASTEXITCODE -ne 0) { Write-NwSetupLog ERROR "Token issuance failed: $LASTEXITCODE"; throw "Token issuance failed: $LASTEXITCODE" }
```

The token issuer in this example is the local owner; `--for-access-profile` selects only the resulting credential's target. A scoped caller cannot issue credentials or widen grants by changing either profile argument. A long-lived browser service is launched in its own terminal as `& $Exe serve --access-profile editing-agent --open-browser --port 1578 --data-dir $DataDir | Out-Host`; direct MCP HTTP service uses `& $Exe bridge --transport http --port 1578 --data-dir $DataDir | Out-Host`. Check `$LASTEXITCODE` after either service exits. Their foreground lifetime is intentional; ordinary CLI requests finish when their selected result/wait policy finishes.

#### MCP and direct HTTP

`numera-wiz bridge [--access-profile <name>]` defaults to official MCP stdio with the effective-principal contract above. `numera-wiz bridge --transport http --port 1578` exposes official MCP Streamable HTTP at `http://127.0.0.1:1578/mcp`; each HTTP request authenticates its own scoped bearer, independent of the server launcher's owner authority. It is not a custom JSON-RPC endpoint with an MCP name. Use the official Rust SDK for supported versions, tool/resource schemas and each version's lifecycle/cancellation rules. An MCP session ID is not an authorization credential.

Implement and qualify both selected protocol profiles explicitly:

| Profile | Required distinction |
|---|---|
| `2026-07-28` | Each request carries version/capability metadata; implement `server/discover`. No initialize handshake is required. HTTP uses per-request POST/JSON or SSE responses, without protocol sessions, a standalone GET event stream or Last-Event-ID resumption. Server-originated JSON-RPC requests are not permitted. |
| `2025-11-25` compatibility | Use its initialize/initialized lifecycle and negotiated version. If HTTP session IDs are issued, enforce their bounded lifecycle and reconnect rules. Apply legacy cancellation/disconnect semantics, not modern ones. |

Use SDK version detection and validate it against actual fixtures; do not mix the two profiles into an undocumented hybrid. Numera's durable project/job state, browser authentication and application SSE snapshot/cursor are separate from MCP transport state. [Modern versioning](https://modelcontextprotocol.io/specification/2026-07-28/basic/versioning) · [Modern transport](https://modelcontextprotocol.io/specification/2026-07-28/basic/transports/streamable-http) · [Legacy transport](https://modelcontextprotocol.io/specification/2025-11-25/basic/transports)

For modern HTTP, validate `MCP-Protocol-Version`, `Mcp-Method` and applicable `Mcp-Name` against body metadata/method/target, including the specified encoded-header form. Test missing/mismatched headers and SDK error codes. Closing a modern HTTP response stream cancels that in-flight request; stdio uses its cancellation notification. It never implicitly cancels an already accepted Numera job. Stdio is bounded newline-delimited UTF-8 JSON-RPC and exits promptly on input EOF; the detached owner still follows its job/lease policy. [Modern stdio](https://modelcontextprotocol.io/specification/2026-07-28/basic/transports/stdio)

Provide bounded discovery tools for capabilities and commands, command schema/details, project reads, typed command execution, plan creation/inspection, job submission/status/cancellation, event reads, authorized asset import and preview requests. Generate named tools for principal task workflows where useful, backed by the same handlers; preserve complete capability access through the validated command registry. Tool names/arguments must never accept an arbitrary shell command as the normal media interface. Paginate large catalogues, return a compact summary first, and expose details on demand to avoid sending hundreds of effect schemas with every interaction.

Long exports return job IDs promptly. MCP request cancellation stops that outstanding request; it does not implicitly destroy a previously accepted durable job. Job cancellation is an explicit domain command. Tool execution failures use the SDK's proper error semantics and the same stable domain error data. Resource URIs are opaque (`numera://project/...`, `numera://job/...`, `numera://artifact/...`) and enforce the caller's grants. Do not expose unrestricted `file://` reads or advertise a resource before its artifact is valid.

A typical stdio client configuration shape is:

```json
{
  "mcpServers": {
    "numera-wiz": {
      "command": "G:\\Program Files\\Numera Wiz\\numera-wiz.exe",
      "args": ["bridge", "--access-profile", "editing-agent", "--data-dir", "G:\\Program Files\\Numera Wiz\\data"]
    }
  }
}
```

Create `editing-agent` in the same data root before using this configuration; a missing explicit profile is a configuration error, never a fallback to owner. Validate each named client's actual configuration format in its installation guide. Direct HTTP setup uses the `/mcp` URL and an explicit scoped `Authorization: Bearer <token>` header populated by the selected client setup flow from the protected token file issued above. Support restrictive token-file input/output with no secret logging; document this as a local preconfigured-bearer connection profile. Do not claim universal remote/OAuth client compatibility; implement the official authorization flow before advertising a client that requires it. Qualify at least the official SDK client for stdio and HTTP, plus the actual human/agent client walkthroughs named in README.

GUI buttons, browser gestures, CLI verbs and MCP tools must create the same normalized request and semantic plan. Presentation-only actions such as focusing a tab are labeled as client/session operations; project/media capabilities never become GUI-only merely because their original donor command depended on selection.

#### Give headless agents the same identifiable media observations

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

Use the following MCP media/transfer contract's selected media limits: aggregate encoded media bytes before base64 at most 512 KiB, structured metadata at most 64 KiB and final JSON-RPC result at most 768 KiB. These are an explicit observation exception to ordinary result-page limits. MCP may return its standard image/audio content plus matching structured metadata. Do not put media base64 in arbitrary text fields or invent a video content type. If the requested encoding exceeds the limit, return the same artifact's bounded manifest with `INLINE_BUDGET_EXCEEDED`; do not secretly reduce quality to force an inline result. Larger/exact artifacts use the authorized transfer contract. [Official result types](https://modelcontextprotocol.io/specification/2026-07-28/server/tools#tool-result)

CLI returns the same observation metadata and can save the artifact to an explicit granted destination. A tool-returned manifest/resource need not appear in global discovery. Every fetch revalidates current access; opaque handles are names, not permission grants. Expired content can be regenerated only against a still-valid captured input or a deliberate new request. Native/browser preview and agent observation agree on graph/time/source/display intent; distinct lossy encodings need not have identical decoded pixels.

Observation creation, discovery and retrieval follow the same object-action policy as other application objects. The engine assigns the effective creator principal; target/project rights and generated-artifact rights are checked through the common policy. Neither knowing a project/observation ID nor receiving a URI grants access to another principal's media, filesystem roots or device permissions.

**P7-J05:** a headless CLI and each MCP transport request a labeled frame and a bounded audio interval from the same frozen fixture used by the GUI. Validate requested versus actual timestamps, graph/source/profile identity, visible/audible content, bounded result size, explicit manifest fallback, expiry and denied access. No window/audio playback device initializes. The reference fixture must make wrong time, proxy, color transform and stale revision independently detectable.

#### MCP media observation and artifact transfer

##### Representation and byte budgets

| Result | Selected representation and hard limit |
| --- | --- |
| Ordinary metadata, catalogue and result pages | Existing 256 KiB serialized result limit. Paginate bounded collections; never silently truncate semantic fields. |
| Artifact manifest | `resources/read` at `numera://artifact/{artifact_id}/manifest` returns an `application/json` text resource, at most 64 KiB of manifest data. It identifies the media MIME type separately from the manifest's own MIME type. |
| `previews.observe` media | Standard MCP `image` or `audio` content with the actual MIME type and base64 data, plus its structured observation metadata. Aggregate encoded media file bytes before base64 are at most 512 KiB; structured metadata is at most 64 KiB; the complete UTF-8 JSON-RPC response is at most 768 KiB. This is the one explicit observation exception to the ordinary result-page limit. |
| `artifacts.read_chunk` | Explicit tool/query returning one requested range: at most 128 KiB of raw bytes before base64 and at most 256 KiB for the complete serialized result. It is not an automatic multi-message file dump. |
| Full media file | Authorized streamed HTTP bytes or copy/export to an explicitly granted native destination. No whole-file base64 `resources/read` for a movie. |

`encoded_byte_count` means the encoded JPEG/PNG/audio-file bytes before base64; it is neither decoded pixel/PCM size nor base64 string length. A bounded serializer checks the final response before writing it. Metadata is not duplicated into large text and structured blocks. Standard media content is created only for an explicit observation request, never automatically embedded into every job/list result. Use the pinned SDK's correct result envelope for each supported protocol version. [Official content types](https://modelcontextprotocol.io/specification/2026-07-28/server/tools) · [Official resource model](https://modelcontextprotocol.io/specification/2026-07-28/server/resources)

The MCP adapter preserves domain-command failure and its structured diagnostic in the official tool error result. A successful status query may truthfully report a failed job without itself being a failed query. Invalid expressions and unsupported parameter keys cannot be accepted and silently discarded by a convenience wrapper. Verify representative accepted/rejected semantic effects through the handler and every adapter; a static check that a field name occurs in source code cannot prove that the handler implements its advertised meaning.

Return `observation_id`, `artifact_id`, `manifest_uri`, `mime_type`, `encoded_byte_count` and the semantic observation identity. If the caller's fixed encoding/quality does not fit, keep the same artifact and identity and return the manifest with `INLINE_BUDGET_EXCEEDED`; do not silently lower quality, resize further, trim audio or report the requested bytes as delivered. A subsequent explicit request may choose a smaller observation. Read-only inspection does not move an editor's playhead or change its selection.

##### Manifest, authorization and large bytes

The artifact manifest includes immutable artifact identity, content digest, byte length, media type, producing observation/plan/job identity where authorized, relevant revision/time metadata, availability/expiry and currently permitted retrieval methods. Omit secrets and unauthorized native paths. Bounded ancillary details use separate authorized pages. An opaque artifact ID or resource URI is only a lookup handle: metadata, bytes, chunks and observations all enforce the shared access-profile object-action policy, applicable retained snapshot policy and current effective principal. Creator/local-owner defaults and explicit delegations come from that policy; do not invent another download role system or infer filesystem authority from an object permission. A previously accepted producer job is not a perpetual read grant for every client that learns its output ID.

When the public service is already enabled, expose the artifact bytes at its authenticated application bytes route under the same principal's bearer-header profile. Never expose the private owner token, runtime credential file or a query-token URL to an MCP client. A returned route is a location, not a new credential. It uses the existing public origin/port, not a second listener. Stdio alone neither silently starts a public listener nor requires an HTTP client: the bridge can fulfill `artifacts.read_chunk` through its private authenticated connection while retaining the principal boundary.

A full download reads a sealed artifact with bounded buffers and backpressure. Support a single HTTP byte range and a digest-based ETag/If-Match precondition for resumption; reject multiple-range amplification and changed-identity reads. Do not assemble the complete file in Rust, WASM or protocol memory. Each new request reauthorizes. Reuse the current revocation/cancellation policy for active streams and close revoked streams without sending further application bytes once the server processes the revocation. Already delivered bytes cannot be revoked. Private media responses use `Cache-Control: no-store`, no authenticated cross-origin redirects and safe attachment delivery.

`artifacts.read_chunk` requires artifact ID, expected content digest, byte offset and requested length. Return the validated offset, actual length, total size, digest, base64 data and `eof`; only a final chunk may be shorter. Reject out-of-range offsets, length overflow and stale identity. Per-request authorization and the existing queue/rate/resource limits apply even when a client repeats chunk requests. Document that this is for bounded inspection or a client-controlled transfer; a large download should use the streaming/native route, not fill an agent conversation with base64.

The initial MCP resource capability is `resources: {}` with no resource subscription or list-change promise; resource reads/listing work without subscriptions. Refresh the authorization-scoped, paginated resource list explicitly. Durable jobs and the application's existing event/query APIs remain the source for progress. Do not manufacture modern server-to-client requests or protocol sessions to implement download progress. Keep the already selected 2026-07-28 and legacy 2025-11-25 lifecycle behaviors distinct.

##### Required evidence in the existing gates

Extend the existing protocol/security/platform suites rather than create another independent CI matrix. Both supported MCP versions and both transports must demonstrate a correctly typed still/audio observation; metadata matching captured revision/time/display identity; over-budget manifest fallback; an artifact larger than the process memory budget read with bounded memory; exact and rejected range/chunk cases; changed digest; unauthorized and revoked principal; no private credentials or native-path leakage; and successful cancellation/cleanup. The documented human/agent setup journey reads the manifest and at least one authorized observation or byte range from the actual produced artifact. Package/SDK versions in the evidence remain tied to the candidate already being qualified.

**B7-J03 — Bounded observation and artifact delivery (WP03/WP11):** both MCP versions/transports return the correct frame/audio content and metadata within the selected byte caps, or the same artifact's manifest on overflow. Transfer an artifact larger than the RAM budget with bounded buffers; verify exact range/chunk identity, stale digest, revocation, cancellation and absence of private owner credentials or unauthorized paths.

#### Complete human and agent setup contracts

README quickstarts must be executable from the release package, not from an unpublished source fixture or a handwritten placeholder ID. Ship a tiny licensed/synthetic quickstart fixture or a deterministic CI-produced fixture pack with its hash, and include the complete valid `job.json` example generated from the current request schema. The example uses a supported CPU conversion, explicit source/output selection and overwrite policy. It does not need a timeline project or an optional component. Its expected output fields include duration tolerance, selected streams, codec/container and decoded sample/frame assertions. The owner GPU path is a separate selected plan.

The human CLI guide must capture the returned `plan_id` and `plan_hash`, inspect the complete plan, submit those captured values, wait or poll by the returned `job_id`, and inspect the reported artifact. It must not tell a beginner to copy literal `<plan-id>`/`<hash>` placeholders into an otherwise runnable quickstart. The same fixture and intent are exercised by the GUI, browser and both MCP transports. All authored setup automation follows the four timestamped log levels, checks exit status and never logs secrets; runtime tests still belong in CI.

Use the existing scoped-profile creation and stdio configuration as the simplest agent setup. Name the actual client and tested release in `docs/agents/compatibility.md`. The minimum automated compatibility witness is the pinned official Rust SDK client on both transports; additionally document a version-tested Claude Code stdio and direct-HTTP setup. This does not require a paid model inference call in every PR test. Protocol integration and real client configuration/connection evidence are distinct from an LLM's decision to invoke a tool.

For Claude Code direct HTTP, one concrete documented configuration uses a project-specific environment variable in the Authorization header:

```json
{
  "mcpServers": {
    "numera-wiz-http": {
      "type": "http",
      "url": "http://127.0.0.1:1578/mcp",
      "headers": {
        "Authorization": "Bearer ${NUMERA_WIZ_MCP_TOKEN}"
      }
    }
  }
}
```

This client documents header environment expansion; it does not automatically read an arbitrary token-file path. The owner setup issues the existing user-only token file, and a documented PowerShell 7 launch flow reads that file without printing its value, supplies the variable only to the launched client process and removes its temporary copy afterward. Do not persist the token in the shared JSON or user/machine environment. The guide includes the exact selected port and a clean missing/empty-token error. Record the tested client version and retest when its configuration/transport behavior changes. [Official client configuration](https://code.claude.com/docs/en/mcp#environment-variable-expansion-in-mcp-json)

For the selected preconfigured-bearer profile, an expired or owner-epoch-invalid HTTP credential produces an authentication error before domain execution. The owner issues a replacement token for the same stable profile/principal, restarts/reconnects the client with that token and retains the original durable request identity when checking a possibly accepted operation. A static header does not promise automatic renewal. Do not extend token lifetime or create a public owner-credential endpoint to hide the setup requirement. Browser one-use pairing and stdio credential bootstrap retain their own already-specified flows.

**D6-J04 — Human/agent setup journey (WP03):** run the actual documented launcher/configuration pair from a clean environment; a preexisting developer token variable cannot make a broken guide pass. The first agent journey is complete only when it can discover a compact capability summary, fetch one command/schema, read the fixture inside its granted root, create/inspect a plan, submit it, observe a terminal outcome and read the authorized artifact. Include one denied outside-root request, one deliberate cancellation and one retry after a lost response. Unrestricted host shell access is not required to complete this MCP journey. The human path runs the same complete generated request and captured plan/job IDs through the packaged CLI.

#### Focused command and access acceptance

Hosted CI must cover a fresh data root obtaining its first authenticated replay scope and completing a durable edit/job; a lost scope-issuance response safely retried; and unauthorized bootstrap/issuance denied. Profile creation/update/revocation use their declared durable resource preconditions, without a recursive scope prerequisite.

Compare one normalized project edit from native GUI, browser, CLI and both MCP transports: it has one project/revision location and rejects duplicate context fields. Separately check `shortcuts.update`, a workspace save and job cancellation against their own resource/state guards, without changing a project head. A long volatile-control stream keeps durable receipt storage independent of event count; stale generations/sequences never reapply an action, and a committed automation/edit result remains durable after restart.

From owner CLI alone, create the profile, connect a scoped bridge, discover commands and plan/submit within the granted roots. Verify denial outside those roots and denial of owner/grant escalation through MCP fields, including the universal command tool. Bare `bridge` discovers with zero grants. Rotation preserves stable principal/replay identity; expiry/revocation prevents new requests while accepted-job authorization follows the lifecycle contract. The packaged Windows gate also verifies delayed synchronous PowerShell JSON/exit behavior and clean direct-spawn stdio MCP.


