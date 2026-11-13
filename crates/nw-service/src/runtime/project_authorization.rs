//! Project observations use one captured policy; request admission remains authoritative.
use super::*;
use nw_domain::projects::{
    ProjectHeader, Revision,
    authorization::{
        ActionAvailability, AvailabilityState, BlockReason, CurrentRight, PROJECT_TARGET_COMMANDS,
        ProjectAction, ProjectAuthorization, RequestGate, RuntimeAuthorization, requirements,
    },
};

pub(super) struct ProjectObservation<'a> {
    pub id: &'a str,
    pub creator: &'a str,
    pub revision: Revision,
    pub supported: bool,
    pub writer_unavailable: bool,
}

pub(super) fn project(
    policy: Option<&AccessProfile>,
    observed: ProjectObservation<'_>,
) -> ProjectAuthorization {
    let allowed = |action| {
        policy.is_none_or(|profile| {
            profile.allows_object(ObjectKind::Project, observed.id, observed.creator, action)
        })
    };
    let read = allowed(ObjectAction::Read);
    let edit = allowed(ObjectAction::Edit);
    let actions = PROJECT_TARGET_COMMANDS
        .into_iter()
        .map(|command| {
            let requirement = requirements(command).expect("Static project command requirements");
            let write = requirement.project_action == Some(ProjectAction::Edit);
            let mut reasons = Vec::new();
            if policy.is_some_and(|profile| !profile.allows_command(command)) {
                reasons.push(BlockReason::CommandNotAllowed);
            }
            if !write && !read {
                reasons.push(BlockReason::ProjectReadNotGranted);
            }
            if write && !edit {
                reasons.push(BlockReason::ProjectEditNotGranted);
            }
            if write && !observed.supported {
                reasons.push(BlockReason::UnsupportedProjectSchema);
            }
            if write && observed.writer_unavailable {
                reasons.push(BlockReason::WriterObservedUnavailable);
            }
            ActionAvailability {
                command_id: command.to_owned(),
                state: AvailabilityState::for_reasons(&reasons),
                reasons,
                unchecked_gates: requirement
                    .request_gates
                    .into_iter()
                    .filter(|gate| *gate != RequestGate::ProjectRights)
                    .collect(),
            }
        })
        .collect();
    ProjectAuthorization {
        schema_version: 1,
        policy_revision: policy.map_or(0, |profile| profile.revision).to_string(),
        observed_project_revision: observed.revision.to_string(),
        current_read: CurrentRight::observed(read, BlockReason::ProjectReadNotGranted),
        current_edit: CurrentRight::observed(edit, BlockReason::ProjectEditNotGranted),
        actions,
    }
}

pub(super) fn query_result(
    policy: Option<&AccessProfile>,
    header: &ProjectHeader,
    mut result: Value,
    request_id: &str,
    maximum_result_bytes: usize,
) -> Result<Response> {
    // Only the summary's existing observation can contribute a writer blocker.
    let writer_unavailable = matches!(
        result.get("writer_status").and_then(Value::as_str),
        Some("other_owner_read_only" | "writer_lease_unavailable_read_only")
    );
    result["authorization"] = value(&project(
        policy,
        ProjectObservation {
            id: header.id.as_str(),
            creator: &header.creator,
            revision: header.revision,
            supported: header.schema_major == 1 && header.envelope_version == 1,
            writer_unavailable,
        },
    ))?;
    if serde_json::to_vec(&result)
        .map_err(|_| {
            DomainError::new("SERIALIZATION_FAILED", "Project query serialization failed")
        })?
        .len()
        > maximum_result_bytes
    {
        return Err(nw_domain::projects::limit(
            "Enriched project query exceeds the page byte budget",
        ));
    }
    let response = Response::success(request_id, result);
    if serde_json::to_vec(&response)
        .map_err(|_| {
            DomainError::new(
                "SERIALIZATION_FAILED",
                "Project response serialization failed",
            )
        })?
        .len()
        > commands::MAX_RESULT_BYTES
    {
        return Err(nw_domain::projects::limit(
            "Project response exceeds the complete byte budget",
        ));
    }
    Ok(response)
}

impl Service {
    pub(super) fn project_discovery(
        &self,
        context: &Context,
        request: &Request,
        policy: Option<&AccessProfile>,
        cancellation: &CancellationToken,
    ) -> Result<Response> {
        let revision = policy.map_or(0, |profile| profile.revision);
        let mut response = super::super::CommandService
            .dispatch(super::super::Principal::LocalUser, request.clone());
        if response.ok && request.command.id == "commands.list" {
            let actions = commands::active_registry()
                .into_iter()
                .filter_map(|descriptor| descriptor.authorization_requirements)
                // Version1 observations retain their original closed project command set.
                // Other tools can publish static requirements without changing this codec.
                .filter(|requirement| {
                    PROJECT_TARGET_COMMANDS.contains(&requirement.command_id.as_str())
                        || ["projects.create", "projects.list"]
                            .contains(&requirement.command_id.as_str())
                })
                .map(|requirement| {
                    let reasons = if policy
                        .is_some_and(|profile| !profile.allows_command(&requirement.command_id))
                    {
                        vec![BlockReason::CommandNotAllowed]
                    } else {
                        Vec::new()
                    };
                    ActionAvailability {
                        command_id: requirement.command_id,
                        state: AvailabilityState::for_reasons(&reasons),
                        reasons,
                        unchecked_gates: requirement.request_gates,
                    }
                })
                .collect::<Vec<_>>();
            if actions.len() != 13 {
                return Err(DomainError::new(
                    "SERIALIZATION_FAILED",
                    "Project discovery requirements are incomplete",
                ));
            }
            response.result.as_mut().ok_or_else(|| {
                DomainError::new("SERIALIZATION_FAILED", "Discovery result is missing")
            })?["authorization"] = value(&RuntimeAuthorization {
                schema_version: 1,
                policy_revision: revision.to_string(),
                actions,
            })?;
        }
        #[cfg(test)]
        self.catalog_test_phase(read_model::CatalogPhase::DiscoveryPublication);
        // Publication cannot combine the old observation with a newer policy.
        let current = self.policy(context)?;
        if current.as_ref().map_or(0, |profile| profile.revision) != revision
            || current
                .as_ref()
                .is_some_and(|profile| !profile.allows_command(&request.command.id))
        {
            return Err(DomainError::new(
                "PERMISSION_DENIED",
                "Discovery authority changed before publication",
            ));
        }
        if cancellation.is_cancelled() {
            return Err(DomainError::new("CANCELLED", "Discovery request cancelled"));
        }
        Ok(response.bounded())
    }
}
