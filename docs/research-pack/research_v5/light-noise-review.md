# Bounded review of FilmCraft light/noise routes and their EffectCraft aliases

## Scope and application

Reviewed the 9 FilmCraft `recipe_family=light` rows and 4 `recipe_family=noise` rows in the current v5 CSV against the already archived, pinned source bodies. No donor family outside those 13 rows was rescanned. The existing cross-donor alias table identified the small related EffectCraft set; four alias rows need corresponding corrections.

Applied `light-noise-corrections.json` by `source_id`, separately under its `filmcraft` and `effectcraft` keys, after concurrent CSV edits finished. Values are replacement values for existing columns. The canonical noise alias and compact Markdown index were updated with the same semantics. The patch changes **11 FilmCraft rows and 4 EffectCraft rows**. It adds no rows and changes no source IDs, target capability IDs, design dispositions, status fields, counts, or donor-identity requirements. The two FilmCraft rows reviewed without a route correction are `bevel_alpha` and `lighting_effects`.

All proposed routes remain **implementation designs awaiting CI qualification**. No application, FFmpeg graph, media fixture, GPU test, or build ran during this review. In particular, the precision-preserving sampling graph described below is not an asserted runtime success or speed result.

## Confirmed corrections

| FilmCraft source ID | Concrete problem in the prior row | Selected correction and focused output oracle |
|---|---|---|
| `turbulent_displace` | Noise generation, LUT and blend never relocated source pixels. The row promised directional, gradient/bulge and twist behavior without specifying how those maps reached a sampler. | Build the admitted vector field and actually sample source RGBA. Qualify native `displace` for its precise map domain; use a bounded packed-source/guide `geq` sampler outside that domain. A coordinate-coded grid plus controlled vector guide must move to independently predicted source coordinates. |
| `cell_pattern` | The common Perlin route produced continuous octave noise, while the promised effect was nearest-site cellular structure. A parameter-difference note did not repair the executable route. | Compute F1/F2/nearest-site ID from a fixed neighborhood in `geq`, then derive the selected cellular mode. Compare a tiny known site layout against exhaustive distance calculations, including cell boundaries and tile edges. |
| `vignette` | Generic add/screen composition cannot implement the required darkening or the signed source-to-color/white interpolation. | Define a shaped border weight and signed interpolation, with preserved alpha. Keep natural native `vignette` gain as a named variant with different falloff. Gray corners must darken or brighten according to sign while the center remains unaffected. |
| `volumetric_rays` | A procedural radial light mask is not a radial accumulation of the actual image highlights. The template did not define any source sampling. | Threshold source highlights, then perform the finite weighted point-to-center gather in `geq`, with at most 48 taps. A moved source highlight must move its shaft; a black frame must produce no rays. |
| `echo_glow` | The generic light recipe omitted the actual repeated blur scales and geometric decay. | Extract highlights once; use 1-12 bounded radii and normalized geometric weights. Isolate one level and verify the multilevel result against an independently weighted sum. |
| `wonder_glow` | Three named source-derived scales were promised, but no three-scale accumulation appeared in the route. | Extract/tint highlights once, blur at the three declared radii, and mix the explicit weights. Single-nonzero-weight fixtures isolate each scale. |
| `glint` | The template could draw a star-shaped lamp rather than streak existing highlights; “directional blur” did not identify an executable primitive or its sampling limit. | Use finite directional sampling of the highlight image, with 2/4/6/8 rays and bounded taps; a small admitted convolution can be an equivalent qualified route. An impulse must form streaks at the declared angles and threshold must suppress a dim impulse. |
| `lens_flare` | The shared template blurred the distinction between authored flare geometry and source-highlight effects. It did not bind ghosts to the authored-center/frame-center axis. | Define the authored core, halo, optional streak and bounded ghost geometry independently of source highlights. A black frame may receive the flare; moving the authored center changes its core/ghost axis. |
| `light_leaks` | The generic mask/threshold description did not distinguish seeded authored washes from source-derived glows or identify the selected lighter Numera variant. | Specify bounded seeded analytic blobs with drift and an entry-edge envelope. Record that this is a named Numera variant of the donor's fractal wash, not its FBM identity. A black frame can receive the wash, with deterministic seed/time behavior. |
| `noise`, `noise_legacy` | The shared noise family conflated sample grain with Perlin/octave texture and left correlated monochrome noise and unclipped source headroom unspecified. | Generate a native noise residual at grain resolution, use one residual for mono or independent RGB residuals for color, and composite in the declared transfer/precision. A direct noise route is only an admitted optimization for matching clipping/format semantics. Check equal mono channel residuals, spatial grain size, alpha and explicit overflow modes. |

The existing bevel route already extracts alpha derivatives and shades them by light direction. The lighting route already derives an optional luminance bump and evaluates a bounded number of light fields before source shading. Their remaining implementation gates are ordinary kernel/format/alpha qualification; this review found no reason to replace those routes with another generic template.

## Important alias distinction: “unclipped” is not shared behavior

The pinned FilmCraft noise implementation leaves out its explicit clamp when clipping is disabled. The pinned EffectCraft `ec.noise.noise` implementation instead applies modulo-one wrapping in that branch. A common `clip=false` field would silently conflate them. The corrected descriptor therefore names **clip**, **preserve headroom**, and **wrap unit interval** separately. EffectCraft's unclipped functional variant selects wrapping; FilmCraft's selects headroom preservation. The random sequence still does not need to match either donor. [FilmCraft noise](https://github.com/storytold/filmcraft/blob/d643a886650bfbf16f24762274054575aa1e8f20/crates/render/src/vfx/color.rs#L332-L367) · [EffectCraft noise](https://github.com/storytold/effectcraft/blob/9db7f3b21700fd821f215782ed1c82afe077880f/crates/effects/src/misc.rs#L448-L480)

The four EffectCraft corrections are:

| EffectCraft source ID | Reconciliation |
|---|---|
| `ec.distort.turbulentdisplace` | Use the same admitted coordinate sampler, map coding, bounds and precision contract as canonical turbulence. |
| `ec.generate.cellpattern` | Use the same bounded F1/F2/site contract. Remove permission to substitute a different noise category under a cellular identity; retain declared tiling/evolution differences. |
| `ec.stylize.ccvignette` | Keep the natural-lens preset and shaped signed-border variant explicitly different inside the shared descriptor. Define highlight protection and preserved alpha. |
| `ec.noise.noise` | Preserve the explicit modulo-wrap variant and correlated monochrome semantics, while retaining the direct native route for compatible clipped sample noise. |

The inspected `ec.generate.lensflare` and `ec.noise.addgrain` routes already describe authored flare geometry and residual grain composition respectively. They need no contradictory-route correction here. No gradient, color-emboss or other color-family row is touched.

## Reachability and bounds of the selected recipes

### Source-plus-guide sampling

`geq` samples its own input image; it cannot magically read an independent second video input. The fallback explicitly solves that data path:

1. Construct one synchronized guide strip with the required vector components using FFmpeg plane/format filters.
2. Stack it below the original source in the **same admitted pixel format and width**. Check padded height, plane strides, alpha layout and total queued bytes before admitting the graph.
3. In `geq`, read guide components from that strip, compute the target source coordinates, apply the declared boundary operation to **each interpolation tap against the original source rectangle**, and sample associated source color and alpha at the same coordinates. Qualify integer lookups plus explicit bilinear weights where necessary; set `alpha_expr` explicitly. Folding only the center coordinate can allow an edge footprint to read the guide strip.
4. Crop the result back to the original extent. Cheap branches can skip expensive guide-only sampling expressions, but admission still charges the full padded output allocation and iteration, plus stack/crop and queue costs.

The implementation must qualify the negotiated stack/plane formats and memory envelope; a filter name alone is not sufficient. Its direct `displace` optimization must likewise use actual map neutral, scale, sampling and out-of-range behavior. The manual's 8-bit example uses 128 as the neutral displacement value. Native map quantization cannot silently narrow a request's amount or floating-point base. [displace](https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/doc/filters.texi#L12060-L12114) · [geq](https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/doc/filters.texi#L15087-L15234) · [vstack](https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/doc/filters.texi#L25940-L25960)

### Cellular neighborhood

The replacement uses one site constrained inside each lattice cell and a fixed 5x5 candidate window. This is a deliberate bounded canonical placement rule; it is not a promise to reproduce a donor's jitter, RNG or HQ supersampling. With sites constrained to their own cells, the central and a nearer adjacent cell provide two candidates closer than any cell outside that window. The local search therefore covers F1 and F2 for the selected rule. The graph computes those distances and site identity in FFmpeg expressions; Rust only constructs the fixed expression and parameter constants. Expression scratch state resets per pixel and does not depend on slice traversal order. [FilmCraft cellular implementation](https://github.com/storytold/filmcraft/blob/d643a886650bfbf16f24762274054575aa1e8f20/crates/render/src/vfx/stylize.rs#L571-L635) · [EffectCraft site search and modes](https://github.com/storytold/effectcraft/blob/9db7f3b21700fd821f215782ed1c82afe077880f/crates/effects/src/generate2.rs#L79-L180) · [FFmpeg expression operations](https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/doc/utils.texi#L799-L1020)

### Light fields

The seven changed light rows now distinguish authored light fields from source-derived highlight fields. Multi-radius glows use named native blur and mix stages. Directional/radial gathers use finite FFmpeg sampling expressions with hard candidate/tap bounds and a declared guide-resolution policy. No new Rust renderer, image sampler, noise generator or DSP engine is introduced. Ordinary source format, alpha, framesync, memory and interface-parity gates remain required; all new focused assertions are folded into those existing suites.

## Source reading record

The following archived material was read for this finite review; the source revisions are also present in the JSON patch:

- FilmCraft `crates/render/src/vfx/lights.rs`: echo glow, directional streak helper/glint, light leaks, radial rays, wonder glow and lens flare bodies.
- FilmCraft `crates/render/src/vfx/distort.rs`: turbulent displacement body; `vfx/stylize.rs`: cell pattern body; `vfx/color.rs`: noise and bounded lighting bodies.
- FilmCraft `crates/render/src/effects.rs`: bevel-alpha branch; `gpufx.rs`: vignette construction and CPU evaluation branch; associated effect definitions where needed.
- EffectCraft `distort2.rs`: turbulent body/spec; `generate2.rs`: cellular search, pattern body and flare geometry; `stylize2.rs`: vignette body; `misc.rs`: full noise body including its overflow branch; relevant alias descriptors.
- Pinned FFmpeg `doc/filters.texi`: `noise`, `perlin`, `displace`, `geq`, `vignette`, `gblur`, `blend`, `mix`, `maskedmerge`, `mergeplanes`, `vstack`, scaling/coverage references; `doc/utils.texi`: expression functions and local state. The archived `vf_geq.c` format list confirms that the selected source includes floating-point RGB/alpha entries; it does not qualify the complete fallback graph.

No new broad research, media execution, implementation claim or completion percentage is inferred from this source review.
