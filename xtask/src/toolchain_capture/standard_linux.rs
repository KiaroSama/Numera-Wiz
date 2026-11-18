//! Build and inspect the Linux CPU media profile in the offline compiler worker.
use super::{bootstrap, metadata, process};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    time::Instant,
};

pub(super) const DELIVERY: &str = "/standard-ffmpeg-linux";
const BINARIES: [&str; 2] = ["ffmpeg", "ffprobe"];
const PLATFORM_LIBRARIES: [&str; 8] = [
    "libc.so.6",
    "libdl.so.2",
    "libm.so.6",
    "libpthread.so.0",
    "libresolv.so.2",
    "librt.so.1",
    "libutil.so.1",
    "ld-linux-x86-64.so.2",
];
const CONFIGURE: &[&str] = &[
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
    "--pkg-config=/usr/bin/pkgconf",
    "--cc=/usr/bin/gcc-13",
    "--cxx=/usr/bin/g++-13",
    "--x86asmexe=/compiler-tools/nasm/install/bin/nasm",
];

fn imports(executable: &Path, deadline: Instant) -> Result<BTreeMap<String, PathBuf>> {
    let output = bootstrap::run_tool(
        process::command("/usr/bin/ldd").arg(executable),
        deadline,
        10,
    )?;
    let mut native = BTreeMap::new();
    for row in output.text()?.lines().map(str::trim) {
        if row.starts_with("linux-vdso.so.1 ") || row.starts_with("/lib64/ld-linux-x86-64.so.2 ") {
            continue;
        }
        let (name, target) = row
            .split_once(" => ")
            .context("Unknown ELF import record")?;
        ensure!(
            !name.is_empty()
                && name.len() <= 128
                && name
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"_+.-".contains(&byte)),
            "Invalid ELF import name"
        );
        let path = target
            .strip_suffix(')')
            .and_then(|text| text.rsplit_once(" (0x"))
            .map(|(path, _)| PathBuf::from(path))
            .context("ELF library could not be resolved")?;
        ensure!(
            path.is_absolute() && path.file_name().is_some_and(|found| found == name),
            "ELF library location differs from its requested name"
        );
        ensure!(
            native.insert(name.to_owned(), path).is_none(),
            "Duplicate ELF import"
        );
    }
    ensure!(
        !native.is_empty() && native.len() <= 96,
        "Unexpected ELF import count"
    );
    Ok(native)
}

fn file_identity(path: &Path) -> Result<Value> {
    let size = fs::metadata(path)?.len();
    ensure!(
        (1..=process::FILE_LIMIT).contains(&size),
        "Unexpected media artifact size"
    );
    let bytes = fs::read(path)?;
    ensure!(
        bytes.len() as u64 == size,
        "Media artifact changed during hashing"
    );
    Ok(json!({"bytes": size, "sha256": metadata::digest(&bytes)}))
}

fn distribution_package(package: &str, notices: &Path, deadline: Instant) -> Result<Value> {
    ensure!(
        !package.is_empty()
            && package.len() <= 128
            && package.bytes().all(|byte| byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || b".+-".contains(&byte)),
        "Invalid native distribution package name"
    );
    let output = bootstrap::run_tool(
        process::command("/usr/bin/dpkg-query").args([
            "--show",
            "--showformat=${Package}\\t${Version}\\t${source:Package}\\t${source:Version}\\n",
            package,
        ]),
        deadline,
        5,
    )?;
    let fields = output.text()?.trim_end().split('\t').collect::<Vec<_>>();
    let [name, version, source, source_version] = fields.as_slice() else {
        anyhow::bail!("Invalid native distribution package metadata");
    };
    ensure!(
        *name == package && !version.is_empty() && !source.is_empty() && !source_version.is_empty(),
        "Missing native distribution source identity"
    );
    let notice = notices.join(format!("{package}.txt"));
    fs::copy(format!("/usr/share/doc/{package}/copyright"), &notice)?;
    Ok(
        json!({"package":name,"version":version,"source":source,"source_version":source_version,
        "notice":file_identity(&notice)?}),
    )
}

fn library_package(path: &Path, notices: &Path, deadline: Instant) -> Result<Value> {
    let real = path.canonicalize()?;
    let mut paths = BTreeSet::from([path, real.as_path()]);
    let legacy = real
        .strip_prefix("/usr")
        .ok()
        .map(|path| Path::new("/").join(path));
    if let Some(legacy) = legacy.as_deref() {
        paths.insert(legacy);
    }
    for candidate in paths {
        let output = process::run(
            process::command("/usr/bin/dpkg-query")
                .arg("--search")
                .arg(candidate),
            deadline,
            5,
            None,
        )?;
        if output.status.success() {
            output.success()?;
            let (owner, _) = output
                .text()?
                .trim_end()
                .split_once(": ")
                .context("Missing library package owner")?;
            let package = owner.split_once(':').map_or(owner, |(package, _)| package);
            return distribution_package(package, notices, deadline);
        }
    }
    anyhow::bail!("No installed distribution package owns {}", path.display());
}

fn relocate(source: &Path, binary_dir: &Path, notices: &Path, deadline: Instant) -> Result<Value> {
    let imports_by_executable = BINARIES.map(|name| imports(&source.join(name), deadline));
    let mut libraries = BTreeMap::new();
    for imported in imports_by_executable {
        for (name, path) in imported? {
            ensure!(
                path.starts_with("/lib/x86_64-linux-gnu")
                    || path.starts_with("/usr/lib/x86_64-linux-gnu"),
                "ELF import falls outside the locked system library directory"
            );
            ensure!(
                libraries
                    .insert(name, path.clone())
                    .is_none_or(|found| found == path),
                "FFmpeg executables resolved different dependency files"
            );
        }
    }
    for name in BINARIES {
        fs::copy(source.join(name), binary_dir.join(name))?;
    }
    let mut payloads = Vec::from(BINARIES.map(str::to_owned));
    let mut provenance = BTreeMap::new();
    for (name, path) in &libraries {
        if !PLATFORM_LIBRARIES.contains(&name.as_str()) {
            fs::copy(path, binary_dir.join(name))?;
            payloads.push(name.clone());
            provenance.insert(name.clone(), library_package(path, notices, deadline)?);
        }
    }
    let mut members = BTreeMap::new();
    for name in payloads {
        let path = binary_dir.join(&name);
        bootstrap::run_tool(
            process::command("/usr/bin/patchelf")
                .args(["--set-rpath", "$ORIGIN"])
                .arg(&path),
            deadline,
            6,
        )?;
        members.insert(name, file_identity(&path)?);
    }
    for name in BINARIES {
        for (library, path) in imports(&binary_dir.join(name), deadline)? {
            if PLATFORM_LIBRARIES.contains(&library.as_str()) {
                ensure!(
                    libraries.get(&library) == Some(&path),
                    "Unexpected platform library resolution"
                );
            } else {
                ensure!(
                    path == binary_dir.join(&library),
                    "Private media library did not relocate"
                );
            }
        }
    }
    let os_provided = libraries
        .keys()
        .filter(|name| PLATFORM_LIBRARIES.contains(&name.as_str()))
        .collect::<Vec<_>>();
    Ok(json!({"files": members, "os_provided": os_provided, "package_provenance": provenance}))
}

fn check_runtime(binary_dir: &Path, delivery: &Path, deadline: Instant) -> Result<Value> {
    let ffmpeg = binary_dir.join("ffmpeg");
    let ffmpeg = ffmpeg.to_str().context("FFmpeg executable path")?;
    let ffprobe = binary_dir.join("ffprobe");
    let ffprobe = ffprobe.to_str().context("FFprobe executable path")?;
    let scratch = delivery.join("qualification");
    fs::create_dir(&scratch)?;
    let mut media = BTreeMap::new();
    for (encoder, codec, options) in [
        ("libx264", "h264", &[][..]),
        (
            "libx265",
            "hevc",
            &["-x265-params", "pools=1:frame-threads=1"],
        ),
        // The locked SVT-AV1 1.7 runtime sizes its own internal worker pool.
        (
            "libsvtav1",
            "av1",
            &["-preset", "12", "-svtav1-params", "lp=1"],
        ),
        (
            "libvpx-vp9",
            "vp9",
            &["-deadline", "realtime", "-cpu-used", "6"],
        ),
    ] {
        let artifact = scratch.join(format!("{codec}.mkv"));
        bootstrap::run_tool(
            process::command(ffmpeg)
                .args([
                    "-hide_banner",
                    "-nostdin",
                    "-v",
                    "error",
                    "-f",
                    "lavfi",
                    "-i",
                    "testsrc2=size=128x128:rate=4:duration=1",
                    "-c:v",
                    encoder,
                    "-threads",
                    "1",
                    "-pix_fmt",
                    "yuv420p",
                ])
                .args(options)
                .args(["-an", "-frames:v", "4"])
                .arg(&artifact),
            deadline,
            30,
        )?;
        let probed = bootstrap::run_tool(
            process::command(ffprobe)
                .args([
                    "-v",
                    "error",
                    "-show_entries",
                    "stream=codec_name,width,height",
                    "-of",
                    "json",
                ])
                .arg(&artifact),
            deadline,
            10,
        )?;
        let probed: Value = serde_json::from_slice(&probed.stdout)?;
        ensure!(
            probed["streams"] == json!([{"codec_name":codec,"width":128,"height":128}]),
            "Encoded smoke fixture does not match {codec}"
        );
        let decoded = bootstrap::run_tool(
            process::command(ffmpeg)
                .args(["-hide_banner", "-nostdin", "-v", "error", "-i"])
                .arg(&artifact)
                .args(["-map", "0:v:0", "-f", "framehash", "-hash", "sha256", "-"]),
            deadline,
            20,
        )?;
        let frame_count = decoded
            .text()?
            .lines()
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .count();
        ensure!(
            frame_count == 4,
            "Incorrect decoded smoke frame count for {codec}"
        );
        let identity = file_identity(&artifact)?;
        fs::write(
            scratch.join(format!("{codec}.framehash.txt")),
            &decoded.stdout,
        )?;
        media.insert(
            codec,
            json!({
                "encoder":encoder,"frames":frame_count,"fixture":identity,
                "decoded_framehash_sha256":metadata::digest(&decoded.stdout)
            }),
        );
    }
    Ok(json!(media))
}

pub(super) fn build(inputs: &Path, locked: &Value, deadline: Instant) -> Result<Value> {
    let archive = inputs.join(locked["filename"].as_str().context("FFmpeg archive name")?);
    bootstrap::verify(
        &archive,
        locked["size"].as_u64().context("FFmpeg archive length")?,
        locked["sha256"]
            .as_str()
            .context("FFmpeg archive SHA-256")?,
    )?;
    let source = Path::new("/build-standard-ffmpeg");
    let delivery = Path::new(DELIVERY);
    let binary_dir = delivery.join("bin");
    let notices = delivery.join("notices");
    let fonts = delivery.join("fonts");
    fs::create_dir(source)?;
    fs::create_dir(delivery)?;
    fs::create_dir(&binary_dir)?;
    fs::create_dir(&notices)?;
    fs::create_dir(&fonts)?;
    bootstrap::run_tool(
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
        50,
    )?;
    let configured = bootstrap::run_tool(
        process::command("/bin/sh")
            .current_dir(source)
            .arg("./configure")
            .args(CONFIGURE),
        deadline,
        180,
    )
    .context("Configure FFmpeg with the reviewed Linux libraries")?;
    ensure!(
        configured
            .text()?
            .contains("License: GPL version 3 or later"),
        "Unexpected FFmpeg license"
    );
    fs::write(delivery.join("configure.txt"), configured.stdout)?;
    let compilation = bootstrap::run_tool(
        process::command("/usr/bin/make")
            .current_dir(source)
            .args(["-j4", "ffmpeg", "ffprobe"]),
        deadline,
        1020,
    )
    .context("Compile the two Linux FFmpeg executables")?;
    fs::write(delivery.join("build.txt"), compilation.stdout)?;
    fs::copy(
        source.join("COPYING.GPLv3"),
        notices.join("COPYING.FFmpeg.GPLv3"),
    )?;
    let font = fonts.join("DejaVuSans.ttf");
    fs::copy("/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf", &font)?;
    let font_package = distribution_package("fonts-dejavu-core", &notices, deadline)?;
    let runtime = relocate(source, &binary_dir, &notices, deadline)
        .context("Relocate the native ELF dependency closure")?;
    let executable = binary_dir.join("ffmpeg");
    let mut facts = BTreeMap::new();
    for name in [
        "version",
        "buildconf",
        "encoders",
        "decoders",
        "muxers",
        "demuxers",
        "filters",
        "bsfs",
        "protocols",
        "license",
    ] {
        let observed = bootstrap::run_tool(
            process::command(executable.to_str().context("FFmpeg executable path")?)
                .arg("-hide_banner")
                .arg(format!("-{name}")),
            deadline,
            10,
        )?;
        if name == "protocols" {
            let protocols: BTreeSet<_> = observed
                .text()?
                .lines()
                .filter_map(|line| line.strip_prefix("  "))
                .collect();
            ensure!(
                protocols == ["fd", "file", "pipe"].into(),
                "Unexpected media protocol closure"
            );
        }
        let path = delivery.join(format!("ffmpeg-{name}.txt"));
        fs::write(&path, observed.stdout)?;
        facts.insert(name, file_identity(&path)?);
    }
    let qualifications = check_runtime(&binary_dir, delivery, deadline)
        .context("Exercise the relocated CPU codec round trips")?;
    Ok(json!({
        "status": "linux_cpu_component_build_candidate",
        "source": locked,
        "configure": CONFIGURE,
        "runtime": runtime,
        "actual_features": facts,
        "cpu_round_trips": qualifications,
        "font":{"file":file_identity(&font)?,"provenance":font_package},
    }))
}
