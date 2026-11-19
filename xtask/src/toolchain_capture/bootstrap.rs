//! Verify the dated compiler package closure before any offline installation.
use super::{CONFIG, metadata, process, standard_linux};
use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::Path,
    process::Command,
    time::{Duration, Instant},
};

const LOCK: &str = include_str!("../../../components/linux-toolchain-package-lock.json");
const SOURCE_LOCK: &str = include_str!("../../../components/linux-standard-source-lock.json");
const PACKAGE_LIMIT: u64 = 32 * 1024 * 1024;
const CLOSURE_LIMIT: u64 = 192 * 1024 * 1024;
const GUEST_PACKAGES: &str = "/verified-packages";
const GUEST_XTASK: &str = "/bootstrap-xtask";
const GUEST_FAILURE: &str = "/component-failure.json";

struct Package<'a> {
    filename: &'a str,
    name: &'a str,
    architecture: &'a str,
    version: &'a str,
    size: u64,
    sha256: &'a str,
}

struct Tool<'a> {
    version: &'a str,
    url: &'a str,
    filename: &'a str,
    size: u64,
    sha256: &'a str,
}

fn text<'a>(value: &'a Value, field: &str) -> Result<&'a str> {
    value[field]
        .as_str()
        .with_context(|| format!("Missing package lock field {field}"))
}

fn number(value: &Value, field: &str) -> Result<u64> {
    value[field]
        .as_u64()
        .with_context(|| format!("Invalid package lock integer {field}"))
}

fn load<'a>(lock: &'a Value, capture: &Value) -> Result<Vec<Package<'a>>> {
    ensure!(
        lock["schema_version"] == 1
            && lock["base"] == capture["image"]
            && lock["platform"] == capture["platform"]
            && lock["snapshot"] == capture["snapshot"],
        "The package lock must supplement the same OCI image and Ubuntu snapshot"
    );
    for field in [
        "base_status_sha256",
        "base_inventory_sha256",
        "final_inventory_sha256",
    ] {
        metadata::sha256(text(lock, field)?)?;
    }
    let rows = lock["packages"]
        .as_array()
        .context("Missing package closure")?;
    let count = number(lock, "package_count")?;
    ensure!(
        (1..=256).contains(&count)
            && count == rows.len() as u64
            && (count..=384).contains(&number(lock, "final_inventory_count")?),
        "Package and final inventory counts exceed their admitted bounds"
    );
    let mut paths = BTreeSet::new();
    let mut identities = BTreeSet::new();
    let mut total = 0;
    let mut packages = Vec::with_capacity(rows.len());
    for row in rows {
        let path = text(row, "filename")?;
        ensure!(
            path.starts_with("pool/")
                && path.ends_with(".deb")
                && path.split('/').all(|part| !matches!(part, "" | "." | ".."))
                && path
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b".+~-_/".contains(&byte)),
            "Unsafe snapshot package path"
        );
        let name = text(row, "name")?;
        let version = text(row, "version")?;
        let architecture = text(row, "architecture")?;
        ensure!(
            !name.is_empty()
                && name.bytes().all(|byte| byte.is_ascii_lowercase()
                    || byte.is_ascii_digit()
                    || b".+-".contains(&byte))
                && !version.is_empty()
                && version
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b".+:~-".contains(&byte))
                && matches!(architecture, "amd64" | "all"),
            "Invalid Debian package identity"
        );
        let basename = path.rsplit('/').next().context("Package basename")?;
        ensure!(
            paths.insert(basename) && identities.insert((name, architecture)),
            "Duplicate package"
        );
        let size = number(row, "size")?;
        ensure!(
            (1..=PACKAGE_LIMIT).contains(&size),
            "Package payload exceeds its bound"
        );
        total = process::check_count(total, size, CLOSURE_LIMIT)?;
        let sha256 = text(row, "sha256")?;
        metadata::sha256(sha256)?;
        packages.push(Package {
            filename: path,
            name,
            architecture,
            version,
            size,
            sha256,
        });
    }
    ensure!(
        total == number(lock, "download_bytes")?,
        "Package closure byte count differs"
    );
    Ok(packages)
}

fn load_nasm(capture: &Value) -> Result<Tool<'_>> {
    let rows = capture["external_build_tool_inputs"]
        .as_array()
        .context("Missing external build tool inputs")?;
    let mut matching = rows.iter().filter(|row| row["name"] == "nasm");
    let row = matching.next().context("Missing NASM build input")?;
    ensure!(matching.next().is_none(), "Multiple NASM build inputs");
    let version = text(row, "version")?;
    let url = text(row, "url")?;
    let filename = url.rsplit('/').next().context("NASM archive filename")?;
    ensure!(
        !version.is_empty()
            && version
                .bytes()
                .all(|byte| byte.is_ascii_digit() || byte == b'.')
            && filename == format!("nasm-{version}.tar.xz")
            && url == format!("https://www.nasm.us/pub/nasm/releasebuilds/{version}/{filename}"),
        "Unexpected NASM source archive"
    );
    let size = number(row, "bytes")?;
    ensure!(
        (1..=process::FILE_LIMIT).contains(&size),
        "NASM archive exceeds its bound"
    );
    let sha256 = text(row, "sha256")?;
    metadata::sha256(sha256)?;
    Ok(Tool {
        version,
        url,
        filename,
        size,
        sha256,
    })
}

pub(super) fn verify(path: &Path, size: u64, expected_sha256: &str) -> Result<()> {
    let mut file = File::open(path)?;
    ensure!(
        file.metadata()?.len() == size,
        "Archive size differs for {}",
        path.display()
    );
    let mut hash = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    let mut total = 0;
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        total = process::check_count(total, count as u64, size)?;
        hash.update(&buffer[..count]);
    }
    ensure!(
        total == size && hex::encode(hash.finalize()) == expected_sha256,
        "Archive digest differs for {}",
        path.display()
    );
    Ok(())
}

fn download(
    url: &str,
    output: &Path,
    size: u64,
    expected_sha256: &str,
    deadline: Instant,
    allow_https_redirect: bool,
) -> Result<()> {
    let mut partial_name = output
        .file_name()
        .context("Missing download filename")?
        .to_owned();
    partial_name.push(".partial");
    let partial = output.with_file_name(partial_name);
    let length = size.to_string();
    let seconds = if size > 64 * 1024 * 1024 { 120 } else { 55 };
    let transfer_seconds = seconds.to_string();
    let mut command = process::command("/usr/bin/curl");
    command.args([
        "--disable",
        "--silent",
        "--show-error",
        "--fail",
        "--proto",
        "=https",
        "--max-time",
        &transfer_seconds,
        "--max-filesize",
        &length,
    ]);
    if allow_https_redirect {
        command.args(["--location", "--max-redirs", "5", "--proto-redir", "=https"]);
    }
    command.args(["--", url]);
    let response = process::run(&mut command, deadline, seconds + 5, Some(&partial))?;
    if response.failure.is_none() && response.status.code() == Some(28) {
        let initial = response.success().expect_err("curl transfer timed out");
        fs::remove_file(&partial).context("Discard the incomplete locked archive before retry")?;
        tracing::warn!(
            archive = ?output.file_name(),
            "Locked archive transfer timed out, retrying once"
        );
        process::run(&mut command, deadline, seconds + 5, Some(&partial))?
            .success()
            .with_context(|| {
                format!("Retry fetch {}; initial transfer: {initial}", output.display())
            })?;
    } else {
        response
            .success()
            .with_context(|| format!("Fetch {}", output.display()))?;
    }
    verify(&partial, size, expected_sha256)?;
    fs::rename(&partial, output)?;
    Ok(())
}

fn inventory(deadline: Instant) -> Result<(String, usize)> {
    let found = process::run(
        process::command("/usr/bin/dpkg-query").args([
            "--show",
            "--showformat=${Package}\\t${Version}\\t${Architecture}\\t${Status}\\n",
        ]),
        deadline,
        10,
        None,
    )?;
    found.success()?;
    let mut records = BTreeSet::new();
    for line in found.text()?.lines() {
        let (identity, status) = line
            .rsplit_once('\t')
            .context("Invalid installed package record")?;
        if status == "install ok installed" {
            ensure!(
                identity.split('\t').count() == 3 && records.insert(identity),
                "Duplicate or malformed installed package record"
            );
        }
    }
    let mut hash = Sha256::new();
    for record in &records {
        hash.update(record.as_bytes());
        hash.update(b"\n");
    }
    Ok((hex::encode(hash.finalize()), records.len()))
}

pub(super) fn run_tool(
    command: &mut Command,
    deadline: Instant,
    seconds: u64,
) -> Result<process::Output> {
    let output = process::run(command, deadline, seconds, None)?;
    output.success()?;
    Ok(output)
}

fn build_nasm(tool: &Tool<'_>, deadline: Instant) -> Result<Value> {
    let source = Path::new("/compiler-tools/nasm");
    let archive = Path::new(GUEST_PACKAGES).join(tool.filename);
    verify(&archive, tool.size, tool.sha256)?;
    fs::create_dir_all(source)?;
    run_tool(
        process::command("/usr/bin/tar")
            .current_dir(source)
            .args([
                "--extract",
                "--no-same-owner",
                "--strip-components=1",
                "--file",
            ])
            .arg(&archive),
        deadline,
        20,
    )?;
    run_tool(
        process::command("/bin/sh")
            .current_dir(source)
            .env("CC", "/usr/bin/gcc-13")
            .env("CXX", "/usr/bin/g++-13")
            .args(["./configure", "--prefix=/compiler-tools/nasm/install"]),
        deadline,
        60,
    )?;
    run_tool(
        process::command("/usr/bin/make")
            .current_dir(source)
            .env("CC", "/usr/bin/gcc-13")
            .env("CXX", "/usr/bin/g++-13")
            .args(["-j4", "install"]),
        deadline,
        160,
    )?;
    let observed = run_tool(
        process::command("/compiler-tools/nasm/install/bin/nasm").arg("--version"),
        deadline,
        10,
    )?;
    let version = observed
        .text()?
        .lines()
        .next()
        .context("Missing NASM version")?;
    ensure!(
        version.len() <= 256 && version.contains(tool.version),
        "NASM reported a different version"
    );
    Ok(json!({"version":version,"archive_sha256":tool.sha256}))
}

fn guest(lock: &Value, packages: &[Package<'_>], nasm: &Tool<'_>) -> Result<()> {
    ensure!(
        std::env::var("NW_TOOLCHAIN_BOOTSTRAP_WORKER").as_deref() == Ok("1")
            && std::env::current_exe()? == Path::new(GUEST_XTASK)
            && Path::new("/.dockerenv").is_file(),
        "Offline package install requires the copied worker in the selected OCI base"
    );
    let deadline = Instant::now() + Duration::from_secs(1320);
    let base_status = fs::read("/var/lib/dpkg/status").context("Read the offline OCI baseline")?;
    ensure!(
        metadata::digest(&base_status) == text(lock, "base_status_sha256")?
            && inventory(deadline)?.0 == text(lock, "base_inventory_sha256")?,
        "Installed base package identities differ from the reviewed OCI capture"
    );
    let mut install = process::command("/usr/bin/apt-get");
    install.env("DEBIAN_FRONTEND", "noninteractive").args([
        "-o",
        "Dir::Etc::sourcelist=/dev/null",
        "-o",
        "Dir::Etc::sourceparts=/nonexistent",
        "--no-install-recommends",
        "--no-install-suggests",
        "--no-remove",
        "--assume-yes",
        "install",
    ]);
    for package in packages {
        let name = Path::new(package.filename)
            .file_name()
            .context("Package filename")?;
        let path = Path::new(GUEST_PACKAGES).join(name);
        verify(&path, package.size, package.sha256)?;
        install.arg(path);
    }
    let installed = process::run(&mut install, deadline, 150, None)?;
    installed
        .success()
        .context("Offline Debian package installation")?;
    let (final_hash, count) = inventory(deadline)?;
    ensure!(
        final_hash == text(lock, "final_inventory_sha256")?
            && count as u64 == number(lock, "final_inventory_count")?,
        "Installed closure differs from the reviewed package inventory"
    );
    let mut observed = serde_json::Map::new();
    for program in [
        "/usr/bin/gcc-13",
        "/usr/bin/g++-13",
        "/usr/bin/make",
        "/usr/bin/pkgconf",
        "/usr/bin/python3",
    ] {
        let result = process::run(
            process::command(program).arg("--version"),
            deadline,
            5,
            None,
        )?;
        result.success()?;
        let version = result
            .text()?
            .lines()
            .next()
            .context("Tool did not identify its version")?;
        ensure!(
            !version.is_empty() && version.len() <= 256,
            "Compiler version exceeds its bound"
        );
        observed.insert(program.into(), json!(version));
    }
    let nasm = build_nasm(nasm, deadline).context("Bootstrap NASM inside the offline builder")?;
    let profile = standard_linux::build(Path::new(GUEST_PACKAGES), &lock["ffmpeg"], deadline)
        .context("Compile and qualify the offline Linux media runtime")?;
    println!(
        "{}",
        json!({
            "final_inventory_sha256":final_hash,
            "package_count":count,
            "package_manager_output_sha256":metadata::digest(&installed.stdout),
            "programs":observed,
            "nasm":nasm,
            "ffmpeg":profile,
        })
    );
    Ok(())
}

fn offline_install(directory: &Path, delivery: &Path, lock: &Value) -> Result<Value> {
    let image = text(lock, "base")?;
    let deadline = Instant::now() + Duration::from_secs(1470);
    let docker = |args: &[&str], seconds: u64| -> Result<process::Output> {
        let result = process::run(
            process::command("/usr/bin/docker").args(args),
            deadline,
            seconds,
            None,
        )?;
        result.success()?;
        Ok(result)
    };
    docker(&["pull", "--platform", "linux/amd64", image], 100)
        .context("Pull the digest-locked Linux build image")?;
    let mount = format!(
        "type=bind,src={},dst={GUEST_PACKAGES},readonly",
        directory.display()
    );
    let created = docker(
        &[
            "create",
            "--platform",
            "linux/amd64",
            "--pull",
            "never",
            "--network",
            "none",
            "--memory",
            "3g",
            "--cpus",
            "4",
            "--pids-limit",
            "128",
            "--security-opt",
            "no-new-privileges",
            "--mount",
            &mount,
            "--env",
            "NW_TOOLCHAIN_BOOTSTRAP_WORKER=1",
            "--entrypoint",
            GUEST_XTASK,
            image,
            "build-standard-linux-profile",
            "--offline-install-worker",
        ],
        15,
    )?;
    let container = created.text()?.trim();
    metadata::sha256(container).context("Identify the temporary offline compiler container")?;
    let result = (|| {
        let executable = std::env::current_exe()?;
        docker(
            &[
                "cp",
                executable
                    .to_str()
                    .context("Compiler worker executable path")?,
                &format!("{container}:{GUEST_XTASK}"),
            ],
            25,
        )?;
        let output = docker(&["start", "--attach", container], 1335)
            .context("Run the locked compiler worker with networking disabled")?;
        let guest: Value =
            serde_json::from_slice(&output.stdout).context("Decode the offline compiler result")?;
        ensure!(
            guest["final_inventory_sha256"] == lock["final_inventory_sha256"]
                && guest["package_count"] == lock["final_inventory_count"],
            "Offline compiler report differs from the reviewed inventory"
        );
        docker(
            &[
                "cp",
                &format!("{container}:{}", standard_linux::DELIVERY),
                delivery
                    .to_str()
                    .context("Standard profile delivery path")?,
            ],
            40,
        )?;
        Ok(guest)
    })();
    if result.is_err()
        && let Some(parent) = delivery.parent()
    {
        let retained = parent.join("worker-failure.json");
        if let Some(retained) = retained.to_str() {
            let _ = docker(
                &["cp", &format!("{container}:{GUEST_FAILURE}"), retained],
                10,
            );
        }
    }
    let mut cleanup = process::command("/usr/bin/docker");
    cleanup.args(["rm", "--force", container]);
    let cleanup = process::run(
        &mut cleanup,
        Instant::now() + Duration::from_secs(15),
        15,
        None,
    )?;
    if result.is_ok() {
        cleanup
            .success()
            .context("Remove the completed offline compiler container")?;
    } else if cleanup.success().is_err() {
        tracing::warn!("Failed to confirm removal of the temporary offline compiler container");
    }
    result
}

fn qualify_relocated(delivery: &Path, image: &str) -> Result<Value> {
    let mount = format!(
        "type=bind,src={},dst=/relocated/media,readonly",
        delivery.display()
    );
    let deadline = Instant::now() + Duration::from_secs(55);
    let run = |executable, arguments: &[&str]| {
        run_tool(
            process::command("/usr/bin/docker")
                .args([
                    "run",
                    "--rm",
                    "--pull",
                    "never",
                    "--platform",
                    "linux/amd64",
                    "--network",
                    "none",
                    "--memory",
                    "512m",
                    "--cpus",
                    "2",
                    "--pids-limit",
                    "64",
                    "--mount",
                    &mount,
                    "--entrypoint",
                    executable,
                    image,
                ])
                .args(arguments),
            deadline,
            20,
        )
    };
    let video = run(
        "/relocated/media/bin/ffprobe",
        &[
            "-v",
            "error",
            "-show_entries",
            "stream=codec_name,width,height",
            "-of",
            "json",
            "/relocated/media/qualification/h264.mkv",
        ],
    )?;
    let video: Value = serde_json::from_slice(&video.stdout)?;
    ensure!(
        video["streams"] == json!([{"codec_name":"h264","width":128,"height":128}]),
        "Fresh container could not inspect the relocated H.264 fixture"
    );
    let subtitles = run(
        "/relocated/media/bin/ffmpeg",
        &["-hide_banner", "-h", "filter=subtitles"],
    )?;
    ensure!(
        subtitles.text()?.contains("wrap_unicode"),
        "Bundled libass has no Unicode-wrap support"
    );
    fs::write(
        delivery.join("qualification/shape.vgs"),
        b"rect 16 16 80 80\nsetcolor red\nfill\n",
    )?;
    fs::write(
        delivery.join("qualification/subtitles.srt"),
        "1\n00:00:00,000 --> 00:00:00,800\nسلام دنیا · Numera Wiz\n",
    )?;
    let render = |filter: Option<&str>| {
        let mut arguments = vec![
            "-hide_banner",
            "-nostdin",
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "color=size=128x128:rate=4:duration=1",
        ];
        if let Some(filter) = filter {
            arguments.extend(["-vf", filter]);
        }
        arguments.extend([
            "-frames:v",
            "4",
            "-pix_fmt",
            "yuv420p",
            "-f",
            "framehash",
            "-hash",
            "sha256",
            "-",
        ]);
        run("/relocated/media/bin/ffmpeg", &arguments)
    };
    let baseline = render(None)?;
    let mut effects = serde_json::Map::new();
    for (name, filter) in [
        (
            "text",
            "drawtext=fontfile=/relocated/media/fonts/DejaVuSans.ttf:text=Numera:fontcolor=white:x=8:y=8",
        ),
        (
            "vector",
            "drawvg=file=/relocated/media/qualification/shape.vgs",
        ),
        (
            "subtitles",
            "subtitles=filename=/relocated/media/qualification/subtitles.srt:fontsdir=/relocated/media/fonts:force_style='FontName=DejaVu Sans':wrap_unicode=1",
        ),
    ] {
        let rendered = render(Some(filter))?;
        let frames = rendered
            .text()?
            .lines()
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .collect::<Vec<_>>();
        let baseline_frames = baseline
            .text()?
            .lines()
            .filter(|line| !line.is_empty() && !line.starts_with('#'));
        ensure!(
            frames.len() == 4 && !frames.iter().copied().eq(baseline_frames),
            "Fresh container produced no {name} pixels with the bundled component"
        );
        fs::write(
            delivery.join(format!("qualification/offline-{name}.framehash.txt")),
            &rendered.stdout,
        )?;
        effects.insert(
            name.into(),
            json!({"frames":frames.len(),"sha256":metadata::digest(&rendered.stdout)}),
        );
    }
    Ok(
        json!({"base":image,"network":"none","subtitles_wrap_unicode":true,"effects":effects,"probed":video}),
    )
}

fn source_closure(profile: &Value, snapshot: &str) -> Result<Value> {
    let locked: Value = serde_json::from_str(SOURCE_LOCK)?;
    ensure!(
        locked["snapshot"] == snapshot,
        "Corresponding sources use a different Ubuntu snapshot"
    );
    let libraries = profile["runtime"]["package_provenance"]
        .as_object()
        .context("Missing native source identities")?;
    let native = libraries
        .values()
        .chain(std::iter::once(&profile["font"]["provenance"]));
    let observed = native
        .map(|row| Ok((text(row, "source")?, text(row, "source_version")?)))
        .collect::<Result<BTreeSet<_>>>()?;
    let sources = locked["source_packages"]
        .as_array()
        .context("Missing corresponding-source lock")?;
    let expected = sources
        .iter()
        .map(|row| Ok((text(row, "name")?, text(row, "version")?)))
        .collect::<Result<BTreeSet<_>>>()?;
    ensure!(
        observed == expected && expected.len() == sources.len(),
        "Native payload and source lock differ"
    );
    Ok(
        json!({"package_count":expected.len(),"source_lock_sha256":metadata::digest(SOURCE_LOCK.as_bytes()),
        "total_source_bytes":number(&locked, "total_source_bytes")?}),
    )
}

fn source_archive(directory: &Path, delivery: &Path, media: &Value, snapshot: &str) -> Result<()> {
    let locked: Value = serde_json::from_str(SOURCE_LOCK)?;
    let sources = locked["source_packages"]
        .as_array()
        .context("Missing corresponding-source lock")?;
    let archive_root = directory.join("corresponding-source");
    fs::create_dir(&archive_root)?;
    let name = text(media, "filename")?;
    fs::copy(directory.join(name), archive_root.join(name))?;
    let deadline = Instant::now() + Duration::from_secs(300);
    let signature_sha = text(media, "signature_sha256")?;
    metadata::sha256(signature_sha)?;
    download(
        &format!("{}.asc", text(media, "url")?),
        &archive_root.join(format!("{name}.asc")),
        number(media, "signature_size")?,
        signature_sha,
        deadline,
        false,
    )?;
    let mut inputs = BTreeMap::new();
    let mut total = 0;
    for source in sources {
        let pool = text(source, "directory")?;
        ensure!(
            pool.starts_with("pool/")
                && pool.split('/').all(|component| {
                    !matches!(component, "" | "." | "..")
                        && component
                            .bytes()
                            .all(|byte| byte.is_ascii_alphanumeric() || b".+~_-".contains(&byte))
                }),
            "Invalid corresponding-source directory"
        );
        for file in source["files"]
            .as_array()
            .context("Missing source archive members")?
        {
            let name = text(file, "filename")?;
            ensure!(
                !matches!(name, "" | "." | "..")
                    && name
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || b".+~_-".contains(&byte)),
                "Invalid corresponding-source filename"
            );
            let size = number(file, "bytes")?;
            ensure!(
                (1..=process::FILE_LIMIT).contains(&size),
                "Corresponding-source payload too large"
            );
            total = process::check_count(total, size, 256 * 1024 * 1024)?;
            let digest = text(file, "sha256")?;
            metadata::sha256(digest)?;
            ensure!(
                inputs.insert(name, (pool, size, digest)).is_none(),
                "Repeated corresponding-source archive"
            );
        }
    }
    ensure!(
        !inputs.is_empty() && total == number(&locked, "total_source_bytes")?,
        "Corresponding-source archive size differs"
    );
    let input_rows = inputs.into_iter().collect::<Vec<_>>();
    std::thread::scope(|scope| -> Result<()> {
        let workers = input_rows
            .chunks(input_rows.len().div_ceil(6))
            .map(|group| {
                let archive_root = &archive_root;
                scope.spawn(move || {
                    group.iter().try_for_each(|(name, (pool, size, digest))| {
                        download(
                            &format!("{snapshot}/{pool}/{name}"),
                            &archive_root.join(name),
                            *size,
                            digest,
                            deadline,
                            false,
                        )
                    })
                })
            })
            .collect::<Vec<_>>();
        for worker in workers {
            worker
                .join()
                .map_err(|_| anyhow::anyhow!("Corresponding-source staging worker stopped"))??;
        }
        Ok(())
    })?;
    fs::write(archive_root.join("ubuntu-source-lock.json"), SOURCE_LOCK)?;
    fs::write(archive_root.join("linux-build-inputs.json"), LOCK)?;
    let destination = delivery.join("corresponding-source.tar");
    let deadline = Instant::now() + Duration::from_secs(40);
    run_tool(
        process::command("/usr/bin/tar")
            .current_dir(directory)
            .args(["--create", "--sort=name", "--format=posix", "--file"])
            .arg(destination)
            .arg("corresponding-source"),
        deadline,
        40,
    )?;
    Ok(())
}

fn run_inner(root: &Path) -> Result<()> {
    let lock: Value = serde_json::from_str(LOCK)?;
    let capture: Value = serde_json::from_str(CONFIG)?;
    let packages = load(&lock, &capture)?;
    let nasm = load_nasm(&capture)?;
    match std::env::args().skip(2).collect::<Vec<_>>().as_slice() {
        [worker] if worker == "--offline-install-worker" => {
            return guest(&lock, &packages, &nasm).inspect_err(|error| {
                crate::package::retain_failure(Path::new(GUEST_FAILURE), error);
            });
        }
        [] => {}
        _ => bail!("Unknown offline compiler argument"),
    }
    ensure!(
        std::env::var("GITHUB_ACTIONS").as_deref() == Ok("true")
            && std::env::consts::OS == "linux"
            && std::env::consts::ARCH == "x86_64",
        "Staging compiler payloads requires a hosted Linux x64 job"
    );
    let source = crate::package::source(root)?;
    let parent =
        nw_platform::security::private_directory(&root.join("target/toolchain-bootstrap"))?;
    let directory = parent.join(format!("packages-{}", std::process::id()));
    fs::create_dir(&directory)?;
    let snapshot = text(&lock, "snapshot")?;
    let deadline = Instant::now() + Duration::from_secs(300);
    let result = std::thread::scope(|scope| -> Result<()> {
        let handles: Vec<_> = packages
            .chunks(packages.len().div_ceil(4))
            .map(|group| {
                let directory = &directory;
                scope.spawn(move || {
                    group.iter().try_for_each(|package| {
                        let name = Path::new(package.filename)
                            .file_name()
                            .context("Package filename")?;
                        download(
                            &format!("{snapshot}/{}", package.filename),
                            &directory.join(name),
                            package.size,
                            package.sha256,
                            deadline,
                            false,
                        )
                    })
                })
            })
            .collect();
        for handle in handles {
            handle
                .join()
                .map_err(|_| anyhow::anyhow!("Package staging worker stopped"))??;
        }
        Ok(())
    });
    if result.is_err() {
        fs::remove_dir_all(&directory)?;
    }
    result.context("Stage the 200 locked Ubuntu package archives")?;
    tracing::info!(
        package_count = packages.len(),
        "Verified the Linux build package inputs"
    );
    download(
        nasm.url,
        &directory.join(nasm.filename),
        nasm.size,
        nasm.sha256,
        deadline,
        true,
    )
    .context("Stage the locked NASM source archive")?;
    let media = &lock["ffmpeg"];
    let media_name = text(media, "filename")?;
    let media_url = text(media, "url")?;
    ensure!(
        media_name == "ffmpeg-9.0.2.tar.xz"
            && media_url == "https://ffmpeg.org/releases/ffmpeg-9.0.2.tar.xz",
        "Unexpected FFmpeg release source"
    );
    let media_sha = text(media, "sha256")?;
    metadata::sha256(media_sha)?;
    download(
        media_url,
        &directory.join(media_name),
        number(media, "size")?,
        media_sha,
        deadline,
        false,
    )
    .context("Stage the signed FFmpeg release source archive")?;
    let delivery = parent.join(format!("profile-{}", std::process::id()));
    let installed = offline_install(&directory, &delivery, &lock)?;
    tracing::info!("Built and qualified the Linux codec runtime in the locked worker");
    let relocated = qualify_relocated(&delivery, text(&lock, "base")?)
        .context("Qualify the relocated runtime in a fresh offline OCI container")?;
    let corresponding = source_closure(&installed["ffmpeg"], snapshot)
        .context("Match actual native imports to their corresponding source records")?;
    source_archive(&directory, &delivery, media, snapshot)
        .context("Assemble the checksum-verified corresponding source archive")?;
    run_tool(
        process::command("/usr/bin/tar")
            .current_dir(&delivery)
            .args([
                "--create",
                "--gzip",
                "--sort=name",
                "--file",
                "linux-x64-ffmpeg-component.tar.gz",
                "bin",
                "fonts",
                "notices",
            ]),
        Instant::now() + Duration::from_secs(65),
        65,
    )?;
    let archives = [
        "linux-x64-ffmpeg-component.tar.gz",
        "corresponding-source.tar",
    ]
    .into_iter()
    .map(|name| Ok((name, crate::package::digest(&delivery.join(name))?)))
    .collect::<Result<BTreeMap<_, _>>>()?;
    let report = json!({
        "schema_version":1,
        "app_source":source,
        "target":"linux-x64",
        "package_lock_sha256":metadata::digest(LOCK.as_bytes()),
        "packages":packages.iter().map(|package| json!({
            "name": package.name, "architecture":package.architecture, "version":package.version,
            "size":package.size, "sha256":package.sha256,
        })).collect::<Vec<_>>(),
        "payload_bytes":lock["download_bytes"],
        "offline_install":installed,
        "offline_relocation":relocated,
        "corresponding_sources":corresponding,
        "archives":archives,
    });
    let mut manifest = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(delivery.join("verified-build.json"))?;
    serde_json::to_writer_pretty(&mut manifest, &report)?;
    manifest.write_all(b"\n")?;
    fs::remove_dir_all(&directory).context("Remove staging archives after building the profile")?;
    tracing::info!(package_count = packages.len(), payload_bytes = number(&lock, "download_bytes")?, path = %delivery.display(), "Verified relocatable Linux CPU component build");
    Ok(())
}

pub(super) fn run(root: &Path) -> Result<()> {
    run_inner(root).inspect_err(|error| {
        if std::env::var("GITHUB_ACTIONS").as_deref() == Ok("true") {
            let directory = root.join("target/toolchain-bootstrap");
            if fs::create_dir_all(&directory).is_ok() {
                crate::package::retain_failure(&directory.join("host-failure.json"), error);
            }
        }
    })
}
