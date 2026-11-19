//! Assemble the checksum-pinned macOS media inputs without installing them on the runner.
use crate::package::{digest, source};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Component, Path, PathBuf},
    process::{Command, Output, Stdio},
};

#[cfg(target_arch = "aarch64")]
use std::os::unix::fs::symlink;
#[cfg(target_arch = "x86_64")]
mod macports;
#[cfg(target_arch = "aarch64")]
const LOCK: &str = include_str!("../../components/macos-arm64-standard-input-lock.json");
#[cfg(target_arch = "x86_64")]
const LOCK: &str = include_str!("../../components/macos-x64-standard-input-lock.json");
const TARGET: &str = if cfg!(target_arch = "aarch64") { "aarch64-apple-darwin" } else { "x86_64-apple-darwin" };
const LABEL: &str = if cfg!(target_arch = "aarch64") { "macos-arm64" } else { "macos-x64" };
const APPS: [&str; 2] = ["ffmpeg", "ffprobe"];
const FFMPEG: &str = "ffmpeg-9.0.2.tar.xz";
const FONT: &str = "dejavu-sans-ttf-2.37.zip";
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
    "--cc=/usr/bin/clang",
    "--cxx=/usr/bin/clang++",
];

fn execute(command: &mut Command, phase: &str) -> Result<Output> {
    let result = command
        .stdin(Stdio::null())
        .output()
        .with_context(|| phase.to_owned())?;
    ensure!(
        result.status.success(),
        "{phase} failed ({}): {}",
        result.status,
        String::from_utf8_lossy(&result.stderr[..result.stderr.len().min(4096)])
    );
    Ok(result)
}

fn field<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    value[key]
        .as_str()
        .with_context(|| format!("Missing macOS input field {key}"))
}

fn basename(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 180
        && !matches!(name, "." | "..")
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_.-+@".contains(&byte))
}

fn http() -> Command {
    let mut curl = Command::new("/usr/bin/curl");
    curl.args([
        "--disable",
        "--fail",
        "--silent",
        "--show-error",
        "--proto",
        "=https",
        "--proto-redir",
        "=https",
        "--max-time",
        "120",
        "--retry",
        "2",
    ]);
    curl
}

fn download(row: &Value, path: &Path, bottle: Option<&str>) -> Result<()> {
    let bytes = row["bytes"]
        .as_u64()
        .context("Locked macOS input byte count")?;
    let sha = field(row, "sha256")?;
    let url = field(row, "url")?;
    ensure!(
        (1..=180 * 1024 * 1024).contains(&bytes)
            && sha.len() == 64
            && sha.bytes().all(|byte| byte.is_ascii_hexdigit())
            && url.starts_with("https://"),
        "Unexpected locked macOS input identity"
    );
    let mut request = http();
    if let Some(name) = bottle {
        let repository = format!("homebrew/core/{}", name.replace('@', "/"));
        ensure!(
            url == format!("https://ghcr.io/v2/{repository}/blobs/sha256:{sha}"),
            "Unexpected Homebrew registry location"
        );
        let token = execute(
            http().args([
                "--max-filesize",
                "16384",
                "--get",
                "--data-urlencode",
                "service=ghcr.io",
                "--data-urlencode",
                &format!("scope=repository:{repository}:pull"),
                "https://ghcr.io/token",
            ]),
            "Request the public Homebrew registry grant",
        )?;
        let grant: Value = serde_json::from_slice(&token.stdout)?;
        request.arg("--oauth2-bearer").arg(field(&grant, "token")?);
    }
    let filename = path
        .file_name()
        .and_then(|name| name.to_str())
        .context("macOS input filename")?;
    ensure!(basename(filename), "Unexpected macOS input filename");
    // curl removes the scoped registry authorization when redirected to the CDN.
    execute(
        request
            .args([
                "--location",
                "--max-redirs",
                "5",
                "--max-filesize",
                &bytes.to_string(),
                "--output",
            ])
            .arg(path)
            .arg("--")
            .arg(url),
        &format!("Download {filename}"),
    )?;
    ensure!(
        digest(path)? == json!({"bytes": bytes, "sha256": sha}),
        "Downloaded macOS input failed its digest check: {filename}"
    );
    Ok(())
}

fn unpack(archive: &Path, root: &Path, prefix: &str) -> Result<()> {
    let listing = execute(
        Command::new("/usr/bin/tar").args(["-tf"]).arg(archive),
        "List verified archive",
    )?;
    let contents = std::str::from_utf8(&listing.stdout)?;
    ensure!(
        !contents.is_empty() && contents.lines().count() <= 120_000,
        "Unbounded macOS archive"
    );
    for line in contents.lines() {
        let member = line.trim_end_matches('/').trim_start_matches("./");
        let mut parts = Path::new(member).components();
        ensure!(
            !member.contains(['\\', ':'])
                && parts.next() == Some(Component::Normal(prefix.as_ref()))
                && parts.all(|part| matches!(part, Component::Normal(_))),
            "Unexpected macOS archive member"
        );
    }
    execute(
        Command::new("/usr/bin/tar")
            .current_dir(root)
            .args(["-xf"])
            .arg(archive),
        "Extract verified macOS archive",
    )?;
    Ok(())
}

#[cfg(target_arch = "aarch64")]
fn package_kegs(lock: &Value, work: &Path) -> Result<BTreeMap<String, PathBuf>> {
    let downloads = work.join("bottles");
    let cellar = work.join("Cellar");
    let opt = work.join("opt");
    for directory in [&downloads, &cellar, &opt] {
        fs::create_dir(directory)?;
    }
    let packages = lock["bottles"].as_array().context("Locked ARM64 bottles")?;
    ensure!(packages.len() == 43, "Incomplete ARM64 bottle closure");
    let mut kegs = BTreeMap::new();
    for row in packages {
        let name = field(row, "name")?;
        let keg_version = field(row, "keg")?;
        ensure!(
            basename(name) && basename(keg_version),
            "Unexpected Homebrew package identity"
        );
        let archive = downloads.join(format!("{name}.tar.gz"));
        download(row, &archive, Some(name))?;
        unpack(&archive, &cellar, name)?;
        let keg = cellar.join(name).join(keg_version);
        ensure!(
            keg.is_dir() && keg.canonicalize()? == keg,
            "Missing exact package keg for {name}"
        );
        symlink(&keg, opt.join(name))?;
        ensure!(
            kegs.insert(name.to_owned(), keg).is_none(),
            "Repeated Homebrew package"
        );
    }
    Ok(kegs)
}

fn pkg_config_files(work: &Path, dependencies: &BuildInputs) -> Result<std::ffi::OsString> {
    let mut search = Vec::new();
    let mut rewritten = BTreeSet::new();
    let prefix = work.to_str().context("UTF-8 media prefix")?;
    let cellar = work.join("Cellar");
    let cellar = cellar.to_str().context("UTF-8 Homebrew Cellar")?;
    ensure!(
        !prefix.contains(char::is_whitespace),
        "The component compiler requires a whitespace-free workspace"
    );
    for keg in dependencies.libraries.values() {
        for subdir in ["lib/pkgconfig", "share/pkgconfig"] {
            let config = keg.join(subdir);
            if !config.is_dir() {
                continue;
            }
            search.push(config.clone());
            for file in fs::read_dir(&config)? {
                let path = file?.path();
                if path.extension().is_none_or(|ext| ext != "pc") {
                    continue;
                }
                let path = path.canonicalize()?;
                ensure!(
                    path.starts_with(keg) && fs::metadata(&path)?.len() <= 128 * 1024,
                    "Invalid pkg-config input from the downloaded keg"
                );
                if !rewritten.insert(path.clone()) {
                    continue;
                }
                let contents = fs::read_to_string(&path)?;
                let mut relocated = contents
                    .replace("@@HOMEBREW_CELLAR@@", cellar)
                    .replace("@@HOMEBREW_PREFIX@@", prefix)
                    .replace("/opt/homebrew/Cellar/", &format!("{cellar}/"))
                    .replace("/opt/homebrew/opt/", &format!("{prefix}/opt/"));
                if let Some(prefix) = &dependencies.macports_prefix {
                    relocated = relocated.replace("/opt/local", prefix.to_str().context("UTF-8 MacPorts prefix")?);
                }
                if relocated != contents {
                    fs::write(path, relocated)?;
                }
            }
        }
    }
    ensure!(
        !search.is_empty(),
        "No reviewed macOS package configuration"
    );
    Ok(std::env::join_paths(search)?)
}

fn apple_tool(executable: &str, path: &Path, args: &[&str], phase: &str) -> Result<Vec<u8>> {
    Ok(execute(Command::new(executable).args(args).arg(path), phase)?.stdout)
}

fn links(path: &Path, library: bool) -> Result<Vec<String>> {
    let output = apple_tool(
        "/usr/bin/otool",
        path,
        &["-L"],
        "Inspect Mach-O load commands",
    )?;
    let mut lines = std::str::from_utf8(&output)?.lines();
    ensure!(lines.next().is_some(), "Missing Mach-O filename");
    if library {
        ensure!(
            lines.next().is_some(),
            "Missing Mach-O library install name"
        );
    }
    lines
        .map(|line| {
            Ok(line
                .trim()
                .rsplit_once(" (compatibility version ")
                .context("Unexpected Mach-O import entry")?
                .0
                .to_owned())
        })
        .collect()
}

fn runpaths(path: &Path) -> Result<Vec<String>> {
    let output = apple_tool(
        "/usr/bin/otool",
        path,
        &["-l"],
        "Inspect Mach-O runtime search paths",
    )?;
    let mut paths = Vec::new();
    let mut rpath = false;
    for line in std::str::from_utf8(&output)?.lines().map(str::trim) {
        if let Some(command) = line.strip_prefix("cmd ") {
            rpath = command == "LC_RPATH";
        } else if rpath && let Some(value) = line.strip_prefix("path ") {
            paths.push(
                value
                    .rsplit_once(" (offset ")
                    .context("Invalid Mach-O runtime search path")?
                    .0
                    .to_owned(),
            );
            rpath = false;
        }
    }
    Ok(paths)
}

fn system_import(path: &str) -> bool {
    path.starts_with("/usr/lib/") || path.starts_with("/System/Library/Frameworks/")
}

fn sign(path: &Path) -> Result<()> {
    apple_tool(
        "/usr/bin/codesign",
        path,
        &["--force", "--sign", "-"],
        "Sign the relocated Mach-O file",
    )?;
    apple_tool(
        "/usr/bin/codesign",
        path,
        &["--verify", "--strict"],
        "Verify the relocated Mach-O signature",
    )?;
    Ok(())
}

#[cfg(target_arch = "aarch64")]
fn pkgconf(keg: &Path) -> Result<PathBuf> {
    let tool = keg.join("bin/pkgconf");
    let imports = links(&tool, false)?;
    let mut command = Command::new("/usr/bin/install_name_tool");
    let mut changed = false;
    for name in imports.iter().filter(|name| !system_import(name)) {
        let filename = name
            .rsplit('/')
            .next()
            .context("pkgconf runtime library name")?;
        ensure!(
            basename(filename)
                && filename.starts_with("libpkgconf.")
                && keg
                    .join("lib")
                    .join(filename)
                    .canonicalize()?
                    .starts_with(keg),
            "Unexpected pkgconf import"
        );
        command.args(["-change", name, &format!("@loader_path/../lib/{filename}")]);
        changed = true;
    }
    if changed {
        execute(command.arg(&tool), "Relocate the pinned pkgconf tool")?;
        sign(&tool)?;
    }
    let version = execute(
        Command::new(&tool).env_clear().arg("--version"),
        "Identify the pinned pkgconf tool",
    )?;
    ensure!(
        std::str::from_utf8(&version.stdout)?.trim() == "3.0.11",
        "Unexpected pkgconf version"
    );
    Ok(tool)
}

fn library_index(kegs: &BTreeMap<String, PathBuf>) -> Result<BTreeMap<String, PathBuf>> {
    let mut index = BTreeMap::new();
    for keg in kegs.values() {
        let root = keg.join("lib");
        if !root.is_dir() {
            continue;
        }
        for entry in fs::read_dir(root)? {
            let entry = entry?;
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| anyhow::anyhow!("Non-UTF-8 Mach-O library name"))?;
            if !name.ends_with(".dylib") {
                continue;
            }
            let path = entry.path().canonicalize()?;
            ensure!(
                basename(&name) && path.starts_with(keg) && path.is_file(),
                "Invalid locked Mach-O library alias"
            );
            ensure!(
                index.insert(name, path).is_none(),
                "Ambiguous locked Mach-O library alias"
            );
        }
    }
    Ok(index)
}

fn relocate(
    source_dir: &Path,
    bin: &Path,
    kegs: &BTreeMap<String, PathBuf>,
) -> Result<BTreeSet<String>> {
    let index = library_index(kegs)?;
    let mut pending = BTreeMap::from(APPS.map(|name| (name.to_owned(), source_dir.join(name))));
    let mut copied = BTreeSet::new();
    let mut systems = BTreeSet::new();
    while let Some((name, original)) = pending.pop_first() {
        ensure!(
            copied.len() < 160,
            "Mach-O dependency closure exceeded its limit"
        );
        let file = bin.join(&name);
        fs::copy(&original, &file)?;
        let library = name.ends_with(".dylib");
        let mut update = Command::new("/usr/bin/install_name_tool");
        for path in runpaths(&file)? {
            update.args(["-delete_rpath", &path]);
        }
        if library {
            update.args(["-id", &format!("@rpath/{name}")]);
        } else {
            update.args(["-add_rpath", "@loader_path"]);
        }
        for import in links(&file, library)? {
            if system_import(&import) {
                systems.insert(import);
                continue;
            }
            let dependency = import.rsplit('/').next().context("Mach-O import name")?;
            let original = index
                .get(dependency)
                .with_context(|| format!("Missing locked Mach-O import {dependency}"))?;
            if dependency != name && !copied.contains(dependency) {
                pending.insert(dependency.to_owned(), original.clone());
            }
            let reference = format!("@rpath/{dependency}");
            if reference != import {
                update.args(["-change", &import, &reference]);
            }
        }
        execute(update.arg(&file), "Relocate the macOS media dependency")?;
        sign(&file)?;
        copied.insert(name);
    }
    for name in &copied {
        let path = bin.join(name);
        let library = name.ends_with(".dylib");
        let expected_paths = if library {
            Vec::new()
        } else {
            vec!["@loader_path".to_owned()]
        };
        ensure!(
            runpaths(&path)? == expected_paths,
            "Relocated Mach-O contains an unexpected runtime search path"
        );
        for import in links(&path, library)? {
            ensure!(
                system_import(&import)
                    || import
                        .strip_prefix("@rpath/")
                        .is_some_and(|name| copied.contains(name)),
                "Relocated Mach-O still depends on an external library"
            );
        }
    }
    Ok(systems)
}

fn sdk_metadata(work: &Path) -> Result<(PathBuf, PathBuf)> {
    let output = execute(
        Command::new("/usr/bin/xcrun").args(["--sdk", "macosx", "--show-sdk-path"]),
        "Locate the selected native macOS SDK",
    )?;
    let sdk = PathBuf::from(std::str::from_utf8(&output.stdout)?.trim());
    ensure!(
        sdk.is_absolute() && sdk.is_dir(),
        "Invalid native macOS SDK"
    );
    let directory = work.join("sdk-pkgconfig");
    fs::create_dir(&directory)?;
    for (name, library) in [
        ("zlib", "z"),
        ("bzip2", "bz2"),
        ("expat", "expat"),
        ("libffi", "ffi"),
    ] {
        let stub = sdk.join("usr/lib").join(format!("lib{library}.tbd"));
        ensure!(stub.is_file(), "Missing SDK system library {name}");
        fs::write(
            directory.join(format!("{name}.pc")),
            format!(
                "Name: {name}\nDescription: macOS SDK system import\nVersion: 0\nLibs: -l{library}\n"
            ),
        )?;
    }
    Ok((sdk, directory))
}

struct BuildInputs {
    libraries: BTreeMap<String, PathBuf>,
    pkg_config: PathBuf,
    lame: PathBuf,
    macports_prefix: Option<PathBuf>,
    assembler: Option<PathBuf>,
}

#[cfg(target_arch = "aarch64")]
fn prepare_inputs(lock: &Value, work: &Path) -> Result<BuildInputs> {
    let libraries = package_kegs(lock, work)?;
    let pkg_config = pkgconf(libraries.get("pkgconf").context("Missing pinned pkgconf keg")?)?;
    let lame = libraries.get("lame").context("Missing pinned LAME keg")?.clone();
    Ok(BuildInputs { libraries, pkg_config, lame, macports_prefix: None, assembler: None })
}

#[cfg(target_arch = "x86_64")]
fn prepare_inputs(lock: &Value, work: &Path) -> Result<BuildInputs> {
    let prefix = macports::stage(lock, work)?;
    let pkg_config = macports::pkg_config(&prefix)?;
    let assembler = prefix.join("bin/nasm");
    ensure!(assembler.is_file(), "Missing Intel NASM executable");
    let libraries = BTreeMap::from([("macports".into(), prefix.clone())]);
    Ok(BuildInputs { libraries, pkg_config, lame: prefix.clone(), macports_prefix: Some(prefix), assembler: Some(assembler) })
}

fn compile(source_dir: &Path, work: &Path, dependencies: &BuildInputs) -> Result<()> {
    let config = pkg_config_files(work, dependencies)?;
    let (sdk, standard_config) = sdk_metadata(work)?;
    let search = std::env::join_paths(std::env::split_paths(&config).chain([standard_config]))?;
    let lame = &dependencies.lame;
    let mut configure = Command::new("/bin/bash");
    configure
        .current_dir(source_dir)
        .env_clear()
        .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
        .env("PKG_CONFIG_LIBDIR", search)
        .env("SDKROOT", &sdk)
        .env("LC_ALL", "C")
        .env("MACOSX_DEPLOYMENT_TARGET", "15.0")
        .arg("./configure")
        .args(OPTIONS)
        .arg(format!("--extra-cflags=-I{}/include", lame.display()))
        .arg(format!(
            "--extra-ldflags=-Wl,-headerpad_max_install_names -L{}/lib",
            lame.display()
        ))
        .arg(format!("--pkg-config={}", dependencies.pkg_config.display()));
    if let Some(assembler) = &dependencies.assembler {
        configure.arg(format!("--x86asmexe={}", assembler.display()));
    }
    let configured = execute(
        &mut configure,
        "Configure FFmpeg with the locked macOS dependencies",
    )?;
    ensure!(
        std::str::from_utf8(&configured.stdout)?.contains("License: GPL version 3 or later"),
        "Unexpected license from the macOS FFmpeg configuration"
    );
    fs::write(work.join("configure.txt"), configured.stdout)?;
    let status = Command::new("/usr/bin/make")
        .current_dir(source_dir)
        .env_clear()
        .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
        .env("LC_ALL", "C")
        .env("MACOSX_DEPLOYMENT_TARGET", "15.0")
        .env("SDKROOT", sdk)
        .args(["-j4", "ffmpeg", "ffprobe"])
        .status()?;
    ensure!(status.success(), "Compile the two macOS FFmpeg executables");
    Ok(())
}

fn clean_process(program: &Path, home: &Path) -> Command {
    let mut command = Command::new(program);
    command
        .env_clear()
        .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
        .env("HOME", home)
        .env("LC_ALL", "C")
        .current_dir(home);
    command
}

fn verify_media(bin: &Path, work: &Path) -> Result<Value> {
    let home = work.join("offline-media-test");
    fs::create_dir(&home)?;
    let ffmpeg = bin.join("ffmpeg");
    let ffprobe = bin.join("ffprobe");
    for tool in [&ffmpeg, &ffprobe] {
        let version = execute(
            clean_process(tool, &home).arg("-version"),
            "Launch the relocated macOS media executable",
        )?;
        ensure!(
            std::str::from_utf8(&version.stdout)?.contains("9.0.2"),
            "Unexpected FFmpeg version in the relocated macOS profile"
        );
    }
    let inventory = execute(
        clean_process(&ffmpeg, &home).args(["-hide_banner", "-protocols"]),
        "Inspect the relocated FFmpeg protocol allowlist",
    )?;
    let protocols = std::str::from_utf8(&inventory.stdout)?
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.ends_with(':'))
        .collect::<BTreeSet<_>>();
    ensure!(
        protocols == ["fd", "file", "pipe"].into(),
        "Unexpected macOS media protocol"
    );
    let outputs = [
        ("libx264", "h264"),
        ("libx265", "hevc"),
        ("libsvtav1", "av1"),
        ("libvpx-vp9", "vp9"),
    ];
    let mut exercised = Vec::new();
    for (encoder, codec) in outputs {
        let output = home.join(format!("{codec}.mkv"));
        execute(
            clean_process(&ffmpeg, &home)
                .args([
                    "-hide_banner",
                    "-loglevel",
                    "error",
                    "-nostdin",
                    "-n",
                    "-f",
                    "lavfi",
                    "-i",
                    "testsrc2=size=128x72:rate=4:duration=1",
                    "-an",
                    "-c:v",
                    encoder,
                    "-threads",
                    "2",
                    "-frames:v",
                    "4",
                ])
                .arg(&output),
            &format!("Encode four frames with {encoder}"),
        )?;
        let probe = execute(
            clean_process(&ffprobe, &home)
                .args([
                    "-v",
                    "error",
                    "-select_streams",
                    "v:0",
                    "-count_frames",
                    "-show_entries",
                    "stream=codec_name,nb_read_frames",
                    "-of",
                    "json",
                ])
                .arg(&output),
            &format!("Decode the four {codec} frames"),
        )?;
        let data: Value = serde_json::from_slice(&probe.stdout)?;
        ensure!(
            data["streams"][0]["codec_name"] == codec
                && data["streams"][0]["nb_read_frames"] == "4",
            "Relocated macOS encoder produced an unexpected stream"
        );
        exercised.push(json!({"codec": codec, "encoder": encoder, "decoded_frames": 4, "output": digest(&output)?}));
    }
    Ok(json!({"protocols": protocols, "round_trips": exercised}))
}

fn sources(lock: &Value, work: &Path, kegs: &BTreeMap<String, PathBuf>) -> Result<PathBuf> {
    let root = work.join("corresponding-source");
    fs::create_dir(&root)?;
    let entries = lock["sources"]
        .as_array()
        .context("Missing exact macOS corresponding sources")?;
    ensure!(
        entries.len() == if cfg!(target_arch = "aarch64") { kegs.len() + 5 } else { 95 },
        "Incomplete macOS corresponding source lock"
    );
    for row in entries {
        let name = field(row, "filename")?;
        ensure!(basename(name), "Unexpected source archive name");
        download(row, &root.join(name), None)?;
    }
    if cfg!(target_arch = "aarch64") {
        let formulae = root.join("homebrew-formulae");
        fs::create_dir(&formulae)?;
        for (name, keg) in kegs {
            let path = keg.join(".brew").join(format!("{name}.rb"));
            ensure!(
                path.canonicalize()?.starts_with(keg),
                "Homebrew formula escaped its reviewed keg"
            );
            fs::copy(&path, formulae.join(format!("{name}.rb")))?;
        }
    } else {
        let recipes = root.join("macports-recipes");
        fs::rename(work.join("port-recipes"), &recipes)?;
        let commit = field(lock, "macports_key_commit")?;
        ensure!(commit.len() == 40 && commit.bytes().all(|byte| byte.is_ascii_hexdigit()), "Unexpected MacPorts key commit");
        let key = json!({"url":format!("https://raw.githubusercontent.com/macports/macports-base/{commit}/keys/archives/macports-archives-2025.pub"),
            "bytes":95,"sha256":lock["macports_key_sha256"]});
        download(&key, &recipes.join("macports-archives-2025.pub"), None)?;
    }
    fs::write(root.join(format!("{LABEL}-standard-input-lock.json")), LOCK)?;
    Ok(root)
}

pub fn build(root: &Path) -> Result<()> {
    let app_source = source(root)?;
    let lock: Value = serde_json::from_str(LOCK)?;
    ensure!(
        lock["target"] == TARGET && lock["schema_version"] == 1,
        "Unexpected macOS component lock"
    );
    let parent = nw_platform::security::private_directory(&root.join("target/macos-component"))?;
    let work = parent.join(format!("build-{}", std::process::id()));
    fs::create_dir(&work)?;
    let result = assemble(root, &work, &lock, &app_source);
    if let Err(error) = &result {
        crate::package::retain_failure(&work.join("failure.json"), error);
    }
    result
}

fn assemble(root: &Path, work: &Path, lock: &Value, app_source: &str) -> Result<()> {
    let dependencies = prepare_inputs(lock, work).context("Stage and identify the macOS compiler dependencies")?;
    let sources = sources(lock, work, &dependencies.libraries).context("Stage the macOS corresponding sources")?;
    unpack(&sources.join(FFMPEG), work, "ffmpeg-9.0.2").context("Unpack the verified FFmpeg release")?;
    unpack(&sources.join(FONT), work, "dejavu-sans-ttf-2.37").context("Unpack the verified DejaVu font")?;
    let source_dir = work.join("ffmpeg-9.0.2");
    compile(&source_dir, work, &dependencies).context("Compile FFmpeg against the staged macOS dependencies")?;
    let profile = work.join("profile");
    let bin = profile.join("bin");
    let fonts = profile.join("fonts");
    fs::create_dir_all(&bin)?;
    fs::create_dir(&fonts)?;
    let imports = relocate(&source_dir, &bin, &dependencies.libraries).context("Relocate macOS private media libraries")?;
    let font = work.join("dejavu-sans-ttf-2.37/ttf/DejaVuSans.ttf");
    ensure!(
        digest(&font)?["sha256"] == lock["font_sha256"],
        "Unexpected macOS fallback font"
    );
    fs::copy(font, fonts.join("DejaVuSans.ttf"))?;
    let media = verify_media(&bin, work).context("Exercise the relocated macOS codecs")?;
    let mut files = Vec::new();
    for dir in [&bin, &fonts] {
        for entry in fs::read_dir(dir)? {
            let path = entry?.path();
            let mut file = digest(&path)?;
            file["path"] = json!(
                path.strip_prefix(&profile)?
                    .to_str()
                    .context("UTF-8 embedded media path")?
            );
            files.push(file);
        }
    }
    files.sort_by(|a, b| a["path"].as_str().cmp(&b["path"].as_str()));
    let manifest = json!({"app_source": app_source, "target": TARGET, "schema_version": 1,
        "files": files, "system_imports": imports, "ffmpeg_source_commit": lock["ffmpeg_source_commit"],
        "verification": media});
    fs::write(
        profile.join("embed-profile.json"),
        serde_json::to_vec_pretty(&manifest)?,
    )?;
    deliver(root, work, &profile, &source_dir, &manifest).context("Build and verify the macOS desktop application")
}

fn deliver(
    root: &Path,
    work: &Path,
    profile: &Path,
    source_dir: &Path,
    manifest: &Value,
) -> Result<()> {
    let binary = crate::package::build_portable_executable(root, profile)?;
    let delivery = work.join("delivery");
    fs::create_dir(&delivery)?;
    let (executable, resources) = crate::package::macos_bundle(&delivery, "cpu-development")?;
    fs::copy(binary, &executable)?;
    fs::copy(
        root.join("LICENSE"),
        resources.join("Numera-Wiz.LICENSE.txt"),
    )?;
    fs::copy(
        root.join("THIRD-PARTY-NOTICES.md"),
        resources.join("THIRD-PARTY-NOTICES.md"),
    )?;
    fs::copy(
        source_dir.join("COPYING.GPLv3"),
        resources.join("FFmpeg.GPLv3.txt"),
    )?;
    fs::copy(
        work.join("dejavu-sans-ttf-2.37/LICENSE"),
        resources.join("DejaVuSans.LICENSE.txt"),
    )?;
    let app = delivery.join("Numera Wiz.app");
    sign(&app)?;
    let home = work.join("first-launch-home");
    fs::create_dir(&home)?;
    let receipt = crate::package::qualify_portable_application(
        clean_process(&executable, &home)
            .arg("--data-dir")
            .arg(home.join("state"))
            .args(["media", "capabilities", "refresh", "--force"]),
    )?;
    let files = manifest["files"]
        .as_array()
        .context("Embedded macOS component list")?;
    for tool in APPS {
        let path = format!("bin/{tool}");
        let row = files
            .iter()
            .find(|entry| entry["path"] == path)
            .context("Embedded macOS media helper")?;
        ensure!(
            receipt["result"][tool]["sha256"] == row["sha256"],
            "First launch used a different media helper"
        );
    }
    let archive = delivery.join(format!("{LABEL}-corresponding-source.tar.gz"));
    execute(
        Command::new("/usr/bin/tar")
            .current_dir(work)
            .args(["-czf"])
            .arg(&archive)
            .arg("corresponding-source"),
        "Archive the pinned macOS corresponding sources",
    )?;
    let packaged_app = delivery.join(format!("numera-wiz-{LABEL}.tar.gz"));
    execute(
        Command::new("/usr/bin/tar")
            .current_dir(&delivery)
            .args(["-czf"])
            .arg(&packaged_app)
            .arg("Numera Wiz.app"),
        "Archive the qualified macOS application",
    )?;
    let package = json!({"source":manifest["app_source"], "target":TARGET,
        "executable":digest(&executable)?, "application_archive":digest(&packaged_app)?,
        "source_archive":digest(&archive)?, "component":manifest, "first_launch":receipt,
        "signing":"ad-hoc", "notarized":false, "release_ready":false});
    fs::write(
        delivery.join("macos-portable-package.json"),
        serde_json::to_vec_pretty(&package)?,
    )?;
    Ok(())
}
