//! Stage the signed Intel runtime inputs in an isolated native prefix.
use super::{basename, download, execute, field, links, sign, system_import};
use crate::package::digest;
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{fs, os::unix::fs::symlink, path::{Component, Path, PathBuf}, process::Command};

fn archive_path(row: &Value, directory: &Path) -> Result<PathBuf> {
    let filename = field(row, "filename")?;
    ensure!(basename(filename) && filename.ends_with(".tbz2"), "Invalid Intel port archive name");
    Ok(directory.join(filename))
}

fn stage_archives(rows: &[Value], downloads: &Path) -> Result<()> {
    std::thread::scope(|scope| -> Result<()> {
        let workers = rows.chunks(rows.len().div_ceil(6)).map(|batch| {
            scope.spawn(move || -> Result<()> {
                for row in batch {
                    let path = archive_path(row, downloads)?;
                    let url = field(row, "url")?;
                    ensure!(url.starts_with("https://mirrors.mit.edu/macports/packages/"), "Unexpected Intel port archive mirror");
                    download(row, &path, None)?;
                    let signature = json!({"url":format!("{url}.sig"), "bytes":159, "sha256":row["signature_sha256"]});
                    download(&signature, &path.with_extension("tbz2.sig"), None)?;
                }
                Ok(())
            })
        }).collect::<Vec<_>>();
        for worker in workers {
            worker.join().map_err(|_| anyhow::anyhow!("Intel port archive transfer stopped"))??;
        }
        Ok(())
    })
}

pub(super) fn stage(lock: &Value, work: &Path) -> Result<PathBuf> {
    let archives = lock["archives"].as_array().context("Intel port archive lock")?;
    ensure!(archives.len() == 64, "Incomplete Intel port archive lock");
    let downloads = work.join("ports");
    let installation = work.join("macports");
    let recipes = work.join("port-recipes");
    for path in [&downloads, &installation, &recipes] {
        fs::create_dir(path)?;
    }
    stage_archives(archives, &downloads)?;
    for row in archives {
        let name = field(row, "name")?;
        ensure!(basename(name), "Invalid Intel port name");
        let archive = archive_path(row, &downloads)?;
        let inventory = execute(Command::new("/usr/bin/tar").arg("-tf").arg(&archive), "List Intel port archive")?;
        let inventory = std::str::from_utf8(&inventory.stdout)?;
        ensure!((1..=120_000).contains(&inventory.lines().count()), "Unbounded Intel port archive inventory");
        for row in inventory.lines() {
            let member = row.trim_start_matches("./").trim_end_matches('/');
            let admitted = matches!(member, "" | ".")
                || Path::new(member).components().all(|component| matches!(component, Component::Normal(_)));
            ensure!(admitted && !member.contains(['\\', ':']), "Unexpected Intel port archive entry");
        }
        if row["source_recipe_commit"].is_string() {
            let recipe = execute(Command::new("/usr/bin/tar").arg("-xOf").arg(&archive)
                .arg("./+PORTFILE"), "Read exact Intel source recipe")?;
            let path = recipes.join(format!("{name}--Portfile"));
            fs::write(&path, recipe.stdout)?;
            ensure!(digest(&path)?["sha256"] == row["portfile_sha256"], "Intel port recipe differs from its archive lock");
            fs::copy(archive.with_extension("tbz2.sig"), recipes.join(archive.file_name()
                .context("Intel port archive name")?).with_extension("tbz2.sig"))?;
        }
        execute(Command::new("/usr/bin/tar").current_dir(&installation).arg("-xf")
            .arg(&archive).arg("./opt/local"), "Stage the authenticated Intel port payload")?;
    }
    let prefix = installation.join("opt/local");
    rebase_port_links(&prefix, &prefix)?;
    Ok(prefix)
}

fn rebase_port_links(directory: &Path, prefix: &Path) -> Result<()> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let path = entry.path();
        if kind.is_dir() {
            rebase_port_links(&path, prefix)?;
        } else if kind.is_symlink() {
            let target = path.read_link()?;
            if let Ok(relative) = target.strip_prefix("/opt/local") {
                ensure!(relative.components().all(|c| matches!(c, Component::Normal(_))),
                    "Invalid absolute Intel port link");
                fs::remove_file(&path)?;
                symlink(prefix.join(relative), &path)?;
            }
        }
    }
    Ok(())
}

pub(super) fn pkg_config(prefix: &Path) -> Result<PathBuf> {
    let tool = prefix.join("bin/pkg-config");
    let imports = links(&tool, false)?;
    let foreign = imports.iter().filter(|name| !system_import(name)).map(String::as_str).collect::<Vec<_>>();
    ensure!(foreign == ["/opt/local/lib/libiconv.2.dylib"]
        && prefix.join("lib/libiconv.2.dylib").canonicalize()?.starts_with(prefix),
        "Unexpected Intel pkg-config dependencies");
    execute(Command::new("/usr/bin/install_name_tool")
        .args(["-change", foreign[0], "@loader_path/../lib/libiconv.2.dylib"]).arg(&tool),
        "Relocate the Intel pkg-config tool")?;
    sign(&tool)?;
    let version = execute(Command::new(&tool).env_clear().arg("--version"), "Identify the Intel pkg-config executable")?;
    ensure!(std::str::from_utf8(&version.stdout)?.trim() == "0.29.2", "Unexpected Intel pkg-config build");
    Ok(tool)
}
