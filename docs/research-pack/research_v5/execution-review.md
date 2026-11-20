# Numera Wiz revision 4: execution-contract review

## Revision 5 resolution status

All seven findings below are resolved in the assembled revision 5 architecture, domain and command contracts. The canonical command CSV also carries the resource-specific revision and volatile-control rules. See `research_v5/review-resolution-ledger.md`. The `research_v4` chapter references identify the historical reviewed baseline; they are not current companion paths or competing instructions. These are applied specification corrections, not observed runtime results.

Review date: 2026-10-21. Scope: the delivered mandate's architecture, domain/wire contract,
GUI interaction, Windows launch behavior, delivery policy, and final source-command mapping.
This is a bounded document/source review. No Numera build, FFmpeg job, GUI test, MCP session,
GPU test or GitHub mutation was performed.

The existing selected worker, shared UI, durable job state machine and FFmpeg boundary can be
retained. The defects below need small, explicit contract corrections. They do not justify
another architecture rewrite or another set of invented product requirements.

## Findings

| ID | Priority | Confirmed document defect |
|---|---|---|
| EX5-01 | P1 | Replay-scope issuance cannot bootstrap under the universal mutation rule. |
| EX5-02 | P1 | The worker's idle rule omits active ordinary requests and transfers. |
| EX5-03 | P1 | Project UUID locks do not serialize publication to a shared destination. |
| EX5-04 | P2 | Session controls inherit a durable project-mutation contract without a defined exception. |
| EX5-05 | P2 | Private transport credentials and the stdio MCP caller's authority are not separated. |
| EX5-06 | P2 | The PowerShell examples do not establish waiting for the selected GUI-subsystem EXE. |
| EX5-07 | P2 | The envelope and canonical field lists need an explicit field-placement rule. |

P1 means an implementation following the text literally can fail a basic promised workflow or
lose the publication exclusion guarantee. P2 means a concrete contract ambiguity or incorrect
invocation example must be closed before implementation/installation documentation is frozen.
These are design findings, not reports of observed failures in an application binary.

## EX5-01 — Replay-scope bootstrap is circular

**Evidence:** `research_v4/mandate-architecture.md`, “Commands, durable project state, and save,”
requires every mutation to have a valid server-issued replay scope.
`research_v4/mandate-domain.md`, “Wire request, response and compatibility,” introduces
`access.replay_scope.create` as the issuer without exempting that state-changing operation.

**Failure:** a newly authenticated client has no replay scope. If the common mutation guard
runs first, it cannot invoke the operation that issues its first scope. Treating the creation
operation as an ordinary authenticated durable mutation does not remove that dependency.

**Minimal correction:** explicitly classify scope issuance as an authenticated bootstrap
operation that does not require an existing replay scope. A retry may issue another bounded
scope; it never repeats a project edit or media job. Bound issuance count/rate and store only
what verification requires. Pairing exchange, token setup and the local owner bootstrap must
also have explicit authentication/issuance rules, rather than implicitly going through a
guard whose prerequisite they create. Keep ordinary durable mutations under the existing
finite replay-scope contract.

**CI assertion:** a fresh data root and authenticated principal obtain a scope and commit a
first edit/job; a lost scope-issuance response and retry do not execute work; unauthenticated
issuance fails. The ordinary duplicate-job and expiry cases remain as already required.

## EX5-02 — Active CLI requests are absent from worker retention

**Evidence:** `mandate-architecture.md`, “Process and transport ownership,” retains the worker
for an accepted job, native/browser editor-client lease, preview resource lease or explicit
service lease, then exits after 15 seconds with none. `mandate-domain.md` correctly says
read-only queries and pure edits do not need a media job. Ordinary CLI and stdio adapters are
not editor clients or explicit foreground services. Public/private routes also stream uploads,
downloads and results outside an active preview.

**Failure:** a slow `media probe`, deep plan/preflight request, or artifact transfer can run
beyond 15 seconds while no listed retention condition holds. A literal implementation can
declare the worker idle and terminate an in-flight valid request. A tiny probe fixture would
not reveal this omission.

**Minimal correction:** add an active-request/work lease. Hold it through authenticated
request handling and response-stream completion, including bounded probe/planning/import and
artifact-transfer work. Owned background work that is not a durable job must hold the same
kind of lease. Release it on completion, cancellation, timeout or disconnect according to the
route's stated policy. Start the idle grace only after all request/work and existing client/job
leases are gone. This does not require keeping an idle stdio bridge's worker forever.

**CI assertion:** with no GUI or service, hold a valid slow probe/transfer longer than the
idle grace; it finishes without owner death. A disconnected or timed-out request releases its
lease, and the otherwise idle owner exits after the grace.

## EX5-03 — UUID-only leases do not protect output paths across owners

**Evidence:** `mandate-architecture.md`, “Project writer lease across portable installations,”
keys the cross-data-root lease by project UUID; canonical path/file identity is retained for
diagnostics. “Commands, durable project state, and save” compares target identity before
replacement. “One job state machine” specifies per-owner publication fencing and per-file
journals. There is no shared destination claim spanning independent owners with different UUIDs.

**Failure:** owners A and B, using different data roots and different project UUIDs, both Save
As to the same destination. Their UUID leases do not conflict. Both can inspect the same old
destination, then replace it in sequence. A pre-rename comparison is not a cross-process
compare-and-replace transaction. The equivalent race exists when two owners export to one
path, especially when an implementation turns “destination did not exist at preflight” into
an ordinary replacing rename.

**Minimal correction:** retain the project-UUID lease and add a narrowly scoped publication
claim for each destination, shared across data roots of the same user. Key a nonexistent path
by the canonical parent directory identity plus platform-normalized basename; handle existing
file aliases according to the stated path policy. Acquire ordered destination claims before
the final identity/overwrite check and retain them through publication/reconciliation.
For “never overwrite,” use the target platform's atomic no-replace operation, not a separate
exists check followed by replacing rename. Do not claim protection from an unrelated hostile
process that ignores advisory claims. This is a small extension of existing publication
ownership, not collaborative editing.

**CI assertion:** two data roots/different UUIDs concurrently Save As to one destination;
two owners export to one absent destination under no-overwrite; a target changes before the
claim/check. Exactly the authorized winner publishes; the other receives an explicit conflict.
Existing same-project lease and multi-file crash tests remain distinct.

## EX5-04 — Volatile session controls need their own acknowledgment semantics

**Evidence:** the architecture says every mutation has a replay scope, and the wire section
says mutations include a durable receipt and committed revision. The 942-row command ledger
contains 371 `session` rows, including `ui.selection.set`, `ui.timeline.viewport.update`,
`ui.monitors.update` and `previews.transport.update`. Many session rows explicitly inherit
the common idempotency/atomic-edit contract. The GUI requires responsive transient drags and
the delivery contract requires bounded state, storage and receipt quotas.

**Defect:** the text does not distinguish volatile view/transport controls from durable
project changes and saved preferences. An implementation can legitimately interpret it as
requiring a redb receipt and durable revision for every selection, scroll or playback change.
That is unnecessary storage/write pressure and leaves the meaning of “committed revision”
undefined for a client-only action. This review does not claim a measured memory failure.

**Minimal correction:** put the durability class in the shared command descriptor:

- Durable project/settings/access mutations and job acceptance use the existing durable
  replay-scope/receipt contract, with the appropriate resource revision.
- Volatile client/session and preview controls use authenticated client/session identity,
  generation and bounded monotonic sequence acknowledgments. They do not increment project
  revision or require retained durable receipts. Resolve toggle/delta gestures to explicit
  state or qualify them against the expected session revision.
- A deliberate “save workspace/preferences” operation persists a coalesced snapshot through
  the durable path. A committed edit at drag completion stays a durable project mutation.

All surfaces retain access to the same handlers. This changes durability, not feature parity.

**CI assertion:** a long sequence of viewport/selection/seek updates keeps durable receipt
storage bounded independently of event count; stale generation/sequence controls are ignored
or rejected; a final project edit is still durable exactly once after reconnect/crash.

## EX5-05 — The stdio adapter lacks a concrete effective-principal contract

**Evidence:** private HTTP uses a protected owner credential and is shared by native GUI,
CLI and stdio adapters. The security section denies unrestricted filesystem rights to paired
browser/MCP tokens. The sample stdio configuration supplies only `bridge` and `--data-dir`.
The document says tokens can be issued by authenticated `access` commands, but does not
define whether the stdio adapter forwards owner authority or acts as a scoped MCP principal.

**Defect:** possessing a transport credential and having media-file grants are conflated or
left to the implementer. One interpretation gives the bridge owner authority; another makes
the sample unusable until an unspecified grant/bootstrap step occurs. Neither outcome is a
complete agent setup contract.

**Minimal correction:** distinguish protected private-transport authentication from the
effective caller principal. Native owner-only local actions may provision a named, stable
agent principal and its explicit roots/device/network grants. `bridge` selects that stored
profile or a scoped credential file and never promotes incoming MCP paths to local-owner
authorization merely because they arrived over private IPC. Give the generated README a
concrete owner bootstrap → grant/profile creation → bridge configuration → discovery → first
planned job sequence. A zero-grant configuration may connect/discover and receive an explicit
permission error; it must not silently gain arbitrary filesystem roots. The already-declared
same-user compromise boundary remains unchanged.

**CI assertion:** an agent principal can process its allowed input/output roots, cannot use
the owner transport to read outside them or create wider grants, and retains its identity on
credential rotation. A native owner-authorized setup can provision the profile without a GUI.

## EX5-06 — PowerShell invocation must account for the GUI executable subsystem

**Evidence:** `mandate-delivery.md` selects a GUI-subsystem Windows executable. The “Human
CLI” examples use standalone `& '.\numera-wiz.exe' ... --json` and `jobs submit ... --wait`.
The current PowerShell native-command processor explicitly backgrounds Windows GUI applications
at the end of a pipeline, while a native command with a downstream pipeline stage is not
backgrounded. `AttachConsole` supplies console attachment/standard handles; it does not alter
the parent's waiting decision.

Primary evidence: [PowerShell native-command implementation](https://github.com/PowerShell/PowerShell/blob/master/src/System.Management.Automation/engine/NativeCommandProcessor.cs),
particularly the `PipelinePosition`/`PipelineLength` and `_isRunningInBackground` branch;
[Microsoft AttachConsole](https://learn.microsoft.com/en-us/windows/console/attachconsole).
The first source is current-source observation, not a fixed Numera dependency.

**Minimal correction:** keep the selected one-file distribution, but make synchronous
PowerShell examples use an actual waiting/capturing invocation. For JSON commands, a qualified
`& $Exe <args> | ConvertFrom-Json` pipeline is a suitable selected form; human output can use
a downstream `Out-Host` stage. Check the native exit code after completion. MCP clients spawn
the EXE directly with owned pipes and process handles and do not use a human display pipeline.
Plain no-argument GUI launch can remain asynchronous. Qualify redirected UTF-8 stdout/stderr
and handle inheritance in the existing early packaged-artifact gate. Do not recommend a
generic `Start-Process -Wait` without considering the detached long-lived worker descendant.

**CI assertion:** from PowerShell 7, a delayed CLI request blocks until the frontend result,
returns parseable JSON and the exact frontend exit code while a previously accepted durable
job/worker may outlive the frontend. Direct MCP spawn remains framing-clean. No additional
Windows frontend binary is required by this correction.

## EX5-07 — Canonical field lists and the envelope disagree on location

**Evidence:** the wire example puts project identity/revision under
`project.id`/`project.expected_revision`, with only command-specific values under `params`.
The authoritative CSV lists `project_id; expected_revision; ...` as canonical parameters for
`clips.split` and many other commands. It describes these as a field-level contract but never
defines that these two fields are flattened shorthand for the envelope.

**Minimal correction:** explicitly define the ledger notation: project/revision and
request/idempotency fields map to the canonical envelope; only operation-specific fields go
under `params`. Do not require both copies or silently pick one when they conflict. Generated
schemas, examples, CLI normalization and MCP adaptation use that one mapping. Resource-specific
session/layout revisions remain their own typed fields where no project revision applies.

**CI assertion:** the same `clips.split` request produced by GUI/CLI/MCP has one project
identity/revision location; duplicated conflicting fields are rejected instead of producing
surface-dependent behavior.

## Retained boundaries and completion limit

No evidence found here requires replacing the selected shared Rust UI, adding a web frontend,
moving ordinary tests onto the owner's machine, or requiring another media renderer. The
existing publication fence, separate job cancellation, bounded snapshots, explicit standard
components and artifact-based owner GPU qualification remain useful requirements.

This review closes its stated contract analysis. It is not a certification of the whole
55,000-word mandate or all effect recipes, and does not manufacture runtime evidence. Apply
the minimal corrections and validate their concrete implementations in the already-required
GitHub CI and matching owner qualification workflow.
