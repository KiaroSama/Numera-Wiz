# Agent integrations

Numera Wiz owns the [local plugin package](../../integrations/numera-wiz/)
and its [workflow skill](../../integrations/numera-wiz/skills/numera-wiz/SKILL.md).
Agents operate the implemented Rust owner through the same command registry as
CLI/UI. FFmpeg/ffprobe own admitted media work. The package does not run FFmWiz
or MuxCls source programs, download runtimes, or replace the Rust MCP adapter.

## Package and prerequisite

| Location | Purpose |
| --- | --- |
| `integrations/numera-wiz/plugin.json` | Portable Agent Plugins 1.0 identity and OpenAI presentation. |
| `integrations/numera-wiz/mcp.json` | Portable stdio server; explicitly selects discovery-only `agent`. |
| `integrations/numera-wiz/.claude-plugin/plugin.json`, `.mcp.json` | Native Claude Code manifest and compatible MCP config. |
| `integrations/numera-wiz/.codex-plugin/plugin.json` | Codex compatibility metadata, synchronized with the portable identity/presentation. |
| `integrations/numera-wiz/.cursor-plugin/plugin.json` | Native Cursor component paths. |
| `integrations/numera-wiz/skills/numera-wiz/` | Self-contained skill, examples and workflows. |
| `integrations/.agents/plugins/marketplace.json` | Explicit local Codex catalogue; root is `integrations/`. |
| `integrations/.claude-plugin/marketplace.json` | Local Claude-compatible catalogue with the same package. |
| `integrations/templates/` | Standalone MCP configurations for clients without the plugin format. |

Use an identified GitHub Actions executable for Windows, Linux or macOS. Set
the executable's actual absolute path, or make `numera-wiz` available in the
client's launch PATH (`numera-wiz.exe` on Windows). GUI clients can have a
different PATH from a terminal. No package-manager runner such as `npx`, `uvx`,
Python or a shell launcher is part of this integration.

The configs intentionally omit host-specific plugin-root expansion: current
Cursor does not expand the standard's `PLUGIN_ROOT`/`PLUGIN_DATA` in `mcp.json`.
The command launches an external installed Rust executable directly and does
not assume the plugin cache contains a media binary. No binary is included here.

The app defaults to its executable directory for data. For an existing owner,
edit the selected private config before use to include its exact `--data-dir`;
use the same admitted `--components-dir` where required. Both options precede
`bridge`. For example, this argument vector is still discovery-only:

```json
["--data-dir","/absolute/numera-data","--components-dir","/absolute/admitted-components","--access-profile","agent","bridge","--transport","stdio"]
```

Do not commit machine paths or credentials to the distributed package. On
Windows, JSON backslashes need escaping, or use accepted slash-separated native
paths. The selected component directory needs both FFmpeg and ffprobe. Missing
components do not prevent discovery but block media work with a typed error.
This is not evidence that the required standard offline payload is delivered.

## Claude Code

From the repository root, validate and load for one session:

```text
claude plugin validate ./integrations/numera-wiz --strict
claude --plugin-dir ./integrations/numera-wiz
```

Invoke `/numera-wiz:numera-wiz`. Check `/mcp` for the actual connected server
and run `commands.list` through it. The native manifest lives inside the plugin,
while `skills/` and `.mcp.json` are at its root. Claude's validator is the
authoritative native manifest check; current docs specify v2.1.281 or later for
its MCP-entry checks. Older clients may need the MCP-only fallback.

For a later requested persistent local install, `integrations/` is also a
Claude-compatible marketplace root. This change does not register it or modify
user settings. Loading the whole repository with `--plugin-dir .` is not the
same as loading this package; pass the exact path above.

## Codex

The portable package has the required root schema declaration and fixed
`skills/`/`mcp.json` locations. `.codex-plugin/plugin.json` is a compatibility
overlay; inline `extensions.com.openai` takes precedence as a complete OpenAI
settings object. Root identity and portable components stay canonical.

For a requested local install on a client with these commands:

```text
codex plugin marketplace add /ABSOLUTE/Numera-Wiz/integrations
codex plugin add numera-wiz@numera-wiz-local
```

Check `codex plugin --help` and `codex plugin marketplace add --help` for the
installed version first. These exact command shapes were confirmed from the
available CLI's help; they were not executed to install a plugin. Start a new
session, inspect `/plugins` or the desktop plugin browser, enable the package
and verify its skill plus MCP connection. Source paths resolve relative to
`integrations/`, not its `.agents/plugins/` subdirectory. Register that root,
not the repository root, which has no integration marketplace at that path.

Current OpenAI documentation separates plugin availability by host surface;
the IDE extension does not provide the plugin browser. Use the MCP template
and supplied skill instructions there when native plugin installation is not
available. A local process package is not made accessible to web/mobile by
saving or syncing its metadata.

## Cursor

Current official docs support root Agent Plugins 1.0 and native Cursor manifests.
This package includes both layouts with equivalent skills/MCP behavior. Import
the package directory from disk through Customize when that action is available,
or use the documented local development location:

1. Copy the whole `integrations/numera-wiz/` directory, including hidden files,
   to `~/.cursor/plugins/local/numera-wiz` only when installation is requested.
2. Restart Cursor or run **Developer: Reload Window**.
3. In **Customize**, verify the `numera-wiz` skill and MCP server, then discover
   the actual tools. A same-named marketplace install can take precedence.

Local plugin imports must be allowed by the host/admin. Current docs state
that Enterprise disables local imports by default; respect that policy. A
symlink pointing outside `~/.cursor/plugins/local` is skipped, so use a real
copy for that flow. No Cursor files were installed by this repository change.

## Standalone MCP and other agents

Use the client's documented stdio configuration, launching the same Rust
executable directly with this argument array:

```json
["--access-profile","agent","bridge","--transport","stdio"]
```

| Client | Checked-in configuration / supported destination |
| --- | --- |
| Claude Code without plugin | Merge `integrations/templates/claude-mcp.json` into a requested project `.mcp.json`, preserving existing servers. |
| Codex without plugin | Merge `integrations/templates/codex-mcp.toml` into the client's selected `config.toml`; it uses `[mcp_servers.numera-wiz]`, not `mcpServers`. |
| Cursor without plugin | Merge `integrations/templates/cursor-mcp.json` into a requested project `.cursor/mcp.json` or selected user config. |
| Other MCP clients | `integrations/templates/generic-mcp.json` illustrates conventional `mcpServers`; adapt the enclosing config to that client's documented schema. |

Do not install both a plugin-provided server and its standalone equivalent under
different names in one client. These templates are alternatives; configuration
formats are not universal. A generic client can use MCP tools even if it has
no skill/plugin installer. Supply the included `SKILL.md` as instructions using
that client's supported method; do not claim automatic skill discovery there.
If a client cannot launch local stdio processes, this package cannot provide
local media access on that surface.

Direct Streamable HTTP is separately implemented at the exact local `/mcp`
origin and requires a protected, scoped exported bearer header. See the
[MCP manual](../manuals/MCP.md). There is no shared bearer in these templates,
OAuth claim, legacy SSE route, unauthenticated HTTP fallback or remote host.
Read the client's supported header/secret facility before configuring it.

## Authority and workflows

The default `agent` remains discovery-only. Tool enumeration comes from the
shared active registry; it does not confer any execution right. An owner must
create an operating profile, inspect its complete grants and explicitly select
it in the local client. See the packaged
[access workflow](../../integrations/numera-wiz/skills/numera-wiz/references/access.md).
Command rights, plan/job actions, creator ownership and independent read/write
roots all remain enforced. New command registration never expands an existing
profile's frozen grants. An agent must not remove `--access-profile`, copy
private owner credentials, modify protected state or substitute a new principal.

The skill covers actual probe, registered source/selection, immutable plan
review, exact-hash submit, observation, cancel and verified artifact delivery.
It includes bounded explicit batches and owner-only administration, without
granting owner-only operations to scoped agents. `artifacts save` is a Rust
native-client operation, while MCP transfers use manifest/chunk tools. There is
no invented MCP `save`, `watch` or generic `run` tool. Commands without a CLI
shorthand use canonical `request --file`; live discovery is authoritative.

Bounded folder intake uses durable `assets.register_folder` and immutable
`assets.folder_inspect`, with per-file probe failures and inventory-only or
excluded companions. Its 32-entry manifest limit and partial-snapshot facts
remain explicit. CLI `assets register-folder` / `assets folder-inspect` expose
that same contract. CLI `jobs wait ID --timeout-seconds 1800` adds a finite
1-86400 second observer over `jobs.inspect`; timeout/Ctrl+C stops observation
without cancelling accepted work. These operations do not promise the complete
donor folder/companion workflow.

Durable calls retain command, params, scope, key and request ID across uncertain
responses. Observe accepted work through jobs/batches; cancelling the MCP call
or observation is not job cancellation. A completed FFmpeg progress stream
does not prove verified publication. Use actual terminal job/artifact facts.

## Validation and remaining qualification

Static checks establish metadata/config syntax and documented routes. They do
not establish native client execution or media/package support. Ordinary source
builds and media/adapter tests remain hosted GitHub Actions work under project
policy. No local Rust build, media job, client installation or remote publication
is part of preparing these files.

Validate the root portable manifest and MCP config against the published
[plugin schema](https://agent-plugins.org/schemas/1.0.0/plugin.schema.json) and
[MCP schema](https://agent-plugins.org/schemas/1.0.0/mcp.schema.json). Parse all
JSON/TOML/YAML files as data; verify matching names/versions, portable/native
presentation, skill frontmatter, contained references and identical default
server arguments. `claude plugin validate ... --strict` additionally checks the
native Claude layout on a suitable installed client. There is no assumed
universal validator for all native formats.

For each actual target client/version and OS, the qualification run must:

1. Load/install the exact package and identified CI-built executable; record
   versions and package/binary hashes. Verify skill discovery and MCP startup
   with protocol stdout clean and diagnostics on stderr.
2. Call `commands.list` and `commands.schema`. Under default `agent`, confirm
   media work is denied and no broader principal can be selected in tool params.
3. Under an owner-provisioned narrow profile, perform a harmless authorized
   probe, plan/review, submit and observe a small admitted fixture, retaining
   exact durable retry identity. Check a lost-response retry returns the same job.
4. Inspect a verified artifact and save/chunk it through the supported client
   facility; verify digest, absent-output behavior and revoked-read denial.
   Exercise accepted-job cancellation separately from stopping observation.
5. Exercise any advertised batch/folder actions at their actual bounded contract,
   malformed envelopes, denied paths, stale plans and resource limits. Record
   transport/client/package evidence rather than inferring parity from schemas.

Native Claude/Codex/Cursor client installation/startup and packaged media
journeys have not been executed for this integration. Full FFmWiz F01-F15,
standalone/embedded MuxCls, editor/preview, standard offline packages and hardware
qualification remain separate completion requirements. A metadata package does
not close those gates.

## Official format evidence

Sources retrieved live on 2026-11-11; docs can evolve. These support the config
and install shapes above, not a claim that every client/version was run:

- [Agent Plugins manifest schema](https://agent-plugins.org/schemas/1.0.0/plugin.schema.json)
  and [MCP schema](https://agent-plugins.org/schemas/1.0.0/mcp.schema.json): root
  schema declarations, portable metadata and typed stdio server fields.
- [OpenAI package guide](https://developers.openai.com/plugins/build/plugins):
  fixed portable locations, OpenAI extension precedence, local marketplace root
  resolution and optional Codex compatibility metadata.
- [OpenAI marketplace management](https://developers.openai.com/codex/enterprise/plugin-management):
  native, Claude-compatible and Agent Plugins discovery; local source paths.
- [OpenAI plugin surfaces](https://learn.chatgpt.com/docs/plugins): Codex CLI
  plugin browser, new-session skill loading and IDE-extension availability limit.
- [OpenAI MCP configuration](https://developers.openai.com/codex/mcp): native
  TOML stdio launch fields and plugin-provided MCP distinction.
- [Claude plugin manifest reference](https://code.claude.com/docs/en/plugins-reference)
  and [creation/loading guide](https://code.claude.com/docs/en/plugins/create):
  native layout, validator, one-session `--plugin-dir` and root skill/MCP paths.
- [Cursor plugins](https://cursor.com/docs/plugins),
  [plugin reference](https://cursor.com/docs/reference/plugins) and
  [MCP reference](https://cursor.com/docs/mcp): both plugin formats, local copy
  path/reload, native component paths, plugin variable limits, project MCP config.

Repository contract evidence: `apps/numera-wiz/src/main.rs`, `human.rs`,
`access.rs`, `artifacts_cli.rs`, `mcp.rs`, the shared
`crates/nw-domain/src/commands.rs` registry and access/grant types,
`docs/manuals/CLI.md` and `docs/manuals/MCP.md`. Runtime capability remains what
the identified binary's current registry and actual execution evidence admit.
