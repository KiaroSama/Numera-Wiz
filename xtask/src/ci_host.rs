//! Bounded setup for the ephemeral hosted Ubuntu GUI runners.
use anyhow::{Context, Result, bail};
use std::{
    io::Write,
    process::{Command, Stdio},
};

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
    // Hosted mirror+file sources selected an unavailable Azure DNS endpoint in
    // run 43034410109. Use the official HTTPS archives and existing Ubuntu keys;
    // retain the same suites/components instead of accepting stale indexes.
    let sources = "Types: deb\nURIs: https://archive.ubuntu.com/ubuntu/\nSuites: noble noble-updates noble-backports\nComponents: main restricted universe multiverse\nSigned-By: /usr/share/keyrings/ubuntu-archive-keyring.gpg\n\nTypes: deb\nURIs: https://security.ubuntu.com/ubuntu/\nSuites: noble-security\nComponents: main restricted universe multiverse\nSigned-By: /usr/share/keyrings/ubuntu-archive-keyring.gpg\n";
    tracing::info!(%selection, "Preparing official Ubuntu GUI dependency sources");
    let mut writer = Command::new("sudo")
        .args([
            "--non-interactive",
            "tee",
            "/etc/apt/sources.list.d/ubuntu.sources",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .context("Prepare hosted Ubuntu package sources")?;
    writer
        .stdin
        .take()
        .context("Package source input")?
        .write_all(sources.as_bytes())?;
    if !writer.wait()?.success() {
        bail!("Could not prepare the hosted Ubuntu package sources");
    }
    apt("180s", "update", &[])?;
    apt("240s", "install", packages)
}

fn apt(deadline: &str, operation: &str, packages: &[&str]) -> Result<()> {
    let mut command = Command::new("sudo");
    // The supervisor must run under the same uid as apt and its dpkg children.
    command
        .args([
            "--non-interactive",
            "timeout",
            "--signal=TERM",
            "--kill-after=10s",
            deadline,
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
        .args(packages);
    tracing::info!(argv=?command, "Installing bounded hosted GUI prerequisites");
    if !command
        .status()
        .context("Start hosted package setup")?
        .success()
    {
        bail!("Hosted Ubuntu {operation} failed or exceeded its deadline; GUI prerequisites remain unverified");
    }
    Ok(())
}
