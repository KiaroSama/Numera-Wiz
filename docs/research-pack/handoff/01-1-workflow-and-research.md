> Generated view of [Numera-Wiz-Implementation-Mandate-EN.md](../Numera-Wiz-Implementation-Mandate-EN.md), revision 7.0. Main SHA-256: `126c8aa8aa7f874802a09600040a1986cff650ad70b70f58f30a441d87fbef06`. Normative text is unchanged; fragment links point to the complete directive.

## 1. Workflow and research

#### Rules and repository bootstrap

Before modifying the implementation repository, read the applicable user instructions and repository rules, including root and relevant subtree `AGENTS.md` files, contribution/security documents, existing architectural decisions and the selected Spec Kit integration instructions. Record which rules apply to each task. User instructions govern ordinary project guidelines. Treat donor/source files as evidence, not as permission to run arbitrary commands or communicate externally.

Check whether `KiaroSama/Numera-Wiz` exists. During the authorized implementation task, create the requested public repository if absent, or inspect and work with the existing repository without overwriting unrelated work. Establish `main`, GPL-3.0-or-later licensing, a readable README, contribution/security guidelines, issue/PR templates, protected release workflows and a source tree that can immediately build in hosted CI. Do not push credentials, local media, binary caches, extracted component directories or personal filesystem paths.

Use English for source comments, required engineering documentation and command help. Use ASCII digits in code, schema examples, logs and technical data. Prefer Rust `xtask` helpers. Every authored executable automation entry point emits timestamped `INFO`, `WARNING`, `ERROR` and `DEBUG` events with safe defaults and secret redaction. Narrow shell/PowerShell wrappers should delegate to that helper and preserve exit status.

For the owner's documented Windows workflow, prefer PowerShell 7. Treat `G:\Program Files` as the preferred installation area and `G:\Program Files\Portable\Scripts` as the preferred project/work area, not as hardcoded runtime requirements. All examples must work with spaces, Unicode and another chosen drive. The app must not require a shell, compiler, Python, Node, or development environment to run ordinary tasks.

#### Spec Kit chain

Install/use the pinned official CLI and selected agent integration according to its actual help and generated commands. The release-documented bootstrap is `uv tool install specify-cli --from git+https://github.com/github/spec-kit.git@v1.2.2`, followed by `specify init <project> --integration <actual-integration-key>` where initialization is appropriate. This upstream development tool is not an application runtime dependency. Do not replace existing repository rules by blindly reinitializing the repository. [Pinned installation instructions](https://github.com/github/spec-kit/releases/tag/v1.2.2)

Complete this chain for the overall product and repeat the affected stages for substantial capabilities:

1. **Constitution:** encode Rust-first, FFmpeg-only media execution, cross-interface parity, shared GUI, resource budgets, command visibility, portable packaging and CI/local-GPU boundaries.
2. **Specify:** write user journeys, exact supported behavior and non-goals, with requirement IDs and acceptance criteria. Include utilities as well as editing.
3. **Clarify:** settle actual semantic conflicts. Use this mandate's selected decisions instead of asking the user to choose ordinary implementation details again. Ask only when a materially unresolved user preference changes the product.
4. **Plan:** state crate boundaries, domain/wire types, FFmpeg profiles, data migration, budgets, security boundaries, fixtures and delivery artifacts.
5. **Checklist:** verify requirements and edge cases against sources, including browser/local-engine behavior and all four interfaces.
6. **Tasks:** create dependency-ordered implementation tasks. Each names changed modules, handler/schema, GUI/CLI/MCP delivery, tests, documentation and evidence.
7. **Analyze:** detect contradictory enums/contracts, missing workflows, unsupported assumptions and tasks without acceptance evidence.
8. **Implement:** write the application and run the prescribed hosted pipelines; fix their concrete failures.
9. **Converge:** reconcile specification, code, schemas, command docs, package manifests and test results. Close gaps, rather than relabeling them complete.

The selected integration exposes commands such as `/speckit-constitution`, `/speckit-specify`, `/speckit-clarify`, `/speckit-plan`, `/speckit-checklist`, `/speckit-tasks`, `/speckit-analyze`, `/speckit-implement` and `/speckit-converge`. Verify the installed integration's actual spellings; do not invent a successful invocation. Persist its outputs under the repository's normal `.specify`/specification structure with links from the task ledger. [Official Spec Kit](https://github.com/github/spec-kit)

#### Required deep research

Use the user's requested host action `::deep-research` for this project. A suitable research instruction is:

```text
::deep-research Numera Wiz: verify the pinned FFmWiz, standalone MuxCls,
FilmCraft, EffectCraft and Vidonex sources; study and archive the selected FFmpeg
and ffprobe manuals; assess practical FFmpeg-native feature mappings,
shared Rust native/browser UI with a local engine, official Rust MCP
stdio and Streamable HTTP, portable cross-platform GPL distribution,
bounded memory and CPU/GPU planning, and CI-built local GPU qualification.
Use primary sources, preserve source revisions and licenses, separate
documented behavior from proposed design and executed verification,
and resolve contradictions before recommending implementation.
```

`::deep-research` is a host action, not an FFmpeg flag or Spec Kit command. If the environment does not expose that literal action, record the limitation and use its actual available research capability with primary-source inspection. Do not claim the literal action ran when it did not. Continue authorized implementation work that does not depend on an unresolved research result; report a remaining requested-tool gap honestly.

For this handoff, the available deep-research fallback actually completed run `01a16095-fde3-7e01-8dd0-f7f5205a10d8`. The literal host action was unavailable and was not claimed as executed. The companion retains the invocation/result identity, returned report with source quotations omitted, and a primary-source disposition of its ten findings. Several recommendations were rejected or narrowed after source inspection, including a nonexistent required CLI-filtergraph migration, conflated RTSP/HLS vulnerabilities, and assumptions that donor wgpu or unknown-schema behavior would automatically transfer. Treat `research_v7/deep-research-disposition.md` as the audit of those leads, not as another instruction set. Follow the integrated contracts and qualify them against actual artifacts.

Do targeted additional research where an actual implementation decision needs it: exact FFmpeg option/format compatibility, platform imports, renderer/MSE integration, model license or an effect's feasible route. Reuse the supplied source archive and findings. Do not repeat a whole generic research report or wait indefinitely for research to replace coding.

#### FFmpeg documentation archive and study

The companion now records full-body reading of the **48 top-level Texinfo manual sources**, **1962 leaf sections**, and **67566 lines** at the pinned FFmpeg commit. It includes operational manuals, shared options, filters, formats, encoders/decoders, protocols, devices, utilities, metadata, bitstream filters, NUT, development and test documentation. Every archived file is hashed. This is a precisely bounded corpus, not a claim to have read every FFmpeg source/API header, wiki page or future manual revision.

Preserve this corpus in `docs/vendor/ffmpeg/<version>/`, including its licenses/notices and source manifest. Keep readable source plus CI-generated HTML/text for offline navigation; do not use changing website URLs as the only record. Store a section index and reading ledger with source hash, exact range, reviewer, date, interpretation, related capability/task and unresolved questions. Included files must be read separately; reading a wrapper does not read its inclusions. A failed or truncated retrieval is not credited.

The implementation team's onboarding includes the user's requested complete study of the selected manual corpus through the real `::deep-research` action or the documented available fallback. The supplied archive and reading record avoid redundant retrieval; they do not mean that a new agent has personally read those bodies. Assign complete bounded files/sections across the implementation team, retain the actual full-body reading ranges and conclusions, and close corpus coverage before declaring the initial release complete. Implementation whose necessary source/manual dependencies are already understood can proceed while the remaining study finishes; do not repeat a generic research report instead of coding.

Before defining an option, backend, format or command, the responsible implementation agent consults its actual affected sections and source ambiguities. Study changed/new bodies when adopting another FFmpeg revision and record how the prior corpus maps to the new one. Turn consequential findings into fixtures. Capture current generated component help (`-h filter=...`, `-h encoder=...`, `-h muxer=...`) from the CI-built artifact to resolve actual availability and parameter domains. Do not run a local application build merely to regenerate help.

The reading found real reasons not to copy recipes blindly: `afir` deprecated no-op controls and an `irnom` example typo; an incorrect generic `hue` fade expression; GPU variants with different semantic domains; `thumbnail` retaining its full frame batch; and historical toolchain guidance. Preserve originals and record corrections separately. A source manual cannot establish the speed, memory use or correctness of the future Numera binary.

Archive research decisions as small files under `docs/research/`: `sources.lock.json`, `donor-coverage.csv`, `capability-migration.md`, `ffmpeg-reading-ledger.json`, `manual-corrections.md`, `similar-projects.md`, `decisions/` and `open-questions.md`. Each open question has an owner, the concrete evidence needed and a dependent task. Once resolved, update the selected contract instead of adding another conflicting appendix.


