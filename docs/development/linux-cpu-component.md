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

The component qualification covers Linux x64 in the recorded Ubuntu 24.04 base.
Application assembly and offline launch, Windows and macOS component closures,
installer/signing, complete distribution evidence and owner-GPU qualification
remain release gates.
