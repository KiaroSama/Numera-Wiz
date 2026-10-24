//! Atomic, fail-closed upgrade of the original development access-profile encoding.
use super::*;
use nw_domain::access::{AccessProfile, ObjectAction, ObjectGrant, ObjectKind, ProfileMode};
use std::collections::BTreeSet;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyProfile {
    id: String, name: String, principal: String, revision: u64, enabled: bool,
    #[serde(default)] client_sessions: bool,
    commands: BTreeSet<String>, objects: BTreeSet<String>, read_roots: Vec<String>, write_roots: Vec<String>,
}

impl Store {
    pub(super) fn upgrade_access_profiles(&self) -> Result<()> {
        let write = self.db.begin_write().map_err(storage)?;
        {
            let mut table = write.open_table(RECORDS).map_err(storage)?;
            let mut keys = Vec::new();
            for row in table.range("profile/"..).map_err(storage)? {
                let (key, _) = row.map_err(storage)?;
                if !key.value().starts_with("profile/") { break; }
                if keys.len() >= 128 { return Err(DomainError::new("RESOURCE_LIMIT", "Profile migration exceeds the retained profile quota")); }
                keys.push(key.value().to_owned());
            }
            for key in keys {
                let bytes = table.get(key.as_str()).map_err(storage)?.ok_or_else(|| storage("Migration profile disappeared"))?.value().to_vec();
                let shape: serde_json::Value = decode(&bytes)?;
                if shape.get("schema_version").is_some() {
                    let current: AccessProfile = decode(&bytes)?;
                    if current.schema_version != 2 { return Err(DomainError::new("STORE_SCHEMA_UNSUPPORTED", "Access profile version is not supported; no profile was migrated")); }
                    continue;
                }
                let old: LegacyProfile = decode(&bytes)?;
                let mut grants = Vec::new();
                for id in old.objects {
                    let mut found = false;
                    for (prefix, kind, action) in [("plan", ObjectKind::Plan, ObjectAction::Submit), ("job", ObjectKind::Job, ObjectAction::Control)] {
                        if table.get(format!("{prefix}/{id}").as_str()).map_err(storage)?.is_some() {
                            grants.push(ObjectGrant { kind, id: id.clone(), actions: BTreeSet::from([ObjectAction::Read, action]) });
                            found = true;
                        }
                    }
                    if !found || table.get(format!("asset/{id}").as_str()).map_err(storage)?.is_some() {
                        return Err(DomainError::new("STORE_SCHEMA_UNSUPPORTED", "Legacy object grant cannot be represented exactly by profile v2; retain the prior application and explicitly revise that policy before upgrading. No profile was migrated"));
                    }
                }
                let discovery = old.id == "agent";
                let current = AccessProfile {
                    schema_version: 2, id: old.id, name: old.name, principal: old.principal, revision: old.revision,
                    enabled: old.enabled, client_sessions: old.client_sessions, mode: if discovery { ProfileMode::Discovery } else { ProfileMode::Operate },
                    owned_object_access: !discovery, allowed_command_ids: old.commands, object_grants: grants,
                    read_roots: old.read_roots, write_roots: old.write_roots,
                };
                table.insert(format!("migration/access-v1/{key}").as_str(), bytes.as_slice()).map_err(storage)?;
                table.insert(key.as_str(), encode(&current)?.as_slice()).map_err(storage)?;
            }
        }
        write.commit().map_err(storage)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn legacy(name: &str, objects: serde_json::Value) -> serde_json::Value {
        serde_json::json!({"id":name,"name":name,"principal":"stable-principal","revision":7,"enabled":true,"client_sessions":false,"commands":["jobs.inspect"],"objects":objects,"read_roots":[],"write_roots":[]})
    }
    #[test]
    fn migration_preserves_principal_explicit_commands_and_old_policy_bytes() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let old = legacy("operator", serde_json::json!(["job-one"]));
        store.put("profile/operator", &old).unwrap();
        store.put("job/job-one", &serde_json::json!({"id":"job-one"})).unwrap();
        drop(store);
        let store = Store::open(root.path()).unwrap();
        let current: AccessProfile = store.get("profile/operator").unwrap().unwrap();
        assert_eq!(current.schema_version, 2);
        assert_eq!(current.principal, "stable-principal");
        assert_eq!(current.revision, 7);
        assert!(current.owned_object_access);
        assert_eq!(current.allowed_command_ids, BTreeSet::from(["jobs.inspect".to_owned()]));
        assert!(current.allows_object(ObjectKind::Job, "job-one", "other", ObjectAction::Control));
        assert!(!current.allows_command("jobs.cancel"));
        assert_eq!(store.get::<serde_json::Value>("migration/access-v1/profile/operator").unwrap().unwrap(), old);
    }
    #[test]
    fn one_nonrepresentable_legacy_grant_rolls_back_the_entire_profile_migration() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let a = legacy("a", serde_json::json!([]));
        let b = legacy("b", serde_json::json!(["foreign-asset"]));
        store.put("profile/a", &a).unwrap();
        store.put("profile/b", &b).unwrap();
        store.put("asset/foreign-asset", &serde_json::json!({"id":"foreign-asset"})).unwrap();
        assert_eq!(store.upgrade_access_profiles().unwrap_err().code, "STORE_SCHEMA_UNSUPPORTED");
        assert_eq!(store.get::<serde_json::Value>("profile/a").unwrap().unwrap(), a);
        assert_eq!(store.get::<serde_json::Value>("profile/b").unwrap().unwrap(), b);
        assert!(store.get::<serde_json::Value>("migration/access-v1/profile/a").unwrap().is_none());
    }
}
