# Numera Wiz repository guidance

Read local `.ai/memory.md` first when present, then this file, the public
implementation mandate and the current development continuation/completion ledgers.
Spec Kit and feature specifications are local-only; do not add them to Git.
Current chat instructions take precedence over historical mandate/session policies.

- Pure Rust owns authored application, UI, service, CLI, MCP and automation logic.
  FFmpeg/ffprobe owns media processing. Keep upstream program sources outside Git.
- Use the existing shared domain/service/registry paths across native/browser UI,
  CLI and MCP. Preserve accepted legacy plan hashes, receipts and raw store data.
- Keep protocol stdout clean; entry points log timestamped diagnostics in English
  without credentials. Text files use explicit UTF-8.
- Ordinary builds, formatting and tests run in hosted GitHub Actions. Preserve the
  original test entry-point count/categories in `ci/test-inventory.json`.
- Validate the exact latest source HEAD and all required jobs; distinguish model,
  media, transport, GUI, development-package and owner-hardware evidence.
- Keep required unfinished work visible; never label a development slice or
  fixture-only FFmpeg profile the complete version 1 product.
- All current work belongs in one separate review PR. Do not merge it, publish a
  release or enable auto-merge. The owner authorized removal of local-only files
  from history with a backup and leased force update; unrelated history/refs
  must not be overwritten.
- Use parallel agents only when the user or an applied skill explicitly requests
  delegation. Coordinate exclusive source ownership and review integrated changes.
