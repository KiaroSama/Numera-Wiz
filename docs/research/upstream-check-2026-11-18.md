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
