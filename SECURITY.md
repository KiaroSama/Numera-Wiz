# Security

This branch is development work, without a qualified portable release. Report
security issues through GitHub private vulnerability reporting if it is enabled;
otherwise contact the repository owner privately and omit exploit details from
public issues. Do not include credentials, local media or personal paths in logs.

The implementation must qualify exact loopback Host/Origin checks, scoped auth,
object actions, native path/secondary-open admission, owned subprocesses, bounded
I/O, durable replay, verified staging/publication and component provenance before
claiming the complete release. Open gates are tracked in the development ledger.

## Active secret scanning

The [Gitleaks workflow](.github/workflows/gitleaks.yml) uses the separately qualified Rust scanner in [xtask/security-scan](xtask/security-scan). It rejects Actions step-debug logging before bootstrap, records one original time budget, creates fresh private Rust/Cargo build homes, installs Rust 1.99.1, then checks out the qualified immutable tool source and builds it with its lock before checking out scan input with complete history. The scanner is built from the trusted pinned source, not from the pull request being scanned. The workflow records compiler, source, manifest, lock and executable provenance for review. Qualification review compares the pre-input executable hash with that scan’s runtime helper hash, and the manifest and lock hashes with the qualified source.

The Rust bootstrap and scan share the original deadline across their steps. Gitleaks 8.30.1 supplies the verified scanning CLI; Rust controls bootstrap, limits, process ownership, complete report validation, classification and safe summary publication. Pull requests scan introduced commits and merge diffs; default branch scans cover fetched history and merge diffs. Built-in rules and standard exclusions define revision coverage, not universal credential coverage.

Successful outcomes distinguish `CLEAN` from `REVIEWED_PUBLIC_DATA`. The latter accepts only findings whose rule, path, line and full historical blob content match reviewed public examples; unknown or changed findings fail. Public summaries contain only rule, file, line and commit. Secret values, snippets, full reports and private bootstrap receipts or scratch data are not published as artifacts. This isolated tool/workflow qualification does not qualify the portable full v1 application or a GPU release.
