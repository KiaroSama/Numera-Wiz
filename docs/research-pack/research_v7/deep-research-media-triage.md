# Deep-research media triage

**Scope:** raw report lines 50–67, 94–124, and related summary claims. Source/document review only; no media execution. Retain the verified MF7 temporal, format-epoch and presentation contracts. The report does not supply new runtime proof.

## 1. Reject the claimed mandatory filter-API migration

Lines 53–59 rely on a secondary article to assert an `avfilter_graph_parse_ptr2` dictionary migration and broken CLI string graphs. That conclusion is unsupported by the inspected pin and irrelevant to the selected process-based integration. The [pinned manual still specifies textual CLI graphs and `avfilter_graph_parse_ptr`](https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/doc/filters.texi#L114-L122); the [actual CLI parser accepts `const char *desc` and invokes the segment parser](https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/fftools/ffmpeg_filter.c#L571-L608). Do not adopt a new binding/API or remove correct graph escaping on this evidence.

## 2. Keep state-loss risk; reject its causal claims and reset policy

Lines 61–65 incorrectly generalize that any parameter change requires teardown. Commands, expressions and topology changes have different handling. The proposed “state reset on apply” label does not make a reset satisfy exact rendering, and a segment boundary alone is not a safe reset point. Keep continuous/materialized state islands and reject unexpected epochs. The [pinned manual explicitly warns of lost state and buffered frames](https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/doc/ffmpeg.texi#L1366-L1380); [source also detects metadata/hardware-context changes](https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/fftools/ffmpeg_filter.c#L3073-L3183). Its “new engine eliminates silent drops” claim contradicts that evidence.

The examples are misclassified: [`tpad` pads video](https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/doc/filters.texi#L24307-L24324), not silence; [`asetpts` changes timestamps](https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/doc/filters.texi#L32460-L32471), not echo samples. The cited particular desynchronization assertion lacks primary evidence here. Its zero-offset/tail test is not a valid independent state-preservation oracle.

## 3. The report does not close temporal-prefix sampling

Its echo discussion concerns state reset, not the corrected tap target/clock. Retain MF7-01: [FilmCraft requests processed layers in timeline seconds and skips prior temporal nodes](https://github.com/storytold/filmcraft/blob/d643a886650bfbf16f24762274054575aa1e8f20/crates/render/src/vfx/mod.rs#L38-L121); [EffectCraft samples preceding effects](https://github.com/storytold/effectcraft/blob/9db7f3b21700fd821f215782ed1c82afe077880f/crates/effects/src/time_fx.rs#L166-L174). Numera’s complete acyclic prefix is an explicit design choice, not a conclusion established by this report.

## 4. Reject the universal native-blending claim and numeric oracle

Lines 114/116 call egui blending linear float; line 118 calls it gamma-space. The report conflates donor media composition with UI presentation and also assumes a wgpu pipeline despite the selected Glow seed. [`Color32` documents encoded-sRGB premultiplication and gamma-space operations](https://docs.rs/ecolor/0.36.2/ecolor/struct.Color32.html).

Line 122’s 50%-white-over-black result of 127/128 is not universal. Linear-light blending followed by sRGB encoding gives approximately 188; encoded-space blending gives approximately 128. The [W3C transfer functions](https://www.w3.org/TR/css-color-4/#color-conversion-code) establish the distinction. Define the compositing domain and compare in a common measurement domain; arbitrary “±3 luma” against RGB bytes is invalid.

## 5. Keep explicit color signaling; reject broad browser assertions

Treat Safari/Chrome/Firefox/Edge defaults and “FFmpeg never writes VUI automatically” in line 114 as unverified configuration/version claims. Tags alone do not perform range/transfer conversion. [WebGL explicitly separates conversion, alpha unpack and texture import](https://registry.khronos.org/webgl/specs/latest/1.0/#PIXEL_STORAGE_PARAMETERS). Retain the selected characterized conversion, explicit signaling and actual-browser qualification; do not infer guaranteed consistency from VUI presence.

## 6. Keep opaque audition; reject the transparency impossibility claim

An opaque H.264 transport cannot retain independent alpha, but it can present the engine’s correctly composited transparent layers over the selected background, subject to disclosed codec loss. Therefore lines 116/120 overstate that all transparent content is necessarily a semantic approximation. MF7’s shared preview-only composition addresses the actual boundary. No new WebCodecs/VP9/AV1 scope is justified.
