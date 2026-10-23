# Numera Wiz

Rust media editing and utilities powered by a local FFmpeg engine, with a shared
native/browser UI, CLI and official MCP transports.

**Development branch; complete application release is not yet qualified.**
All implementation is on `implementation/numera-wiz-v1` for the owner's review.
The main branch intentionally retains only the repository bootstrap.

Read the [authoritative revision 7 mandate](docs/research-pack/Numera-Wiz-Implementation-Mandate-EN.md),
[execution specification](specs/001-numera-wiz/spec.md),
[task ledger](specs/001-numera-wiz/tasks.md) and
[continuation record](docs/development/continuation.md).

The full research pack is preserved with original hashes. Offline FFmpeg manual
sources and licenses are under `docs/vendor/ffmpeg/n9.0.2`.

Current code includes exact rational time, strict command DTOs, the shared MuxCls
resolver, a protected single-owner service, durable asset/plan/job acceptance and
receipt replay, explicit access profiles, bounded process/file/media execution,
publication journals and real CLI/official MCP adapters. The shared native/WASM
workspace has working media intake, cleanup forms, plan review and job controls.
CI builds hash-addressed same-source browser assets and checks actual Chrome boot. [Current manuals](docs/manuals/CLI.md)
describe the actual admitted scope. Hosted
Windows/Linux/macOS CI builds/tests the actual workspace and verifies source
provenance. This does not claim completion of the full MuxCls rewrite, F01-F15,
editing, five-surface parity, offline portable packaging or GPU qualification.

## Human setup

There is no complete portable release download yet. Do not use unqualified
source snapshots as a finished media application. Build/test evidence is available
in the branch's GitHub Actions runs. The intended Windows installation area is
`G:\Program Files\Numera Wiz`; another selected drive/path is supported by design.

## Agent setup

Read `AGENTS.md` and `.specify/memory/constitution.md`. Official Spec Kit 1.2.2
Codex skills are included. Continue the selected tasks rather than restarting a
whole research pass. Installed command spelling is `$speckit-constitution`,
`$speckit-specify`, `$speckit-clarify`, `$speckit-plan`, `$speckit-checklist`,
`$speckit-tasks`, `$speckit-analyze`, `$speckit-implement`, `$speckit-converge`.

The final application will expose `numera-wiz bridge` for stdio MCP and direct
HTTP at configurable loopback port `1578`. These interfaces are not advertised
as qualified until their actual typed operation and authentication tests pass.
No source build or ordinary media test is required on the owner's machine.
Actual GPU qualification uses the matching CI-built harness on the RTX 4070 Ti.

## License

GPL-3.0-or-later for first-party code. See [LICENSE](LICENSE) and
[THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md) for retained upstream material.
