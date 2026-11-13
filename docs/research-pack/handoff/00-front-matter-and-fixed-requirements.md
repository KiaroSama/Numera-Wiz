> Generated view of [Numera-Wiz-Implementation-Mandate-EN.md](../Numera-Wiz-Implementation-Mandate-EN.md), revision 7.0. Main SHA-256: `126c8aa8aa7f874802a09600040a1986cff650ad70b70f58f30a441d87fbef06`. Normative text is unchanged; fragment links point to the complete directive.

# Numera Wiz — Implementation Mandate

**Revision:** 7.0  
**Source baseline:** 2026-10-21  
**Completeness review date:** 2026-10-23  
**Official product name:** Numera Wiz  
**Target public repository:** `KiaroSama/Numera-Wiz` — https://github.com/KiaroSama/Numera-Wiz  
**First-party license:** `GPL-3.0-or-later`  
**Implementation language:** Rust for essentially all handwritten first-party application, UI, service, command, and automation logic  
**Current delivery:** Windows, Linux, macOS, and the same editor in a browser connected to the local native engine  
**Planned later delivery:** Android

## Document map

- [1. Workflow and research](../Numera-Wiz-Implementation-Mandate-EN.md#1-workflow-and-research)
- [2. Functional scope and migration](../Numera-Wiz-Implementation-Mandate-EN.md#2-functional-scope-and-migration)
- [3. Editing and command domains](../Numera-Wiz-Implementation-Mandate-EN.md#3-editing-and-command-domains)
- [4. Engine ownership and durable state](../Numera-Wiz-Implementation-Mandate-EN.md#4-engine-ownership-and-durable-state)
- [5. Domain model and interface contracts](../Numera-Wiz-Implementation-Mandate-EN.md#5-domain-model-and-interface-contracts)
- [6. FFmpeg execution and preview](../Numera-Wiz-Implementation-Mandate-EN.md#6-ffmpeg-execution-and-preview)
- [7. Shared editor and shortcuts](../Numera-Wiz-Implementation-Mandate-EN.md#7-shared-editor-and-shortcuts)
- [8. Resources, security, distribution and CI](../Numera-Wiz-Implementation-Mandate-EN.md#8-resources-security-distribution-and-ci)
- [9. Implementation sequence](../Numera-Wiz-Implementation-Mandate-EN.md#9-implementation-sequence)
- [10. Additional project research](../Numera-Wiz-Implementation-Mandate-EN.md#10-additional-project-research)
- [11. Work packages, traceability and acceptance](../Numera-Wiz-Implementation-Mandate-EN.md#11-work-packages-traceability-and-acceptance)
- [Annex A. Complete FFmWiz behavior](../Numera-Wiz-Implementation-Mandate-EN.md#annex-a-complete-ffmwiz-behavior)
- [Annex B. Donor capability decisions](../Numera-Wiz-Implementation-Mandate-EN.md#annex-b-donor-capability-decisions)
- [Annex C. Source-command closure](../Numera-Wiz-Implementation-Mandate-EN.md#annex-c-source-command-closure)
- [Annex D. Complete MuxCls Rust rewrite](../Numera-Wiz-Implementation-Mandate-EN.md#annex-d-complete-muxcls-rust-rewrite)

## Read this first

You are the implementation agent. Build the application specified in this document, carry its implementation through CI and packaging, and leave auditable evidence for the required behavior. A repository skeleton, a GUI mockup, a catalogue of possible filters, a research report, or a collection of passing mocks is not the requested product.

This is the sole implementation directive. It supersedes previous Numera Wiz mandates. The research companion supplies source evidence, inventories, and reading records; historical proposed recipes are not additional instructions. If you encounter an old requirement to replicate every donor pixel/DSP algorithm, preserve every private donor effect ID, import every donor-native project, or write a new software GUI renderer, do not inherit it. Those requirements were not requested. Preserve the functional baseline specified here, using practical FFmpeg implementations and truthful Numera semantics.

The intended result is a complete, light media editor and utility application: FilmCraft's editor layout and interaction model, FFmWiz's media workflows and the complete MuxCls cleanup/remux rewrite, useful FilmCraft/EffectCraft capabilities implemented by FFmpeg, and first-class CLI/MCP access to the same engine. The first usable vertical slice is a development milestone. It is not permission to shrink the complete initial release to conversion and trimming.

Read in this order: fixed requirements; workflow and research; scope/migration; process and persistence architecture; domain and protocol; media execution; shared GUI; testing/distribution; documentation; implementation work packages and acceptance journeys; the full FFmWiz behavioral annex and the explicit MuxCls rewrite annex. Use bounded reads and retain a task/decision ledger across context windows. A truncated output, retrieved file, or indexed heading does not count as a completed reading.

### What revision 7.0 resolves

The complete architecture is included in this file: composition roots, crate dependencies, module ownership, typed records, command acceptance, planner stages, bounded concurrency, durable tables, recovery, presentation adapters, extension rules and distribution boundaries. Section 4 selects how these parts fit together; Sections 5-8 specify their detailed contracts. Annex D makes the complete MuxCls Rust rewrite explicit, comparing its standalone source with the already required embedded FFmWiz F08 behavior. It is integrated into the shared engine and every interface, without retaining a Python/PowerShell/robocopy runtime dependency.

The main Markdown deliverable itself must contain at least 500,000 UTF-8 bytes. The archive size does not satisfy this request. Achieve the size through actionable architecture, per-source behavior, contracts, acceptance criteria and evidence, not repeated paragraphs, padding, copied logs or fabricated implementation detail. This is a requirement for this handoff artifact, not a rule that every future repository manual must be oversized. The companion validation records the exact delivered byte count.

This revision corrects a concrete temporal-effect mapping error and selects implementation contracts that were still open in the previous handoff. Echo and hold-rate sample the preceding processed layer in clip time. Exact renders cannot silently survive FFmpeg filtergraph reinitialization. Native and browser previews have a common declared presentation target, with separate proof of transport and color behavior.

The shared editor now consumes explicit bounded project/viewport projections. Shortcut export includes the donor's frontend action registration. Sequence-setting changes have defined time/geometry/audio behavior, and unknown future project data has safe edit/render boundaries. CLI/MCP observations identify captured revision, requested and actual time, source/proxy and display identity; bounded image/audio results, manifest resources and streamed/chunked artifact retrieval are selected. Browser startup has concrete CSP/MIME rules, the first probe has a scoped admission route, and object access is defined independently from filesystem paths.

The review records 12 substantive findings and their concrete resolutions in `research_v7/review-resolution-ledger.md`. The source-to-capability corrections are applied in the current CSV/Markdown mappings, not left as an unmerged appendix. The previous work packages, complete FFmWiz baseline, 610 donor effect records, 942 command source records, manual archive and meaningful prior acceptance cases remain. Counts identify coverage and provenance; they do not measure delivered software or prove this specification has no remaining defects.

The companion includes the actual supplementary deep-research invocation/result record and a source-checked disposition of its conclusions. It does not claim that the literal host `::deep-research` action ran. All application, media, browser, MCP and owner-GPU runtime qualification remains future implementation work in the prescribed environments.

### Evidence labels

| Label | Meaning |
|---|---|
| Source observation | Directly supported by a pinned source file or official manual. |
| Selected design | A concrete Numera implementation decision in this mandate. |
| Qualification pending | The design is specified, but the application/build/fixture has not yet exercised it. |
| Runtime verified | Reserved for an actual identified artifact, fixture, platform and recorded result. |
| Unavailable | A specific missing or unsupported component, semantic operation or hardware path; the reason is visible on every interface. |

The research accompanying this directive is source and manual work. No Numera application, local media benchmark, or owner GPU qualification is claimed to have passed. Do not turn source reading into a runtime claim. Conversely, do not treat the absence of an already-built application as an excuse to postpone selected architecture and ordinary implementation decisions.

## Fixed requirements and where completion is demonstrated

| ID | Required outcome | Completion evidence |
|---|---|---|
| R01 | Use the Spec Kit chain and read applicable rules before editing. | Constitution, specifications, clarified decisions, plan, checklists, tasks, analysis and convergence records linked to actual changes. |
| R02 | Perform further research, including the requested host `::deep-research`, and study/store the versioned FFmpeg documentation. | Real research invocation/result or honest tool-availability record; pinned source archive, reading ledger and implemented conclusions. |
| R03 | Name the product Numera Wiz; use a public `KiaroSama/Numera-Wiz` repository and GPL-3.0-or-later. | Repository metadata, license, SPDX/Cargo declarations, notices and corresponding source. |
| R04 | Keep essentially all first-party executable logic in Rust and make the app fast and light. | CI language/dependency report, bounded resource design and measured packaged-artifact results. |
| R05 | Support Windows, Linux and macOS now; reserve Android architecture. | Current platform packages and qualification; a separate Android design/milestone with no fake current support claim. |
| R06 | Explain human installation and AI-agent setup in README. | Download-to-first-job walkthroughs exercised against release artifacts. |
| R07 | Keep multiple complete command/manual documents in the repository. | Generated command/schema examples and maintained task-oriented manuals. |
| R08 | MCP uses the `bridge` argument, supports stdio and direct HTTP, and uses configurable default port `1578`. | Actual client-spawn and HTTP protocol tests, authentication and nondefault-port examples. |
| R09 | Preserve FilmCraft Default shortcuts. | Effective resolver maps, context/focus/collision tests, documented browser-reserved alternatives. |
| R10 | Develop media processing only through FFmpeg-supported operations. | Each processing descriptor lowers to the selected FFmpeg profile; no independent donor DSP/render engine. |
| R11 | Include the complete FFmWiz functional baseline, the full standalone/embedded MuxCls Rust rewrite, and admitted FilmCraft/EffectCraft workflows in the initial release. | Every baseline family, MuxCls module/behavior and donor coverage row has a completed, truthful disposition and acceptance evidence. |
| R12 | Make later commands/filters easy to add without forking interfaces. | One versioned descriptor, typed handler/lowering, shared controls, CLI/MCP schema, documentation and conformance path. |
| R13 | Base the native GUI on FilmCraft and use the same GUI in the browser. | Shared Rust UI crate, workspace/layout/action model, native/web journey and visual comparisons. |
| R14 | Browser processing uses the local native engine. | Same project/job identities and plans; local FFmpeg subprocesses; no browser ffmpeg.wasm processing dependency. |
| R15 | Ship a convenient portable Windows EXE. | Offline clean-machine double-click and CLI/MCP launch, verified embedded-helper extraction, no installation/runtime download for core tasks. |
| R16 | Build the app/components/harnesses and run ordinary automated tests in GitHub CI. | Build/test workflow and artifact provenance. Local source builds are not part of normal development instructions. |
| R17 | Run actual GPU qualification on the owner's machine with CI-built artifacts. | Matching executable, FFmpeg, harness and fixture hashes plus device/driver/result report. |
| R18 | Provide extensive meaningful correctness, security, resilience, resource and integration tests. | Concrete assertions, independent media oracles, fault injection and explicitly recorded gaps. Test count alone is not evidence. |
| R19 | Keep native GUI, browser GUI, CLI and MCP functionally equal throughout development. | Every completed capability passes the generated parity matrix; no postponed agent adapter. |
| R20 | Select the best valid FFmpeg plan for the user's objective. | Explained stream-copy/encode/quality/resource/device decisions using actual capabilities and measurements. |
| R21 | Log precisely and show the effective FFmpeg command before each subprocess executes. | Immutable plan, stage-prepared event, GUI inspector, CLI output and MCP access; bounded redacted logs. |
| R22 | Work in CPU-only mode and prefer qualified GPU routes by default when they satisfy the selected objective. | Default Auto policy, full core CPU suite, stage-specific hardware discovery, explained CPU choices, strict-policy rejection and explicit equivalent fallback behavior. |

These requirements are binding. Numeric budgets introduced later are engineering targets chosen for this design, not numbers the user supplied or performance already measured.

### Meaning of Rust and FFmpeg-only

Use Rust for the domain model, editor, native/browser UI, worker, planner, process supervisor, CLI, MCP/HTTP adapters, persistence, migrations, test harnesses and project automation. Use a Rust `xtask` instead of creating Python/Node orchestration services. Allowed non-Rust support material includes Markdown, schemas, TOML/YAML, assets, minimal HTML, generated wasm-bindgen JavaScript and narrow native packaging/build wrappers. Record exceptions. Do not add Electron, a separate React/Vue frontend, a Node server, Python application code, or an independent Go service.

FFmpeg and its codec/filter/OS dependencies contain native non-Rust code. Keep that boundary explicit in the SBOM and language report. The requirement is not to rewrite FFmpeg in Rust or to hide native dependencies inside a misleading percentage.

The UI may draw interface elements, timeline shapes and guides, and present FFmpeg-produced video/audio. Rust may compute exact edit decisions, parameter interpolation, bounded control curves, ASS/VGS documents, lookup/control assets and graph descriptions. Media decoding, image/audio effects, composition, analysis, transcoding and muxing use the admitted FFmpeg/ffprobe path. Packaging an independent donor shader or DSP processor and sending its output to FFmpeg is not compliance.

CPU-only means CPU media decode/filter/encode without initializing a hardware media device. It does not require inventing a replacement egui software painter. The selected desktop graphics renderer may use the OS graphics stack; CLI, MCP and the worker must start without any display/audio device.

### Research baseline and change policy

| Source/component | Researched pin | Use |
|---|---|---|
| FFmWiz | `661a0d240ee29ca7cda496b635ae8c50e43d916b` | Required media workflow/configuration baseline. |
| MuxCls | `0cf9596f7510f894babf36595efbc827fbbfaf04` | Complete standalone cleanup/remux source compared with embedded FFmWiz F08; mandatory Rust integration. |
| FilmCraft | `d643a886650bfbf16f24762274054575aa1e8f20` | Shared Rust editor structure, editing behavior and Default shortcut resolver. |
| EffectCraft | `9db7f3b21700fd821f215782ed1c82afe077880f` | Functional effect discovery and source provenance; not a private-ID/DSP-identity requirement. |
| Vidonex | `ae04a3a4e7e455dd3f5f8a954fd2ff0c668fa4c6` | Typed graph/compiler organization and validation ideas. |
| FFmpeg | `n9.0.2`, `946fcce07b6dcd0331c8cc609192aeff5e1924f8` | Versioned manuals, source semantics and CI component build seed. |
| Rust | `1.99.1`, edition `2024` | Toolchain seed; lock the actual tested dependency closure. |
| egui/eframe | `0.36.2` family | FilmCraft-aligned shared UI seed, initially Glow. |
| Spec Kit | `1.2.2` | Pinned integration workflow. |
| MCP / official Rust SDK | `2026-07-28`, required legacy compatibility profile `2025-11-25`; `rmcp 3.6.0` | Protocol/version/transport seed; CI must establish actual interoperability. |

Pins are reproducibility inputs, not instructions to ignore security fixes. Recheck official releases/advisories at implementation start and adopt a coherent supported update with a written impact assessment. Do not float release dependencies on `main`. Archive the exact source, configuration, license, dependency and capability manifests used by each artifact.

Primary references: [FFmWiz](https://github.com/KiaroSama/FFmWiz), [MuxCls](https://github.com/KiaroSama/MuxCls), [FilmCraft](https://github.com/storytold/filmcraft), [EffectCraft](https://github.com/storytold/effectcraft), [Vidonex](https://github.com/farshidrezaei/vidonex), [FFmpeg release tree](https://github.com/FFmpeg/FFmpeg/tree/n9.0.2), [Rust release](https://github.com/rust-lang/rust/releases/tag/1.99.1), [Spec Kit release](https://github.com/github/spec-kit/releases/tag/v1.2.2), [eframe renderers](https://docs.rs/eframe/0.36.2/eframe/enum.Renderer.html), [official Rust MCP release](https://github.com/modelcontextprotocol/rust-sdk/releases/tag/rmcp-v3.6.0), [MCP specification](https://modelcontextprotocol.io/specification/2026-07-28).


