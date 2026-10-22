# Continuation — Numera Wiz

Objective: implement the full revision 7 mandate in public KiaroSama/Numera-Wiz,
on the single branch implementation/numera-wiz-v1. Do not merge/close anything.
Main has only GitHub's initial README bootstrap, commit
80ec75ea5a97285276c4173534d1c73de02466ed.

Completed internal outputs: official Spec Kit 1.2.2 initialization/constitution/spec/
plan/task artifacts; preserved 808-member research ZIP content and source hashes;
exact-time/strict-command/MuxCls selection/tag/order domain implementation and
meaningful regression assertions. These are not completed end-user capabilities.

Build and ordinary tests run only in GitHub Actions. No local app build, FFmpeg
transformation or owner GPU run has been executed. Full scope and artifact gates
remain open in tasks.md. Current source tree is a development candidate, not a
complete portable media product. Consult completion.json for explicit statuses.

Current review: draft PR #1. The domain suite passed on all three hosted targets.
The expanded development workspace compiled/tests passed on Linux/macOS at commit
932c1ce5cdc327c63385a975acc75407b26c8ca2; Windows had conditional dead-code lint
findings, now corrected. The first real FFmpeg fixture build completed but exposed
a missing wrapped_avframe decoder for lavfi fixture generation; the configuration
now enables it. The actual CPU media test passed in run 39874893026.
The new raw-JSON/CLI/stdio/HTTP tests and store primitives must retain their own
actual CI results; no result is inferred from the earlier domain tests.

Next executable step: inspect/fix the current hosted native/WASM/media results,
retain the final resolved lockfile and apply the exact CI formatting patch, then
implement the protected owner/redb/authorization/publication integration T010-T020
and close E03-E06 together. Preserve
actual result/run/artifact identities when updating evidence; do not restart a
whole donor survey. Owner GPU testing waits for matching CI-built artifacts.
