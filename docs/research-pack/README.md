# Numera Wiz — implementation research companion

**Revision 7.0 · 2026-10-23 · English handoff**

Start with **[Numera-Wiz-Implementation-Mandate-EN.md](Numera-Wiz-Implementation-Mandate-EN.md)**.
That file is the single implementation directive. It contains the fixed product requirements,
complete architecture and interface contracts, full FFmWiz workflow annex, explicit standalone/embedded MuxCls Rust rewrite, source-informed
effect decisions, implementation sequence, CI/security/resource gates and release documentation.

This companion preserves the evidence needed to implement and audit those decisions. It is
not a built application or a report of passed media/GPU tests. Previous conflicting mandates,
exact-donor-algorithm recipes and obsolete protocol schemas are intentionally absent.

The main MD itself contains **772,543 UTF-8 bytes**, exceeding the requested 500,000-byte minimum.
Section 4 supplies composition roots, crate/target dependencies, concrete module ownership,
typed records, command acceptance, planner and concurrency boundaries, durable tables,
recovery, client adapters and extension/distribution rules. Detailed surrounding chapters
bind those modules to the actual media, protocol, resource and platform contracts.

Annex D makes MuxCls a required complete Rust rewrite inside F08 and the shared command engine.
Its independently pinned standalone source is compared with FFmWiz's embedded copy. Module,
behavior, bug-fix and test evidence stays explicit; a Python launcher or wrapper is not completion.

Revision 7 also resolves temporal-prefix sampling, format-change resets, preview presentation,
bounded editor projections, complete shortcut registration, future-data compatibility, sequence
settings, agent observations, artifact transfer, first-probe admission, browser CSP/MIME and
object-level authorization. The primary ledger preserves 27 earlier findings plus 12 new ones;
the integration review records the associated cross-contract corrections. Retained product,
compiler, persistence, packaging and CI decisions remain binding. These are specification
resolutions, not completed implementation or measured performance.

Use **[handoff/START-HERE.md](handoff/START-HERE.md)** for generated section views of the same
directive. Section 11 includes WP01-WP12, a 36-row original-request/clarification traceability seed
and 63 cross-domain acceptance journeys. These complement the full per-workflow/domain/effect
fixtures; they do not replace them or represent a completed-test count.

## Scope present in this handoff

- All 15 FFmWiz workflow families and 40 distributed configuration keys are specified in the
  main mandate, including the source-confirmed corrections and acceptance cases.
- MuxCls is explicit initial scope: standalone commit `0cf9596f7510f894babf36595efbc827fbbfaf04`,
  all 17 package modules, entry/setup scripts, tests and full source-tree evidence, compared with
  the embedded FFmWiz implementation. Its selected Rust contract and separate coverage ledger
  feed the same shared planner, jobs, artifacts, UI and machine interfaces.
- All 303 FilmCraft effect/transition/intrinsic/graphic inventory rows and all 307 EffectCraft
  entries have individual functional design dispositions, source evidence and processing limits.
- All 942 FilmCraft command definitions/source-derived aliases have an explicit functional
  destination or precise disposition, including 16 verified track-target registrations missing
  from the earlier extractor. They are source records, not 942 independent endpoints.
- The two donor effect catalogues are reconciled through a canonical alias/variant ledger.
  Duplicate source names do not require duplicate processing implementations.
- Full-body reading is recorded for all 48 top-level Texinfo manuals at FFmpeg
  `946fcce07b6dcd0331c8cc609192aeff5e1924f8`: 1962 leaf sections and 67566 lines.
  File hashes and recorded reading-range coverage have been checked.

## Evidence and authority

| Material | Role |
|---|---|
| Main implementation mandate | Binding product/engineering directive; it supersedes earlier drafts. |
| `research_v7/donor-effects-coverage-v7.csv` | Combined 610-row source-to-canonical-capability mapping, route, limits and qualification obligations. |
| `research_v7/command-functional-mapping-final.csv` | Per-source-command canonical operation/alias/parameter/session mapping and explicit limitations. |
| `research_v7/cross-donor-capability-aliases.csv` | Reviewed canonical deduplication and retained semantic variants. |
| Per-donor final CSV/Markdown reports | Detailed source-informed semantics, differences, route designs, resources and primary references. |
| `research_v7/*source-descriptors.json` and `filmcraft-source-commands.json` | Source-only provenance. Donor IDs/defaults/internal algorithms are not automatically Numera API contracts. |
| `research_v4/ffmpeg-section-reading-ledger.json` | Exact manual hash, section/range and actual-reading evidence. |
| `research_v4/manual-findings-*.md` | Consequential manual findings and corrections to turn into implementation fixtures. |
| `research_v7/review-resolution-ledger.md` | Independent final review issues and how the authoritative directive resolves them. |
| `research_v7/integration-cross-review.md` | Cross-contract corrections including transition clocks, retry terminals, retained media, profile bootstrap, object rights, pure shortcut ownership and retired-command receipt replay. |
| `research_v7/muxcls-coverage.csv` | Standalone/embedded MuxCls source behavior to selected Rust owner, shared operation and required acceptance evidence. |
| `research_v7/muxcls-source-review.md` | Pinned comparison and source-confirmed limits; Annex D is the integrated implementation instruction. |
| `research_v7/deep-research-run.json` and `deep-research-disposition.md` | Actual completed supplementary research invocation and claim-by-claim primary-source disposition. The raw returned report is untrusted evidence; its erroneous recommendations do not override the mandate. |
| `research_v7/requirements-traceability.csv` | Original numbered requirements and clarifications mapped to R IDs, main sections, work packages and concrete evidence. |
| `research_v7/acceptance-scenario-index.csv` | Full assertions and R/F/CD/WP/CI references for 63 cross-domain journeys; all implementation/runtime statuses remain unqualified. |
| `handoff/section-manifest.json` | Main-document hash and generated-view hashes; reversing only link qualification reconstructs the authoritative text exactly. |
| `research_v5/` | Retained bounded review/field-patch evidence behind earlier corrections; no superseded mandate or alternative architecture. |
| `research-pack-manifest.json` | SHA-256 and byte size of every other archive member. |

The `research_v2` and `research_v3` directory names identify the retrieval batch of preserved
source material; they do not make older design rules authoritative. Their source bodies retain
upstream content. Generated manifests use portable relative local paths. The archived sources
are selected research extracts, not complete buildable clones of every donor repository.

## Source and manual layout

| Directory | Contents |
|---|---|
| `research_v2/sources/ffmwiz/` | Required non-GUI Python/configuration baseline and its license; provenance for the Rust rewrite. |
| `research_v2/sources/filmcraft/` | Selected Rust UI, editing, effect and project source plus notices/licenses. |
| `research_v2/sources/effectcraft/` | Selected effect/control source plus notices/licenses. |
| `research_v2/sources/vidonex/` | Inspected donor source/license extracts. |
| `research_v2/ffmpeg-upstream/` | Full 48-manual corpus, upstream licenses, configure and inspected implementation files. |
| `research_v2/build-profile-sources/` | Native dependency build/license evidence. |
| `research_v3/requirements-filmcraft-crates__*` | Additional pinned FilmCraft command, UI, rendering and format source inspected for behavior. |
| `research_v3/ffmpeg-source/` and primitive extracts | Additional FFmpeg transport/process/filter source used to resolve manual ambiguity. |
| `research_v3/sources/` | Pinned SDK/store dependency source and release/license evidence. |
| `research_v4/` | Preserved manual-reading evidence and inspected FFmpeg source bodies; no previous mandate. |
| `research_v5/` | Historical review and field-correction evidence preserved by the current contract. |
| `research_v7/` | Current mappings, source-only descriptors, architecture/rewrite review inputs, bounded findings and correction resolutions. Review-input documents are evidence, not separate amendments to the main directive. |
| `research_v7/muxcls-sources/` | Complete pinned standalone public source tree, source hashes and embedded-comparison provenance; no donor tests were executed. |
| `research_v7/security-sources/` | Exact pinned FFmpeg files and clearly classified advisory/commit evidence used to correct the supplementary report. |
| `handoff/` | Automatically generated section views and reading index for the complete current mandate. |

## Interpretation of feature status

`native`, `composed`, `component` and `application` are **design dispositions**. A runnable
feature must still be implemented, present in the actual packaged profile, correct over its
declared domain and qualified for resources and all interfaces. `component` can mean an
already-required standard build dependency or a named optional/user-supplied asset; it does
not automatically mean a downloadable optional feature. `partial` identifies an explicit
semantic boundary. `unsupported` applies to a precisely identified primary feature; a useful
alternative does not count as complete support for that feature.

Every current runtime status remains unimplemented/unexecuted. Source inspection, a valid
filter name and a document-validation pass are not runtime evidence. The first implementation
agent must build and run ordinary tests in GitHub CI. The owner then runs the prebuilt matching
GPU qualification bundle locally, as specified in the mandate.

## Using this pack

1. Read the main directive and applicable repository/user rules.
2. Carry its requirements into the Spec Kit constitution/specification/plan/tasks and actual code.
3. Use the combined mappings to retain source coverage while implementing shared canonical handlers.
4. Consult the affected pinned FFmpeg manual/source sections before freezing options and graphs.
5. Extend the ledgers with real implementation, CI, artifact and owner qualification evidence.
6. Keep source/license notices with reused code; publish corresponding source and build recipes
   for redistributed binary components. This research archive is not a legal-clearance certificate.

No local app builds, FFmpeg media transformations or owner GPU runs were performed to produce
this revision. An actual deep research run completed through the available research capability;
the literal host `::deep-research` action was not exposed or executed. The result was checked
against primary sources and erroneous or inapplicable recommendations were rejected. The
implementation agent must still follow the mandate's real tool-availability and study contract.
