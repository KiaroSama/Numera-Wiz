# First export sample

`clip.mkv` is generated from FFmpeg's `testsrc2`. It contains four 64 × 48 FFV1
frames at 0, 250, 500 and 750 ms. It contains no real-world media or audio.

SHA-256 `9aff62348c868712df08ca9fc2c17cef2d7704b7c680922aafd1ae7c8d5892eb`

```text
ffmpeg -f lavfi -i testsrc2=size=64x48:rate=4:duration=1 -map 0:v:0 -c:v ffv1 -threads 1 -color_range tv -fflags +bitexact -flags:v +bitexact clip.mkv
```

The [first export walkthrough](../../docs/manuals/QUICKSTART.md) uses this sample.
