# Research decisions

## Shared UI
Decision: use the pinned eframe 0.36.2 Glow family and one Rust UI crate.
Rationale: source-compatible with the chosen FilmCraft layout; avoids a second frontend.
Current official API uses `App::ui`, optional `App::logic`, `egui::Panel` and an
actual HtmlCanvasElement for WebRunner. Old update/SidePanel examples are invalid.
Sources: https://github.com/emilk/egui/tree/0.36.2/crates/eframe and
https://github.com/emilk/egui/blob/0.36.2/crates/egui/src/containers/panel.rs .
Alternatives: separate JS frontend and donor wgpu/DSP are excluded by the mandate.

## MCP
Decision: official rmcp 3.6.0 at 74d789515fdee63ef46a1635b801364aad6695e4.
Rationale: shared typed registry with dynamic Tool schemas and official transports.
Manual ServerHandler::call_tool returns CallToolResponse. Default HTTP Origin
validation is disabled; exact hosts/origins and enforcement are necessary. HTTP
request principal comes from authenticated request extensions, never the factory's
owner context. Legacy sessions additionally require principal/epoch binding.
Sources: https://github.com/modelcontextprotocol/rust-sdk/tree/rmcp-v3.6.0 .
Alternatives: handwritten JSON-RPC/MCP framing is rejected.

## MuxCls
Decision: shared pure Rust selector/resolver, complete explicit stream outcomes,
independent attachments/covers/data, keep/set/clear tags and original-probe matching.
Rationale: fixes source index-zero ambiguity, attachment coupling and duplicated previews.
Sources: archived standalone muxlogic.py/models.py plus Annex D. Source inspection
is not runtime verification; full module/batch/portable rewrite remains required.

## Workflow and research availability
Official Spec Kit 1.2.2 was installed from the pinned release, commit
74be0a63e93c7087c5b06556b7561810d8f78a45. `specify init` ran with Codex skills.
Bash helpers are used in this Linux work environment; PowerShell helpers are also
preserved for the owner's workflow. Installed commands are `$speckit-*` skills,
not the historical `/speckit.*` syntax. No literal host `::deep-research` action
was available. No new billed generic research run was started; targeted official
source checks extend the supplied completed research without replacing coding.

## Evidence limits
The input pack's reading ledger belongs to its original researchers. This session
has read selected mandate sections and affected MuxCls/MCP/UI sources. The remaining
full-corpus reading and every media/package/GPU qualification stay explicitly open.

## Process ownership
Decision: process-wrap 10.0.1 supplies Unix process groups and Windows Job Objects
through safe Tokio wrappers. Independent I/O drainers retain fixed byte budgets;
cancel/deadline paths kill the owned containment and await bounded reaping.
Rationale: avoid handwritten platform FFI and direct-child-only cancellation.
Source: https://github.com/watchexec/process-wrap/tree/v10.0.1 .
Limits: owner-crash windows, escaped descendants and independent whole-tree proof
remain real qualification gates. processkit was inspected as an alternative; its
process-group fallback does not provide full membership proof either. No library
name is treated as evidence of unconditional no-orphan behavior.

## Real fixture profile correction
The CI-built n9.0.2 source profile initially omitted wrapped_avframe decoding,
which lavfi video fixture generation requires. The actual media job failed before
producing source media. Enabling that decoder fixed the concrete failure; run
39874893026 subsequently passed real full-byte-copy and remux order/tag assertions.
This limited CPU fixture profile is not the complete standard runtime profile.
