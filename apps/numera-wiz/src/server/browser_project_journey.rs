//! Hosted browser Projects inputs and explicitly reviewed owner policy fixture.
#[path = "../../tests/support/project_workbench.rs"]
mod shared;
use super::tests::{Fixture, browser_click, browser_key, driver};
use base64::Engine as _;
use nw_domain::{
    access::{AccessProfile, ObjectAction, ObjectGrant, ObjectKind, ProfileInput},
    commands::{Request, Response},
};
use serde_json::{Value, json};
use sha2::Digest as _;
use shared::Driver as _;
use std::{
    path::Path,
    time::{Duration, Instant},
};

pub(super) fn project_command(command: &str) -> bool {
    shared::project_commands().contains(&command)
}
pub(super) async fn run(
    fixture: &Fixture,
    client: &reqwest::Client,
    base: &str,
    route: &str,
    directory: &Path,
) {
    let profile: AccessProfile = fixture
        .owner
        .service
        .store
        .get("profile/browser")
        .unwrap()
        .unwrap();
    assert!(
        profile.owned_object_access,
        "Fixture starts with the existing owned-source rights"
    );
    {
        let mut observed = fixture.observed_reports.lock().await;
        observed.clear();
        observed.projects = true;
    }
    let mut browser = BrowserProjects {
        fixture,
        client,
        base,
        route,
        directory,
        watermark: 0,
        original: profile,
    };
    shared::run(&mut browser, "Chrome C").await;
}
struct BrowserProjects<'a> {
    fixture: &'a Fixture,
    client: &'a reqwest::Client,
    base: &'a str,
    route: &'a str,
    directory: &'a Path,
    watermark: u64,
    original: AccessProfile,
}
impl BrowserProjects<'_> {
    fn profile(&self) -> AccessProfile {
        self.fixture
            .owner
            .service
            .store
            .get("profile/browser")
            .unwrap()
            .unwrap()
    }
    fn stable_profile(&self, current: &AccessProfile) {
        assert_eq!(current.id, self.original.id);
        assert_eq!(current.principal, self.original.principal);
        assert_eq!(current.name, self.original.name);
        assert_eq!(current.enabled, self.original.enabled);
        assert_eq!(current.client_sessions, self.original.client_sessions);
        assert_eq!(current.mode, self.original.mode);
        assert_eq!(current.read_roots, self.original.read_roots);
        assert_eq!(current.write_roots, self.original.write_roots);
    }
    async fn capture(&self, name: &str) {
        let png = driver(
            self.client,
            self.base,
            reqwest::Method::GET,
            &format!("{}/screenshot", self.route),
            Value::Null,
        )
        .await;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(png.as_str().unwrap())
            .unwrap();
        std::fs::write(
            self.directory.join(format!("browser-project-{name}.png")),
            bytes,
        )
        .unwrap();
    }
}
impl shared::Driver for BrowserProjects<'_> {
    fn browser(&self) -> bool {
        true
    }
    fn metadata(&self) -> Value {
        let wasm:Vec<_>=crate::web_assets::ASSETS.iter().filter(|asset|asset.path.ends_with(".wasm")).map(|asset|json!({"path":asset.path,"bytes":asset.bytes.len(),"sha256":hex::encode(sha2::Sha256::digest(asset.bytes))})).collect();
        assert_eq!(wasm.len(), 1, "One identified shared WASM application");
        json!({"source":crate::web_assets::UI_SOURCE,"registry":nw_domain::commands::registry_fingerprint(),"root_id":self.fixture.owner.record.root_id,"profile_id":self.original.id,"principal":self.original.principal,"viewport":[1440,861],"observer_capacity":8,"command_deadline_seconds":10,"webdriver_request_deadline_seconds":60,"wasm":wasm})
    }
    async fn mark(&mut self) {
        let mut observed = self.fixture.observed_reports.lock().await;
        observed.clear();
        self.watermark = observed.project_reads;
    }
    async fn click(&mut self, x: u32, y: u32) {
        browser_click(self.client, self.base, self.route, x, y).await;
    }
    async fn key(&mut self, key: shared::Key) {
        if matches!(key, shared::Key::CtrlEnter) {
            driver(self.client,self.base,reqwest::Method::POST,&format!("{}/actions",self.route),json!({"actions":[{"type":"key","id":"keyboard","actions":[{"type":"keyDown","value":"\u{E009}"},{"type":"keyDown","value":"\u{E007}"},{"type":"keyUp","value":"\u{E007}"},{"type":"keyUp","value":"\u{E009}"}]}]})).await;
        } else {
            let value = match key {
                shared::Key::Tab => "\u{E004}",
                shared::Key::Space => " ",
                shared::Key::Escape => "\u{E00C}",
                shared::Key::CtrlEnter => unreachable!(),
            };
            browser_key(self.client, self.base, self.route, value).await;
        }
    }
    async fn replace(&mut self, text: &str) {
        let mut actions = vec![
            json!({"type":"keyDown","value":"\u{E009}"}),
            json!({"type":"keyDown","value":"a"}),
            json!({"type":"keyUp","value":"a"}),
            json!({"type":"keyUp","value":"\u{E009}"}),
        ];
        for character in text.chars() {
            let value = if character == '\n' {
                "\u{E007}".into()
            } else {
                character.to_string()
            };
            actions.push(json!({"type":"keyDown","value":value}));
            actions.push(json!({"type":"keyUp","value":value}));
        }
        driver(
            self.client,
            self.base,
            reqwest::Method::POST,
            &format!("{}/actions", self.route),
            json!({"actions":[{"type":"key","id":"keyboard","actions":actions}]}),
        )
        .await;
    }
    async fn screenshot(&mut self, name: &str) {
        self.capture(name).await;
    }
    async fn observed(&mut self, command: &str) -> shared::Observation {
        let started = Instant::now();
        loop {
            {
                let mut observed = self.fixture.observed_reports.lock().await;
                if let Some(index) = observed
                    .iter()
                    .position(|response| response.command_id == command)
                {
                    let response = observed.remove(index).unwrap();
                    assert_ne!(response.request_id, "local");
                    assert!(!response.request_id.starts_with(shared::OBSERVER_PREFIX));
                    assert_eq!(response.status, 200);
                    assert_eq!(response.response["request_id"], response.request_id);
                    let ok = response.response["ok"] == true;
                    let project = response.response["result"]["project_id"]
                        .as_str()
                        .map(str::to_owned)
                        .or(response.project_id);
                    let observation = shared::Observation {
                        request_id: response.request_id,
                        project,
                        revision: response.response["result"]["revision"]
                            .as_str()
                            .map(str::to_owned),
                        expected_revision: response.expected_revision,
                        code: if ok {
                            "OK".into()
                        } else {
                            response.response["error"]["code"].as_str().unwrap().into()
                        },
                        receipt: response.response["receipt_id"].as_str().map(str::to_owned),
                    };
                    if !ok {
                        assert!(observation.receipt.is_none());
                    }
                    return observation;
                }
            }
            if started.elapsed() >= Duration::from_secs(10) {
                self.capture("input-timeout").await;
                panic!("Chrome Projects input did not complete {command}");
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
    async fn loaded(&mut self, reads: usize) {
        let started = Instant::now();
        loop {
            {
                let mut observed = self.fixture.observed_reports.lock().await;
                let count = observed.project_reads.checked_sub(self.watermark).unwrap();
                if count >= reads as u64 {
                    assert_eq!(
                        count, reads as u64,
                        "Unexpected extra UI projection request"
                    );
                    assert!(
                        observed
                            .iter()
                            .any(|response| response.response["ok"] == true
                                && (response.response["result"]["entity"]["kind"]
                                    == "source_binding"
                                    || response.response["result"]["projection"] == "summary"
                                    || response.response["result"]["items"]
                                        .as_array()
                                        .is_some_and(Vec::is_empty))),
                        "Captured load has no terminal canonical projection"
                    );
                    observed.clear();
                    return;
                }
            }
            if started.elapsed() >= Duration::from_secs(10) {
                self.capture("load-timeout").await;
                panic!("Chrome Projects load did not finish {reads} canonical reads");
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
    async fn no_cleanup_submission(&mut self) {
        assert!(
            !self
                .fixture
                .observed_reports
                .lock()
                .await
                .iter()
                .any(|response| matches!(
                    response.command_id.as_str(),
                    "jobs.submit" | "batches.submit"
                )),
            "Projects input dispatched a hidden Cleanup submission"
        );
    }
    async fn request(&mut self, request: Request, external: bool) -> Response {
        let context = if external {
            self.fixture.owner.service.owner()
        } else {
            self.fixture
                .owner
                .service
                .profile_context("browser")
                .unwrap()
        };
        let id = request.request_id.clone();
        let response = self.fixture.owner.service.dispatch(context, request).await;
        assert_eq!(response.request_id, id);
        response
    }
    async fn policy(&mut self, phase: shared::Policy, project: Option<&str>) -> Value {
        let before = self.profile();
        self.stable_profile(&before);
        let mut intended = before.clone();
        let label = match phase {
            shared::Policy::OlderCommands => {
                for command in shared::project_commands() {
                    assert!(intended.allowed_command_ids.remove(*command));
                }
                "explicit older-allowlist fixture"
            }
            shared::Policy::UpgradeCommands => {
                intended.allowed_command_ids = self.original.allowed_command_ids.clone();
                "explicit owner-reviewed command upgrade"
            }
            shared::Policy::ReadOnlyProject | shared::Policy::EditProject => {
                intended.owned_object_access = false;
                let project = project.unwrap();
                intended
                    .object_grants
                    .retain(|grant| !(grant.kind == ObjectKind::Project && grant.id == project));
                let mut actions = std::collections::BTreeSet::from([ObjectAction::Read]);
                if matches!(phase, shared::Policy::EditProject) {
                    actions.insert(ObjectAction::Edit);
                }
                intended.object_grants.push(ObjectGrant {
                    kind: ObjectKind::Project,
                    id: project.into(),
                    actions,
                });
                if matches!(phase, shared::Policy::ReadOnlyProject) {
                    "explicit project Read with global implicit-owned access disabled"
                } else {
                    "explicit project Read+Edit; source access still denied"
                }
            }
            shared::Policy::RestoreSources => {
                intended.owned_object_access = true;
                "explicit restoration of owned source access"
            }
        };
        let unaffected = |profile: &AccessProfile| -> Value {
            json!(
                profile
                    .object_grants
                    .iter()
                    .filter(|grant| !(grant.kind == ObjectKind::Project
                        && Some(grant.id.as_str()) == project))
                    .collect::<Vec<_>>()
            )
        };
        assert_eq!(unaffected(&before), unaffected(&intended));
        let scope = self
            .request(
                shared::request("access.replay_scope.create", json!({}), None),
                true,
            )
            .await;
        assert!(scope.ok);
        let mut update = shared::request(
            "access.profiles.update",
            json!(ProfileInput {
                name: intended.name.clone(),
                expected_revision: Some(before.revision),
                enabled: intended.enabled,
                client_sessions: intended.client_sessions,
                mode: intended.mode,
                owned_object_access: intended.owned_object_access,
                allowed_command_ids: intended.allowed_command_ids.clone(),
                object_grants: intended.object_grants.clone(),
                read_roots: intended.read_roots.clone(),
                write_roots: intended.write_roots.clone(),
            }),
            None,
        );
        update.replay_scope = Some(
            scope.result.unwrap()["replay_scope"]
                .as_str()
                .unwrap()
                .into(),
        );
        update.idempotency_key = Some(update.request_id.clone());
        let result = self.request(update, true).await;
        assert!(
            result.ok,
            "Owner-reviewed profile update failed: {:?}",
            result.error
        );
        let after = self.profile();
        self.stable_profile(&after);
        assert_eq!(after.revision, before.revision + 1);
        assert_eq!(after.allowed_command_ids, intended.allowed_command_ids);
        assert_eq!(after.owned_object_access, intended.owned_object_access);
        assert_eq!(json!(after.object_grants), json!(intended.object_grants));
        assert_eq!(unaffected(&before), unaffected(&after));
        json!({"fixture_policy_phase":label,"profile_id":after.id,"principal":after.principal,"profile_revision":after.revision.to_string(),"receipt":result.receipt_id,"owned_object_access":after.owned_object_access,"roots_and_unrelated_explicit_grants_preserved":true,"qualification":"The owned flag is global; no per-asset grant or foreign-creator coverage is claimed"})
    }
    async fn reopen(&mut self) -> Value {
        let before = self.profile();
        self.stable_profile(&before);
        assert!(before.owned_object_access);
        driver(
            self.client,
            self.base,
            reqwest::Method::POST,
            &format!("{}/url", self.route),
            json!({"url":"about:blank"}),
        )
        .await;
        let started = Instant::now();
        while !self.fixture.owner.leases.lock().await.is_empty() {
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "Projects page close did not release its lease"
            );
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        self.mark().await;
        driver(
            self.client,
            self.base,
            reqwest::Method::POST,
            &format!("{}/url", self.route),
            json!({"url":format!("{}/",self.fixture.endpoint)}),
        )
        .await;
        let element = driver(
            self.client,
            self.base,
            reqwest::Method::POST,
            &format!("{}/element", self.route),
            json!({"using":"css selector","value":"#boot-status"}),
        )
        .await;
        let element = element["element-6066-11e4-a52e-4f735466cecf"]
            .as_str()
            .unwrap();
        let started = Instant::now();
        loop {
            let hidden = driver(
                self.client,
                self.base,
                reqwest::Method::GET,
                &format!("{}/element/{element}/property/hidden", self.route),
                Value::Null,
            )
            .await;
            if hidden == true {
                break;
            }
            assert!(
                started.elapsed() < Duration::from_secs(30),
                "Same-profile page reopen bootstrap deadline; inspect retained image before any re-pairing change"
            );
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        assert_eq!(self.observed("jobs.list").await.code, "OK");
        let location = driver(
            self.client,
            self.base,
            reqwest::Method::GET,
            &format!("{}/url", self.route),
            Value::Null,
        )
        .await;
        assert!(!location.as_str().unwrap().contains('#'));
        let after = self.profile();
        self.stable_profile(&after);
        assert_eq!(json!(before), json!(after));
        json!({"reopen":"actual Chrome page in the same tab/session","root_id":self.fixture.owner.record.root_id,"profile_id":after.id,"principal":after.principal,"registry":nw_domain::commands::registry_fingerprint(),"source":crate::web_assets::UI_SOURCE,"re_pairing":false,"unknown_pending_reload_claimed":false})
    }
    fn write(&self, name: &str, value: &Value) {
        std::fs::write(
            self.directory.join(format!("browser-project-{name}.json")),
            serde_json::to_vec_pretty(value).unwrap(),
        )
        .unwrap();
    }
}
