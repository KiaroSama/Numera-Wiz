<p align="center">
  <img src="assets/branding/numera-wiz.png" width="240" height="240" alt="Numera Wiz official logo">
</p>

# Numera Wiz

[![Rust CI](https://github.com/KiaroSama/Numera-Wiz/actions/workflows/ci.yml/badge.svg?branch=rewrite%2Frust-completion)](https://github.com/KiaroSama/Numera-Wiz/actions/workflows/ci.yml?query=branch%3Arewrite%2Frust-completion)
[![Rust](https://img.shields.io/badge/Rust-1.99.1-DEA584?logo=rust)](rust-toolchain.toml)
[![Platforms](https://img.shields.io/badge/builds-Windows%20%7C%20Linux%20%7C%20macOS-blue)](.github/workflows/ci.yml)
[![MCP](https://img.shields.io/badge/MCP-stdio%20%2B%20HTTP-blue)](docs/agents/README.md)
[![License](https://img.shields.io/badge/license-GPL--3.0--or--later-blue)](LICENSE)

A **Pure Rust** media application with a local **FFmpeg/ffprobe** engine, shared
native/browser workspace, CLI and official MCP transports. FFmWiz and MuxCls
supply behavioral references for media utilities and the CLI; their Python or
PowerShell runtimes are not part of this repository or application.

**Version 1 is in development.** The review branch contains working cleanup,
registration, planning, job and artifact flows. The complete editor, utility
families and offline portable release described by the
[implementation mandate](docs/research-pack/Numera-Wiz-Implementation-Mandate-EN.md)
remain unfinished. A green development CI is evidence for its exercised scope.
Current work and required gaps are recorded in the
[continuation record](docs/development/continuation.md) and
[completion ledger](docs/development/completion.json).

## Implemented workflows

- Native and streamed browser media intake; exact original-index stream selection
  and metadata/tag/order resolution, using the shared MuxCls rules.
- Authorized folder intake with deterministic relative paths, retained probe
  failures, explicit bounds/exclusions and companion inventories. Inventory does
  not copy companions or qualify the whole MuxCls utility.
- Immutable command review, exactly-once durable acceptance, bounded FFmpeg/copy
  execution, cancellation, recovery and verified no-replace output publication.
- CLI, shared Rust native/WASM controls, stdio and loopback HTTP MCP, scoped
  operator profiles, bounded artifact transfer and verified client Save As.
- [Project state editing](docs/manuals/PROJECTS.md) through shared CLI/MCP commands:
  authorized project catalog discovery, exact sequence grids, place/move/split/lift,
  atomic history/receipts and bounded revision snapshots. Human project controls
  and decoded preview remain unfinished.
- Rust repository automation; no first-party Python, shell, C, Go or JavaScript
  source. Browser glue is generated during the hosted WASM build.

## Human setup and builds

[GitHub Actions](https://github.com/KiaroSama/Numera-Wiz/actions/workflows/ci.yml)
checks out the exact source HEAD and builds Windows x64, Linux x64, macOS ARM64
and macOS Intel artifacts with matching embedded browser assets. Native builds
run before ordinary regression/lint checks; a failed prerequisite build remains
a failure. Inspect all checks on the latest commit before using an artifact.

Development packages include the executable, manuals, source/dependency records
and SHA-256 manifests. They are unsigned review artifacts; the standard bundled
FFmpeg profile and full offline portable qualification are still required. They
are not a finished release download. Ordinary compilation and media tests run
in hosted CI. Owner hardware qualification uses matching prebuilt artifacts.

See the [GUI guide](docs/manuals/GUI.md), [CLI manual](docs/manuals/CLI.md),
[MCP manual](docs/manuals/MCP.md) and [agent setup](docs/agents/README.md).
The CLI exposes `commands list`, `commands schema <id>`, `media probe`,
`assets register-folder`, `streams clean`, `jobs submit`, `jobs wait`,
`jobs cancel`, batch controls and `artifacts save`. Parameters, access grants
and durable retry identities are documented in those manuals.

## Skills, plugins and MCP

The [Numera Wiz plugin](integrations/numera-wiz/README.md) contains the project's
own skill, native Claude Code/Codex/Cursor packaging and portable MCP client
configurations. It calls the same Rust `numera-wiz bridge` executable. No Node
or Python service is introduced. Native client loading/qualification is reported
separately from manifest validation and the official-SDK hosted tests.

MCP uses stdio or direct loopback HTTP on configurable port `1578`. Discovery
comes from actual shared command descriptors. The default `agent` profile can
discover capabilities; the owner must explicitly grant operation, object and
filesystem rights before an agent changes media. The skill explains how to probe,
review, submit, observe, cancel/retry and read authorized results.

## Source policy and contributing

Spec Kit, feature specifications and private `.ai/` memory are local-only and
ignored. Third-party executable source snapshots are excluded, including from
compressed repository archives. Pinned source URLs and original hashes remain
under [research provenance](docs/research/README.md); the original supplied
research companion is kept outside this repository. Offline FFmpeg manuals and
licenses remain in `docs/vendor/ffmpeg/n9.0.2`.

The [upstream review](docs/research/upstream-review-2026-11-11.md) identifies
actual source changes and compatibility gates. `xtask verify-research` checks
retained document hashes, the tracked Pure Rust boundary and the original Rust
test inventory. [Contributing](CONTRIBUTING.md) describes review and validation.

## License

First-party code: **GPL-3.0-or-later**. See [LICENSE](LICENSE) and
[third-party notices](THIRD-PARTY-NOTICES.md). The official owner-supplied logo
is preserved byte for byte; its hash is recorded in [branding](assets/branding/README.md).
