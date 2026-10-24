# Verified client artifact copies

This development path transfers an already published artifact. It does not
submit another producer job or grant the engine a client filesystem path.

In Output details, use **Save verified copy**. On native desktop, enter an
absolute new client destination or leave it blank for the OS Save As picker.
Existing native entries, including links and directories, are preserved.
The browser uses its actual File System Access save picker. A browser without
that capability reports the missing capability; it does not buffer the whole
file into a Blob or expose a bearer URL. Browser destination/overwrite consent
belongs to the browser picker, not the engine's filesystem authorization.
The native held-directory no-replace guarantee is not claimed for browser file
handles.

Headless usage:

```text
numera-wiz --data-dir DATA artifacts save --artifact-id job-JOB --expected-sha256 SHA256 --out ABSOLUTE_NEW_FILE
```

The shared access profile flag selects the current principal as usual. Manifest
lookup and each authenticated byte stream enforce current artifact/job rights.
The adapter rechecks the selected immutable metadata and sends a strong SHA-256
If-Match header. It uses the private connection for native CLI/desktop and the
exact paired public origin for the browser. Redirects are refused; bearer
credentials remain in Authorization headers. The 130-second transfer deadline
is explicit and inherited from the admitted artifact route.

Network frames are split into at most 128 KiB chunks. File writes drain before
the next chunk; length and SHA-256 must match the selected manifest. Native
copies additionally flush, sync and independently reread the entire private
staging file before an atomic held-directory, same-filesystem no-replace link.
Native copies join the existing cross-root destination claims; contention is an
explicit nonblocking publication conflict.
No cross-device copy or replacing rename substitutes for that operation.
Only the owned temporary payload and empty workspace are removed; unknown
entries are preserved. Cleanup failure after successful client publication is
a warning, not a failed producer job. Crash-orphan cleanup is not yet qualified.

Browser writes await writable-stream backpressure, verify all received bytes
before close and await abort on failure. The browser chooses its file handling
semantics; this does not claim native file-ID or crash guarantees. First-party
picker, streaming, hashing and cancellation logic is Rust/WASM.

**Cancel client copy** cancels this transfer independently of the completed
producer job. The UI displays a pending stop while admitted writes and cleanup
drain. A cancellation/publication fence has one winner; after publication begins
the UI waits for its actual result. CLI Ctrl+C requests the same stop and waits.
Closing the desktop requests cancellation without cancelling accepted media
jobs.

This implementation awaits its own hosted checks and actual GUI save journeys.
Prior report/output inspection evidence does not qualify Save As. Complete
download resumption UX, all browser/platform variants, interrupted workspace
retention, package/resource gates and the complete product remain required.

The browser adapter follows the primary
[Chrome File System Access guide](https://developer.chrome.com/docs/capabilities/web-apis/file-system-access):
invoke the actual save picker during user activation, then await writes and the
final close. API support is detected at runtime. No mock picker qualifies it.
