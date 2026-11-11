# Numera Wiz agent plugin

This repository-owned local plugin connects an agent to the existing Rust
`numera-wiz` MCP bridge. It includes a reusable media workflow skill, Agent
Plugins 1.0 metadata and native Claude Code/Cursor/Codex compatibility manifests.
It adds no executable wrapper and requires no Node.js, Python or shell runtime.
The admitted media components remain FFmpeg and ffprobe.

Install an identified CI-built Numera Wiz executable for your platform before
connecting. Make `numera-wiz` resolvable in the client's PATH, or replace `command`
in a private copy of the selected config with the absolute executable path
(`numera-wiz.exe` on Windows). Plugin installation does not install that binary
or a complete offline media payload. Package version `0.1.0` describes this
integration, not a complete media-product release.

The checked-in configuration explicitly selects `agent`, which is
discovery-only. It uses the executable's default data directory. To connect an
existing owner, configure the same absolute `--data-dir` and admitted
`--components-dir` before `bridge` in the argument array. The owner must create
and review an operating profile before replacing `agent` with that profile's
exact name. Never remove `--access-profile` from a media CLI fallback.

See the [installation, sources and validation guide](../../docs/agents/README.md)
in the full repository. When this directory is distributed alone, the skill's
[access guide](skills/numera-wiz/references/access.md),
[call guide](skills/numera-wiz/references/calls.md) and
[operation routes](skills/numera-wiz/references/operations.md) are included.

Quick local load from the repository root in Claude Code:

```text
claude plugin validate ./integrations/numera-wiz --strict
claude --plugin-dir ./integrations/numera-wiz
```

Then invoke `/numera-wiz:numera-wiz` and begin with command discovery. Codex can
register the local marketplace directory `integrations/`; Cursor supports a
local plugin import/copy. Follow the full guide for exact steps and the MCP-only
fallback. No client has been installed or remotely published by checking in
this package, and live client/media/package qualification remains pending.

If exporting a private archive, package only this directory, including hidden
manifests and `.mcp.json`; place the archive outside it. Exclude the adjacent
marketplaces/templates, build outputs and local credentials. Keep one plugin
root and inspect the final archive. The package contains no symlinks or hooks.
