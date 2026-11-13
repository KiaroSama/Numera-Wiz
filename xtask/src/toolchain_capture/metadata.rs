//! Strict framing and identity joins, never a substitute for APT's solver.
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{BufRead, Read},
    path::{Component, Path},
};

pub const SNAPSHOT: &str = "https://snapshot.ubuntu.com/ubuntu/20261101T000000Z";
const STANZA_LIMIT: u64 = 1024 * 1024;
pub type Fields = BTreeMap<String, String>;

pub fn digest(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

pub fn decimal(text: &str) -> Result<u64> {
    ensure!(
        !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit()),
        "invalid unsigned decimal"
    );
    Ok(text.parse()?)
}

pub fn safe_path(text: &str) -> Result<()> {
    ensure!(
        !text.is_empty()
            && text.len() <= 4096
            && text.is_ascii()
            && !text
                .bytes()
                .any(|b| b.is_ascii_control() || b.is_ascii_whitespace())
            && !text.contains(['\\', ':', '\0'])
            && !text.contains("//"),
        "unsafe evidence path"
    );
    ensure!(
        Path::new(text)
            .components()
            .all(|c| matches!(c, Component::Normal(_))),
        "unsafe evidence path"
    );
    ensure!(
        text.split('/')
            .all(|p| p != "." && p != ".." && !p.is_empty() && p.len() <= 255),
        "unsafe evidence path"
    );
    Ok(())
}

pub fn unique_path(seen: &mut BTreeSet<String>, text: &str) -> Result<()> {
    safe_path(text)?;
    ensure!(
        seen.insert(text.to_ascii_lowercase()),
        "duplicate or case-aliased evidence path"
    );
    Ok(())
}

pub fn check_bytes(bytes: &[u8], size: u64, sha256: &str) -> Result<()> {
    ensure!(
        bytes.len() as u64 == size && digest(bytes) == sha256,
        "size/SHA256 mismatch"
    );
    Ok(())
}

pub fn fields(text: &str) -> Result<Fields> {
    ensure!(
        text.len() as u64 <= STANZA_LIMIT && !text.contains('\r'),
        "oversized or CR-containing stanza"
    );
    let mut fields = Fields::new();
    let mut previous: Option<String> = None;
    for line in text.lines() {
        if line.starts_with([' ', '\t']) {
            let key = previous.as_ref().context("orphan continuation")?;
            let value = fields.get_mut(key).context("continuation field")?;
            value.push('\n');
            value.push_str(line.trim_start());
        } else {
            let (name, value) = line.split_once(':').context("malformed deb822 field")?;
            ensure!(
                !name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'),
                "malformed deb822 key"
            );
            ensure!(
                !fields.keys().any(|key| key.eq_ignore_ascii_case(name)),
                "duplicate or case-aliased deb822 field"
            );
            fields.insert(name.to_string(), value.trim_start().to_string());
            previous = Some(name.to_string());
        }
    }
    Ok(fields)
}

pub fn stanzas(
    mut reader: impl BufRead,
    mut visit: impl FnMut(Fields) -> Result<()>,
) -> Result<()> {
    let mut stanza = String::new();
    let mut count = 0_u64;
    loop {
        let mut line = String::new();
        let read = (&mut reader).take(STANZA_LIMIT + 1).read_line(&mut line)?;
        if read == 0 {
            ensure!(stanza.is_empty(), "truncated metadata stanza");
            break;
        }
        ensure!(line.ends_with('\n'), "truncated metadata line");
        ensure!(line.len() as u64 <= STANZA_LIMIT, "oversized metadata line");
        if line == "\n" {
            if !stanza.is_empty() {
                count = count.checked_add(1).context("package count overflow")?;
                ensure!(count <= 1_000_000, "package count exceeded");
                visit(fields(&stanza)?)?;
                stanza.clear();
            }
        } else {
            ensure!(
                (stanza.len() as u64)
                    .checked_add(line.len() as u64)
                    .context("stanza size overflow")?
                    <= STANZA_LIMIT,
                "package stanza exceeded 1MiB"
            );
            stanza.push_str(&line);
        }
    }
    Ok(())
}

pub fn sha256(text: &str) -> Result<()> {
    ensure!(
        text.len() == 64
            && text
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()),
        "invalid SHA256"
    );
    Ok(())
}

pub fn release_index(fields: &Fields, path: &str) -> Result<(u64, String)> {
    safe_path(path)?;
    let mut found = None;
    for line in fields
        .get("SHA256")
        .context("Release has no SHA256 section")?
        .lines()
    {
        if line.is_empty() {
            continue;
        }
        let parts: Vec<_> = line.split_whitespace().collect();
        ensure!(parts.len() == 3, "malformed Release checksum line");
        sha256(parts[0])?;
        safe_path(parts[2])?;
        let size = decimal(parts[1])?;
        if parts[2] == path {
            ensure!(
                found.replace((size, parts[0].into())).is_none(),
                "duplicate Release index"
            );
        }
    }
    found.context("required compressed index missing from authenticated Release")
}

pub fn release(fields: &Fields, pocket: &str) -> Result<()> {
    ensure!(
        fields.get("Suite").map(String::as_str) == Some(pocket),
        "Release suite mismatch"
    );
    ensure!(
        fields.get("Codename").map(String::as_str) == Some("noble"),
        "Release codename mismatch"
    );
    ensure!(
        fields
            .get("Architectures")
            .context("Release architectures")?
            .split_whitespace()
            .any(|a| a == "amd64"),
        "Release lacks amd64"
    );
    for component in ["main", "restricted", "universe", "multiverse"] {
        ensure!(
            fields
                .get("Components")
                .context("Release components")?
                .split_whitespace()
                .any(|c| c == component),
            "Release lacks selected component"
        );
    }
    ensure!(
        !fields.get("Date").context("Release date")?.is_empty(),
        "Release date missing"
    );
    Ok(())
}

pub fn signature_status(success: bool, status: &str) -> Result<Vec<String>> {
    ensure!(success, "archive signature verifier failed");
    let mut fingerprints = BTreeSet::new();
    for line in status.lines() {
        if let Some(record) = line.strip_prefix("[GNUPG:] ") {
            let parts: Vec<_> = record.split_whitespace().collect();
            ensure!(
                !matches!(
                    parts.first().copied(),
                    Some(
                        "BADSIG"
                            | "ERRSIG"
                            | "NO_PUBKEY"
                            | "EXPSIG"
                            | "EXPKEYSIG"
                            | "REVKEYSIG"
                            | "FAILURE"
                    )
                ),
                "archive verifier reported an invalid signature"
            );
            if parts.first() == Some(&"VALIDSIG") {
                ensure!(
                    parts.len() == 11,
                    "missing signing or primary-key fingerprint status"
                );
                let fingerprint = *parts.get(1).context("signature fingerprint missing")?;
                ensure!(
                    fingerprint.len() == 40 && fingerprint.bytes().all(|b| b.is_ascii_hexdigit()),
                    "invalid signer fingerprint"
                );
                fingerprints.insert(fingerprint.to_ascii_uppercase());
                let primary = parts[10];
                ensure!(
                    primary.len() == 40 && primary.bytes().all(|b| b.is_ascii_hexdigit()),
                    "invalid primary-key fingerprint"
                );
                fingerprints.insert(primary.to_ascii_uppercase());
            }
        }
    }
    ensure!(!fingerprints.is_empty(), "no actual VALIDSIG status");
    Ok(fingerprints.into_iter().collect())
}

pub fn package_name(text: &str) -> Result<()> {
    ensure!(
        !text.is_empty()
            && text
                .bytes()
                .next()
                .is_some_and(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
            && text
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"+.-".contains(&b)),
        "invalid package name"
    );
    Ok(())
}

pub fn version(text: &str) -> Result<()> {
    ensure!(
        !text.is_empty()
            && text
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b".+:~-".contains(&b)),
        "invalid Debian version"
    );
    let upstream = if let Some((epoch, rest)) = text.split_once(':') {
        decimal(epoch)?;
        ensure!(!rest.contains(':'), "multiple version epochs");
        rest
    } else {
        text
    };
    ensure!(
        upstream.as_bytes().first().is_some_and(u8::is_ascii_digit),
        "version must start with digit after epoch"
    );
    Ok(())
}

fn architecture(text: &str) -> Result<()> {
    ensure!(
        matches!(text, "amd64" | "all"),
        "unexpected package architecture: {text}"
    );
    Ok(())
}

pub fn inventory(text: &str) -> Result<Vec<Value>> {
    ensure!(text.ends_with('\n'), "truncated dpkg inventory");
    let mut result = Vec::new();
    let mut seen = BTreeSet::new();
    for line in text.lines() {
        let parts: Vec<_> = line.split('\t').collect();
        ensure!(parts.len() == 4, "malformed dpkg inventory");
        package_name(parts[0])?;
        architecture(parts[1])?;
        version(parts[2])?;
        ensure!(
            seen.insert(parts[0].to_string()),
            "duplicate or unsupported multiple-architecture dpkg identity"
        );
        let status: Vec<_> = parts[3].split_whitespace().collect();
        ensure!(
            status.len() == 3
                && matches!(
                    status[0],
                    "install" | "hold" | "deinstall" | "purge" | "unknown"
                )
                && status[1] == "ok"
                && status[2] == "installed",
            "base package is not fully installed: {}",
            parts[0]
        );
        result.push(json!({"name":parts[0],"architecture":parts[1],"version":parts[2],"status":parts[3],"origin":"unchanged_base","change":"unchanged"}));
    }
    ensure!(!result.is_empty(), "empty base inventory");
    Ok(result)
}

pub fn transaction(text: &str) -> Result<Vec<Value>> {
    ensure!(text.ends_with('\n'), "truncated APT simulation output");
    let mut result = BTreeMap::<String, Value>::new();
    let mut conf = BTreeMap::<String, String>::new();
    let mut names = BTreeSet::new();
    for line in text.lines() {
        ensure!(!line.starts_with("Remv "), "APT proposed package removal");
        let Some((kind, rest)) = line.split_once(' ') else {
            continue;
        };
        if !matches!(kind, "Inst" | "Conf") {
            continue;
        }
        let selected = rest.split_whitespace().next().context("APT package name")?;
        let (name, explicit_arch) = selected
            .split_once(':')
            .map_or((selected, None), |(n, a)| (n, Some(a)));
        package_name(name)?;
        let (_, tail) = rest.split_once('(').context("APT operation version")?;
        ensure!(tail.ends_with(')'), "malformed APT operation");
        let selected_version = tail
            .split_whitespace()
            .next()
            .context("APT selected version")?;
        version(selected_version)?;
        let arch = tail
            .rsplit_once('[')
            .context("APT operation lacks architecture")?
            .1
            .strip_suffix("])")
            .context("APT architecture frame")?;
        architecture(arch)?;
        ensure!(
            explicit_arch.is_none_or(|a| a == arch || (a == "amd64" && arch == "all")),
            "APT architecture mismatch"
        );
        let key = format!("{name}:{arch}");
        if kind == "Inst" {
            ensure!(
                names.insert(name.to_string()),
                "duplicate selected package name across architectures"
            );
            ensure!(result.insert(key, json!({"name":name,"architecture":arch,"version":selected_version,"origin":"apt_transaction","operation":line})).is_none(), "duplicate APT Inst");
        } else {
            ensure!(
                conf.insert(key, selected_version.into()).is_none(),
                "duplicate APT Conf"
            );
        }
    }
    for (key, row) in &result {
        ensure!(
            conf.get(key).map(String::as_str) == row["version"].as_str(),
            "APT Inst/Conf mismatch"
        );
    }
    for key in conf.keys() {
        ensure!(result.contains_key(key), "unexpected Conf without Inst");
    }
    Ok(result.into_values().collect())
}

pub fn uris(text: &str) -> Result<Vec<Value>> {
    ensure!(text.ends_with('\n'), "truncated APT URI output");
    let mut result = Vec::new();
    let mut seen = BTreeSet::new();
    for line in text.lines().filter(|line| line.starts_with('\'')) {
        let (url, tail) = line
            .strip_prefix('\'')
            .context("URI quote")?
            .split_once('\'')
            .context("URI quote terminator")?;
        let path = url
            .strip_prefix(&format!("{SNAPSHOT}/"))
            .context("non-snapshot package URI")?;
        safe_path(path)?;
        ensure!(
            path.starts_with("pool/") && !path.contains(['%', '?', '#', '@']),
            "unsupported or unsafe package URI"
        );
        let parts: Vec<_> = tail.split_whitespace().collect();
        ensure!(parts.len() == 3, "malformed APT URI record");
        safe_path(parts[0])?;
        let size = decimal(parts[1])?;
        ensure!(seen.insert(url.to_string()), "duplicate download URI");
        result.push(json!({"url":url,"apt_local_filename":parts[0],"size":size,"apt_reported_checksum":parts[2]}));
    }
    Ok(result)
}

pub fn require_match(rows: &[Value], identity: &str) -> Result<Value> {
    let first = rows
        .first()
        .with_context(|| format!("no authenticated snapshot stanza for {identity}"))?;
    ensure!(
        rows.iter()
            .all(|r| r["stanza_sha256"] == first["stanza_sha256"]
                && r["sha256"] == first["sha256"]
                && r["filename"] == first["filename"]
                && r["size"] == first["size"]),
        "ambiguous differing snapshot stanzas for {identity}"
    );
    let mut joined = first.clone();
    joined["matching_index_refs"] = json!(
        rows.iter()
            .map(|row| row["index_ref"].clone())
            .collect::<Vec<_>>()
    );
    Ok(joined)
}

pub fn selected_identity(fields: &Fields, expected: &Value) -> bool {
    fields.get("Version").map(String::as_str) == expected["version"].as_str()
        && fields.get("Architecture").map(String::as_str) == expected["architecture"].as_str()
        && fields.get("Package").map(String::as_str) == expected["name"].as_str()
}

pub fn same_state(before: &str, after: &str) -> Result<()> {
    ensure!(
        before == after,
        "APT/dpkg state changed between solve and capture"
    );
    Ok(())
}

pub struct DependencyTerm {
    pub name: String,
    pub constraint: Option<(String, String)>,
    pub qualifier: Option<String>,
}

pub fn dependency_terms(group: &str) -> Result<Vec<DependencyTerm>> {
    let mut terms = Vec::new();
    for alternative in group.split('|') {
        let alternative = alternative.trim();
        let (name, constraint) = if let Some((name, tail)) = alternative.split_once('(') {
            let tail = tail
                .strip_suffix(')')
                .context("dependency constraint framing")?;
            let parts: Vec<_> = tail.split_whitespace().collect();
            ensure!(
                parts.len() == 2 && matches!(parts[0], "<<" | "<=" | "=" | ">=" | ">>"),
                "unsupported dependency version relation"
            );
            version(parts[1])?;
            (
                name.trim(),
                Some((parts[0].to_string(), parts[1].to_string())),
            )
        } else {
            (alternative, None)
        };
        let (base, qualifier) = if let Some((base, qualifier)) = name.split_once(':') {
            ensure!(
                matches!(qualifier, "any" | "native" | "amd64"),
                "unsupported dependency architecture qualifier"
            );
            (base, Some(qualifier.to_string()))
        } else {
            (name, None)
        };
        package_name(base)?;
        terms.push(DependencyTerm {
            name: base.to_string(),
            constraint,
            qualifier,
        });
    }
    ensure!(!terms.is_empty(), "empty dependency group");
    Ok(terms)
}

pub fn provider_set(providers: &[String]) -> Result<Vec<String>> {
    let providers: BTreeSet<_> = providers.iter().cloned().collect();
    ensure!(
        !providers.is_empty(),
        "dependency has no satisfying member of actual APT final inventory"
    );
    Ok(providers.into_iter().collect())
}

pub fn provider_version_available(constraint: bool, version: Option<&str>) -> bool {
    !constraint || version.is_some()
}

pub fn qualifier_matches(
    qualifier: Option<&str>,
    multi_arch: Option<&str>,
    architecture: &str,
) -> Result<bool> {
    ensure!(
        matches!(architecture, "amd64" | "all"),
        "unsupported selected provider architecture"
    );
    match qualifier {
        None | Some("native" | "amd64") => Ok(true),
        Some("any") => Ok(multi_arch == Some("allowed")),
        Some(_) => anyhow::bail!("unsupported dependency architecture semantics"),
    }
}

/// Accept exactly one regular ustar file with the caller-selected portable name.
pub fn single_tar(bytes: &[u8], expected: &str, limit: u64) -> Result<Vec<u8>> {
    safe_path(expected)?;
    ensure!(
        bytes.len() >= 1536 && bytes.len().is_multiple_of(512),
        "truncated Docker tar output"
    );
    let header = &bytes[..512];
    let name = std::str::from_utf8(&header[..100])?.trim_end_matches('\0');
    ensure!(
        name == expected && header[345..500].iter().all(|b| *b == 0),
        "unexpected Docker tar path/prefix"
    );
    ensure!(
        matches!(header[156], 0 | b'0') && header[157..257].iter().all(|b| *b == 0),
        "Docker tar link or special entry refused"
    );
    let octal = |field: &[u8]| -> Result<u64> {
        Ok(u64::from_str_radix(
            std::str::from_utf8(field)?.trim_matches(['\0', ' ']),
            8,
        )?)
    };
    let checksum = octal(&header[148..156])?;
    let sum: u64 = header
        .iter()
        .enumerate()
        .map(|(i, b)| {
            if (148..156).contains(&i) {
                32
            } else {
                u64::from(*b)
            }
        })
        .sum();
    ensure!(sum == checksum, "Docker tar header checksum mismatch");
    let size = octal(&header[124..136])?;
    ensure!(size <= limit, "Docker tar file exceeded bound");
    let end = 512_u64.checked_add(size).context("tar size overflow")?;
    let padded = end.checked_add(511).context("tar padding overflow")? / 512 * 512;
    let trailer = padded.checked_add(1024).context("tar trailer overflow")?;
    ensure!(
        trailer <= bytes.len() as u64,
        "truncated Docker tar payload"
    );
    ensure!(
        bytes[end as usize..].iter().all(|b| *b == 0),
        "extra Docker tar entries or nonzero padding"
    );
    Ok(bytes[512..end as usize].to_vec())
}
