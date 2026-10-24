# Reviewed execution rights

New cleanup plans use schema version 2. `required_rights` captures the data-root
identity, exact source file identity and read purpose, and the resolved output
path with `no_replace` collision policy. Native-file plans can transfer only
when the submitter currently has plan Submit plus the same independent source
read/output write rights. No grant IDs or creator credentials are substituted.
Managed input plans explicitly bind the read purpose to the planning principal;
a delegated plan alone cannot transfer that purpose. Current managed-read and
owned-object policy still apply at submission and stage boundaries.

The semantic digest includes those captured requirements, source identity/probe,
resolved media selection/metadata, exact component identities, argv template and
verification contract. It excludes plan/registration IDs, display labels,
creator/audit principal, review creation/expiry timestamps and creation policy
revision. Expiry is independently checked at acceptance. A policy revision change
alone does not require a new hash; withdrawing a required root or managed purpose
still denies work. Accepted jobs retain the submitter's durable authorization and
recheck current relevant policy before stages/publication.

Raw version 2 plan JSON is hashed before typed decoding. Re-encoding must preserve
all hashed semantics; an unknown field cannot vanish in a typed round trip and
still execute. Object authorization precedes compatibility diagnostics on plan
lookups. Rights paths are included in the review only when both paths are visible
under current policy; otherwise `rights_redacted` is true. Command arguments and
job details retain their existing stricter owner projections.

Historical version 1 serialization and hashes are retained, never silently
upgraded. Existing terminal job/artifact facts remain historical facts. Fresh
submission of an old plan requires a new version 2 review. A queued legacy plan
that reaches current stage validation fails safely rather than gaining invented
requirements. Direct legacy digest support exists for history, not admission of
new legacy jobs. Future schema versions fail explicitly.

This batch adds focused domain/store/service assertions and an actual official
MCP cross-principal equivalent-plan hash assertion to the hosted owner journey.
Hosted run 40061709915 passed all four native test/lint/desktop targets, the WASM build, actual engine and CLI/official MCP journeys, actual Linux native input and Chrome interaction. Its cross-principal equivalent-plan assertion passed. Native and large-output evidence is retained in `evidence/native-copy-ci54.json` and `evidence/artifact-http-ci54.json`. This is the current single-source cleanup
contract; multi-source project/dependency graphs, observations, device/network
purposes and the full release acceptance matrix remain required work.


The pending original-object extension also hashes the asset's captured native
file identity in source_read.object. New planning/admission requires it; no
historical missing identity is silently filled. The legacy optional field is
omitted during serialization, retaining prior hashes. See source-inputs.md for
its OS ABI, full digest, current authority and pending qualification boundaries.
