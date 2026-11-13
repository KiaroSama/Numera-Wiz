//! Verify the tracked source boundary and the retained Rust test inventory.
use anyhow::{Context, Result, bail};
use std::{collections::BTreeSet, fs, path::Path, process::Command};

pub fn verify(root: &Path) -> Result<()> {
    let output = Command::new("git")
        .current_dir(root)
        .args(["ls-files", "--cached", "-z"])
        .output()
        .context("Read tracked repository paths")?;
    if !output.status.success() {
        bail!("Cannot inspect the tracked source boundary");
    }
    let paths = std::str::from_utf8(&output.stdout).context("Tracked paths must be UTF-8")?;
    let mut tests = BTreeSet::new();
    let mut rust_files = 0;
    for name in paths.split('\0').filter(|name| !name.is_empty()) {
        if name.starts_with(".ai/")
            || name.starts_with(".specify/")
            || name.starts_with("specs/")
            || name.starts_with(".agents/skills/speckit-")
        {
            bail!("Local-only planning material is tracked: {name}");
        }
        let path = Path::new(name);
        let filename = path.file_name().and_then(|value| value.to_str()).unwrap_or("");
        let build_name = filename.to_ascii_lowercase();
        if ["configure", "makefile", "gnumakefile", "cmakelists.txt"].contains(&build_name.as_str())
            || build_name.ends_with("_makefile")
        {
            bail!("Non-Rust build program is tracked: {name}");
        }
        let extension = path.extension().and_then(|value| value.to_str());
        let normalized_extension = extension.map(str::to_ascii_lowercase);
        let extension = normalized_extension.as_deref();
        if extension.is_some_and(|extension| {
            [
                "py", "pyc", "c", "h", "cc", "cpp", "cxx", "hpp", "go", "sh", "ps1", "bat", "cmd",
                "js", "ts", "tsx", "jsx", "html", "wasm", "mk", "mak", "cmake", "m4",
            ]
            .contains(&extension)
                || [
                    "zip", "gz", "tgz", "bz2", "xz", "7z", "exe", "dll", "so", "dylib",
                ]
                .contains(&extension)
        }) {
            bail!("Non-Rust source, a binary payload or an archive is tracked: {name}");
        }
        if extension == Some("rs") {
            rust_files += 1;
            if !["crates/", "apps/", "xtask/", "test-support/"]
                .iter()
                .any(|prefix| name.starts_with(prefix))
            {
                bail!("Unowned Rust source is tracked outside application/tooling roots: {name}");
            }
            let source = fs::read_to_string(root.join(path))
                .with_context(|| format!("Read UTF-8 Rust source {name}"))?;
            collect_tests(name, &source, &mut tests)?;
        }
    }
    let baseline: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("ci/test-inventory.json"))?)?;
    let mut expected = BTreeSet::new();
    for row in baseline["tests"].as_array().context("Baseline tests")? {
        let path = row["path"].as_str().context("Baseline test path")?;
        let name = row["name"].as_str().context("Baseline test name")?;
        let kind = row["kind"].as_str().context("Baseline test kind")?;
        let ignored = row["ignored"].as_bool().context("Baseline ignored state")?;
        if !expected.insert((path.to_owned(), name.to_owned(), kind.to_owned(), ignored)) {
            bail!("Duplicate baseline test entry: {path}::{name}");
        }
    }
    if tests != expected {
        bail!(
            "Rust test inventory changed: missing {:?}; added {:?}",
            expected.difference(&tests).collect::<Vec<_>>(),
            tests.difference(&expected).collect::<Vec<_>>()
        );
    }
    let directory = root.join("target");
    fs::create_dir_all(&directory)?;
    fs::write(
        directory.join("source-language-report.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "schema_version": 1,
            "application_language": "Rust",
            "media_runtime": "external FFmpeg/ffprobe",
            "rust_source_files": rust_files,
            "retained_test_entry_points": tests.len(),
            "baseline": baseline["baseline"],
            "scope": "Tracked sources only; generated browser glue and external component builds are not first-party sources"
        }))?,
    )?;
    tracing::info!(
        rust_files,
        tests = tests.len(),
        "Rust source boundary and original test inventory verified"
    );
    Ok(())
}

type Test = (String, String, String, bool);
fn collect_tests(path: &str, source: &str, tests: &mut BTreeSet<Test>) -> Result<()> {
    for (marker, kind) in [("#[test]", "unit"), ("#[tokio::test", "async")] {
        for (offset, _) in source.match_indices(marker) {
            let prefix = &source[..offset];
            let line = prefix.rsplit_once('\n').map_or(prefix, |(_, line)| line);
            if !line.trim().is_empty() {
                continue;
            }
            let body = &source[offset + marker.len()..];
            let function = body.find("fn ").context("Test attribute has no function")?;
            let name = body[function + 3..]
                .split(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
                .next()
                .filter(|name| !name.is_empty())
                .context("Test function name")?;
            let ignored = body[..function].contains("#[ignore");
            if !tests.insert((path.to_owned(), name.to_owned(), kind.to_owned(), ignored)) {
                bail!("Duplicate test identity: {path}::{name}");
            }
        }
    }
    Ok(())
}
