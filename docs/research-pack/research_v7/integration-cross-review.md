# Revision 6 integration cross-review

**Date:** 2026-10-22. **Scope:** read-only cross-review followed by root document corrections; no application, FFmpeg, browser, build or GPU execution.

The product, media and delivery contracts were reviewed against each other and the existing lifecycle/command contract before assembly. These findings are additional integration corrections to the 27 primary completeness findings. They do not introduce a competing architecture or an application success claim.

| ID | Concrete inconsistency | Selected resolution in the main mandate | Acceptance evidence still required |
|---|---|---|---|
| IC6-01 | A generic transition paragraph applied overlap-join duration arithmetic to existing timeline cuts. | Section 6 separates fixed-timeline two-sided/one-sided transitions from overlap-join utilities. A transition at an existing cut preserves placements and sequence extent; a utility join constructs new placements. | M6-J01: two five-second timeline clips with a one-second centered dissolve remain ten seconds; overlap join is nine seconds; one-sided fade leaves duration unchanged. |
| IC6-02 | Mapping every timed event through every source span would retime sequence-authored captions twice. | Section 6 records event owner/clock. Source events map through selected occurrences; sequence events use their sequence-edit participation and remaining nesting/output maps. | M6-J10: under a 2x source retime, sequence cue [1,2) stays fixed while source cue [1,2) maps to [0.5,1). |
| IC6-03 | A recovery scenario implied resuming cancelled/interrupted jobs despite immutable terminal states. | M6-J12 separates cancellation from owner failure, preserves completed artifacts/receipts, dispatches eligible queued work, and requires new linked retries for terminal work. Annex A's shorthand recovery references follow that same lifecycle. | M6-J12 and PJ12: no implicit rerun of completed/cancelled/interrupted children; linked retry has its own authorization and result. |
| IC6-04 | The old compact animation row retained only clip/sequence clocks after the detailed contract selected source/clip/sequence. | The compact row now refers to each property's declared anchor and the explicit hold policy. The observed FilmCraft default remains freeze_effect_clock=false. | PJ05 and M6-J01: source-anchored values survive move/split/slip/retime/hold without an accidental clock conversion. |
| IC6-05 | Managed-asset garbage collection omitted objects referenced only by a retained rollback generation. | Sections 5 and 8 put retained media under assets/<content-digest>/ and include retained rollback/recovery generations in the collection root set. | D6-J03 and PJ13: eligible cleanup preserves the old generation's media until all protecting references are deliberately retired. |
| IC6-06 | Whole-package dependency wording could require WP03 parity completion before the intake/compiler/editor tasks needed to complete it. | Section 11 distinguishes implemented foundation prerequisites from the integrated first-slice completion gate. | The Spec Kit task DAG has no circular dependency; the E03-E06 five-surface slice still closes as one capability. |
| IC6-07 | Generic token-file wording could imply unsupported automatic file dereferencing in MCP JSON. | Section 5 selects the named client's documented launcher/header mechanism, clean-environment setup, expiry/reissue and reconnect. | D6-J04: the documented launcher/config pair succeeds without a preexisting developer credential variable and fails clearly when missing. |

## Evidence for source-sensitive corrections

- FilmCraft's [transition model](https://github.com/storytold/filmcraft/blob/d643a886650bfbf16f24762274054575aa1e8f20/crates/project/src/lib.rs#L709-L737), [transition editing](https://github.com/storytold/filmcraft/blob/d643a886650bfbf16f24762274054575aa1e8f20/crates/edit/src/transitions.rs), and [add-transition implementation](https://github.com/storytold/filmcraft/blob/d643a886650bfbf16f24762274054575aa1e8f20/crates/edit/src/lib.rs#L1167) separate timeline span/partners from clip placement.
- FilmCraft's [caption model](https://github.com/storytold/filmcraft/blob/d643a886650bfbf16f24762274054575aa1e8f20/crates/project/src/caption.rs#L115-L133) and [caption lookup](https://github.com/storytold/filmcraft/blob/d643a886650bfbf16f24762274054575aa1e8f20/crates/captions/src/burn.rs#L232-L246) use sequence ticks for authored captions.
- The [frame-hold option](https://github.com/storytold/filmcraft/blob/d643a886650bfbf16f24762274054575aa1e8f20/crates/engine/src/clip_ops.rs#L1353-L1408) defaults holdFilters to false. The mandate retains that observed behavior explicitly rather than inferring it from a keyframe comment.
- The selected client's [official MCP configuration](https://code.claude.com/docs/en/mcp#environment-variable-expansion-in-mcp-json) documents header environment expansion. The file-reading launch step is Numera setup policy, not an automatic behavior attributed to arbitrary MCP clients.

The other resolutions are consistency decisions within Numera's selected lifecycle, retention and implementation workflow; they are not claims about upstream application defects. All runtime evidence remains pending.


## Revision 7 integration decisions

These are resolved specification interactions, not completed software tests.

| ID | Interaction | Selected resolution |
|---|---|---|
| IC7-01 | Named root-grant setup versus creator-scoped object access | Creation selects operate/owned-object access with an explicit finite ordinary-command allowlist; the built-in agent remains discovery-only. New command IDs need owner policy admission. |
| IC7-02 | Immutable reviewed plans versus submission by another principal | Hash normalized required resource/actions, retain creator policy as audit metadata, revalidate under submitter and never rewrite a captured plan or borrow creator credentials. Nontransferable dependencies require replan. |
| IC7-03 | Write-only export directory versus reading produced artifacts | Successful publication establishes an exact job-scoped byte-read purpose. Current job rights and exact publication identity are required; path replacement is ARTIFACT_CHANGED. |
| IC7-04 | Receipt-before-target replay versus explicit grant revocation | Own receipt identity/status survives target deletion; protected result access may return RECEIPT_RESULT_RESTRICTED. Stored results remain unchanged and the operation never runs again. |
| IC7-05 | Checkpoint file access versus live unsaved project access | New import creates creator state; current project grant attaches; otherwise only disk-snapshot inspection or explicit new-UUID clone is allowed. Writer exclusion remains. |
| IC7-06 | fd-only probe versus arbitrary options and rendering stdin controls | Reserve fd/format/protocol options and ordinary demuxer admission. Descriptor 0 is the ffprobe input profile only; render control stdin remains distinct. |
| IC7-07 | Unexpected-format rejection versus suppressing reinitialization diagnostics | Exact bundled route explicitly selects reinit_filter=1, drop_changed=0 and qualified context-aware info observation before retention caps. No general metadata-scanner claim. |
| IC7-08 | Unknown data round-trip versus newer semantic schemas | Supported opaque envelopes round-trip; unsupported majors/envelopes remain read-only; enabled unknown nodes block affected render closures. |
| IC7-09 | MCP error propagation versus successful failed-job inspection | A domain-command error remains a tool error; a successful status query can return a failed job as typed data. No ignored advertised parameter or source-occurrence lint as semantic proof. |
| IC7-10 | PowerShell scripts versus logging and clean protocol stdout | Authored multi-command examples use timestamped four-level stderr logging; JSON result pipelines and exit checks remain, and MCP clients still spawn clean protocol pipes directly. |
| IC7-11 | Operational profile bootstrap versus finite allowlist | Include own-principal access.replay_scope.create explicitly in the operational default set, with no access administration or other-principal scope authority. The first scoped durable-request journey must exercise it. |
| IC7-12 | Headless shortcut discovery versus crate dependency direction | Pure action definitions, default bindings and context resolver belong to nw-domain::registry. nw-ui::actions captures UI context, dispatches host actions and edits shortcuts without owning hidden descriptors. |
| IC7-13 | Retired command execution version versus retained receipt replay | Keep replay-only command/alias/digest metadata through unexpired scopes. A matching accepted receipt remains retrievable; new requests for a retired execution version fail without running an old handler. |
