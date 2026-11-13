# Product implementation review: the remaining service-to-editor seams

Status: design review only. No Numera binary, donor application, build, media process, browser, GPU or runtime test was executed. This review does not upgrade any v6 source row from `DESIGN_ASSIGNED_NOT_IMPLEMENTED_OR_TESTED` or reduce the admitted FFmWiz/FilmCraft/EffectCraft baseline.

The v6 mandate already chooses Rust, one shared native/browser UI, a local FFmpeg owner, command parity, exact media clocks, bounded preview, durable edits and CI qualification. Those decisions are not reopened here. Five narrower gaps still let two competent implementers produce incompatible applications. The corrections below fit the existing crates, command envelope, registry and work packages.

## P7-01 — A coherent snapshot is not yet a bounded editor read model

**Current anchors:** `research_v6/mandate-architecture.md`, “Events and snapshots,” line 128; `research_v6/mandate-gui.md`, “Rendering, responsiveness and preview identity,” lines 90–94; `research_v6/command-functional-mapping-final.csv`, source rows `project.inspect` and `sequence.inspect`.

The selected snapshot lease prevents mixed revisions, and the GUI must virtualize. However, `projects.inspect(project_id; revision?; cursor?)` and `sequences.inspect(project_id; sequence_id; revision?; range?; cursor?)` still do not identify the projection, stable ordering, interval membership, sparse-detail loading or transaction boundary of an update. A literal full-project response followed by a virtualized widget technically satisfies much of the prose while making a large browser project expensive to load and repeatedly serialize. Applying a ripple edit's notifications one entity at a time could briefly show an impossible timeline.

**Selected correction:** specialize the existing read commands into explicit summary/bin/entity and sequence viewport/track/entity projections. Use the existing 256 KiB page budget, immutable snapshot ticket and revision/cursor rules. Return bounded summaries and fetch expensive details separately. Commit view changes atomically by transaction/revision, or invalidate affected projections and replace their coherent pages; do not infer document changes from notification order. `nw-service` owns the projections and `nw-domain` their DTOs. Both GUI targets and machine interfaces read the same contracts.

**Required evidence:** a tiny visible interval inside a large sequence and a small bin page must have bounded responses; an overlapping remote ripple edit must produce either the old coherent view or the new coherent view, never a half-applied revision. This is a missing selected application contract, not evidence of a donor failure.

## P7-02 — A headless shortcut export can omit the frontend that defines the reference defaults

**Current anchor:** `research_v6/mandate-gui.md`, “Keyboard contract,” lines 49–76. The existing requirement for a complete effective map is correct; another list of famous editing keys would not resolve this issue.

**Pinned evidence:** FilmCraft registers frontend shortcut descriptors in `FilmcraftApp::new`, immediately before creating a frame server. `menus::external_commands` combines `UI_COMMANDS`, keyboard panel commands and generated `PanelKind::ALL` actions. The shortcut registry begins with an empty external list; registration rebuilds unmodified built-in bindings. `known()` filters fallback/panel entries, platform collision filtering removes some Ctrl/Command collisions, and resolution searches the focused panel before application bindings. Therefore exporting only `engine::command_specs()` or `Shortcuts::new()` is not the assembled reference map. [UI registration](https://github.com/storytold/filmcraft/blob/d643a886650bfbf16f24762274054575aa1e8f20/crates/ui-egui/src/lib.rs#L538-L543) · [External descriptors](https://github.com/storytold/filmcraft/blob/d643a886650bfbf16f24762274054575aa1e8f20/crates/ui-egui/src/menus.rs#L887-L898) · [Default construction](https://github.com/storytold/filmcraft/blob/d643a886650bfbf16f24762274054575aa1e8f20/crates/engine/src/shortcuts.rs#L515-L627) · [Platform collision rule](https://github.com/storytold/filmcraft/blob/d643a886650bfbf16f24762274054575aa1e8f20/crates/engine/src/shortcuts.rs#L365-L379) · [Resolver](https://github.com/storytold/filmcraft/blob/d643a886650bfbf16f24762274054575aa1e8f20/crates/engine/src/shortcuts.rs#L943-L951)

**Selected correction:** move descriptor availability, not UI execution, into the headless registry. Assemble engine, frontend, panel and keyboard descriptors before generating the default map. Resolve the pinned source map in source order for each platform before mapping donor actions/aliases into canonical Numera operations. Publish deliberately unbound commands and collision reasons too. A host-dependent action remains discoverable with an explicit execution requirement; starting a window/frame server to enumerate it is forbidden.

**Required evidence:** the complete default-map fingerprint is identical when discovered from native GUI, browser, headless CLI and both MCP transports. Focus/platform resolver fixtures, including the already documented Default/Premiere `Shift+E` distinction, must prove behavior rather than just compare counts. Source inspection here establishes the registration trap; it does not claim a runtime-complete map was exported.

## P7-03 — Unknown-data preservation lacks an execution and mutation boundary

**Current anchors:** `research_v6/mandate-domain.md`, “Exact edits and history,” line 74; `research_v6/mandate-delivery.md`, documentation row for `docs/project-format.md`/`docs/interchange.md`, line 288. The existing wording requires preservation and an availability warning but does not select which writes or renders remain valid.

An old application can preserve an unknown effect's JSON and still silently render the surrounding clip without that effect. Conversely, refusing every edit because an optional future UI annotation exists would make ordinary project exchange unnecessarily fragile. Unknown effect semantics, unknown optional metadata and a wholly unsupported schema are different cases.

**Selected correction:** define an explicit preservation boundary. Optional nonsemantic extension locations round-trip unchanged in meaning; unknown required schema semantics open read-only. A known node envelope can preserve an unknown effect payload and its declared dependencies as opaque data, while blocking render plans that depend on that enabled node. An unrelated branch remains editable only when the envelope/reference contract proves the edit safe. Disable, replace or remove is an explicit ordinary edit with a loss/change report, never an implicit export fallback. Unknown references or envelope versions trigger the conservative read-only path.

**Required evidence:** future metadata plus an unknown enabled effect round-trip through an unrelated safe edit; affected export is blocked with the exact node ID; explicit replacement creates one ordinary edit; an unsupported project major version cannot be rewritten. This is a gap in the new format contract, not an obligation to import donor native project files.

## P7-04 — Sequence settings need selected migration semantics, not only a `retime_policy` field

**Current anchor:** `research_v6/command-functional-mapping-final.csv`, `sequence.settings` → `sequences.update`, parameters ending in `settings_patch; retime_policy`. The functional description promises a migration report but leaves the policy undefined.

**Pinned evidence:** FilmCraft's handler defaults `scaleMotion` to false, assigns the new frame rate/sample rate to settings, validates the result and only calls the motion-scaling helper when explicitly requested. That is enough evidence to preserve its default “do not automatically rescale motion” behavior. It is not evidence that every coordinate, arbitrary effect or external asset can be rescaled correctly. [Sequence settings handler](https://github.com/storytold/filmcraft/blob/d643a886650bfbf16f24762274054575aa1e8f20/crates/engine/src/commands.rs#L2183-L2251)

**Selected correction:** frame-rate changes preserve exact elapsed/source times by default and change output cadence/timecode presentation; duration-changing retimes remain explicit edits. Canvas changes preserve authored coordinates by default; optional rescaling enumerates only schema-declared geometric properties and rejects unsupported requested transformations. Audio sample-rate changes preserve duration/pitch, while channel layout changes revalidate the already selected routing rules. Working-space changes retain source interpretation and authored parameter values unless an explicit conversion is supported. Return one atomic change report and invalidate dependent derived artifacts; accepted jobs retain their frozen inputs.

**Required evidence:** 24 → 30 fps preserves exact clip/event times; doubling canvas dimensions does not silently double authored coordinates; changing 48 kHz → 44.1 kHz does not retime audio. These are settings-operation semantics, distinct from the v6 keyframe/timebase fixes.

## P7-05 — Command parity does not yet give a headless agent an inspectable media result

**Current anchors:** `research_v6/mandate-domain.md`, MCP discovery/preview/resource paragraphs, lines 292–294; `research_v6/mandate-architecture.md`, native binary preview envelope, line 147. A preview resource URI is required, but no selected semantic request/result tells an agent which frame it observed or whether it can obtain viewable media without a GUI session.

**Primary protocol evidence:** MCP tools can return image/audio content, resource links and structured results; a tool-returned resource need not appear in `resources/list`. This supports a bounded observation plus manifest design without inventing a video block or treating an artifact URI as an unrestricted filesystem capability. [Official MCP tool-result types](https://modelcontextprotocol.io/specification/2026-07-28/server/tools#tool-result)

**Selected correction:** add `previews.observe` as a bounded read against an explicit captured project revision and sequence/source time. Return requested and actual time, source/proxy identity, graph/display/encoding profile, dimensions or audio layout, content digest and observation/artifact identity. A still image or short audio excerpt may be delivered using the separately selected MCP media-byte limits; larger or exact-quality output returns its authorized manifest/transfer route without hidden quality reduction. The native/browser preview widgets, CLI and MCP refer to the same underlying rendering semantics, while their declared encodings can differ.

**Required evidence:** a headless agent can request and inspect a labeled frame, identify its exact time/revision/quality, and distinguish a stale snapshot or unavailable original from a successful observation. An existing GUI session is not a prerequisite. The build/protocol review owns media transport, byte limits and large-artifact reads; this finding owns observation semantics.

## What is still missing from an actionable handoff

The mandate does not need another layer of feature counts. Before broad implementation, the assigned work packages need five small, reviewable contract outputs with independent fixtures:

| Concrete output | Why an inventory cannot substitute for it |
|---|---|
| Read-projection Rust DTOs and generated schema, including revision/transaction application rules | An operation name does not select which bounded data a timeline or inspector can load. |
| Assembled headless action descriptors and full platform/default-map export | Source definitions alone omit registration and resolution behavior. |
| Project compatibility/opaque-node envelope rules and one future-data fixture | Preserving bytes alone does not establish safe mutation or rendering. |
| `sequences.update` policy enums and before/after report fixture | A settings patch does not define what happens to existing media/edit geometry. |
| `previews.observe` request/result schema and labeled frame/audio fixtures | A resource URI alone does not let an agent interpret the observation. |

These are extensions of the existing schema/fixture/reference-doc outputs, not five new subsystems. The companion contract supplies the decisions and examples. The implementation agent must generate and qualify the artifacts in the already authorized CI workflow; until then they remain planned deliverables.
