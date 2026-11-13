//! Mixed job inventories retain each job codec's states and artifact identity.
use nw_domain::{
    execution::{RemuxProgress, StageProgress},
    media_execution::MediaJobState,
    records::JobState,
    views::JobSummary,
};
use serde_json::Value;

pub(super) enum State {
    Legacy(JobState),
    Media(MediaJobState),
}
impl State {
    pub(super) fn terminal(&self) -> bool {
        match self {
            Self::Legacy(state) => state.terminal(),
            Self::Media(state) => state.terminal(),
        }
    }
    pub(super) fn succeeded(&self) -> bool {
        matches!(
            self,
            Self::Legacy(JobState::Succeeded) | Self::Media(MediaJobState::Succeeded)
        )
    }
    pub(super) fn label(&self) -> String {
        match self {
            Self::Legacy(state) => format!("{state:?}"),
            Self::Media(state) => format!("{state:?}"),
        }
    }
}
pub(super) struct JobRow {
    pub id: String,
    pub state: State,
    pub output_name: String,
    pub artifact_id: String,
    pub remux_progress: Option<RemuxProgress>,
    pub progress: Option<StageProgress>,
    pub warning_count: usize,
    pub error_code: Option<String>,
}
impl JobRow {
    pub(super) fn decode(value: Value) -> Result<Self, String> {
        if value.get("envelope_version").is_some() || value.get("payload_kind").is_some() {
            if value["envelope_version"] != 1 || value["payload_kind"] != "media_job_v1" {
                return Err("Unknown or incomplete media job inventory discriminator".into());
            }
            let id = value["id"]
                .as_str()
                .ok_or("Media job identity is missing")?
                .to_owned();
            nw_domain::projects::validate_id(&id).map_err(|error| error.to_string())?;
            let state = serde_json::from_value(value["state"].clone())
                .map_err(|error| error.to_string())?;
            let output_name = value["output_name"]
                .as_str()
                .ok_or("Media output name is missing")?
                .to_owned();
            if output_name != format!("media-{id}.mkv") {
                return Err("Media output name differs from its managed job identity".into());
            }
            Ok(Self {
                artifact_id: format!("media-{id}-output"),
                id,
                state: State::Media(state),
                output_name,
                remux_progress: serde_json::from_value(value["progress"].clone())
                    .map_err(|error| error.to_string())?,
                progress: None,
                warning_count: 0,
                error_code: serde_json::from_value(value["error_code"].clone())
                    .map_err(|error| error.to_string())?,
            })
        } else {
            let job: JobSummary =
                serde_json::from_value(value).map_err(|error| error.to_string())?;
            Ok(Self {
                artifact_id: format!("job-{}", job.id),
                id: job.id,
                state: State::Legacy(job.state),
                output_name: job.output_name,
                remux_progress: job.remux_progress,
                progress: job.progress,
                warning_count: job.warning_count,
                error_code: job.error_code,
            })
        }
    }
}

#[cfg(test)]
pub(super) fn exercise() {
    use serde_json::json;
    let legacy = JobRow::decode(json!({"id":"legacy-job","state":"succeeded","output_name":"keep.mkv","error_code":null,"warning_count":2})).unwrap();
    assert_eq!(legacy.id, "legacy-job");
    assert!(legacy.state.terminal() && legacy.state.succeeded());
    assert_eq!(legacy.state.label(), "Succeeded");
    assert_eq!(legacy.artifact_id, "job-legacy-job");
    assert_eq!(legacy.warning_count, 2);
    let id = "11111111-1111-4111-8111-111111111111";
    for state in [
        "queued",
        "preparing",
        "running",
        "verifying",
        "publishing",
        "cancelling",
        "succeeded",
        "failed",
        "cancelled",
        "interrupted",
    ] {
        let value = json!({"envelope_version":1,"payload_kind":"media_job_v1","id":id,"state":state,
            "revision":"1","output_name":format!("media-{id}.mkv"),"progress":null,"error_code":null});
        let row = JobRow::decode(value.clone()).unwrap();
        assert_eq!(row.artifact_id, format!("media-{id}-output"));
        assert_eq!(row.state.succeeded(), state == "succeeded");
        assert_eq!(
            row.state.terminal(),
            ["succeeded", "failed", "cancelled", "interrupted"].contains(&state)
        );
        let mut unknown = value.clone();
        unknown["envelope_version"] = json!(2);
        assert!(JobRow::decode(unknown).is_err());
        let mut partial = value.clone();
        partial.as_object_mut().unwrap().remove("payload_kind");
        assert!(JobRow::decode(partial).is_err());
        let mut unsupported = value;
        unsupported["state"] = json!("skipped");
        assert!(JobRow::decode(unsupported).is_err());
    }
}
