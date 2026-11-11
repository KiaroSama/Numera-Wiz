//! Executable assertions, separate from the original application test inventory.
//! A1 creates real clean Git history and requires the incomplete scan contract.
//! It does not start a scanner/proxy or implement their A2 process supervisor.

use nw_security_scan::{ScanFailure, ScanRequest, diagnostic, install_private_panic_hook, scan};
use std::process::ExitCode;

fn main() -> ExitCode {
    install_private_panic_hook();
    #[cfg(target_os = "linux")]
    let result = linux::qualify();
    #[cfg(not(target_os = "linux"))]
    let result: Result<(), &'static str> = Err("SECURITY_QUALIFICATION_HOST_UNSUPPORTED");
    match result {
        Ok(()) => {
            diagnostic("INFO", "SECURITY_REAL_CLEAN_PASS");
            ExitCode::SUCCESS
        }
        Err(category) => {
            diagnostic("ERROR", category);
            ExitCode::FAILURE
        }
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use std::{
        env,
        fs::{self, DirBuilder, File, OpenOptions},
        io::{Read, Write},
        os::unix::fs::{DirBuilderExt, OpenOptionsExt},
        path::{Path, PathBuf},
        process::{Child, Command, Stdio},
        thread,
        time::{Duration, Instant},
    };

    const FIXTURE_BUDGET: Duration = Duration::from_secs(300);
    const SETUP_BUDGET: Duration = Duration::from_secs(15);
    const CLEANUP_GRACE: Duration = Duration::from_secs(5);
    const POLL: Duration = Duration::from_millis(50);
    const CAPTURE_LIMIT: u64 = 1024 * 1024;
    const EXPECTED_SUMMARY: &str = concat!(
        "## Gitleaks\n\nScanner: **8.30.1**\n\n",
        "Scope: **All fetched branches and tags, including full history and merge diffs**\n\n",
        "Rules: Pinned built-in Gitleaks rules with standard exclusions. ",
        "Full history describes revision coverage, not universal byte or credential coverage.\n\n",
        "No matching secrets were found in the scanned Git history.\n",
    );

    type Result<T> = std::result::Result<T, &'static str>;

    struct Fixture {
        root: PathBuf,
        git: PathBuf,
        started: Instant,
        command_count: usize,
        cleanup_verified: bool,
    }

    pub fn qualify() -> Result<()> {
        if env::var("GITHUB_ACTIONS").as_deref() != Ok("true") {
            return Err("SECURITY_QUALIFICATION_HOST_UNSUPPORTED");
        }
        if env::var("RUNNER_DEBUG").as_deref() == Ok("1") {
            return Err("SECURITY_QUALIFICATION_DEBUG_REJECTED");
        }
        let mut fixture = Fixture::create()?;
        let result = fixture.real_clean();
        // A1's admitted Git setup commands cannot launch hooks, signing, filters,
        // editors or maintenance. Direct-child reaping here is not the A2
        // scanner/proxy/fixture descendant-adoption and supervision contract.
        if !fixture.cleanup_verified {
            return Err("SECURITY_QUALIFICATION_CLEANUP_UNVERIFIED");
        }
        fs::remove_dir_all(&fixture.root).map_err(|_| "SECURITY_QUALIFICATION_CLEANUP_FAILED")?;
        result
    }

    impl Fixture {
        fn create() -> Result<Self> {
            let temporary = env::var_os("RUNNER_TEMP")
                .map(PathBuf::from)
                .ok_or("SECURITY_QUALIFICATION_TEMP_UNAVAILABLE")?;
            let temporary = fs::canonicalize(temporary)
                .map_err(|_| "SECURITY_QUALIFICATION_TEMP_UNAVAILABLE")?;
            let git = fs::canonicalize("/usr/bin/git")
                .map_err(|_| "SECURITY_QUALIFICATION_GIT_UNAVAILABLE")?;
            let mut entropy = [0_u8; 16];
            File::open("/dev/urandom")
                .and_then(|mut input| input.read_exact(&mut entropy))
                .map_err(|_| "SECURITY_QUALIFICATION_TEMP_UNAVAILABLE")?;
            let suffix = u128::from_ne_bytes(entropy);
            let root = temporary.join(format!("nw-security-qualification-{suffix:032x}"));
            DirBuilder::new()
                .mode(0o700)
                .create(&root)
                .map_err(|_| "SECURITY_QUALIFICATION_TEMP_UNAVAILABLE")?;
            Ok(Self {
                root,
                git,
                started: Instant::now(),
                command_count: 0,
                cleanup_verified: true,
            })
        }

        fn real_clean(&mut self) -> Result<()> {
            let repository = self.root.join("repository");
            for name in ["repository", "home", "empty-template"] {
                DirBuilder::new()
                    .mode(0o700)
                    .create(self.root.join(name))
                    .map_err(|_| "SECURITY_REAL_CLEAN_SETUP_FAILED")?;
            }
            self.git(&repository, &["init", "--quiet", "--initial-branch=main"])?;
            let mut source = private_file(&repository.join("README.txt"))?;
            source
                .write_all(b"A clean synthetic repository for security qualification.\n")
                .map_err(|_| "SECURITY_REAL_CLEAN_SETUP_FAILED")?;
            drop(source);
            self.git(&repository, &["add", "--", "README.txt"])?;
            self.git(
                &repository,
                &["commit", "--quiet", "--no-gpg-sign", "-m", "Clean fixture"],
            )?;
            self.git(&repository, &["cat-file", "-e", "HEAD^{commit}"])?;
            self.git(
                &repository,
                &[
                    "diff",
                    "--quiet",
                    "--no-ext-diff",
                    "--no-textconv",
                    "--ignore-submodules=all",
                    "HEAD",
                    "--",
                ],
            )?;
            diagnostic("INFO", "SECURITY_REAL_CLEAN_SETUP_PASS");
            self.check_deadline()?;
            let summary = self.root.join("summary.md");
            // Use the same scan entry point as the candidate CLI. A1's handler
            // returns immediately. A2 must add the separately reviewed bounded
            // fixture parent before this can launch a real scanner descendant.
            let result = scan(&ScanRequest {
                repository: &repository,
                summary: Some(&summary),
            })
            .map_err(|failure| match failure {
                ScanFailure::MissingHandler => "SECURITY_REAL_CLEAN_FAIL_MISSING_SCAN_HANDLER",
            })?;
            self.check_deadline()?;
            if result.scanner_version != "8.30.1"
                || result.finding_count != 0
                || result.successful_history_invocations == 0
            {
                return Err("SECURITY_REAL_CLEAN_CONTRACT_FAILED");
            }
            let size = fs::metadata(&summary)
                .map_err(|_| "SECURITY_REAL_CLEAN_SUMMARY_FAILED")?
                .len();
            if size != EXPECTED_SUMMARY.len() as u64
                || fs::read(&summary).map_err(|_| "SECURITY_REAL_CLEAN_SUMMARY_FAILED")?
                    != EXPECTED_SUMMARY.as_bytes()
            {
                return Err("SECURITY_REAL_CLEAN_SUMMARY_FAILED");
            }
            Ok(())
        }

        fn check_deadline(&self) -> Result<()> {
            if self.started.elapsed() >= FIXTURE_BUDGET {
                return Err("SECURITY_QUALIFICATION_TIMEOUT");
            }
            Ok(())
        }

        fn git(&mut self, repository: &Path, arguments: &[&str]) -> Result<()> {
            self.check_deadline()?;
            self.command_count += 1;
            let stdout = self.root.join(format!("git-{}.stdout", self.command_count));
            let stderr = self.root.join(format!("git-{}.stderr", self.command_count));
            let mut command = Command::new(&self.git);
            command
                .current_dir(repository)
                .env_clear()
                .env("PATH", "/usr/bin:/bin")
                .env("HOME", self.root.join("home"))
                .env("XDG_CONFIG_HOME", self.root.join("home"))
                .env("TMPDIR", &self.root)
                .env("LANG", "C")
                .env("LC_ALL", "C")
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_CONFIG_SYSTEM", "/dev/null")
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_ATTR_NOSYSTEM", "1")
                .env("GIT_TERMINAL_PROMPT", "0")
                .env("GIT_TEMPLATE_DIR", self.root.join("empty-template"))
                .env("GIT_AUTHOR_NAME", "Security qualification")
                .env("GIT_AUTHOR_EMAIL", "qualification@example.invalid")
                .env("GIT_COMMITTER_NAME", "Security qualification")
                .env("GIT_COMMITTER_EMAIL", "qualification@example.invalid")
                .arg("--no-pager")
                .args([
                    "-c",
                    "core.hooksPath=/dev/null",
                    "-c",
                    "core.fsmonitor=false",
                    "-c",
                    "core.attributesFile=/dev/null",
                    "-c",
                    "commit.gpgSign=false",
                    "-c",
                    "gc.auto=0",
                    "-c",
                    "maintenance.auto=false",
                ])
                .args(arguments)
                .stdin(Stdio::null())
                .stdout(Stdio::from(private_file(&stdout)?))
                .stderr(Stdio::from(private_file(&stderr)?));
            let mut child = command
                .spawn()
                .map_err(|_| "SECURITY_REAL_CLEAN_SETUP_FAILED")?;
            self.cleanup_verified = false;
            let started = Instant::now();
            let result = loop {
                if self.check_deadline().is_err() || started.elapsed() >= SETUP_BUDGET {
                    break Err("SECURITY_REAL_CLEAN_SETUP_TIMEOUT");
                }
                if captures_within_limit(&stdout, &stderr).is_err() {
                    break Err("SECURITY_REAL_CLEAN_CAPTURE_FAILED");
                }
                match child.try_wait() {
                    Ok(Some(status)) => {
                        self.cleanup_verified = true;
                        break if status.success() {
                            captures_within_limit(&stdout, &stderr)
                        } else {
                            Err("SECURITY_REAL_CLEAN_SETUP_FAILED")
                        };
                    }
                    Ok(None) => thread::sleep(POLL),
                    Err(_) => break Err("SECURITY_REAL_CLEAN_SETUP_FAILED"),
                }
            };
            if !self.cleanup_verified {
                self.cleanup_verified = stop_setup_child(&mut child);
            }
            result
        }
    }

    fn private_file(path: &Path) -> Result<File> {
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)
            .map_err(|_| "SECURITY_QUALIFICATION_PRIVATE_IO_FAILED")
    }

    fn captures_within_limit(stdout: &Path, stderr: &Path) -> Result<()> {
        // Polling and a final check bound accepted output, not instantaneous disk
        // growth. Captures are private setup diagnostics, never published.
        for path in [stdout, stderr] {
            if fs::metadata(path)
                .map_err(|_| "SECURITY_REAL_CLEAN_CAPTURE_FAILED")?
                .len()
                > CAPTURE_LIMIT
            {
                return Err("SECURITY_REAL_CLEAN_CAPTURE_FAILED");
            }
        }
        Ok(())
    }

    fn stop_setup_child(child: &mut Child) -> bool {
        let _ = child.kill();
        let started = Instant::now();
        while started.elapsed() < CLEANUP_GRACE {
            match child.try_wait() {
                Ok(Some(_)) => return true,
                Ok(None) => thread::sleep(POLL),
                Err(_) => return false,
            }
        }
        false
    }
}
