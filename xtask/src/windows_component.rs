//! Assemble the authenticated UCRT64 build inputs and inspect the native runtime payload.
use crate::package::{digest, source};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    env, fs,
    path::{Component, Path, PathBuf},
    process::{Command, Output, Stdio},
};

const LOCK: &str = include_str!("../../components/windows-standard-input-lock.json");
const SOURCE_LOCK: &str = include_str!("../../components/windows-standard-source-lock.json");
const TARGET: &str = "x86_64-pc-windows-msvc";
const APPS: [&str; 2] = ["ffmpeg.exe", "ffprobe.exe"];
const OPTIONS: &[&str] = &[
    "--disable-ffplay",
    "--disable-doc",
    "--disable-debug",
    "--disable-network",
    "--disable-autodetect",
    "--enable-gpl",
    "--enable-version3",
    "--enable-small",
    "--disable-protocols",
    "--enable-protocol=file,pipe,fd",
    "--enable-libx264",
    "--enable-libx265",
    "--enable-libdav1d",
    "--enable-libsvtav1",
    "--enable-libvpx",
    "--enable-libmp3lame",
    "--enable-libopus",
    "--enable-libass",
    "--enable-libfreetype",
    "--enable-libharfbuzz",
    "--enable-libfribidi",
    "--enable-libfontconfig",
    "--enable-libzimg",
    "--enable-librubberband",
    "--enable-libvidstab",
    "--enable-cairo",
    "--enable-ffnvcodec",
    "--enable-nvenc",
    "--enable-nvdec",
    "--pkg-config=/ucrt64/bin/pkgconf",
    "--cc=/ucrt64/bin/gcc",
    "--cxx=/ucrt64/bin/g++",
    "--x86asmexe=/ucrt64/bin/nasm",
];

fn field<'a>(row: &'a Value, name: &str) -> Result<&'a str> {
    row[name]
        .as_str()
        .with_context(|| format!("Missing Windows component field {name}"))
}

fn execute(command: &mut Command) -> Result<Output> {
    command.stdin(Stdio::null());
    let output = command.output().context("Start Windows component tool")?;
    ensure!(
        output.status.success(),
        "Windows component command failed ({}): {}",
        output.status,
        String::from_utf8_lossy(&output.stderr[..output.stderr.len().min(2048)])
    );
    Ok(output)
}

fn asset_folder(root: &Path) -> Result<PathBuf> {
    let parent = nw_platform::security::private_directory(&root.join("target/windows-component"))?;
    let folder = parent.join(format!("build-{}", std::process::id()));
    fs::create_dir(&folder)?;
    Ok(folder)
}

fn filename(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 180
        && !matches!(name, "." | "..")
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_.+~".contains(&byte))
}

fn stage(
    rows: &[Value],
    total: u64,
    maximum_file: u64,
    delivery: &Path,
    curl: &Path,
) -> Result<()> {
    ensure!(
        (1..=128).contains(&rows.len()) && total <= 320 * 1024 * 1024,
        "Windows build input lock exceeds review bounds"
    );
    let mut counted = 0_u64;
    let mut names = BTreeSet::new();
    for row in rows {
        let name = field(row, "filename")?;
        let base = field(row, "base_url")?;
        ensure!(
            filename(name) && names.insert(name.to_ascii_lowercase()),
            "Unsafe archive filename"
        );
        ensure!(
            matches!(
                base,
                "https://repo.msys2.org/mingw/ucrt64/"
                    | "https://repo.msys2.org/msys/x86_64/"
                    | "https://repo.msys2.org/mingw/sources/"
                    | "https://github.com/msys2/msys2-installer/releases/download/2026-09-27/"
                    | "https://github.com/dejavu-fonts/dejavu-fonts/releases/download/version_2_37/"
                    | "https://ffmpeg.org/releases/"
            ),
            "Unrecognized Windows source host"
        );
        let size = row["size"].as_u64().context("Archive byte count")?;
        ensure!(
            (1..=maximum_file).contains(&size),
            "Archive exceeds the package limit"
        );
        counted = counted
            .checked_add(size)
            .context("Windows package byte count overflow")?;
        ensure!(counted <= total, "Unexpected Windows build input bytes");
    }
    ensure!(
        counted == total,
        "Windows component lock byte count differs"
    );
    std::thread::scope(|scope| -> Result<()> {
        let tasks = rows
            .chunks(rows.len().div_ceil(6))
            .map(|group| {
                scope.spawn(move || -> Result<()> {
                    for row in group {
                        let name = field(row, "filename")?;
                        let base = field(row, "base_url")?;
                        let size = row["size"].as_u64().context("Archive byte count")?;
                        let expected_sha = field(row, "sha256")?;
                        let output = delivery.join(name);
                        execute(
                            Command::new(curl)
                                .args([
                                    "--disable",
                                    "--silent",
                                    "--show-error",
                                    "--fail",
                                    "--location",
                                    "--proto",
                                    "=https",
                                    "--proto-redir",
                                    "=https",
                                    "--max-redirs",
                                    "4",
                                    "--max-time",
                                    "90",
                                    "--max-filesize",
                                    &size.to_string(),
                                    "--output",
                                ])
                                .arg(&output)
                                .arg("--")
                                .arg(format!("{base}{name}")),
                        )?;
                        ensure!(
                            digest(&output)? == json!({"bytes":size,"sha256":expected_sha}),
                            "Windows build input differs from its signed identity: {name}"
                        );
                    }
                    Ok(())
                })
            })
            .collect::<Vec<_>>();
        for task in tasks {
            task.join()
                .map_err(|_| anyhow::anyhow!("Windows source transfer stopped"))??;
        }
        Ok(())
    })
}

fn input<'a>(rows: &'a [Value], role: &str) -> Result<&'a str> {
    let mut matches = rows.iter().filter(|row| row["role"] == role);
    let name = field(
        matches
            .next()
            .context("Missing single-role Windows input")?,
        "filename",
    )?;
    ensure!(
        matches.next().is_none(),
        "Repeated single-role Windows input"
    );
    Ok(name)
}

fn unpack(tar: &Path, archive: &Path, root: &Path, prefix: &str) -> Result<()> {
    let contents = execute(Command::new(tar).args(["--list", "--file"]).arg(archive))?;
    let names = std::str::from_utf8(&contents.stdout)?;
    ensure!(
        !names.is_empty() && names.lines().count() <= 180_000,
        "Unbounded Windows archive inventory"
    );
    for name in names.lines() {
        let name = name.trim_end_matches('/').trim_start_matches("./");
        ensure!(
            !name.contains(['\\', ':'])
                && Path::new(name)
                    .components()
                    .all(|part| matches!(part, Component::Normal(_))),
            "Unsafe verified archive path"
        );
        let admitted =
            [".PKGINFO", ".BUILDINFO", ".MTREE", ".INSTALL", ".CHANGELOG"].contains(&name);
        let hook = prefix == "ucrt64"
            && (matches!(
                name,
                "usr" | "usr/share" | "usr/share/libalpm" | "usr/share/libalpm/hooks"
            ) || name
                .strip_prefix("usr/share/libalpm/hooks/")
                .is_some_and(|name| filename(name) && name.ends_with(".hook")));
        ensure!(
            admitted
                || hook
                || name == prefix
                || name
                    .strip_prefix(prefix)
                    .is_some_and(|tail| tail.starts_with('/')),
            "Unexpected archive member directory"
        );
    }
    let mut extract = Command::new(tar);
    extract
        .current_dir(root)
        .args(["--extract", "--file"])
        .arg(archive)
        .args([
            "--exclude=.PKGINFO",
            "--exclude=.BUILDINFO",
            "--exclude=.MTREE",
            "--exclude=.INSTALL",
            "--exclude=.CHANGELOG",
        ]);
    if prefix == "ucrt64" {
        extract.args(["--exclude=usr", "--exclude=usr/*"]);
    }
    execute(&mut extract)?;
    Ok(())
}

fn compiler(kit: &Path, work: &Path, system32: &Path) -> Result<Command> {
    let mut command = Command::new(kit.join("usr/bin/bash.exe"));
    command
        .env_clear()
        .current_dir(work)
        .env("MSYSTEM", "UCRT64")
        .env(
            "SYSTEMROOT",
            system32.parent().context("Windows system root")?,
        )
        .env(
            "PATH",
            env::join_paths([
                kit.join("ucrt64/bin"),
                kit.join("usr/bin"),
                system32.to_owned(),
            ])?,
        )
        .env("LANG", "C")
        .env("LC_ALL", "C")
        .env("TZ", "UTC")
        .args(["--noprofile", "--norc"]);
    Ok(command)
}

fn collect_deps(kit: &Path, directory: &Path, system32: &Path) -> Result<Vec<String>> {
    let tools = kit.join("ucrt64/bin");
    let mut files = BTreeMap::new();
    let mut queue = VecDeque::from(APPS.map(str::to_owned));
    let mut system = BTreeSet::new();
    while let Some(name) = queue.pop_front() {
        let key = name.to_ascii_lowercase();
        if files.contains_key(&key) || system.contains(&key) {
            continue;
        }
        ensure!(
            filename(&name) && (APPS.contains(&name.as_str()) || key.ends_with(".dll")),
            "Unexpected PE import"
        );
        ensure!(
            files.len() + system.len() < 256,
            "Unbounded PE dependency closure"
        );
        let dest = directory.join(&key);
        if !dest.is_file() {
            let candidate = tools.join(&name);
            if candidate.is_file() {
                fs::copy(candidate, &dest)?;
            } else {
                ensure!(
                    key.starts_with("api-ms-win-") || system32.join(&key).is_file(),
                    "Missing private Windows media DLL: {name}"
                );
                system.insert(key);
                continue;
            }
        }
        let imports = execute(
            Command::new(tools.join("objdump.exe"))
                .arg("--private-headers")
                .arg(&dest)
                .env("PATH", env::join_paths([tools.as_path(), system32])?),
        )?;
        let imports = std::str::from_utf8(&imports.stdout)?;
        ensure!(
            imports.contains("file format pei-x86-64"),
            "Unexpected PE architecture"
        );
        queue.extend(
            imports
                .lines()
                .filter_map(|line| line.trim().strip_prefix("DLL Name:").map(str::trim))
                .map(str::to_owned),
        );
        files.insert(key, dest);
    }
    Ok(system.into_iter().collect())
}

fn corresponding_sources(
    files: &[Value],
    work: &Path,
    media_source: &Path,
    font_archive: &Path,
    system32: &Path,
) -> Result<Value> {
    let lock: Value = serde_json::from_str(SOURCE_LOCK)?;
    ensure!(
        lock["schema_version"] == 1,
        "Windows source lock schema differs"
    );
    let dlls = lock["dll_ownership"]
        .as_array()
        .context("Windows private DLL source inventory")?;
    let owners = lock["binary_to_source"]
        .as_array()
        .context("Windows package ownership records")?;
    let sources = lock["source_packages"]
        .as_array()
        .context("Windows source package records")?;
    let base_url = field(&lock, "base_url")?;
    let mut observed = BTreeSet::new();
    for file in files {
        let path = field(file, "path")?;
        let name = path
            .strip_prefix("bin/")
            .filter(|name| name.ends_with(".dll"));
        let Some(name) = name else { continue };
        let owner = dlls
            .iter()
            .find(|row| {
                row["name"]
                    .as_str()
                    .is_some_and(|record| record.eq_ignore_ascii_case(name))
            })
            .with_context(|| format!("No reviewed Windows package owns {name}"))?;
        ensure!(
            owner["bytes"] == file["bytes"] && owner["sha256"] == file["sha256"],
            "Windows DLL differs from its reviewed package: {name}"
        );
        let package = owners
            .iter()
            .find(|row| row["pkgname"] == owner["package"] && row["pkgver"] == owner["version"])
            .context("Missing exact Windows source package identity")?;
        observed.insert((field(package, "pkgbase")?, field(package, "pkgver")?));
    }
    ensure!(
        !observed.is_empty(),
        "No measured Windows runtime dependency closure"
    );
    let archive_root = work.join("corresponding-source");
    fs::create_dir(&archive_root)?;
    let mut archive_inputs = Vec::new();
    let mut total = 0_u64;
    for (name, version) in &observed {
        let row = sources
            .iter()
            .find(|row| row["pkgbase"] == *name && row["version"] == *version)
            .context("Missing exact corresponding Windows source archive")?;
        let archive = field(row, "archive")?;
        archive_inputs.push(json!({"filename":archive,"base_url":base_url,"size":row["bytes"],"sha256":row["sha256"]}));
        archive_inputs.push(json!({"filename":format!("{archive}.sig"),"base_url":base_url,"size":row["signature_bytes"],"sha256":row["signature_sha256"]}));
        total = total
            .checked_add(
                row["bytes"]
                    .as_u64()
                    .context("Windows source archive size")?,
            )
            .and_then(|value| value.checked_add(row["signature_bytes"].as_u64()?))
            .context("Windows source closure byte count overflow")?;
    }
    stage(
        &archive_inputs,
        total,
        150 * 1024 * 1024,
        &archive_root,
        &system32.join("curl.exe"),
    )?;
    for asset in [media_source, font_archive] {
        fs::copy(
            asset,
            archive_root.join(asset.file_name().context("Upstream source basename")?),
        )?;
    }
    fs::write(
        archive_root.join("windows-standard-source-lock.json"),
        SOURCE_LOCK,
    )?;
    fs::write(archive_root.join("windows-standard-input-lock.json"), LOCK)?;
    let output = work.join("windows-x64-corresponding-source.tar");
    execute(
        Command::new(system32.join("tar.exe"))
            .current_dir(work)
            .args(["--create", "--format=pax", "--file"])
            .arg(&output)
            .arg("corresponding-source"),
    )?;
    Ok(json!({"archive":digest(&output)?,"native_packages":observed.len()}))
}

fn bundle_app(root: &Path, work: &Path, profile: &Path, system32: &Path) -> Result<()> {
    let binary = crate::package::build_portable_executable(root, profile)?;
    let delivery = work.join("delivery");
    fs::create_dir(&delivery)?;
    let executable = delivery.join("numera-wiz.exe");
    fs::copy(binary, &executable)?;
    let isolated_state = work.join("first-launch-state");
    let smoke = crate::package::qualify_portable_application(
        Command::new(&executable)
            .current_dir(&delivery)
            .env("PATH", system32)
            .arg("--data-dir")
            .arg(&isolated_state)
            .args(["media", "capabilities", "refresh", "--force"]),
    )?;
    let profile_manifest: Value =
        serde_json::from_slice(&fs::read(profile.join("embed-profile.json"))?)?;
    let items = profile_manifest["files"]
        .as_array()
        .context("Embedded Windows content list")?;
    for (tool, exe) in ["ffmpeg", "ffprobe"].into_iter().zip(APPS) {
        let name = format!("bin/{exe}");
        let managed = items
            .iter()
            .find(|item| item["path"] == name)
            .context("Missing embedded application helper")?;
        ensure!(
            smoke["result"][tool]["sha256"] == managed["sha256"],
            "Portable owner resolved a different media executable"
        );
    }
    fs::write(
        delivery.join("portable-first-launch.json"),
        serde_json::to_vec_pretty(&smoke)?,
    )?;
    let archive = "windows-x64-corresponding-source.tar";
    fs::rename(work.join(archive), delivery.join(archive))?;
    let notice = delivery.join("notices");
    fs::create_dir(&notice)?;
    fs::copy(
        work.join("ffmpeg-9.0.2/COPYING.GPLv3"),
        notice.join("FFmpeg.GPLv3.txt"),
    )?;
    fs::copy(
        work.join("dejavu-sans-ttf-2.37/LICENSE"),
        notice.join("DejaVuSans.LICENSE.txt"),
    )?;
    fs::copy(root.join("LICENSE"), notice.join("Numera-Wiz.LICENSE.txt"))?;
    fs::copy(
        root.join("THIRD-PARTY-NOTICES.md"),
        notice.join("THIRD-PARTY-NOTICES.md"),
    )?;
    let manifest = json!({"source":profile_manifest["app_source"], "target":TARGET,
        "executable":digest(&executable)?,"source_archive":digest(&delivery.join(archive))?,
        "component":profile_manifest,"cpu_first_launch":smoke["result"]["cpu_round_trip"]});
    fs::write(
        delivery.join("portable-package.json"),
        serde_json::to_vec_pretty(&manifest)?,
    )?;
    Ok(())
}

pub fn build(root: &Path) -> Result<()> {
    let app_source = source(root)?;
    ensure!(
        std::env::consts::ARCH == "x86_64",
        "No Windows media recipe for this target"
    );
    let lock: Value = serde_json::from_str(LOCK)?;
    ensure!(
        lock["schema_version"] == 1 && lock["target"] == TARGET,
        "Windows component lock target differs"
    );
    let work = asset_folder(root)?;
    let result = assemble(root, &work, &lock, &app_source);
    if let Err(error) = &result {
        crate::package::retain_failure(&work.join("failure.json"), error);
    }
    result
}

fn assemble(root: &Path, work: &Path, lock: &Value, app_source: &str) -> Result<()> {
    let rows = lock["archives"]
        .as_array()
        .context("Windows input archive records")?;
    let total = lock["total_bytes"].as_u64().context("Windows input size")?;
    let system32 = PathBuf::from(env::var_os("SystemRoot").context("Windows system directory")?)
        .join("System32");
    let inputs = work.join("inputs");
    let extraction = work.join("toolchain");
    fs::create_dir(&inputs)?;
    fs::create_dir(&extraction)?;
    stage(
        rows,
        total,
        64 * 1024 * 1024,
        &inputs,
        &system32.join("curl.exe"),
    )
    .context("Stage the authenticated Windows compiler packages")?;
    let tar = system32.join("tar.exe");
    unpack(
        &tar,
        &inputs.join(input(rows, "msys-base")?),
        &extraction,
        "msys64",
    )?;
    let kit = extraction.join("msys64");
    for row in rows {
        let prefix = match field(row, "role")? {
            "msys-package" => "usr",
            "ucrt-package" => "ucrt64",
            "msys-base" | "ffmpeg-source" | "font" => continue,
            other => anyhow::bail!("Unexpected Windows package role {other}"),
        };
        unpack(&tar, &inputs.join(field(row, "filename")?), &kit, prefix)?;
    }
    let ffmpeg_source = inputs.join(input(rows, "ffmpeg-source")?);
    unpack(&tar, &ffmpeg_source, work, "ffmpeg-9.0.2")?;
    let source_dir = work.join("ffmpeg-9.0.2");
    let configured = execute(
        compiler(&kit, &source_dir, &system32)?
            .arg("./configure")
            .args(OPTIONS),
    )
    .context("Configure FFmpeg with the pinned Windows libraries")?;
    ensure!(
        std::str::from_utf8(&configured.stdout)?.contains("License: GPL version 3 or later"),
        "Unexpected FFmpeg license"
    );
    let compiled = compiler(&kit, &source_dir, &system32)?
        .args(["-c", "exec /usr/bin/make -j4 ffmpeg.exe ffprobe.exe"])
        .status()?;
    ensure!(compiled.success(), "Windows FFmpeg compilation failed");
    let output = work.join("profile");
    let bin = output.join("bin");
    let fonts = output.join("fonts");
    fs::create_dir_all(&bin)?;
    fs::create_dir(&fonts)?;
    for exe in APPS {
        fs::copy(source_dir.join(exe), bin.join(exe))?;
    }
    let imports = collect_deps(&kit, &bin, &system32)?;
    let font_source = inputs.join(input(rows, "font")?);
    unpack(&tar, &font_source, work, "dejavu-sans-ttf-2.37")?;
    let font = work.join("dejavu-sans-ttf-2.37/ttf/DejaVuSans.ttf");
    ensure!(
        digest(&font)?["sha256"] == lock["source_attestations"]["font_sha256"],
        "Unexpected hosted portable font identity"
    );
    fs::copy(font, fonts.join("DejaVuSans.ttf"))?;
    fs::write(output.join("configure.txt"), configured.stdout)?;
    let mut files = Vec::new();
    for group in [&bin, &fonts] {
        for path in fs::read_dir(group)? {
            let path = path?.path();
            ensure!(path.is_file(), "Unexpected portable component entry");
            let relative = path
                .strip_prefix(&output)?
                .to_str()
                .context("UTF-8 component filename")?
                .replace('\\', "/");
            let mut record = digest(&path)?;
            record["path"] = json!(relative);
            files.push(record);
        }
    }
    files.sort_by(|left, right| left["path"].as_str().cmp(&right["path"].as_str()));
    for executable in APPS {
        let version = execute(
            Command::new(bin.join(executable))
                .env_clear()
                .env("PATH", &system32)
                .env(
                    "SYSTEMROOT",
                    system32.parent().context("Windows system root")?,
                )
                .arg("-version"),
        )?;
        ensure!(
            std::str::from_utf8(&version.stdout)?.contains("9.0.2"),
            "Relocated Windows media executable did not identify its version"
        );
    }
    let source_archive =
        corresponding_sources(&files, work, &ffmpeg_source, &font_source, &system32)?;
    fs::write(
        output.join("embed-profile.json"),
        serde_json::to_vec_pretty(&json!({
            "schema_version":1,"target":TARGET,"app_source":app_source,"files":files,"system_imports":imports,
            "ffmpeg_source_commit":lock["ffmpeg_source_commit"],"corresponding_sources":source_archive,
            "status":"native_build_candidate",
        }))?,
    )?;
    bundle_app(root, work, &output, &system32)
        .context("Embed and verify the Windows desktop application")?;
    tracing::info!(profile=%output.display(), "Staged the identified Windows component profile");
    Ok(())
}
