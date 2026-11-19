# Third-party notices and provenance

First-party Numera Wiz code is GPL-3.0-or-later. This repository preserves a
research corpus with reference documents under their own licenses. Numera
records the separate runtime dependency closure in Cargo.lock.

- FFmWiz source: GPLv3; original license in docs/research-pack/research_v2/sources/ffmwiz/.
- Standalone MuxCls: MIT; original copyright/license in
  docs/research-pack/research_v7/muxcls-sources/standalone/LICENSE.
  Shared selection/metadata behavior is translated into nw-domain::mux, with
  intentional fixes documented in the mandate and development specification.
- FilmCraft: MIT/Apache-2.0; original notices/licenses remain in the donor archive.
  Workspace/panel/shortcut behavior is a reference; donor DSP is not a media engine.
- `craft-libs` linked interchange crates use the MIT license reproduced
  below. Cargo resolves them at the same exact revision selected by FilmCraft
  for these format libraries. Numera uses them for interchange metadata.
  [Pinned craft-libs source](https://github.com/storytold/craft-libs/tree/dc6adc3fbd9cf39e4a8210f94106a5b0c3e26e17)
- EffectCraft, Vidonex, codecs/build sources and manuals: retained original
  notices/licenses in their respective archived directories and manifests.
- FFmpeg manuals/source: corresponding GPL/LGPL license files are preserved in
  docs/vendor/ffmpeg/n9.0.2 and the original source archive. No FFmpeg binary is
  redistributed by this initial source commit; portable component closure remains
  an explicit implementation and release task.
Publishing a source archive does not establish native component binary licensing
closure. Each future package must carry corresponding source, configuration,
notices, dependency/SBOM records and exact artifact identity before release.

## craft-libs MIT license

MIT License

Copyright (c) 2026 ArtCraft Team and the craft-libs contributors

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
