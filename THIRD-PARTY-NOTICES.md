# Third-party notices and provenance

First-party Numera Wiz code is GPL-3.0-or-later. This repository preserves a
research corpus that contains source extracts and documents under their own
licenses. They are evidence, not Numera runtime dependencies or build members.

- FFmWiz source: GPLv3; original license in docs/research-pack/research_v2/sources/ffmwiz/.
- Standalone MuxCls: MIT; original copyright/license in
  docs/research-pack/research_v7/muxcls-sources/standalone/LICENSE.
  Shared selection/metadata behavior is translated into nw-domain::mux, with
  intentional fixes documented in the mandate and development specification.
- FilmCraft: MIT/Apache-2.0; original notices/licenses remain in the donor archive.
  Workspace/panel/shortcut behavior is a reference; donor DSP is not a media engine.
- EffectCraft, Vidonex, codecs/build sources and manuals: retained original
  notices/licenses in their respective archived directories and manifests.
- FFmpeg manuals/source: corresponding GPL/LGPL license files are preserved in
  docs/vendor/ffmpeg/n9.0.2 and the original source archive. No FFmpeg binary is
  redistributed by this initial source commit; portable component closure remains
  an explicit implementation and release task.
- Official Spec Kit 1.2.2 generated development helpers/skills retain upstream
  provenance. They are a development-tooling exception to first-party Rust automation.

Publishing a source archive does not establish native component binary licensing
closure. Each future package must carry corresponding source, configuration,
notices, dependency/SBOM records and exact artifact identity before release.
