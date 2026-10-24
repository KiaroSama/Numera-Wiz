# Protected owner diagnostics

The detached owner activates JSONL logging before opening its store and binds
the stable root ID and epoch after service initialization. Native adapters keep
timestamped INFO/WARNING/ERROR/DEBUG on stderr. Protocol stdout carries its declared
CLI/MCP result. Protected first-party events retain UTC time, severity, process,
root/epoch and safe command-completion facts. Complete request/response bodies and
known credential/replay fields are redacted. Generic error values are redacted;
typed codes and controlled job diagnostics retain their existing authorization.
Stderr fields have finite lengths, truncation markers and escaped control text.

The private logs directory has its own OS writer lock. Existing files verify their
own Windows DACL as well as the parent; Unix file ownership must match its protected
parent. A Windows fixture broadens only the credential file's DACL and requires
rejection. The same protected-file adapter covers runtime/token/marker/log reads.

The queue holds 32 records of at most 16 KiB each and uses nonblocking producer
admission. Size/encoding overflow emits a bounded omission record. Rotation retains
five owned files of at most 2 MiB each. Ownership headers bind them to a protected
stable marker. Every existing reserved filename is validated before rotation;
link creation refuses a destination collision. Unknown files are preserved.
Startup reconciles duplicate owned links from interrupted rotation and archives
an interrupted tail before writing complete new records. An incomplete/foreign
ownership header fails closed for explicit owner repair.

logs.status reports configured/healthy state, directory, limits, dropped/omitted
records and write failures. This query requires local-owner authority. Named
operational profiles cannot grant log administration. Disk failure marks the
writer unhealthy and stops file writes with a timestamped warning. Shutdown
attempts a two-second flush; loss cannot be represented as a complete export.
The redb job/receipt/stage/publication facts remain authoritative. Support bundles,
job-scoped log pages, full audit retention/export and corruption-repair workflows
remain required in T011/T012/T036/T039.

Hosted unit cases cover escaping/redaction, record capture limits, rotation,
unowned-file preservation, interrupted tails and duplicate links. The actual owner
media fixture inspects its JSONL, primary levels/time and absence of its issued
bearers. These changes await their own CI result. Package/export/resource
qualification requires the remaining actual boundary evidence.
