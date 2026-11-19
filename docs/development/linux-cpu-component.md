# Offline Linux CPU component

The existing Linux media CI job builds FFmpeg and ffprobe 9.0.2 with GCC 13 and
NASM 3.02. Its toolchain runs inside the recorded Ubuntu 24.04 x64 image with
network access disabled. Build preparation verifies every downloaded Debian
package against its frozen Ubuntu archive digest and verifies the installed
package inventory before compilation. The FFmpeg release tarball matches the
official detached signature and 10,398 tracked content files of source commit
`946fcce07b6dcd0331c8cc609192aeff5e1924f8`.

The component includes x264, x265, dav1d, SVT-AV1, libvpx, MP3 LAME, Opus, libass,
FreeType, HarfBuzz, FriBidi, Fontconfig, zimg, Rubber Band, vid.stab and Cairo.
Its FFmpeg configuration enables GPL version 3 or later and limits compiled
media protocols to `file`, `pipe` and `fd`. The two executables and their private
ELF libraries resolve the companion files relative to their own locations.
DejaVu Sans is included for explicit offline font selection.

Qualification encodes, probes and decodes H.264, HEVC, AV1 and VP9, and records
the decoded frame hashes. A second, freshly started container verifies the
relocated ffprobe. Its text, Cairo and Unicode subtitle checks compare pixels
against an unfiltered reference. CI retains the configuration, executable
feature reports, frame hashes and build inventory.

The [binary package lock](../../components/linux-toolchain-package-lock.json)
records 200 verified packages from the signed Ubuntu archive. The
[corresponding-source lock](../../components/linux-standard-source-lock.json)
records 128 archives for the 39 Ubuntu source packages that own the bundled
libraries and font. The build derives those ownership identities from the
installed packages. It collects the exact upstream files in the separate
corresponding-source archive. The CI component artifact includes both archives
with their byte counts and SHA-256 digests in the build inventory.

The Linux x64 application in CI367 assembled the verified component and the
same-source browser assets. The standalone executable attached to a fresh
protected owner and exercised FFV1 encode and decode on its first launch.
Independent verification matched the ZIP, ELF executable and corresponding
source archive to their digests, and both helper hashes to the capability
receipt. The source was `4db81a61634d0f24db4d705354df173afd63e67c`.
[Exact CI artifact](https://github.com/KiaroSama/Numera-Wiz/actions/runs/44450379802/artifacts/14496316587)

The recorded Ubuntu 24.04 x64 base defines the verified Linux CPU environment.
Full distribution, installer signing and owner hardware qualification remain
release gates.
