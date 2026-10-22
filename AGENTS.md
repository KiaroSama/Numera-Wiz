# Numera Wiz repository rules

Read `.specify/memory/constitution.md`, `specs/001-numera-wiz/` and the authoritative
`docs/research-pack/Numera-Wiz-Implementation-Mandate-EN.md` before changing affected code.
Read applicable subtree rules. No private Rules/hooks were provided in this session;
do not claim to have seen them. Existing user instructions take precedence.

- Work only on `implementation/numera-wiz-v1` until the owner reviews it.
- Never merge/close PRs or issues, enable auto-merge, publish a release or write to main.
- Rust owns first-party runtime and automation; FFmpeg owns media operations.
- All comments, technical docs and help use English and ASCII digits.
- Authored entry points use timestamped INFO/WARNING/ERROR/DEBUG logging. Never log secrets.
- Build and run ordinary tests in GitHub Actions. Local source builds/media runs are
  excluded by the mandate. The owner runs only the matching CI-built GPU harness.
- Preserve research provenance; an archived file is not evidence that this agent read it.
- Every support claim requires actual evidence at its stated unit/media/transport/package boundary.
- Keep unfinished tasks and `docs/development/continuation.md` current. Do not hide required
  work behind stubs, unavailable labels, catalogue counts or fake qualification results.
- Only use subagents when the user or an applied skill explicitly calls for delegation.
