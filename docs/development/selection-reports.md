# Source inventory and selection reports

streams.clean.report is a canonical authorized query over 1-64 explicitly named
registered assets. It loads, checks current asset ownership and read roots, and
releases one asset at a time. A mixed request containing an invisible or missing
asset returns the same non-disclosing failure with no partial inventory. A changed
access-profile revision invalidates a report built under the earlier policy.

The pure domain builder uses the existing cleanup resolver. It reports every
candidate as a resolved copy/remux preview, explicit selection skip or rejection;
no rejected candidate disappears from totals. Selected indices always refer to
the original registered stream snapshot. A skipped/rejected file contributes
no retained streams. Audio/subtitle match counts remain separate from whether
the whole file is skipped. Resolver warnings and errors are explicit.

Unique summaries group original kind, normalized language, title and codec
case-insensitively while retaining the first display spelling. A second summary
also keys by the original stream index. Source and retained counts are separate.
Missing and donor unknown-language spellings share the existing und normalization.
These are original inventory labels, not silently patched output metadata.

This query does not probe files again, admit an output container, create a plan,
verify bytes or produce artifacts. Its report identifies that scope. Registered
metadata may be stale; execution still requires a newly reviewed plan and the
full existing source/component/media/publication checks.

Backpressure admits at most 512 combined summary rows, 64 KiB retained source/
key/warning text and a 224 KiB final report. An excessive explicit asset set fails
with RESOURCE_LIMIT and asks for a smaller set; it never silently truncates
metadata or returns a misleading complete count. Serialization and transport
retain the common result envelope bounds.

Both human adapters use the same Selection report control and present unique
or original-index summaries in pages of 32 rows, with per-file action/match/
warning/error details. Escape closes the report. The report is generation-bound
to the selected asset and current form; editing the form invalidates it. The
canonical query supports multiple explicit assets while the present human
cleanup form still chooses one asset.

New domain fixtures cover original-index grouping, language normalization,
case-insensitive labels, skips/rejections and budget refusal. A service fixture
covers current command/asset/path authority and mixed foreign/absent response
equality. Actual CLI and official SDK stdio/both HTTP lifecycle journeys assert
the media fixture's real registered selection report. Native input additionally
observes its actual canonical completion before an independent report query.
Chrome's real Selection report click is checked through its actual network
response, without calling a domain handler in place of the UI. Only the selected
nonsecret report response and screenshot are retained; raw network headers,
cookies and unrelated request bodies are never written to artifacts.

Run 40078904727 passed all four ordinary native regression suites and the full
actual media/CLI/official SDK/large HTTP job. Its Chrome pointer/network-response
report journey passed, and its actual PNG was personally inspected: all three
source/retained inventory rows are visible. The native report journey was
skipped because Clippy rejected UI items placed after a test module; that order
and the exact hosted formatting changes are corrected in the following revision.
See evidence/selection-reports-ci68.json. Folder discovery, companion work,
batch plans/acceptance, original-container expansion, full accounting/progress,
remaining donor modules, complete F08 and all product/package/resource/GPU gates
remain mandatory. No local compilation or execution is used.

Donor bodies personally read: standalone MuxCls reporting.py, processing.py and
copying.py at 0cf9596f7510f894babf36595efbc827fbbfaf04. Their original bytes and
research statuses remain unchanged. Personal interpretations are in the separate
reading-ledger.json follow-up entries.
