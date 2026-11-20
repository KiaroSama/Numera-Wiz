# Numera Wiz revision 4 — focused requirement-fidelity review

## Revision 5 resolution status

All three findings in this historical review are resolved in the revision 5 mandate: the first functional slice has a single parity completion gate, Auto has an explicit qualified-GPU preference, and browser support has a concrete qualification matrix. See `research_v5/review-resolution-ledger.md`. The `research_v4` chapter references below identify the reviewed baseline and are not current companion paths or unresolved instructions. Application/runtime evidence remains required in CI and the prescribed owner GPU run.

**Date:** 2026-10-21. **Scope:** the delivered English mandate, its root chapter inputs,
the user's explicit 22 requirements and later browser/local-engine/GPU clarifications.
This is a document and source review. No application build, media command, browser session,
CI job or GPU test was executed.

## Result

The main product choices are now substantially aligned with the request. This review found
one concrete sequencing contradiction and two actionable specification gaps. They can be
closed without another architecture rewrite or a larger effect catalogue. Do not claim
that the findings below are observed application bugs: there is no Numera binary in this
handoff.

## RQ-01 — The first implementation sequence postpones parity

**Severity: high; confirmed contradiction in completion criteria.**

The fixed R19 requirement says native GUI, browser GUI, CLI and MCP remain functionally equal
throughout development, and explicitly forbids a postponed agent adapter. Section 2 likewise
requires each completed UI task to have headless equivalents. Nevertheless, Section 9 closes
E03 on a CLI-only probe/export result, E05 on native/browser preview, and only E06 adds both
MCP transports. The E03 and E05 definitions of done can therefore be satisfied while the
same capabilities remain unavailable to other required surfaces.

Evidence:

- `research_v4/mandate-front.md`, R19, and delivered mandate R19.
- `research_v4/mandate-sequence.md`, E03, E05 and E06 (lines 9, 11 and 12).
- `research_v4/scope-migration-policy.md`, section 4 final paragraph: each UI task requires
  native/browser usability and CLI/MCP reachability before completion.

**Minimal correction:** keep the internal dependency order, but make E03–E06 substeps of
one parity-gated first functional slice. They may produce internal CI artifacts, but no
probe/export/preview capability is declared complete or shipped as a completed milestone
until the same implemented operation is exercised through native GUI, browser GUI, CLI,
stdio `bridge` and direct HTTP MCP. Build the thin CLI/MCP adapters when establishing the
command service rather than treating them as a later feature batch. This does not require
building every editor panel before the first service test.

Suggested binding text:

> E03–E06 are dependent engineering substeps of one first functional slice, not separate
> capability-completion milestones. The slice closes only after its implemented operations
> have the required GUI/browser/CLI/stdio-MCP/HTTP-MCP coverage. Internal backend artifacts
> may precede adapters, but they are not advertised as completed user capabilities. Every
> later feature batch keeps the same parity gate.

## RQ-02 — GPU preference is not an effective configuration default

**Severity: medium; confirmed omission relative to the current user preference.**

The media contract names `CpuOnly`, `Auto` and `GpuRequired`, but it never chooses the default
or explains how Auto incorporates the user's explicit GPU priority. The configuration chapter
defines precedence down to a documented default without supplying this particular default.
An implementer could choose CPU by default and still satisfy every sentence currently written.

Evidence:

- Current user preference: GPU use takes priority over CPU use.
- `research_v4/mandate-media.md`, “GPU policy,” line 115.
- `research_v4/mandate-domain.md`, “Dependency and configuration selection,” configuration
  precedence paragraph.
- The existing objective/resource and fallback rules are sound and should be retained.

**Minimal correction:** select `Auto` as the application default, with a GPU-preferred policy
for actually qualified device/stage routes. Preserve the requested output semantics, resource
budget and explicit quality objective. A CPU-only filter in a mixed plan remains valid.
Document a reason when Auto selects a CPU path, such as unsupported functionality, insufficient
memory, unavailable device or a qualified path that is worse for the selected objective.
`CpuOnly` must suppress media-device initialization; `GpuRequired` must fail for its required
stage rather than silently falling back. Persist the user's choice and apply the same effective
configuration across all surfaces. Do not hardcode NVIDIA-only behavior into cross-platform Auto.

This is a configuration/selection clarification, not a demand to copy every filter to CUDA or
to describe all GPU encoders as numerically equivalent to software encoders.

## RQ-03 — Browser support is a release gate with no declared membership

**Severity: medium; confirmed handoff gap, not proof of browser incompatibility.**

The mandate carefully fixes native OS/architecture baselines, then requires successful
“current supported-browser release journeys” without declaring which browser/platform
combinations those are or requiring that matrix to be frozen before selecting the preview
route. A future agent can defer the list or narrow it after an MSE/graphics failure, making
the browser completion gate self-selecting. “Subject to actual browser capability” is an
accurate runtime condition, but it does not define the product's promised support.

Evidence:

- `research_v4/mandate-delivery.md`, “Portable artifacts and platform scope,” line 80:
  explicit native OS/architecture baselines.
- `research_v4/mandate-media.md`, “Native and browser preview,” lines 107–109:
  H.264/AAC candidate, capability qualification and supported-browser release journeys.
- `research_v4/mandate-delivery.md`, Browser preview suite and `docs/browser.md` requirement:
  tests/documentation are required but the initial browser membership is unspecified.

**Minimal correction:** add a small supported-browser matrix before the preview implementation
gate. The implementation agent should select and record at least one named desktop browser
family on each supported OS, its exact CI/release-tested version and required WebGL/MSE/audio
profile. Distinguish additional tested browser families from best-effort combinations. A listed
supported combination must perform playback with audio, editing, seek, export submission and
reconnect; a still-only fallback cannot satisfy that row. If a selected combination fails,
fix the adapter/profile or record a concrete scope change instead of silently deleting the row.
The matrix is an ordinary implementation decision; no user question is required unless the
user has a particular additional browser requirement.

This correction need not promise every browser version or introduce another frontend.

## Matters that should not become invented defects or new obligations

| Topic | Assessment |
|---|---|
| First-party Rust versus FFmpeg/native dependencies | The distinction is explicit and consistent with retaining FFmpeg. Requiring FFmpeg itself to be rewritten in Rust would change the request. |
| One portable EXE with verified embedded-helper extraction | The mandate describes a single distributed Windows executable and offline core operation. It does not pretend that helper processes execute from arbitrary embedded PE bytes in memory. No additional installer should be introduced. |
| Browser uses a local native engine | Shared `nw-ui`, common project/job identity and local FFmpeg execution satisfy the clarified intent. Browser decoding for presentation does not turn it into an ffmpeg.wasm application. |
| Loopback direct HTTP | A reasonable selected default for the expressly local-engine product. The user did not clearly request Internet/LAN hosting; do not add TLS/OAuth deployment merely to fill a checklist. If remote access is later requested, handle that separately. |
| GPU testing on the owner's machine | The matching prebuilt application/harness route is correct. The small real-Windows launch/baseline check within that qualification bundle should remain bounded; it is not permission to move ordinary test suites or builds locally. |
| GPU coverage for every vendor | The owner's RTX 4070 Ti evidence cannot qualify AMD, Intel or Apple hardware. The existing truthful qualification boundary should remain. Do not fabricate cross-vendor results or block all CPU packages on unrelated hardware. |
| Unsupported donor effects | Meaningful native/composed functionality remains required; exact private donor mathematics is not a general requirement. Precise unsupported primary semantics and separately named alternatives are the correct reporting pattern. Media-route accuracy needs the separate technical audit. |
| Full initial release versus first slice | The mandate already says the first slice cannot substitute for the full initial scope. RQ-01 tightens when that slice's capabilities may be marked complete; it does not shrink the product. |
| Spec Kit bootstrap | The official v1.2.2 release does contain the pinned `uv tool install` command used by the mandate. No defect was established there. Actual integration command spelling must still come from the installed integration. |
| Deep research invocation | The literal host-action requirement and honest availability fallback are explicit. Do not invent evidence that `::deep-research` ran. Preserve actual invocation/result provenance. |

The Spec Kit check used the official [v1.2.2 release](https://github.com/github/spec-kit/releases/tag/v1.2.2),
retrieved on the review date. The request to fetch its raw README failed in the browsing tool,
so this review makes no claim to have read that raw file.

## Bounded integration instructions

1. Apply RQ-01 to the sequence and capability completion wording.
2. Apply RQ-02 to effective configuration, GPU selection and their existing CI cases.
3. Apply RQ-03 to the platform/browser declaration and E05 admission gate.
4. Reconcile these edits in the single authoritative mandate; do not append a competing policy.
5. Keep the existing no-runtime-proof boundary and avoid another round of inventory inflation.

These fixes need no application execution in this review. Runtime success remains an implementation
and CI/owner-hardware obligation.
