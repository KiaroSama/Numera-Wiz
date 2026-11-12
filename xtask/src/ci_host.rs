//! Bounded setup for the ephemeral hosted Ubuntu GUI runners.
use anyhow::{Context, Result, bail};
use std::{
    io::Write,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

const PREFETCH_BUDGET: Duration = Duration::from_secs(300);
const INSTALL_BUDGET: Duration = Duration::from_secs(250);
const KILL_GRACE: Duration = Duration::from_secs(10);

pub fn linux_dependencies() -> Result<()> {
    if std::env::var("GITHUB_ACTIONS").as_deref() != Ok("true")
        || std::env::consts::OS != "linux"
        || std::env::consts::ARCH != "x86_64"
    {
        bail!("GUI host package setup is restricted to the hosted Linux x64 CI jobs");
    }
    let release = std::fs::read_to_string("/etc/os-release")?;
    if !release.lines().any(|line| line == "ID=ubuntu")
        || !release.lines().any(|line| line == "VERSION_ID=\"24.04\"")
    {
        bail!("The GUI host package source is defined only for Ubuntu 24.04");
    }
    let selection = std::env::args()
        .nth(2)
        .context("Select desktop, browser or native packages")?;
    let packages: &[&str] = match selection.as_str() {
        "desktop" => &[
            "libxkbcommon-dev",
            "libwayland-dev",
            "libegl1-mesa-dev",
            "libgl1-mesa-dev",
        ],
        "browser" => &[
            "xvfb",
            "xdotool",
            "openbox",
            "imagemagick",
            "dbus-x11",
            "xdg-desktop-portal",
            "xdg-desktop-portal-gtk",
        ],
        "native" => &[
            "xvfb",
            "xdotool",
            "openbox",
            "imagemagick",
            "libxkbcommon0",
            "libxkbcommon-x11-0",
            "libegl1",
            "libgl1-mesa-dri",
            "dbus-x11",
            "xdg-desktop-portal",
            "xdg-desktop-portal-gtk",
        ],
        _ => bail!("Unknown hosted GUI dependency selection"),
    };
    // Run 43034410109 lost the Azure mirror; run 43040412914 also lost both
    // ubuntu.com DNS names. Independent HTTPS mirrors retain Ubuntu's signed
    // release/package hash chain. Never accept stale indexes or bypass signing.
    let prefetch_started = Instant::now();
    for (archive, security) in [
        (
            "https://mirrors.edge.kernel.org/ubuntu/",
            "https://mirrors.edge.kernel.org/ubuntu/",
        ),
        (
            "https://mirrors.mit.edu/ubuntu/",
            "https://mirrors.mit.edu/ubuntu/",
        ),
        (
            "https://archive.ubuntu.com/ubuntu/",
            "https://security.ubuntu.com/ubuntu/",
        ),
    ] {
        if let Err(error) = prepare_sources(prefetch_started, archive, security) {
            tracing::warn!(archive, phase = "prepare", %error, "Ubuntu GUI prerequisite prefetch failed; trying the next authenticated mirror");
            continue;
        }
        if let Err(error) = apt(prefetch_started, PREFETCH_BUDGET, 90, "update", &[], &[]) {
            tracing::warn!(archive, phase = "update", %error, "Ubuntu GUI prerequisite prefetch failed; trying the next authenticated mirror");
            continue;
        }
        tracing::info!(%selection, archive, "Authenticated Ubuntu GUI dependency indexes are ready");
        if let Err(error) = apt(
            prefetch_started,
            PREFETCH_BUDGET,
            240,
            "install",
            &["--download-only"],
            packages,
        ) {
            tracing::warn!(archive, phase = "download", %error, "Ubuntu GUI prerequisite prefetch failed; trying the next authenticated mirror");
            continue;
        }
        tracing::info!(%selection, archive, "Authenticated Ubuntu GUI dependency archives are ready");
        // Only downloads may fall back. Once dpkg starts, any failure is final.
        return apt(
            Instant::now(),
            INSTALL_BUDGET,
            240,
            "install",
            &["--no-download"],
            packages,
        );
    }
    bail!(
        "No Ubuntu mirror supplied complete authenticated indexes and archives within the shared prefetch budget; GUI prerequisites remain unverified"
    )
}

fn prepare_sources(started: Instant, archive: &str, security: &str) -> Result<()> {
    let sources = format!(
        "Types: deb\nURIs: {archive}\nSuites: noble noble-updates noble-backports\nComponents: main restricted universe multiverse\nSigned-By: /usr/share/keyrings/ubuntu-archive-keyring.gpg\n\nTypes: deb\nURIs: {security}\nSuites: noble-security\nComponents: main restricted universe multiverse\nSigned-By: /usr/share/keyrings/ubuntu-archive-keyring.gpg\n"
    );
    tracing::info!(
        archive,
        security,
        "Preparing authenticated Ubuntu GUI dependency sources"
    );
    let mut writer = supervised_command(started, PREFETCH_BUDGET, 5)?
        .args(["tee", "/etc/apt/sources.list.d/ubuntu.sources"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .context("Prepare hosted Ubuntu package sources")?;
    let input_result = writer
        .stdin
        .take()
        .context("Package source input")
        .and_then(|mut input| {
            input
                .write_all(sources.as_bytes())
                .context("Write hosted Ubuntu package sources")
        });
    // Close stdin before waiting, and reap the supervised writer even if input fails.
    let status = writer.wait().context("Wait for hosted package source setup")?;
    input_result?;
    if !status.success() {
        bail!("Could not prepare the hosted Ubuntu package sources");
    }
    Ok(())
}

fn supervised_command(started: Instant, budget: Duration, maximum_work: u64) -> Result<Command> {
    let remaining = budget
        .checked_sub(started.elapsed())
        .and_then(|remaining| remaining.checked_sub(KILL_GRACE))
        .context("Hosted Ubuntu package phase has exhausted its shared deadline")?;
    // Round down so the root-owned supervisor's kill grace fits the phase budget.
    let work_seconds = remaining.as_secs().min(maximum_work);
    if work_seconds == 0 {
        bail!("Hosted Ubuntu package phase has insufficient time for another command");
    }
    let mut command = Command::new("sudo");
    // The supervisor must run under the same uid as apt and its dpkg children.
    command
        .args([
            "--non-interactive",
            "timeout",
            "--signal=TERM",
            "--kill-after=10s",
        ])
        .arg(format!("{work_seconds}s"));
    Ok(command)
}

fn apt(
    started: Instant,
    budget: Duration,
    maximum_work: u64,
    operation: &str,
    options: &[&str],
    packages: &[&str],
) -> Result<()> {
    let mut command = supervised_command(started, budget, maximum_work)?;
    command
        .args([
            "apt-get",
            "-o",
            "Dir::Etc::sourcelist=/etc/apt/sources.list.d/ubuntu.sources",
            "-o",
            "Dir::Etc::sourceparts=-",
            "-o",
            "APT::Update::Error-Mode=any",
            "-o",
            "Acquire::Retries=2",
            "-o",
            "Acquire::https::Timeout=20",
            "-o",
            "Acquire::http::Timeout=20",
            "-o",
            "Dpkg::Use-Pty=0",
            "-y",
            operation,
        ])
        .args(options)
        .args(packages);
    tracing::info!(argv=?command, "Installing bounded hosted GUI prerequisites");
    if !command
        .status()
        .context("Start hosted package setup")?
        .success()
    {
        bail!(
            "Hosted Ubuntu {operation} failed or exceeded its deadline; GUI prerequisites remain unverified"
        );
    }
    Ok(())
}
