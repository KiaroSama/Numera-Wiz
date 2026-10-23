//! Embed only verified same-source UI assets, never arbitrary runtime files.
use sha2::{Digest, Sha256};
use std::{env, fs, path::PathBuf};
fn main() {
    println!("cargo:rerun-if-env-changed=NW_EMBED_WEB_DIR");
    let destination =
        PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR")).join("embedded_web.rs");
    if env::var_os("CARGO_FEATURE_EMBEDDED_WEB").is_none() {
        fs::write(
            destination,
            "pub const UI_SOURCE: &str = \"none\"; pub const ASSETS: &[Asset] = &[];",
        )
        .expect("empty development assets");
        return;
    }
    let directory = PathBuf::from(
        env::var_os("NW_EMBED_WEB_DIR")
            .expect("Embedded UI requires the CI-built NW_EMBED_WEB_DIR"),
    );
    println!(
        "cargo:rerun-if-changed={}",
        directory.join("manifest.json").display()
    );
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(directory.join("manifest.json")).expect("UI manifest"))
            .expect("valid UI manifest");
    assert_eq!(
        manifest["source"].as_str(),
        env::var("GITHUB_SHA").ok().as_deref(),
        "UI source identity mismatch"
    );
    let assets = manifest["assets"].as_array().expect("asset records");
    assert!(assets.len() <= 64);
    let mut code = format!(
        "pub const UI_SOURCE: &str = {:?};\npub const ASSETS: &[Asset] = &[\n",
        manifest["source"].as_str().expect("source")
    );
    let mut total = 0usize;
    for asset in assets {
        let name = asset["path"].as_str().expect("asset path");
        let path = PathBuf::from(name);
        assert!(
            !path.is_absolute()
                && path
                    .components()
                    .all(|part| matches!(part, std::path::Component::Normal(_)))
        );
        assert!(name == "index.html" || name.starts_with("assets/"));
        let mime = asset["mime"].as_str().expect("MIME");
        assert!(
            [
                "application/wasm",
                "application/javascript",
                "text/css",
                "text/html; charset=utf-8"
            ]
            .contains(&mime)
        );
        let full = directory.join(&path);
        let bytes = fs::read(&full).expect("asset bytes");
        total = total.checked_add(bytes.len()).expect("asset byte overflow");
        assert!(total <= 64 * 1024 * 1024);
        assert_eq!(asset["bytes"].as_u64(), Some(bytes.len() as u64));
        let digest = hex::encode(Sha256::digest(&bytes));
        assert_eq!(asset["sha256"].as_str(), Some(digest.as_str()));
        println!("cargo:rerun-if-changed={}", full.display());
        code.push_str(&format!(
            "Asset {{ path: {:?}, mime: {:?}, immutable: {}, bytes: include_bytes!({:?}) }},\n",
            name,
            mime,
            asset["immutable"].as_bool().expect("cache policy"),
            full
        ));
    }
    code.push_str("];\n");
    fs::write(destination, code).expect("embedded namespace");
}
