//! CI-only development packages and exact-source workflow evidence.
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Component, Path},
    process::Command,
};

fn output(command: &mut Command) -> Result<Vec<u8>> {
    tracing::info!(argv = ?command, "Running development packaging tool");
    let result = command.output().context("start packaging tool")?;
    if !result.status.success() {
        bail!(
            "Packaging tool failed: {}",
            std::str::from_utf8(&result.stderr)
                .unwrap_or("<binary or non-UTF-8 packaging diagnostic>")
        );
    }
    Ok(result.stdout)
}

fn source(root: &Path) -> Result<String> {
    if std::env::var("GITHUB_ACTIONS").as_deref() != Ok("true") {
        bail!("Native packaging and workflow evidence belong in GitHub Actions");
    }
    let expected = std::env::var("NW_SOURCE_SHA").context("exact CI source identity")?;
    let actual = String::from_utf8(output(
        Command::new("git")
            .current_dir(root)
            .args(["rev-parse", "HEAD"]),
    )?)?;
    if expected.len() != 40
        || !expected.bytes().all(|byte| byte.is_ascii_hexdigit())
        || actual.trim() != expected
    {
        bail!("Checked-out source differs from the requested CI head");
    }
    Ok(expected)
}

fn digest(path: &Path) -> Result<Value> {
    let mut input = File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mut hash = Sha256::new();
    let mut bytes = 0u64;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = input.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
        bytes = bytes.checked_add(count as u64).context("file byte count")?;
    }
    Ok(json!({"bytes": bytes, "sha256": hex::encode(hash.finalize())}))
}

fn write_json(path: &Path, value: &Value) -> Result<()> {
    fs::write(path, serde_json::to_vec_pretty(value)?)?;
    Ok(())
}

fn copy_tree(source: &Path, destination: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(source)?;
    if metadata.is_dir() {
        fs::create_dir_all(destination)?;
        let mut children = fs::read_dir(source)?.collect::<std::io::Result<Vec<_>>>()?;
        children.sort_by_key(|entry| entry.file_name());
        for entry in children {
            copy_tree(&entry.path(), &destination.join(entry.file_name()))?;
        }
    } else if metadata.is_file() {
        fs::copy(source, destination)?;
    } else {
        bail!(
            "Package inputs must be ordinary files: {}",
            source.display()
        );
    }
    Ok(())
}

fn inventory(directory: &Path, root: &Path, rows: &mut Vec<Value>) -> Result<()> {
    let mut entries = fs::read_dir(directory)?.collect::<std::io::Result<Vec<_>>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        if entry.file_type()?.is_dir() {
            inventory(&entry.path(), root, rows)?;
        } else if entry.file_type()?.is_file() {
            let mut row = digest(&entry.path())?;
            row["path"] = json!(
                entry
                    .path()
                    .strip_prefix(root)?
                    .to_str()
                    .context("Package paths must be UTF-8")?
                    .replace('\\', "/")
            );
            rows.push(row);
        } else {
            bail!("Unexpected non-file package member");
        }
    }
    Ok(())
}

fn summary(message: &str) -> Result<()> {
    if let Some(path) = std::env::var_os("GITHUB_STEP_SUMMARY") {
        writeln!(
            OpenOptions::new().append(true).create(true).open(path)?,
            "{message}"
        )?;
    }
    Ok(())
}

/// Assemble review artifacts; this deliberately does not qualify or publish a release.
pub fn development(root: &Path) -> Result<()> {
    let source = source(root)?;
    let label = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => "linux-x64",
        ("windows", "x86_64") => "windows-x64",
        ("macos", "x86_64") => "macos-x64",
        ("macos", "aarch64") => "macos-arm64",
        _ => bail!("No development package layout for this host"),
    };
    let name = format!(
        "numera-wiz-{}-{label}-{}",
        env!("CARGO_PKG_VERSION"),
        &source[..12]
    );
    let staging = root.join("target/package-staging").join(label);
    if staging.exists() {
        fs::remove_dir_all(&staging)?;
    }
    let package = staging.join(&name);
    let delivery = root.join("target/development-packages");
    fs::create_dir_all(&package)?;
    fs::create_dir_all(&delivery)?;
    let executable = if cfg!(windows) {
        "numera-wiz.exe"
    } else {
        "numera-wiz"
    };
    let binary = root.join("target/debug").join(executable);
    let binary_identity = digest(&binary)?;
    let (binary_destination, resources) = if cfg!(target_os = "macos") {
        let contents = package.join("Numera Wiz.app/Contents");
        fs::create_dir_all(contents.join("MacOS"))?;
        fs::create_dir_all(contents.join("Resources"))?;
        fs::write(
            contents.join("Info.plist"),
            format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\"><dict><key>CFBundleExecutable</key><string>numera-wiz</string><key>CFBundleIdentifier</key><string>org.numerawiz.development</string><key>CFBundleName</key><string>Numera Wiz</string><key>CFBundlePackageType</key><string>APPL</string><key>CFBundleShortVersionString</key><string>{}.{}.{}</string><key>CFBundleVersion</key><string>1</string><key>LSMinimumSystemVersion</key><string>15.0</string><key>NSHighResolutionCapable</key><true/></dict></plist>\n",
                env!("CARGO_PKG_VERSION_MAJOR"),
                env!("CARGO_PKG_VERSION_MINOR"),
                env!("CARGO_PKG_VERSION_PATCH")
            ),
        )?;
        (
            contents.join("MacOS/numera-wiz"),
            contents.join("Resources"),
        )
    } else {
        (package.join(executable), package.clone())
    };
    fs::copy(&binary, &binary_destination)?;
    for file in [
        "LICENSE",
        "THIRD-PARTY-NOTICES.md",
        "README.md",
        "SECURITY.md",
        "Cargo.lock",
    ] {
        fs::copy(root.join(file), resources.join(file))?;
    }
    for directory in [
        "docs/manuals",
        "docs/development",
        "docs/generated",
        "docs/vendor/ffmpeg",
        "examples",
        "components",
    ] {
        copy_tree(&root.join(directory), &resources.join(directory))?;
    }
    let branding = root.join("assets/branding");
    if branding.exists() {
        copy_tree(&branding, &resources.join("assets/branding"))?;
    }
    let web_path = root.join("target/web-ui/manifest.json");
    let web: Value = serde_json::from_slice(&fs::read(&web_path)?)?;
    if web["source"].as_str() != Some(source.as_str()) {
        bail!("Packaged browser assets differ from the native source");
    }
    for asset in web["assets"].as_array().context("web asset records")? {
        let name = asset["path"].as_str().context("web asset path")?;
        let path = Path::new(name);
        if path.is_absolute()
            || !path
                .components()
                .all(|part| matches!(part, Component::Normal(_)))
        {
            bail!("Unsafe browser asset path");
        }
        let observed = digest(&root.join("target/web-ui").join(path))?;
        if observed["sha256"] != asset["sha256"] || observed["bytes"] != asset["bytes"] {
            bail!("Browser asset identity mismatch: {name}");
        }
    }
    fs::write(
        resources.join("embedded-web-manifest.json"),
        fs::read(web_path)?,
    )?;
    let metadata: Value =
        serde_json::from_slice(&output(Command::new("cargo").current_dir(root).args([
            "metadata",
            "--locked",
            "--format-version",
            "1",
        ]))?)?;
    let dependencies: Vec<_> = metadata["packages"].as_array().context("Cargo dependency inventory")?
        .iter().map(|row| json!({"id": row["id"], "name": row["name"], "version": row["version"], "source": row["source"], "license": row["license"]})).collect();
    write_json(
        &resources.join("dependency-inventory.json"),
        &json!({
            "schema_version": 1, "source_sha": source, "packages": dependencies,
            "scope": "Cargo resolved dependency inventory; not a complete third-party license or corresponding-source attestation"
        }),
    )?;
    fs::write(
        package.join("DEVELOPMENT-ARTIFACT.md"),
        format!(
            "# Numera Wiz development artifact\n\nSource: {source}\nTarget: {label}\n\nThis is an unsigned, unnotarized development build, not a qualified release.\nThe shared browser WASM and assets are embedded in the executable; the manifest\nis included for inspection. No browser asset download is required.\n\nThe required standard FFmpeg/ffprobe profile, private native dependencies,\noffline fonts, complete third-party notices and corresponding-source closure\nare NOT bundled or qualified. Media work requires an explicitly selected\nadmitted component directory. The Linux CI fixture profile is test-only and\nis distributed separately, not as a standard profile.\n\nThe Cargo inventory is a development inventory, not a complete release SBOM.\nThe source archive contains tracked first-party sources and archived evidence;\nit does not replace dependency corresponding-source obligations.\n\nWindows: extract the ZIP, then start numera-wiz.exe.\nLinux: extract the tar.gz, then run ./numera-wiz from a writable directory.\nmacOS: extract the tar.gz, then launch\n\"Numera Wiz.app/Contents/MacOS/numera-wiz\" --data-dir \"$HOME/Library/Application Support/Numera Wiz\".\nUse an explicit writable --data-dir outside the app bundle for GUI, CLI and MCP.\nFinder launch without this selection is not qualified: the current default is\nthe executable directory, which would place state inside the app bundle. Gatekeeper may require manual approval\nfor this unsigned application. See docs/manuals under the package resources\n(on macOS: Numera Wiz.app/Contents/Resources/docs/manuals).\n\nPackage assembly does not establish offline-first-launch, complete media,\nportable-root, Windows 11, macOS interaction or GPU qualification.\n"
        ),
    )?;
    let mut files = Vec::new();
    inventory(&package, &package, &mut files)?;
    let compiler = String::from_utf8(output(
        Command::new("rustc").arg("--version").arg("--verbose"),
    )?)?;
    let provenance = json!({
        "schema_version": 1, "kind": "unqualified-development", "source_sha": source,
        "event_sha": std::env::var("GITHUB_SHA").ok(), "target": label,
        "run_url": format!("{}/{}/actions/runs/{}", std::env::var("GITHUB_SERVER_URL")?, std::env::var("GITHUB_REPOSITORY")?, std::env::var("GITHUB_RUN_ID")?),
        "run_attempt": std::env::var("GITHUB_RUN_ATTEMPT")?, "compiler": compiler,
        "native_features": ["desktop", "embedded-web"], "native_profile": "dev with debuginfo stripped",
        "executable": binary_identity, "lockfile": digest(&root.join("Cargo.lock"))?,
        "embedded_web": web, "standard_ffmpeg_profile": "not bundled; release gate open",
        "signing": "unsigned", "notarization": "not notarized", "release_ready": false,
        "files": files
    });
    write_json(&package.join("package-manifest.json"), &provenance)?;
    let extension = if cfg!(windows) { "zip" } else { "tar.gz" };
    let archive = delivery.join(format!("{name}.{extension}"));
    let mut tar = Command::new("tar");
    if cfg!(windows) {
        tar.args(["-a", "-cf"]);
    } else {
        tar.arg("-czf");
    }
    output(tar.arg(&archive).arg("-C").arg(&staging).arg(&name))?;
    let source_archive = delivery.join(format!("{name}-source.tar.gz"));
    output(
        Command::new("git")
            .current_dir(root)
            .args(["archive", "--format=tar.gz"])
            .arg(format!("--prefix=numera-wiz-{}/", &source[..12]))
            .arg("--output")
            .arg(&source_archive)
            .arg(&source),
    )?;
    let provenance_path = delivery.join(format!("{name}-provenance.json"));
    write_json(&provenance_path, &provenance)?;
    let mut checksums = String::new();
    for path in [&archive, &source_archive, &provenance_path] {
        let identity = digest(path)?;
        checksums.push_str(&format!(
            "{}  {}\n",
            identity["sha256"].as_str().context("archive digest")?,
            path.file_name()
                .context("archive name")?
                .to_str()
                .context("Archive names must be UTF-8")?
        ));
    }
    fs::write(delivery.join(format!("{name}-SHA256SUMS")), checksums)?;
    summary(&format!(
        "Development package `{name}` built from `{source}` with embedded same-source browser assets. Standard FFmpeg profile, signing, complete license/source closure and package qualification remain open. SHA-256 manifests accompany the archives."
    ))?;
    tracing::warn!(path = %archive.display(), "Created an unqualified native development artifact");
    Ok(())
}

/// Record this attempt's dependency results without claiming the still-running job passed.
pub fn evidence(root: &Path) -> Result<()> {
    let source = source(root)?;
    let dependencies: Value = serde_json::from_str(
        &std::env::var("NW_REQUIRED_RESULTS_JSON").unwrap_or_else(|_| "{}".into()),
    )?;
    let status = std::env::var("NW_JOB_STATUS").context("current job status")?;
    let evidence = json!({
        "schema_version": 1, "source_sha": source,
        "event_sha": std::env::var("GITHUB_SHA").ok(),
        "run_id": std::env::var("GITHUB_RUN_ID")?, "run_attempt": std::env::var("GITHUB_RUN_ATTEMPT")?,
        "job_id": std::env::var("GITHUB_JOB")?, "runner_os": std::env::var("RUNNER_OS")?, "runner_arch": std::env::var("RUNNER_ARCH")?,
        "current_job_status_before_evidence_upload": status,
        "dependencies_from_this_attempt": dependencies,
        "required_expanded_jobs": ["dependency-lock", "formatting", "web", "browser", "native-ui", "checks (ubuntu-24.04)", "checks (windows-2025)", "checks (macos-15)", "checks (macos-15-intel)", "media", "owner-batch", "owner-transports", "artifact-http"],
        "complete_workflow_conclusion": "pending; inspect all 13 final jobs for this exact run attempt and source SHA",
        "release_ready": false
    });
    let directory = root.join("target/ci-evidence");
    fs::create_dir_all(&directory)?;
    write_json(&directory.join("workflow-evidence.json"), &evidence)?;
    summary(&format!(
        "Exact checked-out head: `{source}`. Run attempt: `{}`. Required job inventory: 13 expanded jobs. Current job status before evidence upload: `{status}`. Dependency results from this attempt:\n\n```json\n{}\n```\n\nThis record is provisional until this job finishes. Final verification must use all 13 jobs of this exact run attempt, with no ancestor-run substitution. A green CI run does not qualify the full v1 release.",
        std::env::var("GITHUB_RUN_ATTEMPT")?,
        serde_json::to_string_pretty(&dependencies)?
    ))?;
    Ok(())
}
