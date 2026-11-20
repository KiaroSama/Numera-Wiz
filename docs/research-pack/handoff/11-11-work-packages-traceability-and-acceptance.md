> Generated view of [Numera-Wiz-Implementation-Mandate-EN.md](../Numera-Wiz-Implementation-Mandate-EN.md), revision 7.0. Main SHA-256: `126c8aa8aa7f874802a09600040a1986cff650ad70b70f58f30a441d87fbef06`. Normative text is unchanged; fragment links point to the complete directive.

## 11. Work packages, traceability and acceptance

### Using this directive as a build handoff

The complete root mandate is authoritative. The companion's `handoff/` directory contains generated, section-sized reading copies of that same directive, a requirement-to-evidence matrix and a concrete work-package index. They are navigation and execution aids; they do not introduce another specification. Preserve their source hash and regenerate them when the mandate changes. The archived source inventories remain the authority for observed donor identities; the selected Numera contracts remain the authority for the new application's behavior.

Start by reading the fixed requirements, scope, process/domain contracts and this work-package table. During each package, read its complete relevant sections and the corresponding FFmWiz/capability/command records. Do not treat the presence of a source ID, task ID, Rust type, mocked handler or generated help page as a delivered behavior. Conversely, do not reopen a selected ordinary design merely because several technically possible alternatives exist. A concrete failed build, source incompatibility, unsupported format or failed independent fixture is a reason to revisit the affected choice.

Use the existing E01-E10 order as the dependency spine. The work packages below divide that order into reviewable engineering outputs without postponing interfaces. A package that exposes a domain capability includes native GUI, browser GUI, CLI, stdio MCP and HTTP MCP access together. The first complete capability remains the E03-E06 parity-gated slice. Later packages may develop internal code concurrently, but incomplete adapters are not advertised as supported. The full admitted initial scope is still required before the initial complete release.

### Work-package index

The module names below express ownership within the selected workspace; they are not an instruction to introduce another crate for every row. Keep the established core, service, FFmpeg, transport, UI and shell boundaries. A module may be renamed with a recorded map; its responsibility and acceptance evidence must remain identifiable.

Dependencies identify required implemented contracts, not a requirement to finish every acceptance case of the preceding package before starting a dependent package. For the first E03-E06 slice, implement WP03's owner/state/authentication/DTO foundation first, then the necessary WP04 intake, WP05 planning/preview and WP06 shared-editor subsets. Their integrated five-surface result closes WP03's first-slice parity gate. Continue the remaining WP04-WP06 scope afterward. A foundation substep is recorded as implemented but unqualified until integrated acceptance evidence exists; it is not a separate release or a completed capability. The task graph must not require the whole WP03 parity gate before its first consumer tasks can start.

| Package | Dependencies and E-step | Concrete implementation output | Completion evidence |
|---|---|---|---|
| WP01 Rules, scope and research intake | First; E01 | Applicable rules record; completed Spec Kit constitution/specification/plan/checklist/tasks/analysis; pinned source/manual archive; complete coverage seed; CI bootstrap; current support/exception decisions. | Original user requirements map to actual planned tasks, no duplicate conflicting contracts, full required manual study is assigned and recorded, the initial CI artifacts exist. Research is not marked as application completion. |
| WP02 Standard runtime and portable launch | WP01; E02 | Exact standard FFmpeg/ffprobe build profile and dependency closure; embedded payload manifest; offline extraction/repair; effective portable paths; `doctor`; headless launch; first-party language and size reports. | CI-built packages start on clean target environments with no compiler/Python/Node; actual feature/import closure matches the manifest; extraction failure preserves the previous usable profile. |
| WP03 Owner, state and all transports | WP02; E03-E06 | Data-root owner, compatibility handshake, durable store/journal, project and destination claims, access profiles, replay scopes, typed DTOs, CLI, stdio and HTTP MCP, public browser pairing/events. | Fresh owner-to-agent setup; identical first edit/plan/job through all surfaces; owner exit/restart, receipt retry, conflict, revocation and active-request lifecycle cases; clean MCP stdout. |
| WP04 Media intake and durable assets | WP03 foundation; integrated first-slice closure with WP03; E04-E06 | Paged file/probe intake; external versus managed asset identity; browser upload/promotion; relink; project collection; proxy/thumbnail/waveform artifacts and leases. | Import then disconnect/restart/cleanup cannot lose a committed managed asset; source replacement invalidates dependent artifacts; original versus proxy selection is inspectable; collected project relocates without stale external staging references. |
| WP05 Timeline compiler and preview | WP03-WP04; E05-E08 | Exact source/sequence/output clocks; canonical timeline/audio graph; stream manifest; parameter-clock lowering; NUT native preview; browser MSE presentation; clock-relative backpressure. | Independent frame/sample and impulse fixtures; preview/export semantic identity; pause/seek/reconnect and stale-generation cases; actual target-browser playback; measured bounded producer/consumer behavior. |
| WP06 Editing and shared workspace | WP03-WP05; E06-E08 | FilmCraft-derived panels/workspaces/resolver; transactional trims/inserts/overwrites/ripple/slip/slide; linked tracks; keyframes; history; navigation and utility entry points. | Explicit edit-invariant fixtures, rollback of whole failed edits, source/sequence keyframe-clock cases, effective Default shortcut maps and native/browser interaction comparisons. Media operations use WP05, not a UI-side engine. |
| WP07 Complete FFmWiz utility baseline | WP03-WP05; E07 | All F01-F15 workflows, the explicit complete MuxCls standalone/embedded Rust rewrite under F08/streams.clean, all 40 legacy configuration keys and their import reports; per-stream maps; cut/retime/join; batch policy; subtitles; metadata/chapters/covers; analysis and capability cache. | Every Annex A/D acceptance case and MuxCls coverage row has an identified executable fixture/assertion; real output streams/timing/tags/content are checked; sampled analyses remain labeled; no GUI prompt is required for machine execution. |
| WP08 Editing media functions | WP05-WP06; E08 | Mixer buses/sends/latency/automation, text/captions/graphics, masks, color/scopes, transitions, nesting/multicam and the admitted interchange subsets. | Multitrack story fixture covers actual project changes, save/reopen, timed audio/video, font/alpha/color behavior and export; each partial or unavailable donor function has its precise recorded boundary. |
| WP09 Canonical effect completion | WP05-WP08; E08 | Implement canonical descriptors in bounded families from the 610-row donor ledger; retain alias/variant semantics and ordinary standard components; generate controls, help and machine schemas. | Each admitted variant has actual FFmpeg-profile availability, independent output assertions, limits and five-surface conformance; all source rows resolve to the implemented canonical owner or an explicit permitted unavailable/data disposition. No count of rows is a count of delivered effects. |
| WP10 Resources and platform integration | Starts with WP02 and continues through WP09; E09 | Scheduler admission and accounting; bounded caches/history; GPU-preferred planner; environment/driver discovery; target-platform filesystem/audio/window adapters; resource regression baselines. | The fixed measurement protocol identifies process tree, browser, VRAM and scratch usage; semantic equivalence is retained when optimizing; CPU-only startup and strict/fallback hardware policy are tested in their prescribed environments. |
| WP11 Manuals and extension path | Starts with WP03, closes after WP09; E10 | Human and agent setup, full command/reference/cookbook files, GUI/shortcuts/browser guides, project/media/security/release guides and a worked registered-feature extension. | Packaged-artifact documentation journeys succeed; required CLI/MCP schemas match; the extension exercise reaches every surface through one handler/lowering and adds independent fixtures rather than separate client implementations. |
| WP12 Release, upgrades and owner qualification | WP01-WP11; E09-E10 | Candidate/payload manifest, CI evidence set, transactional migration/rollback, signed or explicitly unsigned artifact state, prebuilt owner harness, report import/validation, immutable release promotion and corresponding source. | Required CI stages and owner report match the exact qualified payload set; incompatible versions cannot corrupt a live owner/store; interrupted update recovers; publication does not rebuild or silently substitute binaries after qualification. |

WP06 is not a late GUI adapter and WP11 is not permission to postpone help until release. Every earlier package has its own working controls, machine routes and current documentation. WP06 finishes the complete editing workspace; WP11 finishes the complete manual set and onboarding verification.

### Concrete contract outputs before broad feature expansion

Extend the existing Rust registry and generated outputs before expanding the effect catalogue. WP03 owns bounded project/sequence projections, object/action grants, observation/resource DTOs and receipt restrictions; WP04 owns pre-probe admission and dependency resolvers; WP05 owns input epochs, temporal-prefix sampling and presentation profiles; WP06 owns assembled shortcut descriptors and sequence/compatibility behavior. WP11 documents their actual generated requests/results, and WP12 joins their evidence to the existing gates.

Required schema/fixture outputs include `project-projections.schema.json`, `project-compatibility.schema.json`, `sequence-settings.schema.json`, `media-observation.schema.json`, `artifact-transfer.schema.json`, `access-profile.schema.json` and platform-specific `shortcuts-default.json`. Generate them from the shared Rust types and registry; these names are output contracts, not an instruction to maintain independent JSON models. Add `previews.observe` and `artifacts.read_chunk` through the same registry and all interfaces. They are Numera application operations, not invented FilmCraft source rows. The donor inventory's operation count is not the complete application's endpoint count.

For each output, commit one complete positive request/result fixture, one relevant rejected request, and the exact owning handler/adapter/task link. Placeholder-shaped documentation is insufficient for the packaged human/agent quickstart. Capture real returned IDs in that CI journey. Finish these contracts through the first integrated five-surface slice rather than building all panels against mock data or postponing agent observation until release.

### Capability-sized task contract

Within these packages, create tasks around one meaningful operation, shared primitive or supported variant. Every task contains the following information before it is called implementation-ready:

- Stable task ID, related R/F/CD/canonical capability IDs, source pin and relevant manual sections.
- Owning Rust modules and the shared schema/handler/lowering to change; affected platform adapters only where necessary.
- Selected semantics, units, defaults, errors, state class and resource preconditions; every nontrivial time/alpha/channel boundary is named.
- Required profile features and an actual admission rule for formats, parameters, components, CPU/GPU and memory/scratch.
- Native and browser UI entry points plus CLI/MCP discovery/execution mappings; ordinary capability tasks cannot omit one surface.
- One or more independent acceptance assertions, fixture definition/provenance and expected outputs; negative, lifecycle or fault cases where the operation creates that risk.
- Required generated/manual documentation and the CI stage/artifact that will prove completion.

Avoid one task per source alias, one public command per preset or a single enormous task called “implement all effects.” Aliases can share a task after their parameters and semantic differences are reconciled. A shared primitive task is complete only with representative real consumers that expose its format, alpha, time and resource boundaries.

### Requirement and evidence records

Commit `docs/requirements-traceability.csv` and a generated machine-readable completion ledger in the implementation repository. The supplied traceability seed maps every original numbered request and subsequent clarification to the selected R IDs, sections, work packages and required evidence. Refine it with actual task/PR/artifact links; never replace an unmet requirement with a looser paraphrase to turn its status green.

Each completion record has a stable `subject_id`, `subject_kind` (`requirement`, `workflow`, `capability_variant`, `command_domain`, `platform`, `release`), relevant requirement IDs, task IDs, selected semantic/schema revision and an explicit status. Initial statuses are `planned`; use `in_progress`, `blocked`, `implemented_unqualified`, `qualified` and `unavailable` with their stated meanings. `Unavailable` is valid only for an already permitted unsupported semantic function, a precise optional-component/device state or an explicitly outside-scope platform. It is not a substitute for implementing an ordinary required function.

A `qualified` record contains implementation commit/module references; exact application/component/configuration/fixture identities; CI run and artifact references; an asserted domain and observed outcome; per-surface results; and the actual platform/browser/device qualification where relevant. Do not invent run IDs, output hashes, timings, screenshots or owner reports in advance. A unit result, media result, transport result and packaged-artifact result establish different claims and must retain that distinction. Evidence is invalidated by the declared affected-input rules, not merely by whether a Git commit changed.

The release evaluator performs relational checks, not a test-count threshold:

1. Every fixed requirement and F01-F15 family has complete required evidence, with no unexplained unassigned or blocked initial-release obligation.
2. Every supported donor source row reaches a canonical capability/variant and its actual evidence; permitted unavailable and data rows retain their precise disposition. Canonical deduplication must not erase a variant.
3. Every CD01-CD34 domain and source-command disposition is accounted for. UI aliases resolve to the same typed operation; private helpers and unavailable source extensions are not falsely presented as working public commands.
4. Every completed domain capability has equivalent native GUI, browser GUI, CLI, stdio MCP and HTTP MCP behavior. A genuine client/host-only gesture limitation is reported explicitly without hiding a project/media capability.
5. Every published support claim matches a tested target/browser/profile/device entry, and every artifact listed in the release is the artifact covered by the qualification manifest.
6. Source/notices/SBOM, full command/manual outputs, resource reports, update/recovery results and matching owner GPU evidence are present before the complete release claim.

No denominator such as source rows, code lines, manual sections or named tests measures overall application completion. Report concrete finished workflows and remaining blockers, with evidence at the applicable boundary.

### Practical continuation record

Maintain `docs/development/continuation.md` after each completed work package or interrupted session. Record the current objective and selected decisions, exact repository branch/commit, completed and unfinished task IDs, current artifacts/CI runs, unresolved failures with reproduction inputs, and the next executable engineering step. Include only safe local-path examples and no tokens or private media. The next agent resumes that step instead of restarting donor research, duplicating the architecture or assuming an incomplete feature shipped.

Before reporting completion, reconcile the root mandate, implementation specifications, code/schema/help, coverage ledgers, full manuals and qualification records. The final handoff states the actual supported workflows/platforms, selected donor differences, optional/unavailable functions, binary/source links and remaining external prerequisites. It never substitutes a description of what should be built for evidence of what was built.


### Original-request traceability

This is a normalized English restatement of the 25 numbered items/subitems and 11 subsequent clarifications or retained setup preferences. `U` preserves the original numbering; `C` identifies the clarification, including the latest architecture, minimum main-file size and complete MuxCls rewrite requests. It supplements the binding R01-R22 table without weakening it. Source/manual evidence already exists where described; all application implementation/runtime statuses remain unimplemented/unqualified.

| Original item | Required outcome | R IDs | Main sections | Work packages | Required completion evidence |
|---|---|---|---|---|---|
| U01.1 | Run the Spec Kit chain. | R01 | 1;9;11 | WP01 | Spec Kit constitution, spec, plan, tasks, analysis and convergence tied to code. |
| U01.2 | Read and obey applicable rules. | R01 | 1;11 | WP01 | Applicable rule/preference record before edits and a documented precedence resolution. |
| U01.3 | The implementation agent must research beyond this instruction. | R02 | 1;10;11 | WP01;WP05;WP09 | Additional pinned primary evidence, decisions and affected implementation fixtures. |
| U01.4 | Study and save the FFmpeg documentation using the real host ::deep-research capability. | R02 | 1;6;11 | WP01;WP05;WP07;WP09 | Actual invocation/result or honest availability/fallback record; full selected manual corpus and actual reading coverage. |
| U02 | Official name Numera Wiz; public repository; GPL 3 or later. | R03 | front;1;8 | WP01;WP12 | Public KiaroSama/Numera-Wiz metadata, GPL-3.0-or-later declarations, notices and corresponding source. |
| U03 | Research similar repositories and incorporate useful findings into the mandate. | R02;R11;R20 | 2;10;Annex B | WP01;WP05;WP09 | Pinned donor/comparator observations, explicit adoption/rejection and concrete implementation consequences. |
| U04 | Use Rust for essentially all first-party logic; keep the application fast and capable. | R04;R10 | front;4;5;8 | WP02;WP05;WP10 | Language/dependency closure, no separate application runtime, admitted FFmpeg routes and measured packaged behavior. |
| U05 | Support Windows, Linux and macOS now; plan Android later. | R05 | 4;8;9 | WP02;WP10;WP12 | Four native target artifacts, current platform results and a separate honest Android plan. |
| U06 | README must teach installation and setup for people and AI agents. | R06 | 5;8;11 | WP03;WP11 | Complete fixture/request, captured returned IDs and version-tested stdio/direct-HTTP connection journeys. |
| U07 | Keep complete command documentation and tutorials in multiple repository files. | R07 | 5;8;11 | WP11 | Generated complete reference plus task manuals, examples executed against the packaged artifact in CI. |
| U08 | MCP invocation must be bridge, with direct HTTP and configurable default port 1578. | R08 | 4;5;8 | WP03;WP11 | Real MCP stdio/HTTP profile fixtures, port override/conflict behavior, scoped client setup. |
| U09 | Preserve FilmCraft shortcuts. | R09 | 3;7;Annex C | WP06;WP11 | Complete effective FilmCraft Default resolver map, context/focus tests and visible browser alternatives. |
| U10 | Only develop media operations supported by FFmpeg. | R10 | front;2;6;8;Annex B | WP05;WP07;WP08;WP09 | Every admitted processing descriptor lowers to actual profile features; no independent donor DSP/shader/media engine. |
| U11 | Initial complete release covers FFmWiz and admitted FilmCraft/EffectCraft functionality. | R11 | 2;3;6;Annex A;Annex B;Annex C | WP04;WP06;WP07;WP08;WP09;WP12 | F01-F15, 40 configuration keys, every source-row disposition and CD01-CD34 have actual evidence or the precise allowed limit. |
| U12 | The mandate author must study FFmpeg docs so the instruction is correct. | R02 | 1;6;10;Annex A;Annex B | WP01;WP05 | Supplied hash/range reading records and source-informed corrections; future implementation study remains required. |
| U13 | Design the base so later commands and filters are easy to add. | R12 | 5;8;11 | WP03;WP05;WP09;WP11 | One descriptor/schema/handler/lowering path, meaningful extension fixture and all-surface generated help/discovery. |
| U14 | Base the application and GUI on FilmCraft. | R13 | 4;7 | WP06;WP08 | Shared Rust UI, recognizable workspace/panel/interaction model and real editing journeys. |
| U15 | Lightweight operation and speed are essential. | R04;R20 | 4;6;8 | WP02;WP05;WP10 | Bounded queues/cache/active media closure; frozen workloads and complete resident-memory/latency/scratch evidence. |
| U16 | All builds and ordinary tests belong in GitHub CI; minimize local tests. | R16 | 1;8;9 | WP01;WP02;WP12 | Hosted build/test graph and provenance; no ordinary local Cargo/app/media workflow in setup instructions. |
| U17 | Provide extensive CI testing, including security and other correctness dimensions. | R18 | 8;11 | WP03;WP05;WP07;WP08;WP09;WP10;WP12 | Independent assertions across media, editing, security, recovery, fuzz/resource/format/parity suites and complete required-evidence joins. |
| U18 | CLI and MCP capabilities always stay equal to GUI. | R19 | 3;5;7;8;9;11 | WP03;WP04;WP05;WP06;WP07;WP08;WP09 | Every completed operation has native/browser/CLI/stdio-MCP/HTTP-MCP semantic conformance from the first functional slice. |
| U19 | Use the best valid FFmpeg command for each objective. | R20 | 6;8 | WP05;WP07;WP09;WP10 | Objective-aware copy/encode/resource/device decisions, inspectable conversions and actual correctness/quality measurements. |
| U20 | Detailed logging and the final FFmpeg command must be visible before execution. | R21 | 5;6;7;8 | WP03;WP05;WP11 | Timestamped redacted logs, exact argv/sidecar view and stage-prepared event before each stage starts on every surface. |
| U21 | Support both CPU-only and GPU operation. | R22 | 6;8 | WP02;WP05;WP10;WP12 | Core CPU routes without media-device initialization and qualified stage-specific GPU/strict/fallback behavior. |
| U22 | Run the application interface in a browser too. | R13;R14 | 4;7;8 | WP03;WP05;WP06;WP10 | Shared Rust UI assets and actual current browser/platform matrix with authenticated local-engine playback/editing. |
| C01 | The browser interface uses the local native engine. | R14 | 4;6;7 | WP03;WP05 | Native FFmpeg processes and the same project/job/plan identity; browser media rendering is not moved to ffmpeg.wasm. |
| C02 | The browser has the same appearance and working GUI as the desktop app. | R13;R19 | 4;7;11 | WP06;WP10 | Same UI crate/layout/action model with matched viewport comparisons and actual controls; platform limitations explicitly adapted. |
| C03 | GPU tests run locally on the owner's system. | R17 | 8;11 | WP10;WP12 | CI-built predeclared harness/fixtures on Windows 11 Enterprise with RTX 4070 Ti 12 GB; matching owner report imported without executing report content. |
| C04 | A portable EXE should open and work conveniently. | R15 | 4;8 | WP02;WP12 | Single Windows distribution EXE, offline standard payload, bounded verified extraction/repair and clean-machine launch. |
| C05 | Prefer the GPU when a qualified route satisfies the requested objective. | R22;R20 | 6;8 | WP05;WP10 | Default Auto policy explains selected GPU/CPU path, quality/resource admissibility and actual hardware evidence. |
| C06 | Use the owner's Windows/PowerShell/path setup in concrete setup examples. | R06;R15 | 5;8 | WP03;WP11 | PowerShell 7, G:\Program Files installation and G:\Program Files\Portable\Scripts project examples; synchronous CLI versus MCP-owned pipes verified. |
| C07 | Authored automation includes timestamped INFO, WARNING, ERROR and DEBUG logging. | R21 | 1;6;8 | WP01;WP03;WP11 | Four supported primary levels, bounded redaction and tested CLI/MCP stdout separation. |
| C08 | Deliver the implementation instruction in English Markdown with complete actionable detail. | R07 | front;1-11;Annex A-D | WP01;WP11 | One authoritative English Markdown mandate, matching section views, research evidence and resumable task/evidence records. |
| C09 | The main English Markdown instruction must be at least 500 KB through substantive detail. | R07 | front;4;11;Annex A-D | WP01;WP11 | The authoritative MD itself contains at least 500,000 UTF-8 bytes; its exact byte count and content checks are recorded. ZIP bytes and padding do not satisfy this handoff requirement. |
| C10 | Include the complete implementation architecture in the main instruction. | R04;R12;R19 | 4;5;6;7;8;11 | WP02;WP03;WP05;WP06;WP10 | Composition roots, target/crate dependency graph, module ownership, typed models, command lifecycle, planner, concurrency, bounded queues, persistence/recovery, adapters, extension and packaging contracts plus AR7 acceptance cases. |
| C11 | Explicitly rewrite the complete MuxCls subsystem in Rust and integrate it into Numera Wiz. | R04;R10;R11;R19;R21 | 4;5;6;7;11;Annex A/F08;Annex D | WP04;WP05;WP07;WP11;WP12 | Pinned standalone/embedded source comparison, all-module behavior ledger, typed streams.clean contract, shared Rust execution/publication path, native/browser/CLI/both-MCP parity and full MuxCls acceptance evidence. |


### Cross-domain acceptance index

These 63 planned scenarios retain the previous 34 journeys and add 29 focused contracts, including the complete architecture and explicit MuxCls rewrite. They supplement Annexes A/D and per-domain/effect/format fixtures; none is a completed runtime test. Full assertions and joins are in the CSV and the named mandate sections.

| Scenario | Purpose | Requirements | Work packages | CI gates | Main location |
|---|---|---|---|---|---|
| PJ01 | Portable first useful task | R06;R11;R15;R19;R21 | WP02;WP03;WP05 | D6-G03 | Section 11 / Product acceptance |
| PJ02 | Shared editor intake | R11;R13;R14;R19 | WP04 | D6-G03 | Section 11 / Product acceptance |
| PJ03 | Source to timeline | R11;R13;R19 | WP06 | D6-G03 | Section 11 / Product acceptance |
| PJ04 | Edit interval algebra | R11;R18;R19 | WP06 | D6-G01;D6-G03 | Section 11 / Product acceptance |
| PJ05 | Animation through edits | R11;R12;R19 | WP06 | D6-G03 | Section 11 / Product acceptance |
| PJ06 | Proxy then original | R04;R11;R20 | WP04 | D6-G03 | Section 11 / Product acceptance |
| PJ07 | Relink without substitution | R11;R18;R19 | WP04 | D6-G03 | Section 11 / Product acceptance |
| PJ08 | Mixed audio and automation | R11;R19;R20 | WP08 | D6-G03 | Section 11 / Product acceptance |
| PJ09 | Captions and graphics | R10;R11;R19 | WP08 | D6-G03 | Section 11 / Product acceptance |
| PJ10 | Nested and multicam edit | R11;R19 | WP08 | D6-G03 | Section 11 / Product acceptance |
| PJ11 | Concurrent human and agent | R13;R14;R18;R19 | WP03;WP06 | D6-G01;D6-G03 | Section 11 / Product acceptance |
| PJ12 | Reviewed export and retry | R11;R18;R19;R21 | WP03;WP07;WP12 | D6-G03 | Section 11 / Product acceptance |
| PJ13 | Recovery and portable collection | R11;R15;R18 | WP03;WP04;WP12 | D6-G03 | Section 11 / Product acceptance |
| PJ14 | Same working GUI and keyboard | R09;R13;R14;R19 | WP06 | D6-G03 | Section 11 / Product acceptance |
| M6-J01 | Two clips, an overlay, a keyed crop/transform and a nested title | R10;R11;R19;R20 | WP05;WP07;WP08 | D6-G03 | Section 11 / Compiler and FFmWiz acceptance |
| M6-J02 | Labeled VFR frames at `0,40,100,160,220,300` ms, known extent `330` ms | R10;R11;R20 | WP05;WP07 | D6-G01;D6-G03 | Section 11 / Compiler and FFmWiz acceptance |
| M6-J03 | Known contiguous 48 kHz audio plus video with a nonzero offset | R11;R18;R20 | WP05;WP07 | D6-G01;D6-G03 | Section 11 / Compiler and FFmWiz acceptance |
| M6-J04 | Join three segments with video lengths fixed by the request: first has no French audio, second has French and English audio, third has English only | R11;R19;R20 | WP07 | D6-G03 | Section 11 / Compiler and FFmWiz acceptance |
| M6-J05 | One track feeds both main output and a pre-fader send | R10;R11;R19 | WP08 | D6-G03 | Section 11 / Compiler and FFmWiz acceptance |
| M6-J06 | Numbered audio impulses through dry and latency-bearing insert routes into one bus | R10;R11;R18;R20 | WP08 | D6-G03 | Section 11 / Compiler and FFmWiz acceptance |
| M6-J07 | A long sequence whose full source set exceeds the simultaneous decoder budget | R04;R11;R20 | WP05;WP08 | D6-G03;D6-G04 | Section 11 / Compiler and FFmWiz acceptance |
| M6-J08 | Reverse/boomerang across at least three chunks, with a seeded procedural effect and a visible repeated-endpoint policy. Seek and export from the same frozen snapshot. | R04;R11;R18 | WP05;WP07 | D6-G03;D6-G04 | Section 11 / Compiler and FFmWiz acceptance |
| M6-J09 | Incomplete metadata duration, late-discovered stream, deliberately early seeking and a bounded full-scan/materialization cap. | R04;R11;R18 | WP07 | D6-G03 | Section 11 / Compiler and FFmWiz acceptance |
| M6-J10 | A subtitle cue crosses a removed interval | R10;R11;R19 | WP05;WP07;WP08 | D6-G03 | Section 11 / Compiler and FFmWiz acceptance |
| M6-J11 | Valid structure with an intentionally damaged interior media region, a metadata-only remux, and a declared compatible bitstream reframing case. | R11;R18;R20 | WP07 | D6-G03 | Section 11 / Compiler and FFmWiz acceptance |
| M6-J12 | Mixed folder job: encode, stream-copy remux, multiple extracts, one incompatible subtitle, and two files with different final-mix loudness. Exercise separate deliberate-cancel and owner-crash/restart branches after a completed child. | R11;R18;R19;R21 | WP07 | D6-G03;D6-G04 | Section 11 / Compiler and FFmWiz acceptance |
| D6-J01 | Portable launch/repair journey | R04;R05;R15;R18 | WP02 | D6-G03 | Section 8 |
| D6-J02 | Owner/transport lifecycle journey | R08;R14;R18;R19 | WP03 | D6-G03 | Section 4 |
| D6-J03 | Upgrade/rollback journey | R15;R18 | WP12 | D6-G03 | Section 4 |
| D6-J04 | Human/agent setup journey | R06;R07;R08;R19 | WP03;WP11 | D6-G03 | Section 5 |
| D6-J05 | Profile closure journey | R03;R05;R10;R11;R16 | WP02 | D6-G02;D6-G03 | Section 8 |
| D6-J06 | CI completeness journey | R16;R18;R19 | WP12 | D6-G01;D6-G05;D6-G07 | Section 8 |
| D6-J07 | Owner evidence/promotion journey | R05;R16;R17;R22 | WP12 | D6-G05;D6-G06;D6-G07 | Section 8 |
| D6-J08 | Resource evidence journey | R04;R18;R20;R22 | WP10 | D6-G03;D6-G04;D6-G06 | Section 8 |
| P7-J01 | Bounded coherent editor projections | R04;R13;R19 | WP03;WP06 | D6-G03 | 5 / Bounded project projections |
| P7-J02 | Complete headless shortcut registration | R09;R19 | WP03;WP06 | D6-G01;D6-G03 | 7 / Complete shortcut contract |
| P7-J03 | Future project data compatibility | R11;R12;R18 | WP03;WP06 | D6-G03 | 5 / Future project data |
| P7-J04 | Deterministic sequence settings | R11;R19;R20 | WP05;WP06 | D6-G03 | 5 / Sequence setting behavior |
| P7-J05 | Identifiable headless media observations | R08;R19;R20 | WP03;WP05;WP11 | D6-G03 | 5 / Agent media observations |
| MF7-J01 | Temporal processed-prefix and clip-clock taps | R10;R11;R19;R20 | WP05;WP09 | D6-G03 | 6 / Temporal graph prefix |
| MF7-J02 | Nested temporal schedules and resource bounds | R04;R10;R11;R20 | WP05;WP09 | D6-G03;D6-G04 | 6 / Temporal graph prefix |
| MF7-J03 | Variable input epochs and continuous state | R10;R11;R18;R20 | WP05;WP08 | D6-G03 | 6 / Input format changes |
| MF7-J04 | Reconfiguration evidence under log pressure | R18;R21;R22 | WP05;WP10 | D6-G03;D6-G06 | 6 / Input format changes |
| MF7-J05 | Native and browser presentation color | R13;R14;R19;R20 | WP05;WP06 | D6-G03 | 6 / Preview presentation target |
| B7-J01 | Packaged browser CSP/MIME boot | R13;R14;R18;R19 | WP03;WP05;WP06 | D6-G03 | 7 / Packaged browser boot |
| B7-J02 | Pre-probe input and reference admission | R10;R11;R18 | WP02;WP04 | D6-G02;D6-G03 | 6 / Input admission before probe |
| B7-J03 | Bounded authorized observation/artifact bytes | R04;R08;R18;R19 | WP03;WP11 | D6-G03 | 5 / Artifact transfer |
| A7-J01 | Cross-principal object boundaries | R08;R18;R19 | WP03 | D6-G03 | 5 / Object access |
| AR7-J01 | Architecture dependencies and headless boot | R04;R05;R12;R19 | WP02;WP03 | D6-G01;D6-G02;D6-G03 | 4 / Complete implementation architecture |
| AR7-J02 | Shared semantic pipeline for utilities and editing | R10;R11;R19;R21 | WP03;WP05;WP07 | D6-G03 | 4 / Complete implementation architecture |
| AR7-J03 | Cross-process ownership and retired-version replay | R04;R12;R18;R19 | WP03;WP10;WP12 | D6-G03;D6-G05 | 4 / Complete implementation architecture |
| MX7-J01 | Complete selection and per-file resolution | R04;R10;R11;R18;R19;R20;R21 | WP04;WP07 | D6-G03 | Annex D / MuxCls acceptance |
| MX7-J02 | Partial order and metadata precedence | R04;R10;R11;R18;R19;R20;R21 | WP04;WP07 | D6-G03 | Annex D / MuxCls acceptance |
| MX7-J03 | All preservation categories and strict-copy incompatibility | R04;R10;R11;R18;R19;R20;R21 | WP04;WP07 | D6-G03 | Annex D / MuxCls acceptance |
| MX7-J04 | True no-op copy and unchanged media evidence | R04;R10;R11;R18;R19;R20;R21 | WP04;WP07 | D6-G03 | Annex D / MuxCls acceptance |
| MX7-J05 | Output trees, native paths and companion semantics | R04;R10;R11;R18;R19;R20;R21 | WP04;WP07 | D6-G03 | Annex D / MuxCls acceptance |
| MX7-J06 | No new artifact cannot be success | R04;R10;R11;R18;R19;R20;R21 | WP04;WP07 | D6-G03 | Annex D / MuxCls acceptance |
| MX7-J07 | Independent verification catches semantic mismatch | R04;R10;R11;R18;R19;R20;R21 | WP04;WP07 | D6-G03 | Annex D / MuxCls acceptance |
| MX7-J08 | Collision, cancellation and recovery boundaries | R04;R10;R11;R18;R19;R20;R21 | WP04;WP07 | D6-G03 | Annex D / MuxCls acceptance |
| MX7-J09 | Process faults and finite operation limits | R04;R10;R11;R18;R19;R20;R21 | WP04;WP07 | D6-G03 | Annex D / MuxCls acceptance |
| MX7-J10 | Bounded resources, progress and truthful totals | R04;R10;R11;R18;R19;R20;R21 | WP04;WP07 | D6-G03 | Annex D / MuxCls acceptance |
| MX7-J11 | Native/browser/CLI/MCP parity | R04;R10;R11;R18;R19;R20;R21 | WP03;WP04;WP07 | D6-G03 | Annex D / MuxCls acceptance |
| MX7-J12 | Portable CPU-only package and rewrite completion | R04;R10;R11;R18;R19;R20;R21 | WP01;WP04;WP07 | D6-G03 | Annex D / MuxCls acceptance |


### Cross-domain acceptance contracts

#### Cross-domain product acceptance journeys

These journeys join existing command/domain/media suites. Parameterize them across native GUI, browser GUI, CLI, stdio MCP and HTTP MCP where the task is a domain operation. Presentation-specific steps additionally exercise real native/browser controls. No journey requires a second product/test framework, a local source build or moving ordinary tests out of CI. GPU variants run through the prescribed CI-built owner harness.

| Journey | Starting fixture and action path | Required durable/output observations and failure branch |
|---|---|---|
| PJ01 Portable first useful task | Clean data root; open Media tools; inspect a small file; choose a copy/remux or encode intent; inspect command; submit and retrieve output. Repeat with no project through CLI and both MCP transports. | No hidden project/display dependency, correct selected streams and plan, usable error for missing permission/component, and a verified artifact with the same semantic result. |
| PJ02 Shared editor intake | Create project; import one granted local source and identical browser-uploaded bytes; inspect streams; organize a bin; save, close and reopen after ingest/cache cleanup. | Managed bytes remain available, referenced media retains its external role, IDs and project data survive, and an interrupted upload is absent from ready assets. |
| PJ03 Source to timeline | Mark In/Out; drag video only, audio only and linked A/V to explicit patched tracks; insert once and overwrite once. | Captured ranges/streams and target mapping are identical across surfaces; source selection changes during a drag do not alter it; incompatible or locked destinations fail atomically. |
| PJ04 Edit interval algebra | A coordinate/time-labeled sequence with linked tracks, a gap, captions and range markers; split, trim, ripple, roll, slip, slide, lift/extract and undo selected steps. | Independently specified intervals/source maps and marker transformations match after each step; group deltas stay coherent; boundary/lock/handle conflicts create no partial edit. |
| PJ05 Animation through edits | A simple keyframed gain/position or color property; move, split, slip, stretch, reverse and frame-hold its clip; reopen and render selected times. | Declared curve anchors and hold policy produce independent expected values; no split restart; discrete keys hold; collisions follow the selected policy and undo restores keys. |
| PJ06 Proxy then original | A source with nonzero starts or VFR timestamps plus distinguishable channel/time labels; create/attach proxy; edit; make original unavailable; restore it and export. | Preview identifies proxy use and exact correspondence; ordinary final export blocks missing originals; restoration preserves cuts; an explicitly chosen proxy export is labeled. |
| PJ07 Relink without substitution | Two same-named but different sources; relocate one; search and inspect candidates; relink, then attempt a shorter replacement. | Ambiguity is visible, scanning causes no edit, exact relink commits once, changed identities invalidate derived results, and short replacement reports affected clips instead of silently shortening them. |
| PJ08 Mixed audio and automation | Two clip signals, a send/submix and existing track automation; exercise Read/Touch/Latch/Write with one finite pass; stop, undo/reapply and export. | Lane timing, return interval, signal routing and measured gains agree with the shared compiler; second-controller/disconnect/seek cases cannot silently write a competing or bridged curve. |
| PJ09 Captions and graphics | Import timed multilingual cues; edit/split one; create a title/shape; alter duration and one keyframe; output sidecar plus burned/muxed variants. | Cue text/times and declared style conversion survive save/reopen; rendered text/alpha/fonts are correct; missing font or unsupported subtitle style produces the precise action/report. |
| PJ10 Nested and multicam edit | Two small nested sequences and three known-offset camera sources; switch angles, choose fixed/follow-video audio, then flatten a copy. | Exact selected source intervals and audio policy survive nesting/flattening; recursion is rejected; grid paging does not alter angle identity; export matches the captured edit decisions. |
| PJ11 Concurrent human and agent | Native/browser gesture begins at revision N; an authorized agent changes an affected clip before gesture commit; then disconnect/reconnect one client. | Stale preview is discarded or explicitly reconciled; no guessed retry, duplicate edit or hidden selection; refreshed views share authoritative revision and stable IDs. |
| PJ12 Reviewed export and retry | Freeze a range and output profile; submit; continue editing; inspect commands/events; stop future queue dispatch, cancel one job and retry a failure. | Accepted snapshot stays fixed, queue stop differs from cancellation, no false successful publication, retry has a new linked ID, and all completed artifacts remain retrievable. |
| PJ13 Recovery and portable collection | Save, make an accepted unsaved edit, simulate the existing worker-failure fixture, recover; collect a nested project with duplicate basenames and dependencies; reopen outside source roots. Include managed media referenced only by a retained rollback generation, then run eligible cleanup. | Recovered working versus saved revisions are explicit; media ownership is intact; package relocation is valid or reports specific missing dependencies; originals are not deleted. Restoring the retained generation can still open its managed media; the blob is collectable only after all protecting references/generations are deliberately retired. |
| PJ14 Same working GUI and keyboard | Open the same project/revision/theme at equal viewport in native/browser; exercise workspace/dock/monitor/inspector/utility forms and a representative full-keymap context matrix. | Real actions and corresponding canonical requests agree; browser restrictions have visible alternatives; unavailable actions give shared contextual reasons; controls are not static lookalikes. |

Each completed journey records its requirement/domain IDs, actual canonical command versions, input/output hashes, expected interval/sample assertions, UI control identifiers, platform/browser/profile versions and CI run/artifact identity. Link it to existing source-row/effect acceptance cases where applicable. A skipped external prerequisite remains named pending evidence. The release cannot substitute a catalogue count, schema-only pass, screenshot-only pass or passing mock for the journey's observable result.

#### Compiler and FFmWiz acceptance journeys

Each journey runs through the shared domain contract and is reachable from native GUI, browser GUI, CLI and both MCP transports as appropriate. UI adapters may submit the same fixture/request; the independent oracle must not call the same compiler helper to compute its expected answer. Ordinary execution belongs in GitHub CI. Matching GPU routes are exercised with the CI-built owner harness.

| Journey | Fixture and action | Required independent result | Owner |
|---|---|---|---|
| M6-J01 | Two clips, an overlay, a keyed crop/transform and a nested title; export one frame, a thumbnail and GIF from the same marked range. Separately add a one-second centered dissolve to two adjacent five-second timeline clips, then perform the equivalent overlap-join utility and a one-sided fade. | The same labeled source content, layer order, crop, title and mapped time appear in each output under its declared output-format tolerance. Palette generation cannot include removed footage. Timeline dissolve retains ten-second extent; overlap join has nine-second extent; one-sided fade changes neither duration. | WP05/WP07/WP08 |
| M6-J02 | Labeled VFR frames at `0,40,100,160,220,300` ms, known extent `330` ms; keep `[40,160)` and `[220,330)`, then render a 2x VFR-preserving video variant. | Retained frame IDs are `1,2,4,5`; mapped PTS are `0,30,60,100` ms and resolved mapped extent is `115` ms, subject only to separately reported mux timebase representability. No CFR insertion is allowed. | WP05/WP07 |
| M6-J03 | Known contiguous 48 kHz audio plus video with a nonzero offset; cut the same two ranges as M6-J02 without retiming. Also request a non-grid CFR boundary under strict and snap policies. | Audio selects `[1920,7680)` and `[10560,15840)`, totaling `11040` samples before declared codec padding. A/V offset survives normalization. Strict rejects a non-grid video boundary; snap reports exact resolved ticks. | WP05/WP07 |
| M6-J04 | Join three segments with video lengths fixed by the request: first has no French audio, second has French and English audio, third has English only; one audio input ends early. | Stable output language-track identities, explicit silent gaps and no segment-boundary shift from native concat's longest-stream behavior. Metadata and default dispositions apply to the intended output tracks. | WP07 |
| M6-J05 | One track feeds both main output and a pre-fader send; another uses a post-fader send; apply mute, pan, solo and solo-safe choices. | Muting preserves the first track's pre-fader send; main pan does not unexpectedly pan its post-fader send; solo routing retains exactly the declared connected routes. Expected gains/channel contributions are independently calculated. | WP08 |
| M6-J06 | Numbered audio impulses through dry and latency-bearing insert routes into one bus; apply a sample-position gain ramp and mute boundary, then change upstream frame packet sizes. | Impulses align by the declared effective-latency ledger; creative delay remains audible; gain/mute boundaries and final samples are independent of frame packet size within the processor's declared numeric tolerance. | WP08 |
| M6-J07 | A long sequence whose full source set exceeds the simultaneous decoder budget; include one temporal video effect and one stateful audio bus. | Bounded active decoder/filter/queue counts; staged or continuous state islands preserve seams/tails. Comparison with an admitted continuous small reference detects resets; no result is qualified only from resident memory remaining low. | WP05/WP08 |
| M6-J08 | Reverse/boomerang across at least three chunks, with a seeded procedural effect and a visible repeated-endpoint policy. Seek and export from the same frozen snapshot. | Correct frame/sample/chunk order, only the declared repeated endpoints, deterministic global seed/phase behavior and truthful preview approximation labels. Source content cannot be silently replaced by a different restarted noise sequence. | WP05/WP07 |
| M6-J09 | Incomplete metadata duration, late-discovered stream, deliberately early seeking and a bounded full-scan/materialization cap. | Basic probe remains bounded; dependent operations request required analysis; actual index coverage is recorded; hitting the cap is incomplete/failure and never a successful truncated ordinary export. | WP07 |
| M6-J10 | A subtitle cue crosses a removed interval; another overlaps a reverse span; a third source is repeated and held; include chapter/marker events. Separately retime a source clip to 2x without sequence ripple, with one sequence-authored cue and one source cue both initially at [1,2) seconds. | Exact clipped/split/repeated cue intervals from an independent rational mapper, unchanged cue text, stable source/occurrence IDs and correct burn/mux placement. The sequence-authored cue stays [1,2); the source cue maps to [0.5,1). Nest/export applies only remaining parent/output transforms. Unsupported style/bitmap operations are explicit. | WP05/WP07/WP08 |
| M6-J11 | Valid structure with an intentionally damaged interior media region, a metadata-only remux, and a declared compatible bitstream reframing case. | `standard` reports its sampled scope; `full_decode` detects the covered damage; preservation checks use the correct byte/packet/decoded oracle and do not reject legitimate framing changes as unintended pixel changes. | WP07 |
| M6-J12 | Mixed folder job: encode, stream-copy remux, multiple extracts, one incompatible subtitle, and two files with different final-mix loudness. Exercise separate deliberate-cancel and owner-crash/restart branches after a completed child. | Frozen per-file stream decisions, independent loudness statistics and truthful partial artifact states. Recovery reconciles completed/publication states and retains completed artifacts. Previously accepted queued children remain eligible for dispatch under the recovered queue policy. Cancelled/interrupted children stay terminal; renewed execution creates a new linked retry with recorded authorization. Completed children are not rerun implicitly. Exact commands, receipts and verification scopes remain inspectable; no last-child-success shortcut. | WP07 |


