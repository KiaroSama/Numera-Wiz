//! The restoration delta and irreversible B fact share one existing-file transaction.
use crate::{
    RECORDS, Receipt, decode, encode,
    media_generations::{Enablement, guard},
    records::record_quota,
    storage,
};
use nw_domain::{
    DomainError, Result,
    maintenance::{media_lifecycle::*, promotion::MEDIA_V1_CAPABILITIES},
};
use redb::{ReadableDatabase, ReadableTable};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[cfg(test)]
thread_local! {
    pub(crate) static ABORT_INSTALL: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    pub(crate) static UNCERTAIN_INSTALL: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ExposureBarrier {
    pub schema_version: u32,
    pub restoration_id: String,
    pub enablement_id: String,
    pub root_id: String,
    pub source_object: nw_domain::records::NativeObjectIdentity,
    pub source_operation_id: String,
    pub source_activation_id: String,
    pub source_revision: String,
    pub source_preimage_sha256: String,
    pub promoted_archive_sha256: String,
    pub baseline_sha256: String,
    pub accepted_receipt_sha256: String,
    pub history_sha256: String,
    pub prefix_sha256: String,
    pub input_delta_sha256: String,
    pub predecessor_restoration: Option<String>,
}
fn invalid(message: impl Into<String>) -> DomainError {
    DomainError::new("RESTORATION_REFUSED", message)
}
fn bytes(
    table: &impl ReadableTable<&'static str, &'static [u8]>,
    key: &str,
) -> Result<Option<Vec<u8>>> {
    table
        .get(key)
        .map_err(storage)?
        .map(|row| {
            if row.value().len() > crate::MAX_RECORD {
                return Err(invalid("Restoration row exceeds8MiB"));
            }
            Ok(row.value().to_vec())
        })
        .transpose()
}
fn equal(
    table: &impl ReadableTable<&'static str, &'static [u8]>,
    key: &str,
    expected: &[u8],
) -> Result<()> {
    if bytes(table, key)?.as_deref() != Some(expected) {
        return Err(invalid(format!(
            "Retained raw row continuity failed for{}",
            key.split('/').next().unwrap_or("record")
        )));
    }
    Ok(())
}
fn clock(table: &impl ReadableTable<&'static str, &'static [u8]>) -> Result<u64> {
    let value = bytes(table, "meta/clock")?.ok_or_else(|| invalid("Durable clock is missing"))?;
    nw_domain::media_execution::exact(std::str::from_utf8(&value).map_err(storage)?)
}
fn rate(previous: Option<&[u8]>, next: &[u8]) -> Result<()> {
    let (hour, count): (u64, u32) = decode(next)?;
    if count > 32 {
        return Err(invalid("Scope rate exceeds its frozen allowance"));
    }
    if let Some(previous) = previous {
        let (old_hour, old_count): (u64, u32) = decode(previous)?;
        if old_count > 32 || hour < old_hour || (hour == old_hour && count < old_count) {
            return Err(invalid("Scope rate continuity regressed"));
        }
    }
    Ok(())
}
pub(crate) fn receipt(
    table: &impl ReadableTable<&'static str, &'static [u8]>,
    key: &str,
    digest: &str,
    result_id: &str,
    field: &str,
) -> Result<Vec<u8>> {
    if !key.starts_with("receipt/") || key.len() != 72 {
        return Err(invalid("Accepted receipt namespace is invalid"));
    }
    let raw = bytes(table, key)?.ok_or_else(|| invalid("Genuine accepted receipt is missing"))?;
    let value: Receipt = decode(&raw)?;
    if value.digest != digest
        || !value.response.ok
        || value.response.error.is_some()
        || value
            .response
            .receipt_id
            .as_ref()
            .is_none_or(|id| uuid::Uuid::parse_str(id).is_err())
        || value
            .response
            .result
            .as_ref()
            .and_then(|result| result.get(field))
            .and_then(serde_json::Value::as_str)
            != Some(result_id)
    {
        return Err(invalid("Accepted receipt does not prove this operation"));
    }
    Ok(raw)
}
pub(crate) fn validate_prestate(
    baseline: &impl ReadableTable<&'static str, &'static [u8]>,
    source: &impl ReadableTable<&'static str, &'static [u8]>,
    promoted: &impl ReadableTable<&'static str, &'static [u8]>,
    operation: &Enablement,
    restoration: Option<&GenerationRestorationRecord>,
) -> Result<()> {
    let original = guard(baseline)?;
    if original.migration_id != operation.source_operation_id
        || original.activation_id != operation.source_activation_id
        || original.revision != operation.source_revision
    {
        return Err(invalid("Original source guard provenance differs"));
    }
    equal(
        source,
        "meta/rollback-guard",
        &bytes(baseline, "meta/rollback-guard")?
            .ok_or_else(|| invalid("Original guard missing"))?,
    )?;
    let target = guard(promoted)?;
    if target.migration_id != operation.id
        || target.activation_id != operation.activation_id
        || target.validate()? != 0
    {
        return Err(invalid(
            "Promoted generation has business adoption or another guard",
        ));
    }
    equal(
        source,
        "meta/required-capabilities",
        &encode(&MEDIA_V1_CAPABILITIES)?,
    )?;
    equal(
        promoted,
        "meta/required-capabilities",
        &encode(&MEDIA_V1_CAPABILITIES)?,
    )?;
    equal(source, operation.pending_key(), operation.id.as_bytes())?;
    if bytes(promoted, operation.pending_key())?.is_some() {
        return Err(invalid("Enablement must finish before restoration"));
    }
    if clock(source)? < clock(baseline)? || clock(promoted)? < clock(source)? {
        return Err(invalid("Durable clock continuity regressed"));
    }
    for row in baseline.iter().map_err(storage)? {
        let (key, value) = row.map_err(storage)?;
        let key = key.value();
        if [
            "meta/required-capabilities",
            "meta/clock",
            "meta/rollback-guard",
        ]
        .contains(&key)
        {
            continue;
        }
        if key.starts_with("scope-rate/") {
            let source_rate =
                bytes(source, key)?.ok_or_else(|| invalid("Source lost a prior scope rate"))?;
            let promoted_rate = bytes(promoted, key)?
                .ok_or_else(|| invalid("Promoted state lost a prior scope rate"))?;
            rate(Some(value.value()), &source_rate)?;
            rate(Some(&source_rate), &promoted_rate)?;
        } else {
            equal(source, key, value.value())?;
            equal(promoted, key, value.value())?;
        }
    }
    let operation_key = operation.record_key();
    let transformed_key = format!("generation-transform/{}", operation.id);
    let restoration_key = restoration.map(|record| format!("generation-restoration/{}", record.id));
    let promotion_field = if operation.initialization {
        "initialization_id"
    } else {
        "promotion_id"
    };
    let accepted = receipt(
        source,
        &operation.receipt_key,
        &operation.request_digest,
        &operation.id,
        promotion_field,
    )?;
    equal(promoted, &operation.receipt_key, &accepted)?;
    for row in source.iter().map_err(storage)? {
        let (key, value) = row.map_err(storage)?;
        let key = key.value();
        if baseline.get(key).map_err(storage)?.is_some() {
            continue;
        }
        if key == operation.pending_key() || key == operation.receipt_key {
            continue;
        }
        if key == operation_key {
            let mut target: serde_json::Value = decode(
                &bytes(promoted, key)?
                    .ok_or_else(|| invalid("Promoted acceptance journal missing"))?,
            )?;
            target["phase"] = serde_json::json!("accepted");
            target["completed_at"] = serde_json::Value::Null;
            let original: serde_json::Value = decode(value.value())?;
            if target != original {
                return Err(invalid(
                    "Accepted source journal and promoted lineage differ",
                ));
            }
        } else if key.starts_with("scope-rate/") {
            rate(None, value.value())?;
            rate(
                Some(value.value()),
                &bytes(promoted, key)?.ok_or_else(|| invalid("Promoted rate missing"))?,
            )?;
        } else {
            return Err(invalid("Unexplained source business or control addition"));
        }
    }
    for row in promoted.iter().map_err(storage)? {
        let (key, value) = row.map_err(storage)?;
        let key = key.value();
        if source.get(key).map_err(storage)?.is_some() {
            continue;
        }
        if key == "meta/pending-restoration" && restoration.is_some() {
            if value.value() != restoration.expect("present restoration").id.as_bytes() {
                return Err(invalid("Pending restoration identity differs"));
            }
        } else if key == transformed_key {
            let mut expected = operation.clone();
            expected.phase = EnablementPhase::Accepted;
            if value.value() != encode(&expected)? {
                return Err(invalid("Target transformation evidence differs"));
            }
        } else if restoration_key.as_deref() == Some(key) {
            if value.value() != encode(restoration.expect("matched restoration key"))? {
                return Err(invalid("Restoration acceptance journal differs"));
            }
        } else if restoration.is_some_and(|record| key == record.receipt_key) {
            let record = restoration.expect("matched restoration receipt");
            receipt(
                promoted,
                key,
                &record.request_digest,
                &record.id,
                "restoration_id",
            )?;
        } else if key.starts_with("scope-rate/") {
            rate(None, value.value())?;
        } else {
            return Err(invalid(
                "Unexplained promoted business/control addition blocks restoration",
            ));
        }
    }
    Ok(())
}
pub(crate) fn delta_digest(
    source: &impl ReadableTable<&'static str, &'static [u8]>,
    promoted: &impl ReadableTable<&'static str, &'static [u8]>,
    baseline: &impl ReadableTable<&'static str, &'static [u8]>,
    record: &GenerationRestorationRecord,
) -> Result<String> {
    let mut hash = Sha256::new();
    hash.update(b"NumeraWiz.RestorationControlDelta.v1\0");
    for row in promoted.iter().map_err(storage)? {
        let (key, value) = row.map_err(storage)?;
        let key = key.value();
        if key.starts_with("receipt/") || key.starts_with("scope-rate/") || key == "meta/clock" {
            if let Some(old) = source.get(key).map_err(storage)?
                && key.starts_with("receipt/")
                && old.value() != value.value()
            {
                return Err(invalid("Unequal receipt collision is fatal"));
            }
            hash.update((key.len() as u64).to_le_bytes());
            hash.update(key.as_bytes());
            hash.update((value.value().len() as u64).to_le_bytes());
            hash.update(value.value());
        }
    }
    hash.update(
        bytes(baseline, "meta/required-capabilities")?
            .ok_or_else(|| invalid("Original capability bytes missing"))?,
    );
    hash.update(encode(record)?);
    Ok(hex::encode(hash.finalize()))
}
pub(crate) fn install(
    database: &redb::Database,
    baseline: &impl ReadableTable<&'static str, &'static [u8]>,
    promoted: &impl ReadableTable<&'static str, &'static [u8]>,
    operation: &Enablement,
    record: &GenerationRestorationRecord,
    barrier: &ExposureBarrier,
) -> Result<()> {
    let mut write = database.begin_write().map_err(storage)?;
    write
        .set_durability(redb::Durability::Immediate)
        .map_err(storage)?;
    write.set_two_phase_commit(true);
    {
        let mut source = write.open_table(RECORDS).map_err(storage)?;
        let barrier_key = format!("restoration-control/{}", record.id);
        if source.get(barrier_key.as_str()).map_err(storage)?.is_some() {
            return Err(invalid(
                "B already exists: overlay reapplication is forbidden forever",
            ));
        }
        validate_prestate(baseline, &source, promoted, operation, Some(record))?;
        if delta_digest(&source, promoted, baseline, record)? != barrier.input_delta_sha256 {
            return Err(invalid("Reviewed complete control delta differs"));
        }
        let mut namespaces = std::collections::BTreeSet::new();
        for row in source.iter().map_err(storage)? {
            let (key, _) = row.map_err(storage)?;
            namespaces.insert(key.value().split('/').next().unwrap_or("").to_owned());
        }
        namespaces.insert("restoration-control".into());
        namespaces.insert("generation-restoration".into());
        if namespaces.len() > 128 {
            return Err(invalid("Restoration would exceed128 namespaces"));
        }
        for row in promoted.iter().map_err(storage)? {
            let (key, value) = row.map_err(storage)?;
            let key = key.value();
            if key.starts_with("receipt/") {
                let old = bytes(&source, key)?;
                if let Some(old) = old {
                    if old != value.value() {
                        return Err(invalid("Unequal receipt collision is fatal"));
                    }
                    continue;
                }
                record_quota(&source, key)?;
                source.insert(key, value.value()).map_err(storage)?;
            } else if key == "meta/clock" || key.starts_with("scope-rate/") {
                record_quota(&source, key)?;
                source.insert(key, value.value()).map_err(storage)?;
            }
        }
        let capabilities = bytes(baseline, "meta/required-capabilities")?
            .ok_or_else(|| invalid("Original capability bytes missing"))?;
        source
            .insert("meta/required-capabilities", capabilities.as_slice())
            .map_err(storage)?;
        source.remove(operation.pending_key()).map_err(storage)?;
        let journal_key = format!("generation-restoration/{}", record.id);
        record_quota(&source, &journal_key)?;
        source
            .insert(journal_key.as_str(), encode(record)?.as_slice())
            .map_err(storage)?;
        #[cfg(test)]
        if ABORT_INSTALL.with(|remaining| {
            let count = remaining.get();
            remaining.set(count.saturating_sub(1));
            count == 1
        }) {
            return Err(invalid("Injected abort before the atomic exposure barrier"));
        }
        let encoded = encode(barrier)?;
        if encoded.len() > 65536 {
            return Err(invalid("Exposure barrier exceeds64KiB"));
        }
        record_quota(&source, &barrier_key)?;
        source
            .insert(barrier_key.as_str(), encoded.as_slice())
            .map_err(storage)?;
    }
    write.commit().map_err(storage)?;
    #[cfg(test)]
    if UNCERTAIN_INSTALL.with(|remaining| {
        let count = remaining.get();
        remaining.set(count.saturating_sub(1));
        count == 1
    }) {
        return Err(invalid(
            "Injected uncertain return after the complete durable B commit",
        ));
    }
    Ok(())
}
pub(crate) fn has_barrier(
    database: &redb::ReadOnlyDatabase,
    id: &str,
) -> Result<Option<ExposureBarrier>> {
    let read = database.begin_read().map_err(storage)?;
    let table = read.open_table(RECORDS).map_err(storage)?;
    bytes(&table, &format!("restoration-control/{id}"))?
        .map(|raw| decode(&raw))
        .transpose()
}
pub(crate) fn validate_continuity(
    preimage: &impl ReadableTable<&'static str, &'static [u8]>,
    current: &impl ReadableTable<&'static str, &'static [u8]>,
    promoted: &impl ReadableTable<&'static str, &'static [u8]>,
    baseline: &impl ReadableTable<&'static str, &'static [u8]>,
    operation: &Enablement,
    record: &GenerationRestorationRecord,
    barrier: &ExposureBarrier,
) -> Result<()> {
    let current_guard = guard(current)?;
    let original_revision = nw_domain::media_execution::exact(&operation.source_revision)?;
    if current_guard.migration_id != operation.source_operation_id
        || current_guard.activation_id != operation.source_activation_id
        || current_guard.validate()? < original_revision
    {
        return Err(invalid(
            "Post-exposure guard provenance or revision regressed",
        ));
    }
    let adopted = current_guard.validate()? > original_revision;
    let barrier_key = format!("restoration-control/{}", record.id);
    let journal_key = format!("generation-restoration/{}", record.id);
    equal(current, &barrier_key, &encode(barrier)?)?;
    equal(current, &journal_key, &encode(record)?)?;
    equal(
        current,
        "meta/required-capabilities",
        &bytes(baseline, "meta/required-capabilities")?
            .ok_or_else(|| invalid("Original capability bytes missing"))?,
    )?;
    if bytes(current, operation.pending_key())?.is_some() || clock(current)? < clock(promoted)? {
        return Err(invalid(
            "Armed source pending state or clock continuity differs",
        ));
    }
    for row in promoted.iter().map_err(storage)? {
        let (key, value) = row.map_err(storage)?;
        if key.value().starts_with("receipt/") {
            equal(current, key.value(), value.value())?;
        } else if key.value().starts_with("scope-rate/") {
            rate(
                Some(value.value()),
                &bytes(current, key.value())?
                    .ok_or_else(|| invalid("Carried scope rate disappeared"))?,
            )?;
        }
    }
    for row in preimage.iter().map_err(storage)? {
        let (key, old) = row.map_err(storage)?;
        let key = key.value();
        if key == operation.pending_key()
            || [
                "meta/clock",
                "meta/required-capabilities",
                "meta/rollback-guard",
            ]
            .contains(&key)
            || key.starts_with("scope-rate/")
        {
            continue;
        }
        match bytes(current, key)? {
            Some(actual) if actual == old.value() => {}
            Some(actual) if adopted || key.starts_with("credential/") => {
                crate::restoration_legacy::validate_transition(key, old.value(), &actual)?;
                validate_changed_business(current, key, &actual)?;
            }
            None if adopted || key.starts_with("credential/") => {
                crate::restoration_legacy::validate_removed(preimage, current, key, old.value())?;
            }
            _ => {
                return Err(invalid(
                    "Post-exposure source lost or changed a protected raw row",
                ));
            }
        }
    }
    for row in current.iter().map_err(storage)? {
        let (key, value) = row.map_err(storage)?;
        let key = key.value();
        if preimage.get(key).map_err(storage)?.is_some() || key == barrier_key || key == journal_key
        {
            continue;
        }
        if key.starts_with("receipt/") {
            if promoted.get(key).map_err(storage)?.is_none() {
                if !adopted {
                    return Err(invalid(
                        "Unexplained post-exposure receipt without business adoption",
                    ));
                }
                let receipt: Receipt = decode(value.value())?;
                nw_domain::media_execution::hash(&receipt.digest)?;
                if receipt
                    .response
                    .receipt_id
                    .as_ref()
                    .is_none_or(|id| uuid::Uuid::parse_str(id).is_err())
                {
                    return Err(invalid("Post-exposure receipt identity is invalid"));
                }
            }
        } else if key.starts_with("scope-rate/") {
            rate(None, value.value())?;
        } else if adopted || key.starts_with("credential/") {
            validate_changed_business(current, key, value.value())?;
        } else {
            return Err(invalid(
                "Unexplained post-exposure row with unchanged source guard",
            ));
        }
    }
    Ok(())
}
fn validate_changed_business(
    table: &impl ReadableTable<&'static str, &'static [u8]>,
    key: &str,
    raw: &[u8],
) -> Result<()> {
    let namespace = key.split('/').next().unwrap_or("");
    match namespace {
        "profile" => {
            let profile: nw_domain::access::AccessProfile = decode(raw)?;
            if profile.schema_version != 2
                || profile.revision == 0
                || profile.principal.is_empty()
                || key != format!("profile/{}", profile.id)
                || profile.allowed_command_ids.len() > 1024
                || profile.object_grants.len() > 256
                || profile.read_roots.len() > 32
                || profile.write_roots.len() > 32
            {
                return Err(invalid("Changed access profile fails its typed bounds"));
            }
        }
        "plan" => {
            let plan = nw_domain::plans::decode(decode(raw)?)?;
            if key != format!("plan/{}", plan.id) {
                return Err(invalid("Changed legacy plan key differs"));
            }
        }
        "job" => {
            let job = crate::records::decode_job(raw)?;
            if key != format!("job/{}", job.id)
                || (job.state == nw_domain::records::JobState::Succeeded
                    && (job.artifact.is_none() || job.verification.is_none()))
            {
                return Err(invalid(
                    "Changed legacy job fails its typed identity/outcome invariants",
                ));
            }
        }
        "asset" => {
            let asset: nw_domain::records::Asset = decode(raw)?;
            if key != format!("asset/{}", asset.id) || asset.creator.is_empty() {
                return Err(invalid("Changed asset identity differs"));
            }
            nw_domain::media_execution::hash(&asset.identity.sha256)?;
            nw_domain::media_execution::exact(&asset.identity.bytes)?;
        }
        "job-auth" => {
            let authorization: nw_domain::access::ExecutionAuthorization = decode(raw)?;
            if authorization.principal.is_empty() {
                return Err(invalid("Changed job authorization is empty"));
            }
        }
        "cancel" => {
            if !decode::<bool>(raw)? {
                return Err(invalid("Cancellation fact cannot be false"));
            }
        }
        "publication" => {
            let publication: nw_domain::access::Publication = decode(raw)?;
            if key != format!("publication/{}", publication.job_id)
                || publication.identity.path != publication.output
            {
                return Err(invalid("Changed publication identity differs"));
            }
            nw_domain::media_execution::hash(&publication.identity.sha256)?;
            nw_domain::media_execution::exact(&publication.identity.bytes)?;
        }
        "project" => crate::projects::validate_restored_project(table, key, raw)?,
        _ => crate::restoration_legacy::validate(table, key, raw)?,
    }
    Ok(())
}
