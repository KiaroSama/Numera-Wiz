# MCP manual — current development scope

The bridge uses official rmcp 3.6.0, with the 2026-07-28 and 2025-11-25 protocol
profiles selected in code. Actual named-client/profile qualification remains open.

## Stdio

Launch the CI-built executable with argument `bridge`. A conventional client
configuration has the following shape where the client supports `mcpServers`:

```json
{"mcpServers":{"numera-wiz":{"command":"G:\\Program Files\\Numera Wiz\\numera-wiz.exe","args":["bridge"]}}}
```

Default authorization is discovery-only `agent`. Logging stays on stderr.
`tools/list` is generated from the active shared registry; typed tool input is
`{"params":{...}}`. `commands.list` accepts `{"params":{}}`.
An attempt to resolve media selection as this agent returns PERMISSION_DENIED.
It does not acquire local-user rights through its parent process.

## Direct HTTP

On the currently admitted non-Windows development path, `bridge --transport http`
or `serve` starts `/mcp` on exact loopback port 1578 unless another port is explicit.
`--token-file` creates a new protected token file and never prints the credential.
Existing token files are not overwritten. Configure a client's supported bearer
header facility from that file; a token-file path is not a universal MCP option.

Every request checks exact Host, optional exact Origin, current bearer expiry and
bounded request admission. This development endpoint is stateless for both selected
protocol profiles; supplied MCP session IDs are rejected. Do not advertise OAuth
or unrestricted remote deployment. Windows HTTP setup remains blocked on its ACL
adapter; operational profiles, pairing, persistent owner sessions and full media
access remain required implementation tasks.

## Shared semantics and evidence

All current calls dispatch through nw-service and nw-domain. Domain failures are
structured tool errors; malformed protocol requests follow official SDK errors.
The hosted SDK-client integration exercises a real stdio process and ensures
clean framing/discovery plus denied inherited authority. HTTP application boundary
fixtures exercise actual loopback requests. Complete HTTP protocol, browser,
media/job parity and packaged-client qualification are separate open gates.
