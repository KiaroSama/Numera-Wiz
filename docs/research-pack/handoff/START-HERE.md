# Start here: Numera Wiz implementation handoff

The complete [English mandate](../Numera-Wiz-Implementation-Mandate-EN.md) is authoritative. These generated section views contain the same normative text, with a provenance banner and internal links directed to the complete document. They are not separate amendments.

**Revision:** 7.0. **Main SHA-256:** `126c8aa8aa7f874802a09600040a1986cff650ad70b70f58f30a441d87fbef06`.

## Read and execute

1. Read the front matter and Section 1, identify applicable user/repository rules, and start the real Spec Kit/research workflow.
2. Read Sections 2-8 and Annexes A/D before freezing an implementation plan. The complete architecture and MuxCls rewrite are required scope. Read the complete relevant donor/command rows while implementing each family; indexing or retrieving a file is not reading it.
3. Carry Section 11's WP01-WP12 outputs and requirement/evidence records into actual tasks. Foundation dependencies are distinct from integrated first-slice completion.
4. Build and run ordinary tests in GitHub CI. Owner GPU qualification uses only the matching CI-built application/harness/fixtures.
5. Keep the continuation record current. Advance an implementation/qualification status only with real code, artifact and result references.

## Section views

| File | Content |
|---|---|
| [00-front-matter-and-fixed-requirements.md](00-front-matter-and-fixed-requirements.md) | Front matter and fixed requirements |
| [01-1-workflow-and-research.md](01-1-workflow-and-research.md) | 1. Workflow and research |
| [02-2-functional-scope-and-migration.md](02-2-functional-scope-and-migration.md) | 2. Functional scope and migration |
| [03-3-editing-and-command-domains.md](03-3-editing-and-command-domains.md) | 3. Editing and command domains |
| [04-4-engine-ownership-and-durable-state.md](04-4-engine-ownership-and-durable-state.md) | 4. Engine ownership and durable state |
| [05-5-domain-model-and-interface-contracts.md](05-5-domain-model-and-interface-contracts.md) | 5. Domain model and interface contracts |
| [06-6-ffmpeg-execution-and-preview.md](06-6-ffmpeg-execution-and-preview.md) | 6. FFmpeg execution and preview |
| [07-7-shared-editor-and-shortcuts.md](07-7-shared-editor-and-shortcuts.md) | 7. Shared editor and shortcuts |
| [08-8-resources-security-distribution-and-ci.md](08-8-resources-security-distribution-and-ci.md) | 8. Resources, security, distribution and CI |
| [09-9-implementation-sequence.md](09-9-implementation-sequence.md) | 9. Implementation sequence |
| [10-10-additional-project-research.md](10-10-additional-project-research.md) | 10. Additional project research |
| [11-11-work-packages-traceability-and-acceptance.md](11-11-work-packages-traceability-and-acceptance.md) | 11. Work packages, traceability and acceptance |
| [12-annex-a-complete-ffmwiz-behavior.md](12-annex-a-complete-ffmwiz-behavior.md) | Annex A. Complete FFmWiz behavior |
| [13-annex-b-donor-capability-decisions.md](13-annex-b-donor-capability-decisions.md) | Annex B. Donor capability decisions |
| [14-annex-c-source-command-closure.md](14-annex-c-source-command-closure.md) | Annex C. Source-command closure |
| [15-annex-d-complete-muxcls-rust-rewrite.md](15-annex-d-complete-muxcls-rust-rewrite.md) | Annex D. Complete MuxCls Rust rewrite |

## Machine-readable handoff seeds

- [Original-request traceability](../research_v7/requirements-traceability.csv): 25 numbered items/subitems plus 11 clarifications/setup preferences, including the complete architecture, main-file size and explicit MuxCls rewrite, mapped to R IDs, sections, work packages and completion evidence.
- [Cross-domain acceptance index](../research_v7/acceptance-scenario-index.csv): 63 planned journeys, with full assertion text and R/F/CD/WP/CI references. These supplement the full workflow/domain/effect fixtures.
- [MuxCls behavior coverage](../research_v7/muxcls-coverage.csv) and [source comparison](../research_v7/muxcls-source-review.md): complete module reading, selected Rust owners, fixed source limitations and required CI evidence.
- [Primary review resolutions](../research_v7/review-resolution-ledger.md) and [integration corrections](../research_v7/integration-cross-review.md): concrete prior specification gaps and the selected resolution.
- [Combined donor coverage](../research_v7/donor-effects-coverage-v7.csv) and [source-command coverage](../research_v7/command-functional-mapping-final.csv): source-to-canonical mappings with truthful implementation/runtime status.
- [Research pack index](../README.md): pinned source/manual corpus, reading evidence and preservation boundaries.

Do not edit a section view separately. Changes to the directive must regenerate all views and their manifest so the handoff cannot diverge. This pack specifies work to build and qualify; it contains no Numera binary and no fabricated runtime report.
