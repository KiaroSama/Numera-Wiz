# Implementation Plan: Numera Wiz

**Branch**: `implementation/numera-wiz-v1` | **Date**: 2026-10-23
**Spec**: [spec.md](spec.md)

## Summary
Implement the complete revision 7 mandate in dependency order E01-E10/WP01-WP12.
Start with exact shared types and the cleanup resolver, then owner/store/media
execution, all adapters, complete editing/utilities, profile closure and release
qualification. Internal development artifacts are explicitly unqualified.

## Technical Context
Rust 1.99.1/edition 2024; shared serde/schemars contracts; redb 4.4.0 with 8 MiB
cache; Tokio/Axum; official rmcp 3.6.0; eframe/egui 0.36.2 Glow; FFmpeg n9.0.2
source baseline. Cargo.lock records the resolved tested dependency closure.
Targets: Windows/Linux/macOS plus shared UI WASM. Android remains later.

## Constitution Check
Rust/FFmpeg ownership, shared contracts, bounded execution, hosted testing and
honest qualification are required. Current portable payload, browser parity,
full scope and owner-GPU gates remain open, so release is blocked. They are
implementation tasks, not accepted reductions. Upstream Spec Kit scripts are a
documented development-tooling exception. No third-party runtime is inferred
from archived research sources.

## Project Structure
- `crates/nw-domain`: exact time, media/selection types, command DTOs and registry.
- `crates/nw-platform`: native identity/path/publication/process/logging ports.
- `crates/nw-engine`: bounded probe, normalization, immutable plans, execution/verify.
- `crates/nw-store`: single-owner redb, scopes, receipts, assets, plans and jobs.
- `crates/nw-service`: auth/dispatch/routers, job scheduling and shared acceptance.
- `crates/nw-ui`: shared Rust workspace/client; no native engine in WASM.
- `apps/numera-wiz`: thin native mode dispatch and official MCP adapters.
- `xtask`: Rust verification, schema/manual generation and qualification tooling.
- `docs/research-pack`: byte-preserved authoritative research/provenance archive.
- `docs/vendor/ffmpeg/n9.0.2`: offline manual source and license corpus.

## Phase 0: Research
Use supplied sources and targeted primary inspection. The official MCP and UI
APIs were researched in parallel according to the installed plan skill. Important
changes: eframe 0.36 requires App::ui and Panel; rmcp 3.6 uses CallToolResponse
and explicit HTTP Origin enforcement. Details are in `research.md`.

## Phase 1: Contracts and Safety
Validate decimal-string exact times, strict enums, explicit no-match policies,
original-snapshot patch predicates, immutable source/output identities and command
state classes before execution. Structural digests exclude request correlation.
Authentication never derives from caller params; object access is checked apart
from roots. Publication uses owned staging and no-replace primitives after verification.

## Verification and Delivery
Hosted CI runs formatting, lint, domain/storage regressions, bounded subprocess
faults, actual FFmpeg fixtures, native builds and WASM closure checks. Qualification
is per boundary; a passing domain test does not complete F08 or a platform.
Required package and owner gates remain blocked until exact artifacts exist.
