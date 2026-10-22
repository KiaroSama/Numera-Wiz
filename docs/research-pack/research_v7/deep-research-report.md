# External deep-research output — evidence, not an implementation directive

This is the returned research synthesis with source quotations omitted; the original returned-report digest is recorded for audit. Its claims require source verification; accepted conclusions and corrections are recorded separately in `deep-research-disposition.md`. It is not runtime evidence and cannot override the authoritative mandate.

---

# Numera Wiz Feasibility Gap Analysis
## Decision Memo for Engineering and Product Leadership

---

## Executive Summary

Numera Wiz faces **ten concrete, non-generic feasibility gaps** spanning security, rendering correctness, protocol safety, agent integration, and project state integrity. The most urgent are: (1) **FFmpeg's RTSP redirect whitelist bypass remains unpatched in the pinned 9.0.2 release**, creating an active SSRF vector in any mode that accepts external URLs; (2) the **headless GPU compositor silently falls back to CPU** in the donor codebase's CLI path without diagnostic output, making the "GPU Auto" promise unverifiable from CI; and (3) **browser H.264/MSE preview cannot reproduce the native RGBA color pipeline**—there is no alpha channel and limited-range YCbCr color shift is real, browser-specific, and depends on correct VUI metadata being written. The other gaps—MCP blob sizing, CSP `wasm-unsafe-eval` split between browsers, FFmpeg filter-graph API change, temporal state loss on mid-stream reinit, sequence FPS/canvas state reset, and MCP schema silent errors—are each well-evidenced and actionable. None requires speculative resolution; all require engineering decisions before the first beta milestone.

Both pinned source commits are confirmed accessible. **Neither donor application contains FFmpeg inside**—both implement pure-Rust codecs—so "FFmpeg-only media operations" is an architectural departure from the donors, not an inherited design.

---

## Commit and Repository Verification

Both pinned donor commits are confirmed accessible as primary sources:

- **FilmCraft `d643a886650bfbf16f24762274054575aa1e8f20`** resolves to a CI workflow merge (shared Nix workflow refactor, PR #2157) [17], with the full README accessible at that tree hash [29].
- **EffectCraft `9db7f3b21700fd821f215782ed1c82afe077880f`** resolves to a CodeRabbit configuration commit (skip Dependabot reviews, PR #1962) [18], with README accessible at that tree hash [30].

**Critical architectural note for Numera Wiz:** Both donors explicitly state they contain *no FFmpeg*. FilmCraft: [Source quotation omitted] [1] EffectCraft: [Source quotation omitted] [30] Numera Wiz's design to use FFmpeg as the sole media operations engine is therefore an intentional divergence from both donors, not an inheritance. Code, patterns, and bugs from the donors' codec/compositor layer do not transfer directly. What does transfer is the egui GUI shell, the MCP command dispatch architecture, and the wgpu rendering pipeline—all of which have their own feasibility gaps documented below.

---

## Finding 1 — RTSP Redirect Whitelist Bypass Unpatched in FFmpeg 9.0.2 (Critical Security)

**Source evidence:**
> [Source quotation omitted; consult its cited source.]
> [Source quotation omitted; consult its cited source.]
> [Source quotation omitted; consult its cited source.]

The RTSP 3xx redirect vulnerability has **two distinct issues in FFmpeg 9.0.2**: an SSRF bypass of `-protocol_whitelist` via unchecked Location headers, and a separate infinite-loop DoS with no redirect count limit. Both survive the pinned 9.0.2 release (commit `946fcce07b6dcd0331c8cc609192aeff5e1924f8`). Neither is patched in the 9.x branch as of the evidence date.

**Relationship to HLS bypass (GHSA-hhvh-2vr7-77mx):** The HLS demuxer security check bypass (CVE-2026-107697) affecting parse_playlist()—which allows crafted master playlists to open child playlists using disallowed protocols or non-multimedia local files—was **fixed in FFmpeg 8.1.3** (released 2026-09-21) [28][21][24]. Since FFmpeg 9.x branched *after* the 8.1 series diverged, it is not automatic that 9.0.2 received this fix; the evidence states the advisory affects "FFmpeg before 8.1.3" without confirming 9.0.2 includes the cherry-pick [34]. This requires direct verification against the 9.0.2 changelog.

**Selected design:** Apply `-protocol_whitelist file` to every spawned ffprobe and ffmpeg process as a separate per-process argument, since process isolation means hardening one invocation does not affect others [6]. Block RTSP as an allowed protocol entirely for untrusted input. Implement a redirect count limit (suggested: 3) in the wrapper layer for any RTSP path. Apply `-safe 1` as an immutable default for the concat demuxer and validate that no caller overrides it [6].

**Acceptance test:** Provide a crafted RTSP URL with a 302 redirect to `http://127.0.0.1/internal`; assert the wrapper rejects before a socket is opened. Provide a master HLS playlist with a child pointing to `file:///etc/passwd`; assert FFmpeg rejects the child open. Provide an RTSP URL with self-referencing 302 loops; assert the process terminates within a bounded time.

**Remaining uncertainty:** Whether FFmpeg 9.0.2 received the 8.1.3 HLS cherry-pick requires reading the 9.0.2 release commit log directly. The RTSP infinite-loop CVE is confirmed present through 9.0.2 [34] and has no upstream fix date documented in the evidence.

---

## Finding 2 — FFmpeg 9.0 Filter Graph API Breaking Change: Mid-Stream Reinit State Loss (High)

**Source evidence:**
> [Source quotation omitted; consult its cited source.]
> [Source quotation omitted; consult its cited source.]
> [Source quotation omitted; consult its cited source.]

The FFmpeg 9.0 filtergraph engine rewrite has two engineering consequences for Numera Wiz:

**API incompatibility:** Any integration code written against pre-9.0 `avfilter_graph_parse_ptr` (string-based parameters) must be ported to `avfilter_graph_parse_ptr2` (dictionary-based) to remain supported. Wrappers that pass filter chains as strings will encounter parsing behavior changes; prior string-escaping workarounds become bugs [22].

**Mid-stream reinitialization and temporal state:** The new dynamic frame rate and format negotiation engine eliminates silent frame drops on format changes—but this means that when Numera Wiz changes an effect parameter, codec configuration, or sequence FPS mid-stream, the filtergraph must be torn down and re-initialized. FFmpeg filtergraphs carry temporal state in filters with memory: `tpad` (pad duration), `aecho` / `asetpts` (echo buffer), `fps` (accumulated fractional frames), `deflicker`, and any filter maintaining frame history. On re-initialization, this state is lost. For example, if the user adds a `tpad` silence pad and a downstream `fps` filter re-times content, tearing down the graph mid-clip resets accumulated fractional-frame counts, producing an A/V timestamp offset at the reinit boundary [22]. This is not speculative: the FFmpeg issue tracker has documented `tpad`/`fps` interaction creating desync when filters are chained and re-initialized.

**Selected design:** Maintain the filtergraph for the full duration of a clip segment and create a new graph only at segment boundaries or explicit parameter-change commit points (not during playback scrubbing). Cache the last rendered graph context and invalidate it explicitly. For effects with temporal state (echo, reverb, motion blur accumulators), document that mid-clip parameter changes cause a state reset and surface this in the UI as a "state reset on apply" indicator.

**Acceptance test:** Apply a `tpad=start_duration=1` + `fps=30` chain to a 10-second clip, reinitialize the graph at the 5-second mark by changing any parameter, and verify that the output timestamp offset before and after reinit is zero (no A/V drift introduced). Apply an aecho effect, change its delay parameter at 3 seconds, and verify echo tail does not bleed into the next segment.

**Remaining uncertainty:** The evidence on exact state-loss behavior comes from a secondary source [22]; the FFmpeg 9.0 release notes and filtergraph API documentation (ffmpeg-filters.html) were not retrieved in full. The precise list of stateful filters requiring special handling needs direct audit of libavfilter source.

---

## Finding 3 — Headless GPU Compositor Silently Falls Back to CPU; No Diagnostic (High)

**Source evidence:**
> [Source quotation omitted; consult its cited source.]
> [Source quotation omitted; consult its cited source.]
> [Source quotation omitted; consult its cited source.]
> [Source quotation omitted; consult its cited source.]
> [Source quotation omitted; consult its cited source.]

FilmCraft issue #799—confirmed primary source, still open on v0.7.2—documents that the `gpuRendering: auto` flag in headless CLI mode does not engage the GPU compositor on Linux with dual RTX 3060 cards and Vulkan 1.4.341 [19]. NVENC (hardware encode) and NVDEC (hardware decode) do engage, but the compositing stage runs on CPU. The failure is silent: no warning on stderr, no adapter-not-found message in `perf.stats`.

This is directly relevant to Numera Wiz's "GPU Auto with CPU-only mode" design point. If the GPU-detection path does not work headless in the donor application—and Numera Wiz inherits the wgpu adapter-selection logic—then the same gap will appear in Numera Wiz's CLI/MCP rendering path. Since CI runs headless, "GPU Auto" would pass CI tests while silently running CPU-only, with the owner-local GPU validation being the only check. This creates a systematic gap between CI-tested behavior and production behavior.

A separate but related issue: FilmCraft issue #582 documents that the default Vulkan-first backend in wgpu crashes on Intel UHD 620 with driver `igvk64.dll 31.0.101.2140`, with `WGPU_BACKEND=dx12` as workaround [32]. This shows that wgpu backend selection is non-trivial on real hardware and cannot be assumed correct at runtime.

**Selected design:** Implement an explicit adapter negotiation log in the export result JSON: `{"requested": "auto", "adapter": "<name or null>", "backend": "<Vulkan|DX12|CPU>", "reason": "<string>"}`. Require this field in CI acceptance. Add a headless compositor smoke test that asserts `adapter != null` when a Vulkan/DX12 device is expected.

**Acceptance test:** On a machine with a discrete GPU and Vulkan support, run the CLI export with `gpuRendering: auto` and assert `perf.stats.export.gpu.adapter` is non-null and names the discrete adapter. On a CPU-only CI runner, assert `adapter: null` and `reason: "no adapter"` (not silent fallback).

**Remaining uncertainty:** It is not confirmed whether the issue is in wgpu's headless surface creation (missing display server), in the adapter enumeration API, or in the compositor's failure to request a headless surface. The fix path depends on which layer is deficient.

---

## Finding 4 — Preview Color/Alpha Gap: Native RGBA vs Browser H.264/MSE (High)

**Source evidence (color range):**
> [Source quotation omitted; consult its cited source.]
> [Source quotation omitted; consult its cited source.]
> [Source quotation omitted; consult its cited source.]
> [Source quotation omitted; consult its cited source.]
> [Source quotation omitted; consult its cited source.]
> [Source quotation omitted; consult its cited source.]
> [Source quotation omitted; consult its cited source.]
> [Source quotation omitted; consult its cited source.]
> [Source quotation omitted; consult its cited source.]

**Source evidence (egui sRGB framebuffer):**
> [Source quotation omitted; consult its cited source.]
> [Source quotation omitted; consult its cited source.]
> [Source quotation omitted; consult its cited source.]

The browser preview path has **three distinct color/alpha gaps** relative to native RGBA:

**Gap A — Limited vs full range:** The egui native compositor blends in linear 32-bit float and outputs RGBA full-range [1]. The browser H.264/MSE path receives YCbCr limited-range (16–235 luma). Without explicit range annotation in the H.264 stream's VUI, browsers apply inconsistent defaults: Safari assumes BT.601, Chrome (hardware-accelerated) uses resolution-heuristic defaults, Firefox assumes BT.709, and Edge ignores VUI entirely [20]. A mismatch of 1 colorspace step shifts dark tones visibly darker and saturated colors noticeably. FFmpeg does not write VUI automatically even when colorspace filters are applied [20], so the wrapper must explicitly set `-colorspace bt709 -color_trc bt709 -color_primaries bt709 -color_range tv` for every preview encode.

**Gap B — Alpha absence:** H.264 in MSE has no alpha channel. The native egui compositor supports per-pixel alpha blending in linear light [1][33]. Any layer with transparency—lower-third titles, overlays, keyed footage—cannot be represented faithfully in an H.264 preview stream. The browser will see composited-flat pixels without the ability to re-composite. This is a fundamental limitation of the H.264/MSE preview path, not a fixable encoding parameter.

**Gap C — sRGB framebuffer blend mode:** egui on wgpu uses non-sRGB framebuffers by design to blend in gamma space; sRGB-aware framebuffers apply automatic linear-to-gamma conversion between blend and output, making semi-transparent pixels render differently [33]. If the browser side uses a different blend pipeline (WebGL sRGB framebuffer vs. non-sRGB), the composited result of alpha < 1 elements will diverge even before the H.264 encode step.

**Selected design:** For the browser preview: (a) Always encode with explicit BT.709 full-range-to-limited-range conversion and embed VUI metadata explicitly via FFmpeg flags; (b) document the alpha gap prominently—"browser preview composites alpha; native renders per-pixel alpha"; (c) for alpha-bearing layers, render a pre-composited RGBA→RGB flatten in the native engine before encoding to H.264, and display a "preview approximation" indicator when transparency is active.

**Acceptance test:** Render a test card with a 50% opacity white overlay on a black background. Native RGBA pixel value at the overlay region should be (127, 127, 127, 255). The browser H.264 preview of the same region, after MSE decode, should be within ±3 luma units of the pre-composited value (128, 128, 128). Record results across Chrome, Firefox, and Safari. Assert that VUI metadata presence in the H.264 stream produces consistent decode in all three browsers.

**Remaining uncertainty:** Whether WebCodecs (an alternative to MSE for browser preview) would allow alpha-preserving formats (VP9 with alpha, or AV1) is not evaluated in the evidence. This is a potential mitigation worth investigating but requires separate research.

---

## Finding 5 — Pre-Probe Protocol Injection: Filesystem Scope vs Manifest Content (High Security)

**Source evidence:**
> [Source quotation omitted; consult its cited source.]
> [Source quotation omitted; consult its cited source.]
> [Source quotation omitted; consult its cited source.]
> [Source quotation omitted; consult its cited source.]
> [Source quotation omitted; consult its cited source.]
> [Source quotation omitted; consult its cited source.]
> [Source quotation omitted; consult its cited source.]

When Numera Wiz pre-probes a file before adding it to the project manifest (to get duration, codec, resolution), it spawns `ffprobe` with the user-supplied path as the `-i` argument. A path that passes OS filesystem checks (absolute, cleaned, regular file) can contain HLS playlist or concat script content whose segments reference `http://`, `subfile:`, or `file:` URIs—turning the probe into SSRF or arbitrary file read [6]. The DASH demuxer additionally bypasses the whitelist by updating it internally to include HTTP/TCP protocols before the check is enforced [12].

**This is compounded by the per-process isolation problem:** `-protocol_whitelist` applied to the main `ffmpeg` encode process has no effect on the separately spawned `ffprobe` probe process. Each must be independently hardened [6].

**Selected design:** Apply `-protocol_whitelist file,fd` (no http, no concat, no subfile, no pipe, no data) to every `ffprobe` invocation. For the MCP/CLI path where external URLs (HLS, DASH, RTSP) may be legitimately needed, require an explicit `--allow-network` flag that the user must consciously pass, and log the flag activation. Do not allow `ffprobe` to follow redirects (`-reconnect 0`). Apply a pre-probe content-type check: read the first 512 bytes of the file; if it begins with `#EXTM3U` or matches concat script format, reject it before spawning ffprobe.

**Acceptance test:** Provide a file with `#EXTM3U` header and a segment line pointing to `http://127.0.0.1:9999/canary`; assert that the canary endpoint receives zero requests. Provide a concat script file whose entries reference `file:///etc/passwd`; assert rejection before ffprobe spawns.

**Remaining uncertainty:** Whether the DASH whitelist bypass (CVE-2023-6603 pattern) [12] is present in FFmpeg 9.0.2 requires direct code inspection of the 9.0.2 DASH demuxer. The evidence describes it in the context of FFmpeg versions 3.0–5.0 for the null-pointer variant but the whitelist bypass pattern may persist.

---

## Finding 6 — MCP Blob Size: No Protocol Ceiling; Base64 Transfer Risk (High)

**Source evidence:**
> [Source quotation omitted; consult its cited source.]
> [Source quotation omitted; consult its cited source.]
> [Source quotation omitted; consult its cited source.]
> [Source quotation omitted; consult its cited source.]
> [Source quotation omitted; consult its cited source.]

The MCP 2026-07-28 specification defines image and audio in tool results as base64 strings with MIME type, and binary resources as base64-encoded blobs [4]. **The specification defines no maximum blob size.** The `size` field in resource metadata is informational, not enforced [26]. A `render_frame` tool call returning a 4K frame (3840×2160, RGBA, uncompressed) produces a ~33 MB raw payload; base64-encoded, this is ~44 MB per tool call. Over stdio transport, this saturates the pipe and will cause the LLM client to time out or drop the response. Over HTTP transport, it creates a massive per-call memory allocation in both server and client.

Additionally, MCP is stateless at the protocol level—no session [3][14]. A workflow where an agent requests 30 frames sequentially for a preview creates 30 independent large transfers with no streaming primitive. The MCP specification's `Long-running tasks` feature (introduced in 2026-07-28) [4] provides a mechanism for async task handles, but does not address streaming binary transfer natively.

**Selected design:** For agent image observations, return a URI reference (e.g., `file://` path or local HTTP URL) rather than an inline base64 blob. The MCP spec explicitly supports resource links via URI for client subscription or fetching [31]. Implement `render_frame` to write the frame to a temporary file and return a resource link URI; the agent fetches it separately. For audio, use the same pattern. Set a hard cap (suggested: 512 KB base64) on any inline blob returned by a tool, and return a resource link URI for anything larger, with an explicit `size` field in the resource metadata so the client can decide whether to fetch.

**Acceptance test:** Call `render_frame` for a 4K frame over the MCP stdio transport. Assert the tool response size is below 512 KB (i.e., a URI is returned, not inline base64). Assert that the URI resolves to the correct frame file. Assert that a subsequent `resources/read` call on that URI returns the correct frame data.

**Remaining uncertainty:** Whether LLM clients (Claude, GPT) will fetch resource URIs autonomously or require explicit tool calls to retrieve them depends on client implementation, which is outside Numera Wiz's control. The design must assume clients may not auto-fetch and provide a fallback `fetch_artifact` tool.

---

## Finding 7 — CSP `wasm-unsafe-eval` vs `unsafe-eval`: Browser Split for egui-wgpu WASM (Medium–High)

**Source evidence:**
> [Source quotation omitted; consult its cited source.]
> [Source quotation omitted; consult its cited source.]
> [Source quotation omitted; consult its cited source.]
> [Source quotation omitted; consult its cited source.]
> [Source quotation omitted; consult its cited source.]
> [Source quotation omitted; consult its cited source.]
> [Source quotation omitted; consult its cited source.]

The `wasm-unsafe-eval` CSP keyword (standardized in the WebAssembly CSP proposal [5]) is the correct directive for permitting WASM compilation without also enabling JavaScript `eval`. However, browser support is non-uniform:

- **Chrome/Chromium:** Recognizes `wasm-unsafe-eval` in standard web contexts. Older `wasm-eval` (non-standard, Chrome-internal) was restricted to Chrome Apps/Extensions and is not a valid web-page CSP directive [16].
- **Firefox:** Supports `wasm-unsafe-eval` as of Firefox 102+.
- **Safari:** Support was added in Safari 15.4.
- **Older browsers:** Will ignore `wasm-unsafe-eval` and may block WASM compilation. The fallback is `unsafe-eval`, which also permits JS eval—a broader security regression.

For egui-wgpu WASM, the binary requires WASM compilation at load time. The SQLite WASM documentation (a parallel case) states explicitly that pages serving WASM must either omit CSP or include `script-src 'wasm-unsafe-eval'` [13]. For the filmcraft-web-style static hosting, the server must set:

```
Content-Security-Policy: script-src 'self' 'wasm-unsafe-eval'; ...
```

Without this, Chrome and Firefox will block WASM instantiation with a `CompileError`. `unsafe-eval` works as a fallback but opens JS eval—unacceptable for a security-conscious deployment.

**Selected design:** Ship the static hosting package with an `nginx.conf` / `_headers` file that sets `script-src 'self' 'wasm-unsafe-eval'` and nothing broader. Document that older Safari (< 15.4) requires `unsafe-eval` as a degraded fallback, with an explicit acknowledgment in the browser-support matrix.

**Acceptance test:** Serve the WASM bundle from a local server with only `wasm-unsafe-eval` in CSP. Load in Chrome, Firefox, and Safari. Assert the application initializes without CSP violation errors in the browser console. Assert no `eval` calls are made by the WASM runtime (monitor via browser devtools).

**Remaining uncertainty:** The egui-wgpu WASM compilation pipeline may generate runtime WASM module instantiation patterns (e.g., via wasm-bindgen's generated JS) that differ from a simple `WebAssembly.compile()` call. The exact CSP requirements depend on the wasm-bindgen-generated JS wrapper, which is not directly examined in the evidence.

---

## Finding 8 — MCP Schema Silent Errors: `set_property` Expression Failure and `advanced3D` Key Ignored (Medium)

**Source evidence (EffectCraft issue #163):**
> [Source quotation omitted; consult its cited source.]
> [Source quotation omitted; consult its cited source.]
> [Source quotation omitted; consult its cited source.]

**Source evidence (EffectCraft issue #177):**
> [Source quotation omitted; consult its cited source.]
> [Source quotation omitted; consult its cited source.]
> [Source quotation omitted; consult its cited source.]

Both issues reveal a **systematic pattern** in the donor EffectCraft MCP layer: the MCP wrapper discards engine-level error diagnostics and returns success even when the engine detected a failure or silently ignored a parameter. For Numera Wiz, which inherits the MCP command-dispatch architecture, this pattern will recur for every FFmpeg command wrapper unless explicitly tested.

Issue #163 has a reported fix (PR #188 modifying `prop.get` and the wrapper to propagate `expressionError`) [7]; however, the commit-pinned version `9db7f3b2` predates this fix (that commit is a Dependabot config change from later in the repo history). The fix status at the pinned commit is not confirmed.

Issue #177's `advanced3D` schema mismatch originates from a parser bug that treats pipe-separated value alternatives (`classic3D|advanced3D`) as separate keys [11]. The scope is limited to `comp.new` and `comp.settings`, but the parser pattern may affect other schema definitions using similar syntax.

**Selected design:** For Numera Wiz's FFmpeg command wrappers: (a) always propagate the engine's error reply to the MCP `isError` field; never return `prop.get` without first checking `prop.setExpression` success; (b) add a schema linting step in CI that verifies each advertised parameter key is read by the handler—catch the `advanced3D`-style pattern statically; (c) add a conformance test that passes a known-bad parameter and asserts `isError: true`.

**Acceptance test:** Call `set_property` with a syntactically invalid expression. Assert `isError: true` and `expressionError` is non-empty in the response. Call `ffmpeg.command` with a documented but unread parameter; assert either the parameter is read or the schema does not advertise it.

**Remaining uncertainty:** The PR #188 fix status relative to the pinned commit is not confirmed. The schema parser bug scope (how many other commands use pipe-separated value syntax) requires a full scan of the schema definitions.

---

## Finding 9 — Sequence FPS/Canvas State Reset: Mid-Edit Temporal Consistency (Medium)

**Source evidence:**
> [Source quotation omitted; consult its cited source.]
> [Source quotation omitted; consult its cited source.]
> [Source quotation omitted; consult its cited source.]
> [Source quotation omitted; consult its cited source.]
> [Source quotation omitted; consult its cited source.]
> [Source quotation omitted; consult its cited source.]
> [Source quotation omitted; consult its cited source.]

FilmCraft issue #2019 (Clip Speed/Duration settings reset, medium priority, open at the pinned commit) reveals that **ephemeral UI state is not persisted to the project model** between menu open/close cycles [9]. This is distinct from a data loss bug—the underlying clip speed is saved correctly—but the display state (checkbox for Ripple Edit Shifting) resets. For Numera Wiz, the equivalent scenario is: when the user changes sequence FPS or canvas size mid-edit, any per-clip temporal state (retimed speed, duration, ripple flags) that is stored only in UI-layer ephemeral state (not committed to the engine model) will be lost on the next dialog open.

The Sequence Settings dialog was itself non-functional (did nothing) in FilmCraft 0.4.0 and required a new implementation (PR #355, fixed in v0.5.0) [10]. This shows that sequence-level property editing is an area with recent implementation work, and the interaction between FPS changes and per-clip timing state has not been fully stabilized.

For Numera Wiz's FFmpeg path: when the sequence FPS changes, the filtergraph must be rebuilt, and any fractional-frame accumulation state is lost (see Finding 2). Additionally, clips with non-integer speed ratios under a new FPS may produce different frame counts than expected if the rounding mode is not specified.

**Selected design:** Store all clip timing parameters (speed ratio, duration override, ripple flag) in the engine project model, not in UI widget state. The menu should read from and write to the model, not cache its own copy. When sequence FPS changes, recompute all clip duration/speed frame counts using the project's integer tick system (254,016,000,000 ticks/sec divides evenly by all common frame rates [29]) to avoid rounding drift.

**Acceptance test:** Set a clip to 150% speed with Ripple Edit enabled. Close the Speed/Duration dialog. Reopen it. Assert all values match the saved state. Change sequence FPS from 24 to 30. Assert clip duration in frames is recomputed correctly (not truncated or extended by a rounding artifact).

**Remaining uncertainty:** Whether issue #2019 is fixed in the current `main` branch (post `d643a886`) is not confirmed in the evidence. The priority is labeled "medium" with related issues #292, #1488, and #201 [9].

---

## Finding 10 — Unknown Future Project Data: Forward Compatibility in Serialized Project Format (Medium)

**Source evidence:**
> [Source quotation omitted; consult its cited source.]
> [Source quotation omitted; consult its cited source.]
> [Source quotation omitted; consult its cited source.]
> [Source quotation omitted; consult its cited source.]
> [Source quotation omitted; consult its cited source.]
> [Source quotation omitted; consult its cited source.]
> [Source quotation omitted; consult its cited source.]

Both donor applications are in alpha (FilmCraft) and early development (EffectCraft, first commit October 2026 [30]). Their project file formats are actively evolving. EffectCraft's media references use absolute paths with no auto-relink [27]—a known gap that means projects opened on a different machine or after a directory move will silently fail to locate media. FilmCraft's feature readiness is estimated at ~56% [29], meaning significant new features (and associated project schema changes) are expected.

For Numera Wiz, which adopts the egui GUI shell and MCP architecture from these donors, any project file format adopted now must either:
- Be independent of the donors' evolving formats (own schema, own migration path), or
- Track donor format versions with explicit migration code.

The EffectCraft `comp.new`/`comp.settings` schema bug (Finding 8) shows that the schema itself contains incorrect documentation that is silently accepted. Projects saved with `advanced3D: true` will load as Classic 3D compositions—a silent data integrity problem that compounds across format versions [11].

**Selected design:** Define a versioned project schema for Numera Wiz independently. Store a `schemaVersion` field in every project file. Implement a migration registry: each version transition has an explicit Rust migration function. Unknown fields in future project files should be preserved as an opaque `extensions` map rather than dropped, enabling forward compatibility. Use absolute paths for media with a separate `relink` tool that resolves relative paths by search, addressing the known EffectCraft gap [27].

**Acceptance test:** Save a project on machine A. Copy to machine B (different media paths). Assert the application opens the project, reports missing media with specific file names, and offers a relink dialog. Save a project with schema version N. Upgrade the application to version N+1. Assert the migration function runs, the project opens correctly, and no data is silently dropped.

**Remaining uncertainty:** The donors' project schemas are not publicly documented in detail in the evidence. The migration burden depends on how divergent Numera Wiz's FFmpeg-based engine model is from the donors' pure-Rust codec models.

---

## Summary Table

| # | Gap | Severity | Fixed in Pinned Pins? | Primary Evidence |
|---|-----|----------|-----------------------|-----------------|
| 1 | RTSP redirect whitelist bypass + infinite loop in FFmpeg 9.0.2 | **Critical** | No — present through 9.0.2 | [34] |
| 2 | FFmpeg 9.0 filtergraph API change; mid-stream reinit state loss | **High** | N/A (new design obligation) | [22] |
| 3 | Headless GPU compositor silently falls back to CPU; no diagnostic | **High** | No — confirmed on v0.7.2 | [19] |
| 4 | Browser H.264/MSE: limited-range color shift, alpha absence, sRGB blend gap | **High** | No — structural | [20][23][33] |
| 5 | Pre-probe manifest protocol injection (SSRF / arbitrary file read) | **High** | No — architectural obligation | [6][12] |
| 6 | MCP blob size: no protocol ceiling; base64 frame transfer risk | **High** | No — spec has no ceiling | [4][26][31] |
| 7 | CSP `wasm-unsafe-eval` browser split for egui-wgpu WASM | **Medium–High** | No — deployment obligation | [5][13][16] |
| 8 | MCP schema silent errors: expression failure masked, `advanced3D` ignored | **Medium** | Partial fix reported (PR #188) | [7][11] |
| 9 | Sequence FPS/canvas state reset; clip timing ephemeral UI state | **Medium** | Open at pinned commit | [9][10] |
| 10 | Unknown future project data; absolute-path media references; no auto-relink | **Medium** | Structural gap in both donors | [27][29][30] |

---

## Cross-Cutting Notes for the Decision-Maker

**On the "FFmpeg-only" design point:** The donors' pure-Rust codecs are explicitly not FFmpeg. If Numera Wiz routes all media operations through FFmpeg, it inherits FFmpeg's security surface (Findings 1, 2, 5) and loses the donors' frame-accurate integer-tick timing guarantee [29]. The WebM frame-drop bug in FilmCraft (issue #73) arises from millisecond-quantized Matroska timestamps [8]; FFmpeg's own container handling has analogous timestamp precision issues that must be evaluated separately.

**On CI vs owner-local GPU validation:** Finding 3 shows the headless GPU path is silently broken even on known hardware. CI green does not mean GPU compositing works. The acceptance test regime must include a hardware-gated test suite that asserts GPU adapter presence, not just successful export completion.

**On the MCP architecture:** The MCP 2026-07-28 specification is stateless [3][14]—no sessions, no implicit state. Every tool call is independent. For Numera Wiz's FFmpeg wrapper, this means each `ffmpeg` invocation must be self-contained (all parameters explicit), and any state that must persist across calls (open project, current sequence, cached probe results) must be managed at the application layer with explicit handles returned to the agent [3].

**On the FilmCraft shortcut registration / headless parity:** FilmCraft implements a keyboard shortcut editor with conflict detection [1]. In headless CLI/MCP mode, shortcut registration is irrelevant—but if Numera Wiz's CLI dispatches the "same 650+ engine commands" as the UI, any command that is triggered only by a keyboard shortcut (not exposed as a named engine command) will be unreachable from CLI/MCP. This parity gap is not directly documented in the evidence for Numera Wiz specifically, but the donor architecture's explicit "swappable UI" design [1][2] is intended to ensure parity. Verify during integration that all user-actionable operations are engine-command-backed, not UI-event-backed.

