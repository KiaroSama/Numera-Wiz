# Cross-donor capability identity and shared implementation contract

**Status:** source-informed design reconciliation; no application implementation or runtime execution. **Inputs:** the final FilmCraft/EffectCraft functional mappings and the clean `filmcraft-source-descriptors.json` / `effectcraft-source-descriptors.json` provenance records. Source URLs are retained in every CSV row.

`cross-donor-capability-aliases.csv` covers all **296 EffectCraft target proposals / 307 source records**. It supplies concrete canonical destinations and variant contracts. The 303 FilmCraft source records remain in their full ledger. This reconciliation addresses duplicate implementations; it does not certify working processors.

## Apply these rules when generating the combined registry

1. Normalize the proposal namespace to `nw.*`. Do not create two capabilities differing only by that prefix.
2. Resolve every EffectCraft proposal to `unified_target_capability_id`. The row's `unified_contract` and `required_variant_or_parameter_migration` define the combined design. Do not overwrite one donor's controls with the other donor's column values.
3. Apply **only** the explicit `filmcraft_ids_to_remap` field to FilmCraft IDs. The five ordinary iris/shape-wipe variants become one `nw.transition.wipe.iris` descriptor with a validated shape enum. Preserve their names as presets/UI aliases with the correct parameters.
4. `filmcraft_related_capability_ids` is a comparison/reference field. It is **not** an instruction to alias every listed ID. Distinct-context rows deliberately preserve graph placement, input preconditions or meaningful function.
5. The selective-retention row uses a concrete default RGB target and a conditional hue target. RGB distance and hue distance have separate schemas. `additional_conditional_target_ids` must be retained; do not collapse these into one unlabeled tolerance control.
6. Shared descriptors implement the union through typed variants, not inert controls. A field that applies only to one kernel, color model, contour or processor is rejected or hidden in other variants. Native defaults and units remain explicit.
7. Implement each `shared_lowering_owner` once. Distinct effect descriptors, presets, intrinsic properties and transitions may compose the same primitive. A distinct public capability is not a reason to create a second pixel/DSP engine or copy a filter compiler.

The registry descriptor/version owns GUI controls, browser controls, CLI/MCP schema, defaults, plan generation and qualification. Source IDs/defaults remain provenance unless a separate compatibility feature is authorized.

## Reconciliation result

| Resolution | EffectCraft proposal IDs | Meaning |
|---|---:|---|
| Shared descriptor | 35 | Individually compared matching tasks and control domains. Matching spelling alone was not the rule. |
| Shared descriptor with explicit variants | 54 | Differently named equivalents or useful richer contracts; migration/variant fields are stated per row. |
| Split by explicit selector | 1 | RGB retention and hue retention remain separately typed capabilities. |
| Distinct context, shared primitive | 29 | The task/placement differs, while a concrete lower-level implementation is shared. |
| Distinct capability | 147 | No equivalent FilmCraft task is claimed; common primitive/compiler infrastructure is still reused. |
| Typed project record | 11 | Parameter/result containers, not fake media effects or automatic analysis. |
| Precise unsupported semantic identity | 19 | Preserve the unavailable primary feature and separately named useful alternatives. |

After the five iris aliases are reconciled, the combined design has **481 distinct target IDs** across both donor ledgers, including data records and unsupported identities. This is an inventory sanity check, not a count of implemented effects. `cross-donor-capability-aliases-summary.json` records the derivation; every row is still `NOT_IMPLEMENTED_OR_EXECUTED`.

## Examples that must not lose semantics

The v7 temporal correction replaces the unqualified source-time Echo alias with explicit clip-clock sampling of the preceding processed layer. Posterize Time uses the same upstream-input contract and retains its existing `nw.video.time.posterize` to `nw.video.time.hold_rate` resolution. Parameter clocks and requested sample clocks are distinct. Numera deliberately includes earlier temporal nodes in an acyclic upstream prefix; FilmCraft's skipped-prior-temporal limitation is source provenance, not an inherited canonical restriction. No alias ID, resolution, count or runtime status changes. [FilmCraft frame access](https://github.com/storytold/filmcraft/blob/d643a886650bfbf16f24762274054575aa1e8f20/crates/render/src/vfx/mod.rs#L38-L121) · [EffectCraft temporal input](https://github.com/storytold/effectcraft/blob/9db7f3b21700fd821f215782ed1c82afe077880f/crates/effects/src/time_fx.rs#L159-L220)

| Compared entries | Concrete decision |
|---|---|
| Echo | One processed-layer echo with signed clip-clock taps, explicit average normalization, finite endpoints and admitted sample schedules; source-clock behavior requires a named variant. |
| Posterize Time / hold rate | One upstream-layer hold capability at quantized clip-relative times; preserve output cadence and duration, with explicit phase/rounding/endpoints and admitted animated schedules. |
| Gaussian, channel and directional blur | Share native blur implementations. Directional blur has explicit `dblur` versus rotated-axis-Gaussian kernel variants; ordinary Gaussian remains one `gblur`. |
| Mosaic | One block-pixelation capability, using downsample/upscale. Picture repetition belongs to a separate tiling capability. |
| Black & White | One monochrome descriptor with `luminance_matrix` and `hue_mix` variants. The simple matrix mode does not incur the selective mode's extra stages. |
| Noise and Add Grain | Shared noise capability with `sample_noise` and `grain` patterns. Clipping, headroom preservation and modulo-one wrapping are separate overflow variants: the donors' `clip=false` branches differ. A native fast route is admitted only for matching semantics. HSL noise remains a distinct domain-specific processor. |
| RGB/hue selective retention | Default RGB and explicit hue target; native YUV-derived `hsvhold` is not advertised as the same metric as HSL angular distance. |
| Two donors' Extract | FilmCraft's monochrome range visualization preserves alpha; EffectCraft's range key changes coverage. These stay distinct. |
| Two donors' Plane to Sphere | FilmCraft inserts a flat plane into a sphere; the reviewed EffectCraft entry is a tiny-planet projection. These stay distinct despite the matching label. |
| Inserted transform/timewarp versus intrinsic properties | Reuse transform/time-map primitives while retaining clip stack position, source/effect clock and intrinsic state. |
| Path stroke versus alpha outline | Authored path painting and deriving an outline from existing alpha remain distinct. |
| Iris/circle/box/cross/diamond/star reveal | One validated shape-wipe descriptor with explicit shape-specific fields. Corner-origin inset and corner-angle wipe remain separate. |
| Point/face controls | Typed project data remains independent of rendering and unavailable automatic recognition. |

These are selected implementation contracts, not legacy preset conversion guarantees. Qualification still checks each active variant's options, precision, alpha, timing, resource use and parity.
