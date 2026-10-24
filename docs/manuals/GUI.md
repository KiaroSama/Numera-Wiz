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
streams follow in their original order. Preservation checkboxes independently
control extra video, cover pictures, attachments, data, unknown streams, tags,
dispositions and chapters. Tag rules match the original inventory and support
Keep, Set and Clear. Later explicit edits win for each field.

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
the common command interface for its frozen plan, prepared stages, verification
and published artifact.

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
identify the candidate head, actual CI merge commit and executable digest. Windows/macOS compilation is separate from native interaction
qualification. Neither screenshot establishes preview, playback, complete
five-surface parity or the full initial application scope.
