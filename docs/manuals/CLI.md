# Command-line manual — current development scope

The actual executable is `numera-wiz`; diagnostics use timestamped INFO, WARNING,
ERROR and DEBUG entries on stderr. JSON and MCP stdout remain machine-readable.
The complete media application and portable release remain under implementation.

## Commands

| Invocation | Current behavior |
|---|---|
| `numera-wiz commands` | Return the active development registry and schemas. |
| `numera-wiz request --file request.json` | Run one bounded canonical pure query as the local user. |
| `numera-wiz request` | Read the same query from stdin, at most 256 KiB. |
| `numera-wiz bridge` | Official stdio MCP with the discovery-only agent principal. |
| `numera-wiz bridge --transport http --token-file <new-file>` | Discovery-only loopback HTTP MCP, default port 1578. |
| `numera-wiz serve --port 1578 --token-file <new-file>` | Same development application API and `/mcp`; exact port conflict is an error. |
| `numera-wiz doctor` | Report current implementation limits without initializing media GPU or GUI. |
| `numera-wiz` in a desktop artifact | Open the shared Rust development query workspace. |

Windows direct HTTP token-file creation deliberately remains blocked until the
user-restricted ACL adapter is qualified. Stdio discovery and local pure queries
remain available. No operating access profile, file-processing endpoint or job
submission is advertised as complete.

## Active canonical queries

`commands.list`, `commands.schema` and `streams.clean.resolve` have real handlers.
Other typed future records are not offered as implemented tools. The local-user
resolver accepts an explicit observed stream inventory and cleanup intent; it
performs no filesystem access or media processing. The default MCP agent may
only discover contracts; it cannot inherit local-user authority.

The executable fixture `apps/numera-wiz/tests/fixtures/clean-resolve.json` is a
complete resolver request: keep audio index 0, remove subtitles, retain the
independent attachment and return source indices `[0,1,3]`. The action is `remux`
as a pure decision, not a claim that a file was actually remuxed by that query.

## Errors and limits

Unknown versions/context fields, duplicate JSON members on the application
route, non-object params, invalid selectors and excessive stream/tag collections
are rejected. Query errors return `ok=false` and a stable domain code. CLI exits
nonzero for a failed command; inspecting a future failed job will remain a query.
HTTP request/result budgets are 256 KiB, with exact Host/Origin and bearer checks.
Full transport, lifecycle and five-surface qualification remain open.

## Windows invocation

Use PowerShell 7 with the selected CI-built development executable. The preferred
future installation area is `G:\Program Files\Numera Wiz`:

```powershell
& 'G:\Program Files\Numera Wiz\numera-wiz.exe' commands
```

Do not run ordinary builds/tests locally. The owner's eventual GPU harness is a
separate matching CI artifact, not a source-build prerequisite.
