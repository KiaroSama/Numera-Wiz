//! Version 2 group-only fixture: public producers before A, read/replay thereafter.
use super::*;
use nw_domain::projects::{EntityId, ProjectId};

pub(super) struct LinkedProject {
    id: String,
    bin: String,
    sequence: String,
    tracks: [String; 2],
    sources: [String; 2],
    clips: [String; 2],
    occurrences: [String; 2],
    group: String,
    moved_images: BTreeMap<String, Value>,
    accepted: Vec<(Value, Value)>,
}

impl LinkedProject {
    async fn projection(
        &self,
        owner: &mut Owner,
        command: &str,
        mut params: Value,
        replacement: &mut Option<String>,
        revision: u64,
        evidence: &Evidence,
    ) -> Result<Value> {
        if let Some(id) = replacement.take() {
            params["replace_snapshot"] = json!(id);
        }
        let mut request = query(command, params);
        request["project"] = json!({"id":self.id});
        let response = owner.command(&request, evidence).await?;
        let result = response["result"].clone();
        ensure!(
            result["project_revision"] == revision.to_string(),
            "Linked projection revision differs"
        );
        *replacement = Some(text(&result, "snapshot_id")?);
        Ok(result)
    }

    async fn clip_images(
        &self,
        owner: &mut Owner,
        revision: u64,
        replacement: &mut Option<String>,
        evidence: &Evidence,
    ) -> Result<BTreeMap<String, Value>> {
        let mut images = BTreeMap::new();
        for id in &self.clips {
            let result = self
                .projection(
                    owner,
                    "projects.inspect",
                    json!({"projection":"entity","entity_id":id}),
                    replacement,
                    revision,
                    evidence,
                )
                .await?;
            ensure!(
                result["entity"]["id"] == *id && result["entity"]["kind"] == "clip",
                "Linked clip identity/kind differs"
            );
            images.insert(id.clone(), result["entity"].clone());
        }
        Ok(images)
    }

    fn clips_match(
        &self,
        images: &BTreeMap<String, Value>,
        moved: bool,
        group: Option<&str>,
    ) -> Result<()> {
        ensure!(
            images.len() == 2,
            "Linked fixture requires exactly two clips"
        );
        for i in 0..2 {
            let clip = &images[&self.clips[i]];
            let start = match (i, moved) {
                (0, false) => json!({"num":"0","den":"1"}),
                (0, true) | (1, false) => json!({"num":"1","den":"4"}),
                _ => json!({"num":"1","den":"2"}),
            };
            let source_range = if i == 0 {
                json!({"start":{"num":"0","den":"1"},"end":{"num":"2","den":"1"}})
            } else {
                json!({"start":{"num":"1","den":"1"},"end":{"num":"5","den":"2"}})
            };
            let duration = if i == 0 {
                json!({"num":"2","den":"1"})
            } else {
                json!({"num":"3","den":"2"})
            };
            ensure!(
                clip["id"] == self.clips[i]
                    && clip["kind"] == "clip"
                    && clip["owner_id"] == self.tracks[i]
                    && clip["sequence_id"] == self.sequence
                    && clip["track_id"] == self.tracks[i]
                    && clip["source_id"] == self.sources[i]
                    && clip["occurrence_id"] == self.occurrences[i]
                    && clip["source_range"] == source_range
                    && clip["sequence_start"] == start
                    && clip["duration"] == duration
                    && clip["rate"] == json!({"num":"1","den":"1"})
                    && clip["link_group_id"] == json!(group)
                    && clip["references"]
                        == json!([{"role":"ownership","target":self.tracks[i]},{"role":"source","target":self.sources[i]}]),
                "Linked clip geometry/source/occurrence/group/reference differs"
            );
        }
        // Independently fixed starts/durations prove ends A=9/4, B=2 after the
        // move and preserve the original pairwise start offset of 1/4.
        Ok(())
    }

    fn changed_pair(&self, result: &Value) -> Result<()> {
        let actual = result["changed_ids"]
            .as_array()
            .context("Linked changed IDs")?;
        ensure!(
            actual.len() == 2
                && actual.iter().filter(|id| **id == self.clips[0]).count() == 1
                && actual.iter().filter(|id| **id == self.clips[1]).count() == 1,
            "Linked operation did not change exactly both members"
        );
        Ok(())
    }

    pub(super) async fn create(
        owner: &mut Owner,
        scope: &str,
        directory: &Path,
        evidence: &Evidence,
    ) -> Result<(Self, Value)> {
        let mut accepted = Vec::new();
        let create = project_write(
            owner,
            durable(
                scope,
                "compat-linked-project-create",
                "projects.create",
                json!({"name":"Compatibility linked project"}),
                None,
            ),
            0,
            directory,
            evidence,
            &mut accepted,
        )
        .await?;
        let id = text(&create, "project_id")?;
        let bin = text(&create, "root_bin_id")?;
        let sequence = project_write(owner, durable(scope, "compat-linked-sequence-create", "sequences.create", json!({"name":"Linked sequence","settings":{"width":64,"height":48,"frame_rate":{"num":"4","den":"1"},"sample_rate":48000,"audio_layout":"stereo"},"tracks":[{"kind":"video","name":"V1"},{"kind":"video","name":"V2"}]}), Some((&id, 0))), 1, directory, evidence, &mut accepted).await?;
        ensure!(
            sequence["created"]["tracks"]
                .as_array()
                .is_some_and(|tracks| tracks.len() == 2),
            "Linked sequence requires exactly two explicit tracks"
        );
        let tracks = [
            text(&sequence["created"]["tracks"][0], "id")?,
            text(&sequence["created"]["tracks"][1], "id")?,
        ];
        let sequence = text(&sequence["created"], "sequence_id")?;
        let mut sources = Vec::new();
        let mut clips = Vec::new();
        for (i, track) in tracks.iter().enumerate() {
            let (key, source_range, sequence_start) = if i == 0 {
                (
                    "compat-linked-place-a",
                    json!({"start":{"num":"0","den":"1"},"end":{"num":"2","den":"1"}}),
                    json!({"num":"0","den":"1"}),
                )
            } else {
                (
                    "compat-linked-place-b",
                    json!({"start":{"num":"1","den":"1"},"end":{"num":"5","den":"2"}}),
                    json!({"num":"1","den":"4"}),
                )
            };
            let place = project_write(owner, durable(scope, key, "clips.place", json!({"sequence_id":sequence,"track_id":track,"source":{"kind":"solid_color","rgba":[0,0,0,1],"duration":{"num":"4","den":"1"}},"source_range":source_range,"sequence_start":sequence_start,"collision":"reject"}), Some((&id, 1 + i as u64))), 2 + i as u64, directory, evidence, &mut accepted).await?;
            ensure!(
                place["created"]["clip_ids"]
                    .as_array()
                    .is_some_and(|ids| ids.len() == 1),
                "Linked placement requires one clip"
            );
            sources.push(text(&place["created"], "source_id")?);
            clips.push(
                place["created"]["clip_ids"][0]
                    .as_str()
                    .context("Linked placed clip")?
                    .to_owned(),
            );
        }
        let mut linked = Self {
            id,
            bin,
            sequence,
            tracks,
            sources: [sources[0].clone(), sources[1].clone()],
            clips: [clips[0].clone(), clips[1].clone()],
            occurrences: [String::new(), String::new()],
            group: String::new(),
            moved_images: BTreeMap::new(),
            accepted,
        };
        // Share one replacement chain through all pre-A intermediate projections.
        let mut replacement = None;
        let placed = linked
            .clip_images(owner, 3, &mut replacement, evidence)
            .await?;
        linked.occurrences = [
            text(&placed[&linked.clips[0]], "occurrence_id")?,
            text(&placed[&linked.clips[1]], "occurrence_id")?,
        ];
        linked.clips_match(&placed, false, None)?;
        let result = project_write(
            owner,
            durable(
                scope,
                "compat-linked-link",
                "clips.link",
                json!({"clip_ids":linked.clips,"link_policy":"expand"}),
                Some((&linked.id, 3)),
            ),
            4,
            directory,
            evidence,
            &mut linked.accepted,
        )
        .await?;
        linked.group = text(&result["created"], "link_group_id")?;
        linked.changed_pair(&result)?;
        linked.created_pair(&result)?;
        let grouped = linked
            .clip_images(owner, 4, &mut replacement, evidence)
            .await?;
        linked.clips_match(&grouped, false, Some(&linked.group))?;
        let result = project_write(owner, durable(scope, "compat-linked-move", "clips.move", json!({"clip_ids":[linked.clips[0]],"delta":{"num":"1","den":"4"},"collision":"reject","link_policy":"expand"}), Some((&linked.id, 4))), 5, directory, evidence, &mut linked.accepted).await?;
        linked.changed_pair(&result)?;
        let moved = linked
            .clip_images(owner, 5, &mut replacement, evidence)
            .await?;
        linked.clips_match(&moved, true, Some(&linked.group))?;
        let result = project_write(
            owner,
            durable(
                scope,
                "compat-linked-unlink",
                "clips.unlink",
                json!({"clip_ids":[linked.clips[1]],"link_policy":"expand"}),
                Some((&linked.id, 5)),
            ),
            6,
            directory,
            evidence,
            &mut linked.accepted,
        )
        .await?;
        linked.changed_pair(&result)?;
        linked.created_pair(&result)?;
        let unlinked = linked
            .clip_images(owner, 6, &mut replacement, evidence)
            .await?;
        linked.clips_match(&unlinked, true, None)?;
        let result = project_write(
            owner,
            durable(
                scope,
                "compat-linked-undo-unlink",
                "history.undo",
                json!({}),
                Some((&linked.id, 6)),
            ),
            7,
            directory,
            evidence,
            &mut linked.accepted,
        )
        .await?;
        linked.changed_pair(&result)?;
        ensure!(
            result["history"]["can_redo"] == true,
            "Undo must retain unlink redo"
        );
        // The final semantic capture consumes this last pre-A ticket rather than
        // leaving one additional retained ticket for every intermediate revision.
        linked.moved_images = moved;
        let stable = linked
            .semantic_with_replacement(owner, &mut replacement, evidence)
            .await?;
        ensure!(linked.accepted.len() == 8, "Linked producer count differs");
        evidence.record(json!({"phase":"actual-linked-project-created","fixture_version":2,"variant":"public-group-only","project_id":linked.id,"root_bin_id":linked.bin,"sequence_id":linked.sequence,"track_ids":linked.tracks,"source_ids":linked.sources,"clip_ids":linked.clips,"occurrence_ids":linked.occurrences,"link_group_id":linked.group,"revision":"7","producer_commands":linked.accepted.iter().map(|(request, response)|json!({"command_id":request["command"]["id"],"request_id":request["request_id"],"receipt_id":response["receipt_id"]})).collect::<Vec<_>>(),"stable_projection_sha256":sha(&serde_json::to_vec(&stable)?),"registered_media":false}))?;
        Ok((linked, stable))
    }

    fn created_pair(&self, result: &Value) -> Result<()> {
        let ids = result["created"]["clip_ids"]
            .as_array()
            .context("Linked created member IDs")?;
        ensure!(
            ids.len() == 2
                && ids.iter().filter(|id| **id == self.clips[0]).count() == 1
                && ids.iter().filter(|id| **id == self.clips[1]).count() == 1,
            "Topology result must name exactly both linked members"
        );
        Ok(())
    }

    pub(super) async fn semantic(&self, owner: &mut Owner, evidence: &Evidence) -> Result<Value> {
        self.semantic_with_replacement(owner, &mut None, evidence)
            .await
    }

    async fn semantic_with_replacement(
        &self,
        owner: &mut Owner,
        replacement: &mut Option<String>,
        evidence: &Evidence,
    ) -> Result<Value> {
        let summary = self
            .projection(
                owner,
                "projects.inspect",
                json!({"projection":"summary"}),
                replacement,
                7,
                evidence,
            )
            .await?;
        ensure!(
            summary["project_id"] == self.id
                && summary["root_bin_id"] == self.bin
                && summary["name"] == "Compatibility linked project"
                && summary["entity_count"] == 8
                && summary["sequence_count"] == 1
                && summary["history"]["can_undo"] == true
                && summary["history"]["can_redo"] == true,
            "Linked header/history differs"
        );
        let mut entities = self.clip_images(owner, 7, replacement, evidence).await?;
        self.clips_match(&entities, true, Some(&self.group))?;
        ensure!(
            entities == self.moved_images,
            "Undo did not restore exact revision-5 clip images/group"
        );
        for (id, kind) in [
            (&self.bin, "bin"),
            (&self.sequence, "sequence"),
            (&self.tracks[0], "video"),
            (&self.tracks[1], "video"),
            (&self.sources[0], "source_binding"),
            (&self.sources[1], "source_binding"),
        ] {
            let result = self
                .projection(
                    owner,
                    "projects.inspect",
                    json!({"projection":"entity","entity_id":id}),
                    replacement,
                    7,
                    evidence,
                )
                .await?;
            ensure!(
                result["entity"]["id"] == *id && result["entity"]["kind"] == kind,
                "Linked entity identity/kind differs"
            );
            entities.insert(id.clone(), result["entity"].clone());
        }
        ensure!(
            entities.len() == 8
                && !entities.contains_key(&self.group)
                && entities[&self.bin]["member_count"] == 3
                && entities[&self.bin]["owner_id"].is_null()
                && entities[&self.bin]["references"] == json!([])
                && entities[&self.sequence]["owner_id"] == self.bin
                && entities[&self.sequence]["references"]
                    == json!([{"role":"ownership","target":self.bin}])
                && entities[&self.sequence]["track_count"] == 2
                && entities[&self.sequence]["settings"]
                    == json!({"width":64,"height":48,"frame_rate":{"num":"4","den":"1"},"sample_rate":48000,"audio_layout":"stereo"}),
            "Linked live entity/root/sequence facts differ"
        );
        for i in 0..2 {
            ensure!(
                entities[&self.tracks[i]]["owner_id"] == self.sequence
                    && entities[&self.tracks[i]]["references"]
                        == json!([{"role":"ownership","target":self.sequence}])
                    && entities[&self.tracks[i]]["sequence_id"] == self.sequence
                    && entities[&self.tracks[i]]["grid"] == json!({"num":"4","den":"1"})
                    && entities[&self.tracks[i]]["clip_count"] == 1
                    && entities[&self.tracks[i]]["order"] == i
                    && entities[&self.sources[i]]["owner_id"] == self.bin
                    && entities[&self.sources[i]]["references"]
                        == json!([{"role":"ownership","target":self.bin}])
                    && entities[&self.sources[i]]["source_kind"] == "solid_color"
                    && entities[&self.sources[i]]["duration"] == json!({"num":"4","den":"1"}),
                "Linked track/source semantics differ"
            );
        }
        let tracks = self
            .projection(
                owner,
                "sequences.inspect",
                json!({"sequence_id":self.sequence,"projection":"track_page","limit":128}),
                replacement,
                7,
                evidence,
            )
            .await?;
        let viewport = self.projection(owner, "sequences.inspect", json!({"sequence_id":self.sequence,"projection":"viewport","track_ids":self.tracks,"range":{"start":{"num":"0","den":"1"},"end":{"num":"3","den":"1"}},"limit":128}), replacement, 7, evidence).await?;
        ensure!(
            tracks["next_cursor"].is_null() && viewport["next_cursor"].is_null(),
            "Linked finite projections require more pages"
        );
        let track_items = tracks["items"].as_array().context("Linked track page")?;
        let clip_items = viewport["items"].as_array().context("Linked viewport")?;
        ensure!(
            track_items.len() == 2 && clip_items.len() == 2,
            "Linked track/clip page counts differ"
        );
        for i in 0..2 {
            ensure!(
                track_items
                    .iter()
                    .filter(|item| item["id"] == self.tracks[i]
                        && item["kind"] == "video"
                        && item["clip_count"] == 1)
                    .count()
                    == 1
                    && clip_items
                        .iter()
                        .filter(|item| item["id"] == self.clips[i]
                            && item["track_id"] == self.tracks[i]
                            && item["source_id"] == self.sources[i]
                            && item["sequence_start"] == entities[&self.clips[i]]["sequence_start"]
                            && item["duration"] == entities[&self.clips[i]]["duration"])
                        .count()
                        == 1,
                "Linked track/viewport membership differs"
            );
        }
        let stable = json!({"project_id":self.id,"root_bin_id":self.bin,"revision":"7","history":summary["history"],"entities":entities,"track_items":track_items,"clip_items":clip_items});
        evidence.record(json!({"phase":"semantic-linked-project","fixture_version":2,"binary":owner.binary.label,"project_id":self.id,"revision":"7","can_redo":true,"link_group_id":self.group,"stable_projection_sha256":sha(&serde_json::to_vec(&stable)?)}))?;
        Ok(stable)
    }

    pub(super) async fn replay(&self, owner: &mut Owner, evidence: &Evidence) -> Result<()> {
        for (request, response) in &self.accepted {
            let replay = owner.command(request, evidence).await?;
            same_receipt(response, &replay)?;
            ensure!(
                *response == replay,
                "Linked replay changed the whole accepted Response"
            );
        }
        Ok(())
    }

    pub(super) fn distinct(&self, project: &Project, semantic: &Value) -> Result<()> {
        let mut ids = BTreeSet::new();
        for id in [
            &project.id,
            &project.bin,
            &project.sequence,
            &project.track,
            &project.source,
            &project.left,
            &project.right,
        ] {
            ensure!(
                ids.insert(id.clone()),
                "Original fixture identities collide"
            );
        }
        for clip in [&project.left, &project.right] {
            ensure!(
                ids.insert(text(&semantic["entities"][clip], "occurrence_id")?),
                "Original occurrence identities collide"
            );
        }
        ProjectId::parse(&self.id)?;
        for id in [
            &self.id,
            &self.bin,
            &self.sequence,
            &self.tracks[0],
            &self.tracks[1],
            &self.sources[0],
            &self.sources[1],
            &self.clips[0],
            &self.clips[1],
            &self.occurrences[0],
            &self.occurrences[1],
            &self.group,
        ] {
            EntityId::parse(id)?;
            ensure!(
                ids.insert(id.clone()),
                "P/L project/entity/occurrence/group identities collide"
            );
        }
        Ok(())
    }

    pub(super) fn receipts(&self, project: &Project, prior: &[&Value]) -> Result<()> {
        let mut receipts = BTreeSet::new();
        let mut requests = BTreeSet::new();
        let mut keys = BTreeSet::new();
        for response in prior {
            ensure!(
                receipts.insert(text(response, "receipt_id")?),
                "Profile/migration receipts collide"
            );
            let request_id = text(response, "request_id")?;
            ensure!(
                requests.insert(request_id.clone()) && keys.insert(request_id),
                "Profile/migration request IDs/keys collide"
            );
        }
        for (request, response) in project.accepted.iter().chain(&self.accepted) {
            ensure!(
                receipts.insert(text(response, "receipt_id")?)
                    && requests.insert(text(request, "request_id")?)
                    && keys.insert(text(request, "idempotency_key")?),
                "P/L/profile/migration durable identities collide"
            );
        }
        Ok(())
    }

    pub(super) fn dependencies(
        &self,
        project: &Project,
        snapshot: &Snapshot,
        checkpoint: &str,
        evidence: &Evidence,
    ) -> Result<()> {
        let mut expected = BTreeMap::new();
        for clip in &self.clips {
            let entity = EntityId::parse(clip)?;
            expected.insert(
                format!("project-dependency/{}/{}/{}", self.id, self.group, clip),
                serde_json::to_vec(&entity)?,
            );
        }
        let mut global = BTreeMap::new();
        let mut unlinked = BTreeMap::new();
        let mut linked = BTreeMap::new();
        for (key, value) in &snapshot.rows {
            let parts = key.split('/').collect::<Vec<_>>();
            if parts[0] != "project-dependency" {
                continue;
            }
            ensure!(
                parts.len() == 4,
                "Dependency key has noncanonical delimiter boundaries"
            );
            ProjectId::parse(parts[1])?;
            EntityId::parse(parts[2])?;
            EntityId::parse(parts[3])?;
            ensure!(
                parts[1] == project.id || parts[1] == self.id,
                "Dependency belongs to an unexplained project"
            );
            global.insert(key.clone(), value.clone());
            if parts[1] == project.id {
                unlinked.insert(key.clone(), value.clone());
            } else {
                linked.insert(key.clone(), value.clone());
            }
        }
        ensure!(
            unlinked.is_empty()
                && linked == expected
                && global == expected
                && expected.len() == 2
                && snapshot.facts.namespaces.get("project-dependency").copied() == Some(2)
                && !snapshot
                    .rows
                    .contains_key(&format!("project-entity/{}/{}", self.id, self.group)),
            "Expected exact P-empty/L-two/global-disjoint-union dependency maps and opaque group"
        );
        let mut retained_receipts = Vec::new();
        let receipt_rows = snapshot
            .rows
            .iter()
            .filter(|(key, _)| key.starts_with("receipt/"))
            .map(|(key, bytes)| Ok((key, bytes, serde_json::from_slice::<Value>(bytes)?)))
            .collect::<Result<Vec<_>>>()?;
        for (request, response) in &self.accepted {
            let matches = receipt_rows
                .iter()
                .filter(|(_, _, receipt)| receipt["response"] == *response)
                .collect::<Vec<_>>();
            ensure!(
                matches.len() == 1,
                "Linked accepted Response missing or duplicated in raw receipts"
            );
            let (key, bytes, _) = matches[0];
            retained_receipts.push(json!({"command_id":request["command"]["id"],"request_id":request["request_id"],"receipt_id":response["receipt_id"],"key":key,"bytes":bytes.len(),"sha256":sha(bytes)}));
        }
        let header: Value = serde_json::from_slice(
            snapshot
                .rows
                .get(&format!("project/{}", self.id))
                .context("Linked raw header missing")?,
        )?;
        let history: Value = serde_json::from_slice(
            snapshot
                .rows
                .get(&format!("project-history-summary/{}", self.id))
                .context("Linked raw history missing")?,
        )?;
        ensure!(
            header["revision"] == "7" && history["can_redo"] == true,
            "Closed linked revision/redo differs"
        );
        let rows = |map: &BTreeMap<String, Vec<u8>>| {
            map.iter().map(|(key,value)| json!({"key":key,"value":String::from_utf8_lossy(value),"bytes":value.len(),"sha256":sha(value)})).collect::<Vec<_>>()
        };
        evidence.record(json!({"phase":"project-dependency-coverage","fixture_version":2,"variant":"public-group-only","checkpoint":checkpoint,"project_id":project.id,"linked_project_id":self.id,"project_dependency_rows":unlinked.len(),"linked_dependency_rows":linked.len(),"global_dependency_rows":global.len(),"expected_dependency_rows":expected.len(),"expected_map":rows(&expected),"actual_global_map":rows(&global),"link_group_id":self.group,"member_ids":self.clips,"linked_revision":header["revision"],"linked_history":history,"retained_linked_receipts":retained_receipts,"base_project_namespaces_required_nonempty":POPULATED_PROJECT_NAMESPACES,"combined_project_namespaces_required_nonempty":combined_namespaces(),"nonempty_group_dependency_preservation":"verified-at-checkpoint","mixed_av_preservation":"unqualified","semantic_input_dependency_preservation":"unqualified","registered_media":false}))?;
        Ok(())
    }

    pub(super) fn preserved(&self, original: &Snapshot, current: &Snapshot) -> Result<()> {
        // Supplement the unchanged GLOBAL projects_identical comparisons with an
        // A/D L-only check: the sole adoption edit is in P. Exact delimiter
        // boundaries include every project-* namespace, even unknown ones.
        let rows = |snapshot: &Snapshot| {
            snapshot
                .rows
                .iter()
                .filter(|(key, _)| {
                    let Some((namespace, rest)) = key.split_once('/') else {
                        return false;
                    };
                    (namespace == "project" || namespace.starts_with("project-"))
                        && (rest == self.id
                            || rest
                                .strip_prefix(self.id.as_str())
                                .is_some_and(|suffix| suffix.starts_with('/')))
                })
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect::<BTreeMap<_, _>>()
        };
        ensure!(
            rows(original) == rows(current),
            "P adoption changed linked project raw key set/bytes"
        );
        Ok(())
    }

    pub(super) fn completion(&self) -> Value {
        json!({"fixture_version":2,"variant":"public-group-only","linked_project_id":self.id,"linked_final_revision":"7","link_group_id":self.group,"member_ids":self.clips,"linked_producer_count":self.accepted.len(),"linked_receipt_ids":self.accepted.iter().map(|(_, response)|response["receipt_id"].clone()).collect::<Vec<_>>()})
    }
}

pub(super) fn combined_namespaces() -> Vec<&'static str> {
    POPULATED_PROJECT_NAMESPACES
        .iter()
        .copied()
        .chain(["project-dependency"])
        .collect()
}
