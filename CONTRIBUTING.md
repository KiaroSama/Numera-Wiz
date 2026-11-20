# Contributing

Keep authored executable logic in Rust and media operations in FFmpeg/ffprobe.
Read AGENTS.md, the implementation mandate and the current development ledger.
Spec Kit, feature specifications and private memory belong only in the ignored
local workspace. Do not commit donor program sources or compressed copies.

Use one review branch and PR; do not merge, enable auto-merge or publish a release.
Build, format and run ordinary tests in GitHub Actions. Every source head builds
for Linux, Windows and macOS with its matching shared WASM assets. Retain the
original test entry points and categories; strengthen their assertions when
extending behavior. Do not edit a baseline or golden result to conceal failure.

The `xtask verify-research` gate verifies retained research hashes, tracked source
languages and `ci/test-inventory.json`. New first-party entry points must use
structured timestamped logging without secrets. Document actual exposed options,
limits, fixture evidence and pending qualifications. Preserve third-party notices
when translating behavior. Signing, release promotion and owner GPU evidence are
separate gates; development artifacts do not satisfy them.
