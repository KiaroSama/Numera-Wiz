# Explicit access setup

The distributed MCP configurations pin `--access-profile agent`. They grant
discovery and own replay-scope bootstrap only. An existing profile does not
automatically receive new command grants when the registry grows.

For an explicit setup request, the protected local owner can create a named
profile with selected read/write roots. The following native-owner commands
are administration, not ordinary agent media calls:

```text
numera-wiz --data-dir DATA access profiles create --name operator --read-root INPUT_ROOT --write-root OUTPUT_ROOT --json
numera-wiz --data-dir DATA access profiles inspect --name operator --grants-out NEW_GRANTS.json --json
numera-wiz --data-dir DATA access profiles update --name operator --expected-revision REVISION --grants-file NEW_GRANTS.json --json
```

Creating an operating profile captures the current ordinary command set and
permits creator-owned objects within its selected roots. Inspect it before use.
For narrower authorization, edit the exported full grant document and replace
the policy at the exact inspected revision. Preserve needed existing grants:
update replaces the complete document rather than adding one field. The file
has `enabled`, `client_sessions`, `mode`, `owned_object_access`,
`allowed_command_ids`, `object_grants`, `read_roots`, `write_roots`.
Profile name and expected revision belong in update arguments.

Plan Read and Submit are separate actions; Job Read and Control are separate
actions. Grants contain kind, exact ID and actions, for example
`{"kind":"plan","id":"ACTUAL_PLAN_ID","actions":["read","submit"]}`.
Write actions require Read. Shared roots, opaque IDs and manifest URLs do not
grant another principal's assets/jobs. Turning off `owned_object_access`
withdraws inherited creator access, while explicit grants still apply.

The owner can then change a private copy of the selected client configuration
from `agent` to the exact reviewed profile name, and set the same absolute
executable, `--data-dir` and `--components-dir` for every call. Do not edit a
profile or switch to owner authority to bypass a denial. Scoped MCP parameters
cannot select another profile. Agent media workflows must not remove the
profile selector from CLI commands.

For an explicitly requested HTTP listener, the owner runs:

```text
numera-wiz --data-dir DATA --access-profile operator bridge --transport http --port 1578 --token-file NEW_PROTECTED_TOKEN_FILE
```

The token file is never overwritten. Configure the client's supported private
bearer-header facility locally, not a committed secret. Additional audience-bound
credentials use `access tokens issue --for-access-profile operator --port 1578
--out NEW_PROTECTED_TOKEN_FILE --json`. Revoke a selected credential with
`access tokens revoke --token-id TOKEN_ID --json`; profile identity stays stable.
Do not use internal `access.credentials.issue` as a credential-export shortcut.
Named profiles cannot administer profiles, tokens or protected owner logs.

Finite media deadlines remain required for scoped callers. An explicit disabled
deadline is protected-owner-only; never request it under an operating profile.
Protected state/runtime/ingest/log paths are not media targets. Preserve live
state after version/store conflicts; stopped-root generation migration is a
separate explicitly authorized owner maintenance workflow with its own reviewed
plan, expiry, exact receipt identity and unfinished rollback/package boundaries.
