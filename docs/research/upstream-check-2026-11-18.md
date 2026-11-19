# Upstream check, November 18, 2026

FFmWiz and MuxCls still point to the revisions reviewed on November 13. This
check found no new donor changes to import for the Rust CLI. [Previous donor review](upstream-donor-check-2026-11-13.json)

The FFmpeg fixture still builds from `n9.0.2` at
`946fcce07b6dcd0331c8cc609192aeff5e1924f8`, the newest stable tag returned by
GitHub. The comparison with the previously observed development head returned
173 commits and 187 file entries. Selected entries include FFV1 remap scratch
storage and caption-queue changes. Those changes still need a release or
backport assessment before they can qualify a distribution profile. The
experimental NUT transport uses the unchanged stable source and explicitly
enables its demuxer, muxer and raw frame decoder in the Linux fixture build.
[FFmpeg development comparison](https://github.com/FFmpeg/FFmpeg/compare/4345a7b451052025cd5e9632e2725f345baaabfd...342cb6374fee2e7a86b35235203e3eefc9572969)

The latest `ffmpeg-skill` instructions call for probing the inputs, preferring
stream copy when possible and checking the output independently. Its recent
changes emphasize frame and sample boundaries and preserving audio alignment
during variable frame rate conversion. The new Rust fixture uses stream copy
for its normalized inputs and checks the common clock offset, stream profile,
frame count and full decoded sample content. Hosted evidence is pending.
[Skill at the reviewed revision](https://github.com/kajisho5/ffmpeg-skill/blob/514c354237a18869eb03cf709814249457e326d3/SKILL.md)

FilmCraft's latest compare summary reports 357 commits and returns 250 commit
records. The selected changes move codec and interchange work into shared
libraries, including a common FCPXML/FCP7 path. EffectCraft's summary reports
451 commits, returns 250 commit records and reaches GitHub's 300-file limit.
Selected changes include reversed audio and shared Opus export. These are
inputs to the interchange and audio work. Full source coverage and adoption
remain open. [FilmCraft comparison](https://github.com/storytold/filmcraft/compare/19d46c0748cf5c0ef5b0b2a421b85e8ae6d99db6...688444e75d059c0d2a5c13be75fc3cb210efebd1) · [EffectCraft comparison](https://github.com/storytold/effectcraft/compare/a5ddc932578a94a826b0ee824ad2719a0e6dd3cd...3f1cd00745d41345632b38beece0b09c9e265e69)

Vidonex's head is unchanged. Shotcut's four new commits include a hardware
encoder default fix and a keyframe-loop change that was reverted. The encoder
fix applies hardware defaults when loading default presets and preserves
explicit settings in existing presets. This informs the remaining hardware
profile work. [Vidonex source](https://github.com/farshidrezaei/vidonex/tree/ae04a3a4e7e455dd3f5f8a954fd2ff0c668fa4c6) · [Shotcut comparison](https://github.com/mltframework/shotcut/compare/f291f3f2531f6861e62cdbf90650ff0a6a072f03...7f7fab384e422e3d01525e50146fdc41be718e06)

The official registry lists rmcp 3.11.0 and redb 4.6.0. The rmcp change set
includes bounded HTTP responses and stricter capability handling. The registry
also declares new Unicode and percent-encoding dependencies. redb 4.6.0
changes recovery from damaged slots, backend quiescence during close, and
checks for open tables at commit. The source pins now select these releases.
The downloaded crate hashes matched the official index. The SDK keeps the
existing tool and resource capabilities, and redb keeps its stable backend
interface without the experimental API feature. The refreshed lock still
requires hosted resolution and the existing wire and prior-package trials.
[rmcp 3.11.0 changes](https://github.com/modelcontextprotocol/rust-sdk/compare/rmcp-v3.10.0...rmcp-v3.11.0) · [redb 4.6.0 changes](https://github.com/cberner/redb/compare/v4.5.0...v4.6.0)

The stable Rust manifest reports 1.100.0. The registry versions for the current
eframe and Tokio packages are still 0.37.0 and 1.54.1. The verified application
build matrix still uses the mandate's Rust 1.99.1 seed. [Rust stable manifest](https://static.rust-lang.org/dist/channel-rust-stable.toml) · [eframe](https://crates.io/crates/eframe) · [Tokio](https://crates.io/crates/tokio)

The next FilmCraft revision delegates Radiance HDR decoding to a bounded
shared decoder. The subsequent EffectCraft revision delegates PSD block
framing and RIFF, RF64 and XMP markers to shared libraries. All 15 changed
file patches in these two later comparisons were reviewed. A generated RIFF
and RF64 pair tested against the pinned FFprobe read the Unicode cue label
and sample offset, and inferred a chapter end from the file duration. Numera
now inspects the original cue and `ltxt` length through the registered source
handle in the CLI, MCP and shared panels. This covers RIFF and RF64 cue
metadata. HDR, PSD and XMP workflows remain open. [FilmCraft comparison](https://github.com/storytold/filmcraft/compare/688444e75d059c0d2a5c13be75fc3cb210efebd1...6aa67684948a585ef3f40d81a63bbf228f9f320a) · [EffectCraft comparison](https://github.com/storytold/effectcraft/compare/3f1cd00745d41345632b38beece0b09c9e265e69...f3c28669a243622fde9ee8eb9a23cae6cac7057d)

The later EffectCraft delta moves PSD scalar blocks, section-divider recovery
and bounded text-layer headers to Craft PSD revision `62f7a7e`. The eight
changed-file patches include tests for short optional fields, repeated
blocks and descriptor bounds. Layered PSD import remains outside the current
FFmpeg render profile. FFmWiz, MuxCls and FilmCraft are still at the revisions
above. [EffectCraft comparison](https://github.com/storytold/effectcraft/compare/f3c28669a243622fde9ee8eb9a23cae6cac7057d...ad952917628648dc630f3f00fab86227a70a8dba)

FFmpeg Skill 3.12.1 retains bounded diagnostics when the open-GOP multi-range
test fails. The actual test continues to compare 150 decoded frames, their
40 ms spacing and content around both joins. Numera's Rust mixed export uses
bounded subprocess diagnostics and verifies decoded timestamps and samples
before publishing. The selective open-GOP stream-copy workflow remains open.
[FFmpeg Skill comparison](https://github.com/kajisho5/ffmpeg-skill/compare/514c354...fe577168f39dd8effdf8c95b91ce16986a511f37)

The next FFmpeg Skill change binds files discovered through parsed flags and
input readers. Its tests cover joined flag syntax, changed side inputs and
sidecar collisions. Numera's chapter review captures the parsed FFmetadata
document in the reviewed plan and stages that content for FFmpeg. The
existing CLI journey now reviews both flag forms and checks that editing the
sidecar after review changes a new review while the accepted earlier plan
publishes its original chapters. [FFmpeg Skill update](https://github.com/kajisho5/ffmpeg-skill/commit/ab4c2484d1dd0cc399613e7fe76938ffcf95ca97)

EffectCraft's later PSD adapter reads the shared bounded descriptor prefixes
for layer effects, solid fill and brightness/contrast. The five changed
files were reviewed, including checks that short records preserve earlier
state and oversized descriptors fail admission. Layered PSD import still
requires a matching Numera workflow. [EffectCraft comparison](https://github.com/storytold/effectcraft/compare/ad952917628648dc630f3f00fab86227a70a8dba...851642f6ec395c47a1843b6f6cf435c08de4f4eb)

## November 19 upstream changes

FFmWiz remains at `b4968caf` and MuxCls at `1269bd71`. FFmpeg Skill 3.13.2
adds build-license observation, signature-based WAV selection, typed retime
reports, and selection of which tools its MCP server advertises. Numera now
reads the identified FFmpeg executable's `-L` declaration in the existing
capability report. Numera's registered input admission already binds RIFF and
RF64 WAVE to the WAV demuxer. The HDR matrix, image-sequence, and caption
changes relate to workflows still in development. [FFmpeg Skill changes](https://github.com/kajisho5/ffmpeg-skill/compare/ab4c2484d1dd0cc399613e7fe76938ffcf95ca97...2cb864c2af4c3d67623132a0ed2974e83619c023)

FilmCraft advances the shared DNG decoder and TIFF traversal together to
Craft Libraries `3b8e426a`. EffectCraft delegates PSD brightness and exposure
block writes to `027416ac` and exposes one-, two-, and four-view selection in
the composition viewer. The application and dependency changes inform the
remaining image-format and viewer work. [FilmCraft change](https://github.com/storytold/filmcraft/commit/3d37c0f1f1bfd937f76d8fc929f1b2ef828ebf2c) · [EffectCraft changes](https://github.com/storytold/effectcraft/compare/851642f6ec395c47a1843b6f6cf435c08de4f4eb...686fb3fa0485ab4af74c60e343203572fc37138f) · [PSD writers](https://github.com/storytold/craft-libs/commit/027416ac9f36b60229c595f942d36bf74f0ed4d2)

The next FFmpeg Skill change compares the requested source in-point with a
separately selected position in the output. Numera's reference comparison
accepts distinct original frame positions for the two files. Its hosted
regression now prepares a separate lossless cut, aligns its first two frames
with the original in-point and checks both metrics. An independent FFmpeg
check matched all decoded bytes for that alignment. The hosted regression
still needs to run on the comparison implementation. [FFmpeg Skill change](https://github.com/kajisho5/ffmpeg-skill/commit/b8fbd1ce844883ecf678be2386ce1ad92692de1d)

FilmCraft moves AAF, timeline and caption dependencies to Craft Libraries
`3b8e426a`. The caption adapter converts exact tick and frame-rate types at the
project boundary and preserves the sidecar's export-relative interval.
EffectCraft moves OpenEXR scanline writing to `craft-exr` and moves PSD hue and
levels block construction to `craft-psd`, with byte-level writer regressions.
These changes inform the pending interchange and image workflows. FFmWiz and
MuxCls remain at the previously reviewed revisions. [FilmCraft change](https://github.com/storytold/filmcraft/commit/f67d38b35cc5af6853ef5a6921ccba014512694b) · [EffectCraft comparison](https://github.com/storytold/effectcraft/compare/686fb3fa0485ab4af74c60e343203572fc37138f...665bfac0b33904b1dcda6de889f43f43a94ad3f8)


## November 19, 06:34 UTC follow-up

FFmWiz and MuxCls remain at `b4968caf` and `1269bd71`. FilmCraft,
EffectCraft, Vidonex and Shotcut remain at the revisions reviewed above.
FFmpeg's newest stable release remains 9.0.2. The observed development
head includes RTP AV1 reassembly work. Numera's held local-file media
routes use the stable tools. [FFmpeg development head](https://github.com/FFmpeg/FFmpeg/commit/4da74f951349f82e77cebc00be64423ccf8f6844)

FFmpeg Skill's latest changes cover Chinese phrase wrapping, observed
picture geometry and right-to-left caption punctuation. Numera's
SubRip route lets libass determine reading order across each whole
caption using `Encoding=-1`. The hosted journey compares ordinary cues
and cues with explicit direction marks in the same FFmpeg output.
The independent FFmpeg comparison reproduced equal visible pictures in
Persian, Arabic, Hebrew and English. The default layout gave a
different Hebrew picture for the marked and unmarked question.
[FFmpeg Skill changes](https://github.com/kajisho5/ffmpeg-skill/compare/b8fbd1ce844883ecf678be2386ce1ad92692de1d...828efbb3a861e2e51529f60246741d13590c86a0)

Craft Libraries now tests JPEG XL capability against the enabled codec.
JPEG XL decoding remains pending in Numera's admitted image profiles.
rmcp remains at 3.11.0. redb 5.0 changes its file-lock protocol, backend
return types and read-only range lifetimes. Numera currently uses redb
4.6.0 across its store and prior-package trials. A major upgrade needs
a verified prior-package open and handoff across the supported systems.
[Craft Libraries change](https://github.com/storytold/craft-libs/commit/56c3c833bb869a140271e2aceec2112322cacc58) · [redb 5.0 changes](https://github.com/cberner/redb/releases/tag/v5.0.0)

FFmWiz uses average bitrate for `quality_vbr` and `strict_size`. Its peak and
buffer are two and four times the average for `quality_vbr`, and one and two
times the average for `strict_size`. The Numera CPU AVC planner applies those
controls for admitted H.264 configurations and retains their square-pixel
output rule. An independent FFmpeg exercise verified both outputs at the six
original variable presentation times.
[FFmWiz bitrate lowering](https://github.com/KiaroSama/FFmWiz/blob/b4968cafb3dcce15b1c4313763d74c9fd07748a6/ffmwiz/support/L01_encode_opts.py#L105-L131)
