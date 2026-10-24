# Numera Wiz Constitution

## Core Principles

### I. One Rust application and one media engine
Handwritten application, UI, service and automation logic MUST use Rust. Media
decode, DSP, composition, encode and mux MUST use admitted FFmpeg/ffprobe paths.
The browser MUST run the shared Rust UI against the local native owner. Donor
sources are behavioral evidence, never an alternative application runtime.

### II. One authoritative command contract
Domain types, validation, availability and handlers MUST be shared by native UI,
browser UI, CLI, stdio MCP and HTTP MCP. Durable operations MUST have scoped
idempotency and revision guards. Discovery MUST distinguish implemented internals
from qualified user capabilities. No surface may silently change media semantics.

### III. Verified bytes before publication
Plans MUST capture stream identities, input fingerprints, output claims and exact
argv. Subprocesses MUST use argument vectors, bounded I/O and finite deadlines by default.
The mandate's explicit typed disabled cleanup deadline is admitted only for the
protected local owner; scoped callers cannot disable it. Accepted plans freeze
that choice and all adapters preserve the current service admission policy.
Every invocation MUST be visible before execution. Publication MUST verify owned
staging and MUST NOT infer success from exit zero or a pre-existing destination.
Copy, packet remux and decoded render are different preservation claims.

### IV. Bounded resources and explicit authority
One worker MUST own each data root. Authentication, object rights and filesystem
grants MUST be checked independently. Media processes, queues, diagnostic buffers,
history and scratch MUST have budgets. CPU-only operation MUST avoid media GPU
initialization. Auto MUST prefer qualified GPU routes only when they satisfy the
requested objective; unsupported hardware MUST have a precise reason.

### V. Evidence controls support claims
Builds and ordinary tests MUST run in GitHub CI. Owner GPU qualification MUST use
matching CI-built binaries, fixtures and harnesses. A source inspection, schema,
unit test or screenshot MUST NOT be represented as packaged runtime qualification.
Required initial scope includes F01-F15, complete MuxCls, admitted donor functions
and all command domains; a milestone MUST NOT be called the complete release.

## Delivery and Observability

The public project is KiaroSama/Numera-Wiz under GPL-3.0-or-later. Current delivery
targets are Windows, Linux and macOS; Android remains a later adapter. Windows
release delivery MUST include offline portable components and same-build browser
assets. Documentation MUST cover human and agent setup and multiple full manuals.
Authored executable entry points MUST emit timestamped INFO, WARNING, ERROR and
DEBUG diagnostics, keep protocol stdout clean and exclude credentials.

## Development and Review

Use constitution → specify → clarify → plan → checklist → tasks → analyze →
implement → converge. The authoritative revision 7 mandate and research corpus
remain preserved with provenance. User instructions take precedence. All work in
this review session MUST stay on implementation/numera-wiz-v1. Do not merge,
close PRs/issues, enable auto-merge, publish releases or modify main.
Unfinished obligations remain explicit in tasks and the continuation record.

## Governance

Amendments require a concrete reason, affected requirements, migration impact and
an incremented semantic version. Reviews check constitution, mandate, code,
schemas, manuals and evidence together. Exceptions MUST name their scope and
closure task; they MUST NOT weaken a requirement to manufacture completion.
The supplied upstream Spec Kit shell/PowerShell helpers are development tooling
exceptions, not Numera runtime dependencies.

**Version**: 1.0.1 | **Ratified**: 2026-10-23 | **Last Amended**: 2026-10-24

Amendment 1.0.1 applies the authoritative Annex D explicit-disabled timeout
contract without ambient environment settings. It affects cleanup policy,
planning, supervision and adapters. Historical optional fields remain absent
with unchanged plan hashes; no store reset or implicit migration is authorized.
