# Shared media workspace

This guide covers the implemented development Stream Cleanup workspace. Project
editing, preview/playback, complete folder/companion workflows and portable
release packaging are still tracked in the task ledger. Use the matching hosted
CI desktop executable and its identified component profile when reviewing it.
The small Linux fixture profile is a test component, not the standard distribution.

## Open and import

Start the desktop executable with its explicit data and component directories.
No subcommand opens the native workspace. The browser uses the same Rust UI,
served by the local owner; use `serve --open-browser` with a named profile whose
command, object and filesystem rights match the intended work. See the CLI guide
for the current launch arguments. A browser session never interprets an uploaded
file's name as an authorized native filesystem path.

Choose media, drop a file, or enter a granted engine path and select Add media.
The native picker registers the selected path. The browser streams its selected
file to private managed storage, verifies the declared length and seals the
object before registration. Inspect the original stream inventory, including the
absolute source indices; index 0 is valid. The media catalog retains the original
display name even when managed storage uses a content digest.

## Review cleanup

Choose audio and subtitle selectors independently: all tracks, no tracks, exact
language codes, original indices, or title fragments. State what a selection
matching nothing means: skip this file, retain an empty selection, or reject it.
An audio-only source is not silently treated as a supported video cleanup job.

Partial order fields use comma-separated original indices. Unlisted retained
streams follow in their original order. Invalid comma-separated numbers reject
the whole entry; empty cells, signs, fractions, exponents and overflow are not
silently ignored. Preservation checkboxes independently
control extra video, cover pictures, attachments, data, unknown streams, tags,
dispositions and chapters. Tag rules match the original inventory and support
Keep, Set and Clear. Choose the explicit stream kind, including video, and use
original indices/languages to limit the rule. Later explicit edits win for each
field. The source is unchanged.

Selection report opens the same owner's source inventory and pure rule preview.
It shows every candidate as ready, skipped or rejected, with separate source and
retained counts. Switch between unique streams and original indices; tables use
pages of 32 rows. Per-file details show original retained indices, matches,
warnings and errors. Escape closes the report. Changing the form invalidates
it. This is registered metadata, not a fresh media probe or a produced output.
The current human form selects one asset; the canonical report also accepts a
bounded explicit set. Folder discovery and batch execution remain required.

File timestamps has three policies. Preserve timestamps is the default and
records a warning if exact access/modification values cannot be retained.
Require reviewed timestamps freezes the source values in the plan and refuses
publication if exact retention fails. New timestamps leaves the new object's
times. These are filesystem times, separate from packet timestamps. Output
ownership, permissions and ACLs follow owned-staging policy. Reading the output
later can change its access time.

Review the suggested destination or enter an authorized new destination. Review
cleanup plan freezes the selected source identity, component identities, outcomes,
output and invocation. The plan pane shows retained/dropped tracks and the
verification scope. Changing the form invalidates the reviewed plan. An existing
destination is preserved; it is not overwritten by a retry.

## Run and inspect

Run reviewed plan, or press Ctrl+Enter on Windows/Linux and Cmd+Enter on macOS.
This shortcut has the same guard and submission handler as the button: a current
reviewed plan is required, and it is unavailable while a request is unresolved.
If a connection fails, Reconnect and retry the same request keeps its replay
identity instead of creating another job.

The job area shows accepted work, progress states, terminal outcomes and failures.
Cancel requests cancellation before the durable publication fence; cancellation
after that fence is reported as too late. Closing a client releases its client
lease while accepted jobs continue in the owner. Inspect the accepted job through
Job details control for its frozen plan, prepared stages, exact timestamp
outcomes, warnings, verification and published artifact. The summary indicates
recorded warning counts.

For a successful job, Output details requests its currently authorized artifact
manifest and displays verified size, SHA-256 and job identity. Copy manifest URI
copies only a lookup handle; the recipient still needs current job/command rights.
The control does not grant general access to the export directory. Native/browser
streamed Save As remains required work.

A true unchanged copy must match complete source/output bytes. Remux must pass
structure and full packet-read verification under the declared clock policy.
Unadmitted framing changes fail verification; decoded fallback is not yet
qualified. A child exit code of zero alone never publishes an artifact.

## Evidence boundary

Hosted Chrome has exercised Choose media, streamed intake, Review and Run with
an independent byte-equality assertion. Hosted Linux X11 has exercised the actual
CI desktop executable, native path entry, registration, Review and keyboard Run,
with independent byte equality. The retained native screenshot and JSON report
identify the candidate head, actual CI merge commit and executable digest.
Run 40079710323 additionally exercised actual native Selection report input,
canonical completion, Escape and the subsequent output workflow; its report
and opened manifest PNGs were personally inspected. Real Chrome report input
was checked against the browser's actual canonical HTTP response. Windows/macOS compilation is separate from native interaction
qualification. Neither screenshot establishes preview, playback, complete
five-surface parity or the full initial application scope.

## Container profiles under qualification

The source adds an initial original-family MOV/MP4 MPEG-4 Part 2 ordinary-video
packet-copy route alongside Matroska. It is undergoing separate media CI. It
never silently drops unsupported retained streams, transcodes, changes brands
or strips required tags. An incompatible combination fails before execution.
See [profile scope](../development/original-container-profiles.md) for exact
limits and pending evidence. This is not full MOV/MP4 or complete F08 support.


## Save verified output bytes

Open Output details, then use Save verified copy. On native desktop, enter a
new absolute client file path or leave the destination blank for the OS Save As
picker. The browser uses its own File System Access save picker. Its selected
folder does not become an engine filesystem grant. The default filename uses
the authorized output basename when it is a safe portable component, otherwise
a bounded artifact-based fallback.

The copy has a separate progress/cancel control. Cancelling it does not cancel
the completed producer. Wait for active writes and cleanup; a request to stop
is not displayed as a confirmed stop. Length and SHA-256 are checked before
client publication. See [scope and limits](../development/client-artifact-save.md).
Actual Linux native destination entry, Save pointer input and the genuine OS
Save As picker passed independent saved/source byte equality in CI79. Chrome
Save and other platform variants are still undergoing hosted qualification.
Previous Output details screenshots alone do not qualify those routes.
