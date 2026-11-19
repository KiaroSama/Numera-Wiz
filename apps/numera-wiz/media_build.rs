//! Validate the exact hosted Windows payload before embedding it into the application.
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, env, fs, path::PathBuf};

fn admitted(group: &str, name: &str) -> bool {
    let Some((stem, extension)) = name.rsplit_once('.') else {
        return false;
    };
    let base = stem.split('.').next().unwrap_or_default().to_ascii_uppercase();
    if stem.is_empty()
        || !stem.as_bytes()[0].is_ascii_alphanumeric()
        || name.len() > 128
        || !name.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
        || matches!(base.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (base.len() == 4 && (base.starts_with("COM") || base.starts_with("LPT"))
            && base.as_bytes()[3].is_ascii_digit())
    {
        return false;
    }
    match group {
        "bin" => matches!(name, "ffmpeg.exe" | "ffprobe.exe") || extension == "dll",
        "fonts" => matches!(extension, "ttf" | "otf"),
        _ => false,
    }
}

pub fn generate() {
    println!("cargo:rerun-if-env-changed=NW_EMBED_MEDIA_DIR");
    println!("cargo:rerun-if-env-changed=NW_SOURCE_SHA");
    if env::var_os("CARGO_FEATURE_EMBEDDED_MEDIA").is_none() {
        return;
    }
    assert_eq!(env::var("GITHUB_ACTIONS").as_deref(), Ok("true"));
    assert_eq!(env::var("TARGET").as_deref(), Ok("x86_64-pc-windows-msvc"));
    let directory = PathBuf::from(env::var_os("NW_EMBED_MEDIA_DIR").expect("Hosted media profile directory"))
        .canonicalize().expect("canonical hosted media profile");
    let manifest_path = directory.join("embed-profile.json");
    println!("cargo:rerun-if-changed={}", manifest_path.display());
    assert!(fs::metadata(&manifest_path).expect("media manifest metadata").len() <= 64 * 1024);
    let manifest: serde_json::Value = serde_json::from_slice(&fs::read(&manifest_path).expect("media manifest"))
        .expect("valid media manifest");
    assert_eq!(manifest["schema_version"], 1);
    assert_eq!(manifest["target"], "x86_64-pc-windows-msvc");
    let source = env::var("NW_SOURCE_SHA").expect("exact app source for embedded media");
    assert!(source.len() == 40 && source.bytes().all(|byte| byte.is_ascii_hexdigit()));
    assert_eq!(manifest["app_source"].as_str(), Some(source.as_str()));
    let assets = manifest["files"].as_array().expect("media payload records");
    assert!((3..=256).contains(&assets.len()));
    let mut paths = BTreeSet::new();
    let mut identity_records = Vec::new();
    let mut code = String::from("const FILES: &[EmbeddedFile] = &[\n");
    let mut total_bytes = 0;
    for asset in assets {
        let path = asset["path"].as_str().expect("media asset path");
        let (group, name) = path.split_once('/').expect("media asset directory and name");
        assert!(admitted(group, name) && paths.insert(path.to_ascii_lowercase()));
        let sha = asset["sha256"].as_str().expect("media SHA-256");
        let bytes = asset["bytes"].as_u64().expect("media file size");
        assert!((1..=96 * 1024 * 1024).contains(&bytes));
        total_bytes += bytes;
        assert!(total_bytes <= 192 * 1024 * 1024);
        let file = directory.join(path);
        let info = fs::symlink_metadata(&file).expect("ordinary media file");
        assert!(info.is_file() && info.len() == bytes);
        assert_eq!(file.canonicalize().expect("canonical media file"), file);
        let observed = hex::encode(Sha256::digest(fs::read(&file).expect("media asset bytes")));
        assert_eq!(observed, sha);
        println!("cargo:rerun-if-changed={}", file.display());
        identity_records.push(format!("{path}\t{bytes}\t{sha}\n"));
        code.push_str(&format!(
            "EmbeddedFile {{ group: {group:?}, name: {name:?}, sha256: {sha:?}, bytes: include_bytes!({file:?}) }},\n"
        ));
    }
    assert!(paths.contains("bin/ffmpeg.exe") && paths.contains("bin/ffprobe.exe"));
    assert!(paths.iter().any(|path| path.starts_with("fonts/")));
    identity_records.sort_unstable();
    let profile = hex::encode(Sha256::digest(identity_records.concat()));
    code.push_str(&format!("];\nconst PROFILE: &str = {profile:?};\n"));
    let directory = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR"));
    fs::write(directory.join("embedded_media.rs"), code).expect("embedded media source");
}
