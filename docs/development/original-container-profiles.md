# Original-family MOV/MP4 preservation follow-up

The development planner adds explicit mov and mp4 mux routes alongside the
existing Matroska route. The initial MOV/MP4 profile retains original-family
ordinary MPEG-4 Part 2 video. It can remove additional video streams and change
retained title/language metadata without requesting any encode. The muxer name,
complete maps/metadata/dispositions, component identities and verification scope
remain frozen in the semantic plan and revalidated before execution.

This profile deliberately admits only exact known source major/minor/compatible
brands, matching vendor metadata, reproducible label lengths, retained primary
default and supported authored tags. It requires an original self-contained MOV
family source in the same mov or mp4/m4v family and the existing held descriptor/
closed-reference admission. It rejects unadmitted retained audio, covers,
subtitles, attachments, data, codecs, nonempty chapters, metadata stripping and
cross-family conversion with INCOMPATIBLE_PRESERVATION before media execution.
These are required future routes, not accepted reductions of full F08.

All expected authored metadata remains mandatory. MOV/MP4 never borrows
Matroska's undefined-language omission or duration-tag rewrite. A generated
container encoder is permitted only when no authored value is retained, as
declared in the reviewed profile. Packet payload, original exact rational
PTS/DTS/duration, flags, codec parameters/extradata and exposed side data all use
the existing full verifier. No structural-only or decoded fallback is inferred.
A muxer exit zero still cannot publish an unverified result.

No-op byte copy accepts the same filename-extension hint ignoring ASCII case;
it retains full original bytes and does not invoke a muxer. Output object/path
authority and collision rules are unchanged. Human metadata rules now expose
all canonical stream-kind choices, including ordinary video, through the same
shared form. Unsupported preservation combinations still produce the common
typed error on every adapter.

New hosted media fixtures create actual two-video MPEG-4 MOV and MP4 originals,
assert unchanged case-varied-extension copies, then remove the second video and
set the first video's language/title. Their ordinary full packet/tag/clock and
output probe oracles remain mandatory. Synthetic admission negatives reject
brand changes, unrepresentable tags, defaults, stripping and other codecs.
These additions await their own CI; no MOV/MP4 pass or packaged support is
credited yet. Other codecs/containers and all remaining F08/product/package/
resource/owner gates remain required.

Primary pinned source inspected: FFmpeg 9.0.2
946fcce07b6dcd0331c8cc609192aeff5e1924f8, libavformat/movenc.c
(Git blob 6f93eb3e36fb47bd2b8d0f6f7acccbf82fcf505e): ftyp brand/version
generation, MOV vendor fields, handler labels, 31-byte compressor labels,
track title udta and global metadata paths. Source inspection is not runtime
qualification.

Run 40080655520 passed all four native targets, lock and WASM. The first actual
MOV remux failed the mandatory title assertion before publication. Pinned movenc
writes track title to a name atom; pinned mov demux reports it as name. The next
revision explicitly projects only an unambiguous MOV/MP4 name to canonical title
and retains all raw source tags in stream_details. Prepared argv forces exact
unchanged title lowering and clears duplicate raw-name metadata. Dual name/title
records remain distinct during inspection and are incompatible with this route.
Independent fixture checks inspect the actual name atom; a second actual
registration/remux keeps the title while changing language. These corrections
await CI. No packet/metadata assertion is weakened; Matroska raw name tags are
unchanged. See evidence/original-container-ci70.json.

Run 40081501150 reached the subsequent MP4 iteration after completing all actual
MOV first/second round-trip packet, tag and independent raw-name assertions. The
combined media test then failed MP4 preflight: the pinned ISO demux path omits
vendor_id, whereas the initial gate incorrectly required a rendered zero vendor.
The correction requires exact FFMP for QuickTime and exact absence for ISO MP4;
it does not allow arbitrary vendors or omit any observed tag assertion. Legacy
raw-name snapshots retain their prior lowering, while current canonical title
projections explicitly lower the alias. New compatibility/negative units cover
both. MP4 and complete-job qualification still await CI; see
original-container-ci71.json.
