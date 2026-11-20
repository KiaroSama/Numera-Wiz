> Generated view of [Numera-Wiz-Implementation-Mandate-EN.md](../Numera-Wiz-Implementation-Mandate-EN.md), revision 7.0. Main SHA-256: `126c8aa8aa7f874802a09600040a1986cff650ad70b70f58f30a441d87fbef06`. Normative text is unchanged; fragment links point to the complete directive.

## 7. Shared editor and shortcuts

#### Extraction boundary and visual reference

Adapt FilmCraft's Rust presentation and interaction code against the new service client. Its `ui-egui` crate directly depends on donor rendering, media, audio-DSP, GPU, export, text and TTS components; copying it unchanged would import another processing engine. Extract theme/docking/menus/panels/selection logic and useful pure edit algorithms, while replacing media work with Numera requests. CI checks that those donor processing crates do not enter either shipped native or browser dependency closures.

Use one `nw-ui` crate, one set of assets/theme/layouts, one editor read model and one action mapping for native and web. The native shell and Rust/WASM browser adapter supply platform file selection, transport and presentation APIs. No separate simplified web dashboard, WebView clone or second frontend implementation. At equal logical viewport, theme, project revision, selection and playhead, the visible panels, controls and interaction hierarchy must agree. Native window chrome, OS font rasterization and reserved shortcuts are documented platform differences.

Read the pinned source modules for `theme.rs`, `dock.rs`, `dock_shared.rs`, menus, state, keyboard and relevant panels before transplanting them. Use Numera identity, preserve applicable attribution and audit fonts/icons. Do not replace a required working panel with a static imitation. [Pinned GUI source](https://github.com/storytold/filmcraft/tree/d643a886650bfbf16f24762274054575aa1e8f20/crates/ui-egui)

#### Packaged browser boot and media policy

The first browser adapter is single-threaded Rust/WASM UI and same-origin fetch/MSE/WebGL presentation. FFmpeg processing stays in the native owner. Do not copy FilmCraft's in-browser media engine, AudioWorklet processing, OPFS recovery store or media worker assumptions into this adapter. The first adapter does not require SharedArrayBuffer, WASM atomics, COOP/COEP or a service worker. A later need for a worker or cross-origin isolation is a separately measured adapter change, not a reason to loosen the initial policy silently.

Serve the embedded `index.html`, external bootstrap module, wasm-bindgen glue, WASM and approved UI assets from the one selected loopback origin. The WASM response is `application/wasm`; JavaScript modules use a JavaScript MIME type; CSS uses `text/css`; every route sends `X-Content-Type-Options: nosniff`. The bootstrap and HTML use `Cache-Control: no-store`; content-addressed immutable assets may use immutable caching. Application API, authentication and private media responses are not stored by shared caches. No CDN, remote font, inline script, dynamic JavaScript evaluation or downloaded frontend bundle is required at launch.

The selected starting HTTP CSP is:

```text
default-src 'none'; script-src 'self' 'wasm-unsafe-eval'; script-src-attr 'none'; style-src 'self'; style-src-attr 'unsafe-inline'; img-src 'self' blob:; media-src 'self' blob:; connect-src 'self'; font-src 'self'; worker-src 'none'; object-src 'none'; base-uri 'none'; frame-ancestors 'none'; form-action 'none'
```

The style-attribute exception permits the controlled egui/browser adapter's dynamic canvas, input and accessibility positioning; it does not authorize inline JavaScript or untrusted HTML. Keep stylesheet elements external. `wasm-unsafe-eval` authorizes WASM compilation without allowing general JavaScript `eval`. Media blob URLs originate from authenticated same-origin bytes and are revoked on source replacement, session teardown and failed initialization. These permissions supplement exact Host/Origin validation and the existing pairing/bearer rules; they do not replace them. [WASM policy](https://developer.mozilla.org/en-US/docs/Web/HTTP/Reference/Headers/Content-Security-Policy/script-src) · [Streaming MIME requirement](https://developer.mozilla.org/en-US/docs/WebAssembly/Reference/JavaScript_interface/instantiateStreaming_static)

The asset router serves executable resources only from the embedded, hash-verified asset namespace. User media, project metadata and arbitrary artifacts cannot be served as active same-origin HTML, SVG documents, scripts or styles. Ordinary artifact downloads use attachment disposition and safe content types; preview endpoints expose only their admitted media MIME types. API JSON is never promoted to a script response. Surface a bounded bootstrap failure panel for missing/invalid assets, WASM compile failure, blocked WebGL or runtime panic; include an error code and reload path without printing credentials or project contents.

Qualify this composition early in the existing packaged browser gate, before broad panel migration: enforce the actual CSP from the packaged local server; load WASM by streaming; pair; edit; fetch a preview with authorization; append MSE segments; display the video in the shared canvas with overlays; hear audio after the required user gesture; and clean up after reload/disconnect. Verify wrong MIME, disabled required CSP permissions and executable-artifact attempts fail predictably. Native/WASM model parity and successful compilation alone are not evidence for this browser contract. Preserve the existing real-browser/Safari evidence policy.

**B7-J01 — Packaged browser boot and presentation (WP03/WP05/WP06):** load the real embedded app under its enforced CSP/MIME policy, pair, edit and play authorized MSE video/audio with canvas overlays in every required browser. Reject wrong MIME, foreign executable assets and disabled necessary permissions with the declared bootstrap error; prove cleanup after reload and disconnect.

#### Browser support and version qualification

The browser is a client of the local native engine and uses the same `nw-ui` code and feature registry as the desktop window. The initial required browser matrix is explicit:

| Supported desktop platform | Required browser families |
|---|---|
| Windows 11 x64 | Microsoft Edge stable and Google Chrome stable |
| Ubuntu 24.04 x64 | Google Chrome stable and Mozilla Firefox stable |
| macOS 15 Intel and ARM64 | OS-compatible Safari and Google Chrome stable, on both architectures |

These are qualification targets, not claims that tests have already passed. Before the first preview-adapter decision, CI resolves the selected browser releases and records exact browser version/build, distribution, OS version/build, architecture, runner-image identity, automation driver/version, rendering backend and negotiated playback codecs in a versioned qualification manifest. A release freezes and publishes the actual tested manifest; `stable` describes how candidates are selected and is not a floating release-test dependency. Requalify affected cases after a browser or OS change. Do not label an untested version, browser family or Linux distribution supported merely because it shares an engine.

Each required matrix entry must exercise the packaged local engine with shared-UI startup, authenticated media/file access, real audio/video playback, seek, reconnect, bounded buffering and an operation that creates an inspected output. Run the deeper engine-specific regressions on representative Chromium, Firefox and Safari entries, and use focused compatibility checks for the remaining entries. Record the actual host OS: Windows Server CI browser evidence does not become Windows 11 desktop evidence by relabeling it; use the existing owner desktop-launch qualification for the corresponding real-Windows check without creating a separate local test regime.

Safari support requires real Safari on the qualifying macOS target using its associated `safaridriver`. A WebKit automation build may provide useful engine regression coverage, but it is not Safari and does not qualify Safari's media, permissions or OS integration. Keep released-browser binaries and patched automation builds distinct for the other browser families too. Keep ordinary browser qualification in GitHub Actions; an unavailable real-Safari runner/test is a named pending release gate, not a reason to substitute a WebKit pass or move normal browser suites onto the owner's GPU machine. CI records actual codec/MSE support and verifies the selected audition route; a browser capability probe alone is not proof of successful playback. [Safari WebDriver](https://developer.apple.com/documentation/webkit/testing-with-webdriver-in-safari) · [Automation browser/build distinctions](https://playwright.dev/docs/browsers)

#### Open the application and reach real work

The first usable window offers **New project**, **Open project** and **Media tools** within the same shared FilmCraft-derived shell. A utility task does not require creating a timeline. Media tools expose the complete FFmWiz F01-F15 workflows through designed forms backed by the same command descriptors used by CLI/MCP. The window displays the selected engine/profile, connection state and pending background jobs without starting decoders, scanners or optional model runtimes merely to paint an empty workspace.

Opening an existing project shows the saved checkpoint revision separately from the recovered working revision when they differ. A missing-source/component report identifies affected items and available actions; it does not replace the project with an empty document. The user can inspect/edit unaffected data and use admitted proxies while unavailable originals remain visible. Export planning reports every relevant unresolved dependency before submission.

The native and browser windows use the same commands, read model, panels, layouts and task forms. A browser file picker, native picker, engine-root browser and authorized CLI path are ingress adapters to the same asset model. Their authority and ownership differ explicitly; their downstream editing semantics do not. Keep platform dialogs, browser-reserved keys, permission prompts and download destinations in the adapter boundary.

#### Workspaces, panels and everyday workflows

Preserve the nine reference workspaces: Editing, Assembly, Color, Effects, Audio, Captions and Graphics, Learning, Review and All Panels. Preserve the roles of all 28 reference panels:

Project; Media Browser; Libraries; Info; Effects; Markers; History; Source; Effect Controls; Audio Clip Mixer; Metadata; Program; Timeline; Tools; Audio Meters; Audio Track Mixer; Lumetri Color; Lumetri Scopes; Essential Graphics; Essential Sound; Text to Speech; Properties; Text; Events; Progress; Reference Monitor; Timecode; Project Notes.

Product-specific panel names can become neutral Numera names while retaining their roles. Component-dependent functionality has clear install/availability information. Standard required functions must work in the delivered release; endless unavailable placeholders are not completion.

The reference Editing layout places Source/Effect Controls/Clip Mixer/Metadata on the upper left, Program centrally, and Properties/Graphics/Text on the upper right. The lower area contains Project/Browser/Libraries/Info/Effects/Markers/History, Tools, Timeline and Meters. Source defaults are a 0.57 top/bottom split, 450-point inspector, 0.29 lower project share, 64-point tools and 116-point meters. Treat them as layout reference values, then clamp for smaller windows. Save validated split/tab/floating layouts per workspace with finite node/depth/geometry bounds; one corrupt layout must not destroy other saved workspaces. [Dock source](https://github.com/storytold/filmcraft/blob/d643a886650bfbf16f24762274054575aa1e8f20/crates/ui-egui/src/dock.rs)

Add a convenient task entry for FFmWiz utilities—convert, remux, cut, batch, tracks, metadata, subtitles and analysis—inside the same shell. Users must not create a timeline project to replace an audio track or inspect a file. Utility jobs and timeline exports share plans, queue, logs, capability discovery and command inspector.

Expose **Stream Cleanup (MuxCls)** as a designed Media tools entry in the native and browser shell. Its scan/inventory view, per-file selection preview, audio/subtitle selectors and ordering, tag patches, independent preservation policies, destination/batch options, exact plan review and job outcomes come from Annex D's shared typed contracts. Reuse ordinary panels/widgets and the common queue; do not embed a terminal prompt, launch the donor program or require a timeline. Every selectable processing option is equally available through CLI and both MCP transports.

The editor supports bins/import/relink/proxies; source/program monitors and In/Out; multitrack linked editing; all selected trim/ripple/roll/slip/slide operations; locks/sync locks/snapping; markers; nesting/multicam; undo/history; effect stacks/masks/keyframes; color/LUT/scopes; audio buses/mixers/automation; captions/graphics; supported interchange; jobs/export and recovery. The command-domain and effect tables define exact admitted subsets. Do not substitute an unrelated native effect and conceal the difference under a donor label.

#### Keyboard contract

FilmCraft Default combines command defaults, panel tables and only nonconflicting fallback bindings. It is not identical to choosing a Premiere preset. Preserve the effective resolver, context precedence, primary modifier normalization and text/IME handling. These anchor bindings are source-confirmed:

| Action | Reference default |
|---|---|
| Select; track select forward/backward | `V`; `A` / `Shift+A` |
| Ripple; roll; rate stretch | `B`; `N`; `R` |
| Razor; slip; slide | `C`; `Y`; `U` |
| Pen; hand; zoom; type | `P`; `H`; `Z`; `T` |
| Play/stop; shuttle | `Space`; `J`, `K`, `L` |
| In/Out; marker | `I`, `O`; `M` |
| Insert/overwrite | comma / period |
| Add Edit; snapping | primary-modifier + `K`; `S` |
| Ripple previous/next edit to playhead | `Q` / `W` |
| Match frame; select at playhead | `F`; `D` |
| Timeline zoom in/out/sequence | `=` / `-` / backslash |
| Export | primary-modifier + `M` |
| Enable clip in FilmCraft Default | `Shift+E` |
| Project / Source / Timeline / Program | `Shift+1` / `2` / `3` / `4` |
| Effect Controls / Track Mixer / Effects | `Shift+5` / `6` / `7` |
| Media Browser / Clip Mixer | `Shift+8` / `9` |
| Editing through Captions and Graphics | `Alt+Shift+1` through `6` |
| Learning / Review / workspace reset | `Alt+Shift+7` / `8` / `0` |
| Maximize focused / hovered panel | `Shift+backtick` / backtick |
| Shortcut editor | primary-modifier + `Alt+K` |

The actual effective source map is authoritative when platform/context details differ; do not force all macOS Ctrl/Cmd distinctions into one alias. The alternative Premiere-compatible preset uses a different Shift+E mapping. Generate platform maps and test them against the pinned resolver. Provide search, remapping, collision detection, reset and persistence. Browser-reserved keys have visible alternate bindings; every associated domain action remains accessible through menus and CLI/MCP. [Resolver](https://github.com/storytold/filmcraft/blob/d643a886650bfbf16f24762274054575aa1e8f20/crates/engine/src/shortcuts.rs) · [Preset tables](https://github.com/storytold/filmcraft/blob/d643a886650bfbf16f24762274054575aa1e8f20/crates/engine/src/shortcut_presets.rs)

#### Assemble FilmCraft's complete shortcut contract without starting its UI

The pinned reference assembles frontend commands inside `FilmcraftApp::new`; `Shortcuts::new` alone has no external descriptors. `menus::external_commands` adds UI command definitions, keyboard commands and generated panels. Default construction includes known-command filtering, ordered panel/default tables, conflict-free Premiere fallback and platform-specific Ctrl/Command handling. The focused-panel resolver wins before application bindings. These are inputs to extraction, not implementation details that can be discarded. [Registration](https://github.com/storytold/filmcraft/blob/d643a886650bfbf16f24762274054575aa1e8f20/crates/ui-egui/src/lib.rs#L538-L543) · [Descriptor assembly](https://github.com/storytold/filmcraft/blob/d643a886650bfbf16f24762274054575aa1e8f20/crates/ui-egui/src/menus.rs#L887-L898) · [Construction](https://github.com/storytold/filmcraft/blob/d643a886650bfbf16f24762274054575aa1e8f20/crates/engine/src/shortcuts.rs#L515-L627) · [Resolution](https://github.com/storytold/filmcraft/blob/d643a886650bfbf16f24762274054575aa1e8f20/crates/engine/src/shortcuts.rs#L943-L951)

Place pure action descriptors in the shared headless registry before command/keymap discovery. A descriptor may require a live client to execute a focus/fullscreen gesture, but listing its parameters/default keys must not initialize a frame server, window, GPU or audio device. Keep source action identity and context separate from the canonical handler: aliases sharing a handler may retain different fixed parameters or bindings.

Generate the full effective Default map for each supported platform from the assembled pinned definitions/resolver, then map each source action to its canonical Numera action and fixed arguments. Record source order, platform, context, chord, canonical action/arguments, host requirement, origin/default/fallback, source anchor and any unbound/collision reason. Apply source resolution before canonical alias consolidation. The existing full-map artifact must cover every bindable action, including those deliberately unbound. A static inventory is evidence of source coverage; only the assembled CI export establishes the reference effective map.

Keep browser-reserved chords as explicit host overrides with visible alternatives and the same canonical operation. Do not call a browser substitution the original FilmCraft chord. Persistent remapping uses its own saved-profile revision; a per-client focus/selection snapshot does not become an implicit global selection for an agent. The complete map is searchable and discoverable from every surface.

**P7-J02:** compare the assembled reference resolver with the generated Numera map for platform/context cases and key collisions. Assert full registry/map fingerprints across all clients; headless discovery starts no media or presentation subsystems. Preserve the already documented Default/Premiere `Shift+E` difference, panel precedence, aliases and intentionally unbound actions. This closes the export mechanism; it does not replace the existing full keyboard acceptance suite.

#### Concurrent interaction and contextual availability

Every long GUI gesture captures the project revision, stable selection, source/sequence clock, relevant target-state revision and initial values. Temporary previews are visibly transient and have bounded lifetime. A conflicting remote edit or source replacement invalidates the gesture. The UI refreshes the affected view and offers an explicit reapply/cancel decision; it never automatically resubmits a delta against different content. Disconnection cancels unsent volatile samples and preserves the last coherent view with the existing disabled-stale-mutation rule. Reconnection obtains a coherent snapshot/event cursor before accepting new edits.

Nonconflicting inspection, scrolling and playback controls remain usable where their session contract permits. Selections refer to stable IDs and are pruned/remapped when a committed operation reports deleted/replaced IDs. One drag or automation pass commits at most one logical undo item; rejection leaves the original project unchanged and removes its preview overlay. A separate client cannot acknowledge another client's outstanding control sequence.

The shared action read model distinguishes **available**, **requires selection/input**, **blocked by lock/conflict**, **requires permission/component/device**, and **unsupported on this client**. It includes safe reason codes and the next relevant action. Derive it from the same command validation and the exact target/resource/capability revisions. Treat it as advisory, not authorization; execution revalidates. Do not reveal inaccessible media paths or private target details in an availability explanation. CLI/MCP can ask the same context question using explicit IDs without borrowing GUI focus.

Generate a full effective FilmCraft Default keymap artifact containing platform, context, normalized key/chord, canonical action and fixed parameters, text/IME suppression rule, precedence, source evidence and browser alternate where needed. The existing anchor table is a readable introduction, not the complete binding set. Each visible action uses that map; remapping persists under the saved-profile contract. Keyboard cases must cover the same focused action with a text field active, the timeline active, a floating panel active and a browser-reserved conflict. A shortcut suppressed by the browser still has the same menu/action and typed headless operation.

#### Rendering, responsiveness and preview identity

UI rendering never waits synchronously for ffprobe, a media process, a long directory scan or a network request. The service-backed read model applies coherent snapshots/events. Disable stale mutations on disconnect and keep a visible connection state. The UI cannot quietly create a separate project engine when the local service is unavailable.

Virtualize large bins, effect lists, timeline clips, keyframes, thumbnails and waveform pages. Query visible intervals rather than paint every project element. Repaint on input/events/playback demand, with idle throttling. Keep texture allocation, decoded queues and asset caching within the budget. Scope/waveform media analysis comes from FFmpeg; Rust can aggregate already-produced bounded statistics for display without introducing a new audio DSP path.

Native and browser previews use the same overlay/widget geometry. The selected browser adapter uploads decoded MSE video into the shared canvas texture so selection handles, safe margins, masks, popovers, clipping and dock resize remain coherent. This is an early integration test and performance gate. Do not claim it is zero-copy. A lossy browser audition profile need not be pixel-identical to the raw native preview, but both must identify the same project/time/effects and expose their quality mode.

Compare actual native/browser screenshots at controlled viewport/DPI/theme and test interactions, focus, scrolling, detached/floating groups, maximized monitors, high DPI, keyboard navigation, IME and readable errors. Use stable reference scenes and meaningful structural comparisons; a platform antialiasing difference is not a broken command, and a matching screenshot is not proof of working media operations.

#### Export review, queue and delivery

One export review model captures sequence/project revision, exact range, original/proxy policy, selected output streams, codec/container, color/alpha, audio layout/loudness, subtitles/chapters, GPU/CPU policy, profile/preset revision, output destinations and overwrite policy. Show requested and admitted choices, estimated resources, warnings and the actual immutable plan/command inspector. The frozen range identifies whether it came from the full sequence, In/Out or an explicit selection; subsequent playhead/selection changes cannot modify a queued export. The media compiler's stated tail and frame/sample-grid policies remain authoritative.

Changing any semantic setting invalidates the reviewed plan. A stale source, profile or grant produces a specific recovery action: inspect changes and regenerate, or retain the prior accepted job's snapshot where already valid. It does not silently rewrite the reviewed command. Save a preset as portable processing intent without source paths, credentials or a previous job's resolved plan ID. Applying it resolves against the actual selected input/capability profile again.

Queue controls distinguish submit, pause future dispatch, resume dispatch, cancel a selected active job and retry as a linked new job. Display pending, active, verifying, publishing and terminal outcomes from the authoritative job service. The output list identifies each verified artifact, stream summary, size/hash and publication outcome; partial multi-output success remains explicit. Reveal/open/download is available only for an artifact the caller is authorized to access and whose current state supports that action.

The browser may download a verified artifact through an authenticated opaque resource stream with bounded transfer. This is distinct from engine publication to an explicitly granted native path. Closing the download tab does not cancel an already completed export or delete its published artifact. A failed download can be retried under current authorization without rerendering the media. The native UI, CLI and MCP use the same artifact identity and result metadata.


