//! Materialize the media payload in an owned content-addressed directory.
use anyhow::{Context, Result, ensure};
use nw_platform::{files, security};
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};
use tokio_util::sync::CancellationToken;

struct EmbeddedFile {
    group: &'static str,
    name: &'static str,
    sha256: &'static str,
    bytes: &'static [u8],
}

include!(concat!(env!("OUT_DIR"), "/embedded_media.rs"));

fn ordinary_entries(path: &Path) -> Result<BTreeSet<String>> {
    ensure!(
        security::private_directory(path)? == path,
        "The installed media directory was redirected"
    );
    fs::read_dir(path)?
        .map(|entry| {
            entry?
                .file_name()
                .into_string()
                .map_err(|_| anyhow::anyhow!("Non-UTF-8 media profile entry"))
        })
        .collect()
}

fn verify(profile: &Path) -> Result<PathBuf> {
    ensure!(
        ordinary_entries(profile)? == ["bin".to_owned(), "fonts".to_owned()].into(),
        "The installed media profile contains unexpected directories"
    );
    for group in ["bin", "fonts"] {
        let entries = FILES.iter().filter(|file| file.group == group);
        let expected = entries
            .clone()
            .map(|file| file.name.to_owned())
            .collect::<BTreeSet<_>>();
        let directory = profile.join(group);
        ensure!(
            ordinary_entries(&directory)? == expected,
            "The installed media profile has unexpected files"
        );
        for entry in entries {
            let path = directory.join(entry.name);
            let length = entry.bytes.len() as u64;
            security::verify_private_file(&path, length)?;
            let identity = files::identity(&path, &CancellationToken::new())?;
            ensure!(
                Path::new(&identity.path) == path
                    && identity.bytes.parse::<u64>() == Ok(length)
                    && identity.sha256 == entry.sha256,
                "The installed media file {} is inconsistent with the executable",
                entry.name
            );
        }
    }
    Ok(profile.join("bin"))
}

pub(super) fn installed(root: &Path) -> Result<PathBuf> {
    let parent = root.join("embedded-media");
    ensure!(
        security::private_directory(&parent)? == parent,
        "The embedded media parent was redirected"
    );
    let destination = parent.join(PROFILE);
    if destination.try_exists()? {
        return verify(&destination);
    }
    let staging = tempfile::Builder::new()
        .prefix(".component-")
        .tempdir_in(parent)?;
    for group in ["bin", "fonts"] {
        security::private_directory(&staging.path().join(group))?;
    }
    for entry in FILES {
        security::write_private(
            &staging.path().join(entry.group).join(entry.name),
            entry.bytes,
            false,
        )?;
    }
    verify(staging.path())?;
    match fs::rename(staging.path(), &destination) {
        Ok(()) => Ok(destination.join("bin")),
        Err(_) if destination.try_exists()? => verify(&destination),
        Err(error) => Err(error).context("Publish the complete embedded media profile"),
    }
}
