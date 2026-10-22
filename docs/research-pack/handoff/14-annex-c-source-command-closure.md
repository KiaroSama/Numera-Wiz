> Generated view of [Numera-Wiz-Implementation-Mandate-EN.md](../Numera-Wiz-Implementation-Mandate-EN.md), revision 7.0. Main SHA-256: `126c8aa8aa7f874802a09600040a1986cff650ad70b70f58f30a441d87fbef06`. Normative text is unchanged; fragment links point to the complete directive.

## Annex C. Source-command closure

Pinned FilmCraft revision: `d643a886650bfbf16f24762274054575aa1e8f20`. Research date: 2026-10-21. This is an implementation design and source-behavior mapping; no application was built, run or tested.

### What is closed

**Every one of the original 926 inventory rows now has a concrete canonical operation or explicit ordered operation set, typed design parameters, functional behavior, disposition, route, limitations, source URL and CD01–CD34 acceptance journey.** No row has an unassigned target or a generic execute placeholder. The source spelling is a provenance key; Numera does not need to expose 926 legacy commands. The CSV is the complete per-row deliverable.

**The source review also found 16 real registrations missing from the earlier extractor.** `keyboard::commands()` extends `target_toggles()`, whose local `tt!` macro defines `timeline.toggleTargetV1`–`V8` and `timeline.toggleTargetA1`–`A8`. These are appended as a separately labeled supplement, sharing `ui.timeline.targeting.update`. Thus this ledger contains **942 source definitions/derived IDs**. Registry assembly consumes `sequence.applyDefaultTransitionsToSelection` into `trim.applyDefaultTransition`, giving **941 source-resolved public entries in this inventory**, including disabled entries; this is not a compiled or enabled runtime count.

[Track-target source registrations](https://github.com/storytold/filmcraft/blob/d643a886650bfbf16f24762274054575aa1e8f20/crates/engine/src/keyboard.rs#L814) · [Consumed registration helper](https://github.com/storytold/filmcraft/blob/d643a886650bfbf16f24762274054575aa1e8f20/crates/engine/src/project_tools.rs#L210).

The exact canonical IDs in this CSV supersede illustrative family names in the earlier domain table. In particular use `projects.*`, `audio.profiles.*`, `presets.*`, `graphics.templates.*`, `capture.*`, `media.frame.export`, and `effects.add/update/remove`. The mandate determines security, persistence, lifecycle and FFmpeg admission; this ledger does not replace those contracts. The FFmWiz F01–F15 workflows and effect capability mappings remain independent completion obligations.

### How to implement these rows

Implement one Rust command descriptor and handler per meaningful typed operation, and parameterized aliases for numbered cameras, track targets, labels, directions, views, panels and fixed increments. A semicolon-separated target denotes an explicit macro or typed dispatch to existing operations, not a generic endpoint. For example quick export invokes `timeline.render` then `jobs.submit`; an empty GUI dialog invocation is not an empty machine mutation. Machine clients must supply stable IDs, time, parameters and authorization.

`canonical_parameters` is a field-level design contract, not a claim that a generated JSON Schema already exists. Implement the exact Rust schema, units, enum/domain bounds, defaults, errors and examples under the mandate. Parameters shown as alternatives describe one typed operation; `source_action_binding` records different meanings of source aliases sharing it. Frozen presets/preferences and client-resolved toggles must be part of the actual normalized request. Never reevaluate a toggle on retry.

The CSV is a flattened human-readable design inventory. `project_id` maps to `project.id` in the canonical request envelope. `expected_revision` maps to `project.expected_revision` only when it is the optimistic precondition for that project; neither is repeated under `params`. Qualify other revision shorthand by its owning resource: for example, a shortcut profile's `profile_id; expected_revision` becomes command-specific `params.profile_id; params.expected_profile_revision`. A save's captured `revision` remains `params.revision`. Session generation and sequence fields belong to the session contract. Only command-specific arguments go under `params`, and no unrelated resource revision masquerades as a project revision. Duplicate or conflicting context fields are rejected. The shared schema generator owns this normalization for every interface.

P/S surface labels and the source `session` disposition describe access/placement, not durability. The invoked command descriptor declares `query`, `bootstrap`, `durable_mutation` or `volatile_session` according to its actual behavior. Selection, seek, playback, viewport and transient drag updates use authenticated session generations and bounded sequence acknowledgments without one persistent receipt per event. Saved shortcuts, preferences and workspaces remain durable; the final project edit from a gesture remains durable. A row that selects between inspecting, transiently applying and saving a profile dispatches explicitly to the relevant typed contract. Macros preserve each child operation's class and documented atomic boundary. There is no blanket rule that all 371 source session rows are volatile or that every P operation changes the project.

A project edit commits one revision/undo unit through the worker. A processing intent returns an immutable plan; `jobs.submit` accepts durable execution. A job that generates media and later inserts it into a project has two honest outcomes: the verified media artifact and a separate revision-checked placement/attachment edit. A project change during the job does not mutate its accepted snapshot or permit an attachment to a stale/deleted asset.

P operations have native GUI, browser GUI, independent CLI, stdio `bridge`, and HTTP MCP parity. S operations address a named client/profile: headless callers can inspect/change declared presentation data, but native focus, browser fullscreen, OS clipboard and microphone permissions still require an actual capable host. Return an explicit client/gesture/unavailable result. Underlying media/project workflows remain callable without a window.

### Dispositions

| Disposition | Rows | Meaning |
|---|---:|---|
| `alias` | 149 | A fixed parameter/action binding to an existing operation; legacy spelling need not be public. |
| `branding` | 9 | Numera-owned onboarding/help/about behavior replaces donor-specific marketing/demo identity. |
| `macro` | 11 | A named composition or explicit typed dispatch of existing operations; one undo unit where the row says so. |
| `operation` | 389 | A meaningful shared application/media operation, with exact fields and limits in its row. |
| `private_helper` | 2 | A consumed registration or owned-clock helper; preserve public behavior without inventing another public endpoint. |
| `qualified_operation` | 9 | A supported workflow whose automatic/provider-dependent processing requires the actual admitted FFmpeg route and component evidence. |
| `session` | 371 | Presentation/profile action with an explicit client or saved-profile target. |
| `source_unavailable_extension` | 2 | The source registration is disabled; Numera keeps honest unavailability or implements the explicit native extension, without claiming donor runtime support. |

Unsupported independent systems are identified separately in `independent_system_disposition`, rather than deleting an entire useful workflow because one donor engine is unsuitable. This specifically excludes copying donor DSP/renderers, Kokoro runtime/download semantics, a proprietary plugin host, donor cloud accounts and Flash/broadcast runtimes. Private donor project/preset interchange remains optional.

### All 34 command domains

| Domain | Source rows | Canonical operation examples |
|---|---:|---|
| CD01 `application.preferences_cache` | 7 | `settings.schema`, `cache.inspect`, `cache.prune`, `labels.presets.list`, `labels.presets.import`, `labels.presets.export`, `labels.presets.remove` |
| CD02 `editing.audio_mixer_automation` | 25 | `mixer.inspect`, `mixer.strips.update`, `mixer.controls.update`, `automation.gestures.begin`, `automation.gestures.end`, `automation.record.start`, `automation.record.stop`; 15 further operation IDs in the CSV |
| CD03 `editing.audio_profiles` | 9 | `audio.profiles.inspect`, `audio.profiles.assign`, `audio.profiles.clear`, `audio.profiles.update`, `audio.profiles.apply`, `audio.profiles.save`, `audio.profiles.remove`; 2 further operation IDs in the CSV |
| CD04 `editing.bins_asset_views` | 28 | `ui.asset_views.get`, `ui.asset_views.update`, `ui.asset_columns.list`, `ui.asset_columns.update`, `assets.query`, `ui.asset_view_presets.list`, `ui.asset_view_presets.save`; 16 further operation IDs in the CSV |
| CD05 `editing.captions` | 21 | `captions.tracks.add`, `captions.tracks.remove`, `captions.tracks.update`, `captions.styles.update`, `captions.add`, `captions.split`, `captions.merge`; 9 further operation IDs in the CSV |
| CD06 `editing.clipboard_import` | 1 | `assets.import_and_place` |
| CD07 `editing.clips` | 45 | `sequences.create_from_assets`, `bins.add_from_items`, `assets.offline.add`, `projects.close`, `projects.save_many`, `clips.attributes.apply`, `clips.attributes.reset`; 16 further operation IDs in the CSV |
| CD08 `editing.collect_consolidate` | 1 | `projects.package_plan` |
| CD09 `editing.color` | 15 | `sequences.color.update`, `assets.interpretation.update`, `color.spaces.list`, `presets.list`, `presets.apply`, `presets.preview_plan`, `assets.color.inspect`; 6 further operation IDs in the CSV |
| CD10 `editing.effect_presets` | 7 | `presets.list`, `presets.save`, `presets.apply`, `presets.remove`, `presets.rename`, `presets.export`, `presets.import` |
| CD11 `editing.graphic_templates` | 17 | `graphics.templates.list`, `graphics.templates.apply`, `graphics.templates.export`, `graphics.templates.import`, `graphics.templates.remove`, `graphics.controls.update`, `graphics.controls.inspect`; 9 further operation IDs in the CSV |
| CD12 `editing.graphics` | 49 | `graphics.layers.add`, `graphics.layers.align`, `graphics.layers.distribute`, `graphics.layers.reorder`, `ui.selection.navigate`, `graphics.layers.reset`, `clips.duration.reset`; 7 further operation IDs in the CSV |
| CD13 `editing.masks_tracking` | 12 | `masks.add`, `masks.remove`, `masks.update`, `masks.vertices.update`, `masks.transform`, `masks.vertices.add`, `masks.vertices.remove`; 4 further operation IDs in the CSV |
| CD14 `editing.media_browser` | 10 | `assets.browser.roots`, `assets.browser.list`, `ui.media_browser.update`, `ui.selection.set`, `ui.media_browser.favorites.update`, `ui.media_browser.recents.clear`, `ui.media_browser.settings`; 3 further operation IDs in the CSV |
| CD15 `editing.multicam` | 40 | `multicam.angles.cut`, `multicam.angles.select`, `clips.synchronize`, `assets.merge_av`, `multicam.create`, `multicam.clips.update`, `multicam.flatten`; 8 further operation IDs in the CSV |
| CD16 `editing.music_remix_qualified_analysis` | 4 | `audio.remix.plan`, `audio.remix.inspect`, `audio.remix.revert` |
| CD17 `editing.narration_and_qualified_tts` | 7 | `narration.voices.list`, `narration.preview_plan`, `narration.create_plan`, `narration.update_plan`, `narration.inspect`, `narration.render_plan`, `components.install` |
| CD18 `editing.notes_metadata_events` | 6 | `ui.events.list`, `ui.events.clear`, `assets.metadata.inspect`, `assets.metadata.update`, `projects.notes.get`, `projects.notes.update` |
| CD19 `editing.project_sequence_core` | 183 | `projects.create`, `projects.create_from_template`, `sequences.create`, `bins.add`, `assets.generators.add`, `assets.generators.update`, `assets.import_example_pack`; 93 further operation IDs in the CSV |
| CD20 `editing.project_tools_interchange` | 29 | `bins.search.add`, `assets.query`, `bins.search.update`, `bins.remove`, `ui.sequence_tabs.update`, `projects.close_many`, `projects.templates.save`; 19 further operation IDs in the CSV |
| CD21 `editing.proxy_ingest` | 7 | `proxies.create_plan`, `proxies.attach`, `proxies.detach`, `assets.relink`, `previews.source_policy.update`, `presets.list`, `projects.ingest.update` |
| CD22 `editing.relink` | 10 | `assets.availability.scan`, `assets.availability.inspect`, `assets.relink`, `assets.refresh`, `assets.replace_source`, `assets.relink_plan`, `assets.search_files`; 1 further operation IDs in the CSV |
| CD23 `editing.sequence_tools` | 6 | `transitions.apply_defaults`, `audio.mix_peak_normalize.plan`, `transcripts.generate_plan`, `sequences.simplify`, `ui.captions.visibility`, `markers.update` |
| CD24 `editing.sequence_tools_nested` | 26 | `timeline.reverse_match_frame`, `ui.navigation.seek`, `ui.timeline.settings`, `clips.join_through_edits`, `timeline.through_edits.list`, `sequences.extract_subsequence`, `tracks.remove`; 5 further operation IDs in the CSV |
| CD25 `editing.transcript_and_qualified_asr` | 14 | `transcripts.generate_plan`, `transcripts.update`, `transcripts.remove`, `transcripts.inspect`, `transcripts.search`, `components.list`, `components.install`; 5 further operation IDs in the CSV |
| CD26 `editing.voice_capture` | 4 | `capture.settings`, `capture.start`, `capture.synchronize`, `capture.stop` |
| CD27 `media.analysis_scene` | 1 | `analysis.scenes.plan` |
| CD28 `media.analysis_scopes` | 1 | `analysis.scopes.request` |
| CD29 `media.exports_jobs` | 19 | `presets.list`, `presets.inspect`, `presets.save`, `presets.remove`, `presets.favorite`, `presets.import`, `presets.export`; 9 further operation IDs in the CSV |
| CD30 `presentation.dock_workspaces` | 28 | `ui.panels.show` |
| CD31 `presentation.menus_dispatch` | 139 | `ui.layout.update`, `ui.panels.close`, `ui.locale.update`, `previews.transport.update`, `ui.timeline.viewport.update`, `ui.appearance.update`, `previews.settings.update`; 23 further operation IDs in the CSV |
| CD32 `presentation.shortcuts` | 16 | `shortcuts.list`, `shortcuts.inspect`, `shortcuts.update`, `shortcuts.remove`, `shortcuts.history.undo`, `shortcuts.history.redo`, `shortcuts.conflicts`; 9 further operation IDs in the CSV |
| CD33 `presentation_and_edit_dispatch` | 82 | `ui.navigation.seek`, `ui.navigation.nested`, `ui.selection.at_time`, `ui.selection.navigate`, `clips.extend_nearest_edit`, `clips.move`, `clips.move_tracks`; 12 further operation IDs in the CSV |
| CD34 `presentation_and_edit_dispatch` | 73 | `ui.layout.update`, `ui.window.update`, `ui.focus.navigate`, `ui.focus.set`, `ui.workspaces.apply`, `ui.monitors.update`, `previews.transport.update`; 18 further operation IDs in the CSV |

### Source semantics that must survive implementation

| Source behavior | Binding implementation decision |
|---|---|
| `metadata.set` | Updates undoable project log fields. It is not a media-container tag rewrite. FFmWiz metadata remux is a separate processing intent. |
| `edit.pasteMedia` | Imports and sequentially places assets as one undo unit; free-track placement searches outward and adds tracks, while insert mode ripples. Machine input is authorized assets, not a hidden clipboard. |
| `sequence.normalizeMixTrack` | Normalizes sample peak in dBFS, with bounded remeasurement for post-fader processing. It is not LUFS normalization. |
| `sequence.simplify` | Creates a copy with explicit selected cleanup options; leaves the original sequence unchanged. |
| `export.queue.stop` | Stops future dispatch while the active encode finishes. `jobs.cancel` is separate. Retry creates a new linked job. |
| `clip.volumeUp` and relatives | Shift both the base Volume value and every Volume keyframe by the dB delta, rather than inserting one point at the playhead. |
| Slip/nudge/selected End | Slip Left advances source content; Move/Slide Left moves earlier on the timeline. Selected End seeks the last frame before exclusive Out. |
| `textPanel.delete` / `rippleDelete` | Transcript tab routes to lift/extract; Captions tab routes to ordinary caption clear/ripple delete. Machine requests name the typed target. |
| `sequence.applyDefaultTransitionsToSelection` | Source assembly consumes this definition into `trim.applyDefaultTransition`; implement the selected-edge behavior once. |
| `clip.restoreCaptionsFromSource` | Donor is always disabled. Numera may implement its qualified FFmpeg subtitle-extraction route; never describe this as observed working donor behavior. |
| `file.closeAllOtherProjects` | Donor is always unavailable with one project. Do not invent multi-project windows just to satisfy a dead menu item; return NOTHING_TO_CLOSE when applicable. |
| Project Manager | Collect copies media; consolidate transforms used ranges plus handles. Preserve nested/time mappings, publish relocated project last, and never delete originals implicitly. |
| Voice-over | Keep capture-start/preroll versus retained record-point timing and one final placement edit. Synthetic input is explicit CI-only fixture behavior, never a silent real-recording fallback. |
| Multicam | Decode only the current grid page, initially at most 16 cells; stable angle IDs survive display order changes. |

### Automatic features and precise limits

Manual transcript import/edit/captions, manual masks/keyframes and explicit musical edit lists remain usable without optional analysis components. Qualified ASR must disclose its real segment/word timing and diarization output; an FFmpeg Whisper filter name does not establish all donor transcriber options. TTS provider voice IDs/languages/model downloads are not interchangeable: donor Kokoro downloads cannot be represented as if they were flite voices.

Music remix means rearranging beat-aligned segments with preserved intro/outro and controlled crossfades, reporting the actual duration error. `atempo` alone does not fulfill it. Automatic beat/structure analysis needs a concrete admitted route; manual/imported beat grids support the editing/render path. Avoid the donor whole-clip audio and quadratic similarity allocation.

FFmpeg `find_rect` can support a qualified rectangular-template position-tracking subset. Rotation, scale and deforming path tracking remain unavailable until supported by a real admitted route; `vidstab` global stabilization is not equivalent. Automatic color matching similarly needs a real bounded route and honest limits; manual LUT/curves/reference comparison stays available.

Graphic templates preserve text/layer/style/timing/constraint editing and FFmpeg-native composition. They do not authorize a custom graphic rasterizer or Adobe/MOGRT host. Each rich-text/vertical-text/shape capability must use the native rendering subset stated by its descriptor, with conversion/unavailable reporting.

### Dynamic menu helpers outside fixed registrations

The menu source also dispatches recent-project entries by generated position (`file.openRecent.<index>`), `file.clearRecent`, and `help.shortcuts`. These are not counted as extra fixed registry definitions. Preserve the workflows with `projects.recent.list`, `projects.recent.clear`, an ordinary authorized `projects.open` of a resolved recent entry, and `ui.dialogs.open(dialog=shortcuts)`. A recent path never grants new filesystem authority, and a positional menu ID is not a stable project ID. [Pinned menu routing](https://github.com/storytold/filmcraft/blob/d643a886650bfbf16f24762274054575aa1e8f20/crates/ui-egui/src/menus.rs#L569).

### Acceptance and evidence status

Every CSV row includes its CD01–CD34 end-to-end acceptance journey. Implement registry-generated schema/parity coverage plus the independent output/edit oracles required by the mandate. Cases sharing one handler may be parameterized, but cover their distinct action bindings and failure domains. A renamed row, a disabled button or a source count is not acceptance.

The generator checked only artifact structure: all original source IDs retained, all 16 supplemental IDs linked to actual `tt!` registrations, all rows assigned, 34 domains, required fields, canonical plural project names, and absence of unassigned/generic-execute targets. These are document-integrity checks, not application tests or proof of FFmpeg admission, interoperability, performance, security or release readiness. All such implementation evidence remains a CI/qualified owner-hardware obligation.


