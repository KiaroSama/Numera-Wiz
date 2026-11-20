# Stopped-generation migration foundation

The application still uses store schema2 and generation1. No data-migration
command is active. These primitives await hosted checks and do not qualify an
upgrade or rollback.

The closed inspector opens pinned redb4.4.0 read-only with8MiB cache. It never
creates, repairs or upgrades its source. Raw-record hashing preserves receipt
bytes and exact numeric strings. Limits:64GiB,50000records,8MiB per record,
128namespaces. A live writer returns OWNER_BUSY; an unclean database returns
STORE_RECOVERY_REQUIRED. Empty and unknown files remain untouched.

DatabaseSlot retains transaction leases through drop/commit. Closing refuses
live transactions and pending writer admission while retaining the generation
OS lock. Reopening or attaching a selected closed copy preserves the same Store,
root identity, signing key and receipt kernel. Application integration must also
drain request, authentication and job work.

RootLease supplies a root-wide OS lock and bounded immutable hash-linked
activation records. Typed internal locations prevent caller paths. Appending
checks both source/target root IDs and equal replay signing keys. Unsupported
versions/capabilities and broken chains fail without fallback. Limits:64KiB per
record,64events,256directory members. Interrupted metadata-write files remain
charged; their names are not ownership/deletion proof.

The copy/attachment test uses schema2 at another physical location. It tests
transaction and identity boundaries only. It does not prove schema3 migration,
reader-floor protection, old-binary compatibility or adoption/rollback.
The application owner/client are not yet connected to this selector.

Next integration must use ordinary owner durable_mutation scope/key/receipts.
Bootstrap exemption applies only to the mandate's listed identity/pairing/scope/
credential operations. Required work includes maintenance-only legacy admission,
common recovery before planning, reader-floor acceptance, complete quiescence,
manifest-owned streamed copies, separate validated generations, recoverable
activation, adoption/rollback guards, retained signing/version/alias/digest
interpretations, generation/media retention and actual old/new binary faults.

Primary source: cberner/redb v4.4.0 src/db.rs
(blob a6a38767ca5cd18da82c98239561efe93bdf0467), src/error.rs
(blob dd8234a18e93fdcbff3638155ca3a5c0764299d0). ReadOnlyDatabase cannot repair
unclean shutdown. A live write transaction defers Database close; transaction
leases prevent switching before that writer releases its database.
