//! Published bytes use a current job-read purpose, independently of output roots.
use super::*;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use nw_domain::artifacts::{ARTIFACT_CHUNK_BYTES, ArtifactChunk, ArtifactManifest, ChunkInput};
use nw_platform::artifacts::ArtifactReader;
use tokio::sync::OwnedSemaphorePermit;

pub struct ArtifactTransfer {
    service: Arc<Service>,
    context: Context,
    artifact_id: String,
    command: String,
    publication: Publication,
    reader: ArtifactReader,
    _capacity: OwnedSemaphorePermit,
    _request: OwnedSemaphorePermit,
}
impl ArtifactTransfer {
    pub fn bytes(&self) -> u64 {
        self.reader.bytes()
    }
    pub fn sha256(&self) -> &str {
        &self.publication.identity.sha256
    }
    pub fn ensure_current(&self) -> Result<()> {
        let _guard = self.service.mutation.lock().map_err(|_| lock_error())?;
        let (_, publication) =
            self.service
                .artifact_authority(&self.context, &self.artifact_id, &self.command)?;
        if publication.identity != self.publication.identity
            || publication.proof != self.publication.proof
        {
            return Err(DomainError::new(
                "ARTIFACT_CHANGED",
                "Stored publication identity changed",
            ));
        }
        self.reader.check()
    }
    pub fn read_range(&mut self, offset: u64, length: usize) -> Result<Vec<u8>> {
        self.ensure_current()?;
        let bytes = self.reader.read_range(offset, length)?;
        self.ensure_current()?;
        Ok(bytes)
    }
}
impl Service {
    fn artifact_authority(
        &self,
        context: &Context,
        artifact_id: &str,
        command: &str,
    ) -> Result<(Job, Publication)> {
        let policy = self.policy(context)?;
        if policy.as_ref().is_some_and(|p| !p.allows_command(command)) {
            return Err(DomainError::new(
                "PERMISSION_DENIED",
                "Artifact command is outside the current profile allowlist",
            ));
        }
        let id = artifact_id
            .strip_prefix("job-")
            .ok_or_else(DomainError::object_not_found)?;
        let key = named("job", id).map_err(|_| DomainError::object_not_found())?;
        let job: Job = self
            .store
            .get(&key)?
            .ok_or_else(DomainError::object_not_found)?;
        self.object_action(
            context,
            ObjectKind::Job,
            &job.id,
            &job.creator,
            ObjectAction::Read,
        )?;
        if job.state != JobState::Succeeded {
            return Err(DomainError::new(
                "ARTIFACT_UNAVAILABLE",
                "This job has no successful published artifact",
            ));
        }
        let publication: Publication =
            self.store.get(&named("publication", id)?)?.ok_or_else(|| {
                DomainError::new(
                    "ARTIFACT_UNAVAILABLE",
                    "Retained physical publication proof is unavailable",
                )
            })?;
        if publication.read_purpose_version != 1 {
            return Err(DomainError::new(
                "ARTIFACT_UNAVAILABLE",
                "A supported retained publication byte-read purpose is unavailable",
            ));
        }
        let intent = job.owned_staging.as_ref().ok_or_else(|| {
            DomainError::new(
                "ARTIFACT_UNAVAILABLE",
                "Retained owned workspace identity is unavailable",
            )
        })?;
        let proof = Path::new(&publication.proof);
        let valid_proof_name = proof
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(|n| n.strip_prefix(".numera-proof-"))
            .is_some_and(|n| Uuid::parse_str(n).is_ok());
        if !publication.published
            || publication.job_id != job.id
            || intent.job_id != job.id
            || job.artifact.as_ref() != Some(&publication.identity)
            || publication.output != publication.identity.path
            || publication.output != job.plan.output
            || proof.parent() != Some(Path::new(&intent.directory))
            || !valid_proof_name
        {
            return Err(DomainError::new(
                "ARTIFACT_CHANGED",
                "Retained artifact purpose does not match its owning publication",
            ));
        }
        Ok((job, publication))
    }
    pub fn begin_artifact_transfer(
        self: &Arc<Self>,
        context: Context,
        artifact_id: &str,
        expected_sha256: Option<&str>,
        cancel: CancellationToken,
    ) -> Result<ArtifactTransfer> {
        self.open_artifact_transfer(
            context,
            artifact_id,
            expected_sha256,
            cancel,
            "artifacts.read_chunk",
        )
    }
    fn open_artifact_transfer(
        self: &Arc<Self>,
        context: Context,
        artifact_id: &str,
        expected_sha256: Option<&str>,
        cancel: CancellationToken,
        command: &str,
    ) -> Result<ArtifactTransfer> {
        let capacity = self.transfers.clone().try_acquire_owned().map_err(|_| {
            DomainError::new(
                "RESOURCE_LIMIT",
                "Two artifact transfer slots are already active",
            )
        })?;
        let request =
            self.requests.clone().try_acquire_owned().map_err(|_| {
                DomainError::new("RESOURCE_LIMIT", "Owner request capacity reached")
            })?;
        let publication = {
            let _guard = self.mutation.lock().map_err(|_| lock_error())?;
            self.artifact_authority(&context, artifact_id, command)?.1
        };
        if expected_sha256.is_some_and(|digest| digest != publication.identity.sha256) {
            return Err(DomainError::new(
                "ARTIFACT_CHANGED",
                "Expected digest does not match the published artifact",
            ));
        }
        let reader =
            ArtifactReader::open(&publication.identity, Path::new(&publication.proof), cancel)
                .map_err(|error| {
                    if error.code == "FILESYSTEM_ERROR" {
                        DomainError::new(
                            "ARTIFACT_UNAVAILABLE",
                            "Artifact reader could not open its protected working index",
                        )
                    } else {
                        error
                    }
                })?;
        let transfer = ArtifactTransfer {
            service: self.clone(),
            context,
            artifact_id: artifact_id.into(),
            command: command.into(),
            publication,
            reader,
            _capacity: capacity,
            _request: request,
        };
        transfer.ensure_current()?;
        Ok(transfer)
    }
    pub(super) fn artifact_manifest(
        self: &Arc<Self>,
        context: &Context,
        id: &str,
        cancel: &CancellationToken,
    ) -> Result<ArtifactManifest> {
        let transfer = self.open_artifact_transfer(
            context.clone(),
            id,
            None,
            cancel.clone(),
            "artifacts.manifest",
        )?;
        let (job, _) = self.artifact_authority(context, id, "artifacts.manifest")?;
        let can_read = self
            .policy(context)?
            .as_ref()
            .is_none_or(|p| p.allows_command("artifacts.read_chunk"));
        Ok(ArtifactManifest {
            schema_version: 1,
            artifact_id: id.into(),
            manifest_uri: format!("numera://artifact/{id}/manifest"),
            job_id: job.id,
            plan_id: job.plan.id,
            mime_type: "application/octet-stream".into(),
            encoded_byte_count: transfer.bytes().to_string(),
            sha256: transfer.sha256().into(),
            availability: "retained_published_object".into(),
            expires_at: None,
            retrieval_methods: if can_read {
                vec![
                    "artifacts.read_chunk".into(),
                    "authenticated_http_when_enabled".into(),
                ]
            } else {
                Vec::new()
            },
            bytes_route: can_read.then(|| format!("/api/v1/artifacts/{id}/bytes")),
            max_chunk_bytes: ARTIFACT_CHUNK_BYTES as u32,
        })
    }
    pub(super) fn artifact_chunk(
        self: &Arc<Self>,
        context: Context,
        input: ChunkInput,
        cancel: CancellationToken,
    ) -> Result<ArtifactChunk> {
        let (offset, length) = input.range()?;
        let mut transfer = self.begin_artifact_transfer(
            context,
            &input.artifact_id,
            Some(&input.expected_sha256),
            cancel,
        )?;
        let bytes = transfer.read_range(offset, length)?;
        Ok(ArtifactChunk {
            artifact_id: input.artifact_id,
            offset: offset.to_string(),
            actual_length: bytes.len() as u32,
            total_size: transfer.bytes().to_string(),
            sha256: transfer.sha256().into(),
            eof: offset + bytes.len() as u64 == transfer.bytes(),
            data_base64: STANDARD.encode(bytes),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    async fn fixture() -> (
        tempfile::TempDir,
        Arc<Service>,
        Context,
        String,
        nw_domain::records::FileIdentity,
        Vec<u8>,
    ) {
        let (root, service, context, request, output) =
            super::super::tests::preflight_fixture().await;
        let plan: Plan = service
            .store
            .get(&named("plan", request.params["plan_id"].as_str().unwrap()).unwrap())
            .unwrap()
            .unwrap();
        let id = Uuid::new_v4().to_string();
        let workspace = nw_platform::security::private_directory(
            &root.path().join(format!(".numera-job-{id}")),
        )
        .unwrap();
        let proof = workspace.join(format!(".numera-proof-{}", Uuid::new_v4()));
        let bytes = (0..ARTIFACT_CHUNK_BYTES * 2 + 17)
            .map(|n| (n % 251) as u8)
            .collect::<Vec<_>>();
        std::fs::write(&output, &bytes).unwrap();
        std::fs::hard_link(&output, &proof).unwrap();
        let identity = files::identity(&output, &CancellationToken::new()).unwrap();
        let job = Job {
            file_times: None,
            id: id.clone(),
            creator: context.principal.clone(),
            plan,
            state: JobState::Succeeded,
            prepared_argv: None,
            prepared_stages: Vec::new(),
            owned_staging: Some(nw_domain::records::StagingIntent {
                job_id: id.clone(),
                directory: files::path_text(&workspace).unwrap(),
                payload: files::path_text(&workspace.join("payload")).unwrap(),
            }),
            artifact: Some(identity.clone()),
            verification: Some(
                "Synthetic artifact transfer fixture; no media process was run".into(),
            ),
            error: None,
        };
        let publication = Publication {
            read_purpose_version: 1,
            job_id: id.clone(),
            proof: files::path_text(&proof).unwrap(),
            output: identity.path.clone(),
            identity: identity.clone(),
            verified: job.verification.clone().unwrap(),
            published: true,
        };
        service
            .store
            .put(&named("job", &id).unwrap(), &job)
            .unwrap();
        service
            .store
            .put(&named("publication", &id).unwrap(), &publication)
            .unwrap();
        let mut profile: AccessProfile = service.store.get("profile/preflight").unwrap().unwrap();
        profile.allowed_command_ids = [
            "artifacts.manifest",
            "artifacts.list",
            "artifacts.read_chunk",
            "media.probe",
        ]
        .map(String::from)
        .into();
        profile.read_roots.clear();
        profile.write_roots.clear();
        service.store.put("profile/preflight", &profile).unwrap();
        (root, service, context, format!("job-{id}"), identity, bytes)
    }
    #[tokio::test]
    async fn output_purpose_needs_current_job_rights_but_no_general_export_read_grant() {
        let (_root, service, context, id, identity, expected) = fixture().await;
        let manifest = service
            .dispatch(
                context.clone(),
                Request::query("artifacts.manifest", json!({"artifact_id":id})),
            )
            .await;
        assert!(manifest.ok, "{manifest:?}");
        let result = manifest.result.unwrap();
        assert_eq!(result["sha256"], identity.sha256);
        assert!(!result.to_string().contains(&identity.path));
        let chunk = service.dispatch(context.clone(), Request::query("artifacts.read_chunk", json!({"artifact_id":id,"expected_sha256":identity.sha256,"offset":(ARTIFACT_CHUNK_BYTES - 7).to_string(),"length":31}))).await;
        assert!(chunk.ok, "{chunk:?}");
        let result = chunk.result.unwrap();
        assert_eq!(
            STANDARD
                .decode(result["data_base64"].as_str().unwrap())
                .unwrap(),
            expected[ARTIFACT_CHUNK_BYTES - 7..ARTIFACT_CHUNK_BYTES + 24]
        );
        assert_eq!(result["eof"], false);
        let denied = service
            .dispatch(
                context.clone(),
                Request::query("media.probe", json!({"path":identity.path})),
            )
            .await;
        assert_eq!(denied.error.unwrap().code, "PATH_DENIED");
        let listed = service
            .dispatch(context.clone(), Request::query("artifacts.list", json!({})))
            .await;
        assert_eq!(listed.result.unwrap()["total_count"], 1);
        let mut transfer = service
            .begin_artifact_transfer(
                context.clone(),
                &id,
                Some(&identity.sha256),
                CancellationToken::new(),
            )
            .unwrap();
        let mut policy: AccessProfile = service.store.get("profile/preflight").unwrap().unwrap();
        policy.owned_object_access = false;
        policy.revision += 1;
        service.store.put("profile/preflight", &policy).unwrap();
        assert_eq!(
            transfer.read_range(0, 1).unwrap_err().code,
            "OBJECT_NOT_FOUND"
        );
        let denied = service
            .dispatch(
                context.clone(),
                Request::query("artifacts.manifest", json!({"artifact_id":id})),
            )
            .await
            .error
            .unwrap();
        let absent = service
            .dispatch(
                context.clone(),
                Request::query("artifacts.manifest", json!({"artifact_id":"job-absent"})),
            )
            .await
            .error
            .unwrap();
        assert_eq!(value(&denied).unwrap(), value(&absent).unwrap());
        let listed = service
            .dispatch(context, Request::query("artifacts.list", json!({})))
            .await;
        assert_eq!(listed.result.unwrap()["total_count"], 0);
    }
    #[tokio::test]
    async fn replaced_output_capacity_and_restart_do_not_invent_publication_identity() {
        let (root, service, context, id, identity, expected) = fixture().await;
        let first = service
            .begin_artifact_transfer(
                context.clone(),
                &id,
                Some(&identity.sha256),
                CancellationToken::new(),
            )
            .unwrap();
        let second = service
            .begin_artifact_transfer(
                context.clone(),
                &id,
                Some(&identity.sha256),
                CancellationToken::new(),
            )
            .unwrap();
        assert_eq!(
            service
                .begin_artifact_transfer(context.clone(), &id, None, CancellationToken::new())
                .err()
                .unwrap()
                .code,
            "RESOURCE_LIMIT"
        );
        drop(first);
        drop(second);
        drop(service);
        let service = Service::open(root.path().join("data"), None).await.unwrap();
        let mut transfer = service
            .begin_artifact_transfer(
                context.clone(),
                &id,
                Some(&identity.sha256),
                CancellationToken::new(),
            )
            .unwrap();
        assert_eq!(transfer.read_range(0, 8).unwrap(), expected[..8]);
        std::fs::rename(&identity.path, root.path().join("original-output.mkv")).unwrap();
        std::fs::write(&identity.path, expected).unwrap();
        assert_eq!(
            transfer.read_range(0, 1).unwrap_err().code,
            "ARTIFACT_CHANGED"
        );
        drop(transfer);
        assert_eq!(
            service
                .begin_artifact_transfer(
                    context,
                    &id,
                    Some(&identity.sha256),
                    CancellationToken::new()
                )
                .err()
                .unwrap()
                .code,
            "ARTIFACT_CHANGED"
        );
    }
}

#[cfg(test)]
mod migration_tests {
    use super::*;
    #[test]
    fn older_publication_has_no_invented_byte_purpose() {
        let value = json!({"job_id":"old-job","proof":"removed-proof","output":"old-output","identity":{"path":"old-output","bytes":"1","sha256":"00"},"verified":"Historical development fact","published":true});
        let publication: Publication = serde_json::from_value(value).unwrap();
        assert_eq!(publication.read_purpose_version, 0);
    }
}
