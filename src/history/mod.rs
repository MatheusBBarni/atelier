use crate::ids::new_id;
use crate::orchestrator::ArtifactReference;
use anyhow::{anyhow, Context, Result};
use chrono::{SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct HistoryEvent {
    pub schema_version: u32,
    pub event_id: String,
    pub session_id: String,
    pub run_id: Option<String>,
    #[serde(default)]
    pub group_id: Option<String>,
    pub step_id: Option<String>,
    pub timestamp: String,
    pub kind: String,
    pub payload: Value,
    #[serde(default)]
    pub payload_truncated: bool,
}

impl HistoryEvent {
    pub fn new(
        session_id: impl Into<String>,
        run_id: Option<String>,
        step_id: Option<String>,
        kind: impl Into<String>,
        payload: Value,
    ) -> Self {
        Self::new_with_group(session_id, run_id, None, step_id, kind, payload)
    }

    pub fn new_with_group(
        session_id: impl Into<String>,
        run_id: Option<String>,
        group_id: Option<String>,
        step_id: Option<String>,
        kind: impl Into<String>,
        payload: Value,
    ) -> Self {
        Self {
            schema_version: 1,
            event_id: new_id(),
            session_id: session_id.into(),
            run_id,
            group_id,
            step_id,
            timestamp: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
            kind: kind.into(),
            payload,
            payload_truncated: false,
        }
    }
}

/// Cross-session UI flags persisted at the `.atelier/` data root (ADR-004).
/// Lives outside `sessions/`, so it survives `clean_sessions` and a fresh launch.
/// New show-once flags are added here with `#[serde(default)]` for forward
/// compatibility — never a new file per flag.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
struct PersistentUiState {
    #[serde(default)]
    first_approval_explainer_shown: bool,
}

/// Root-level filename for [`PersistentUiState`].
const UI_STATE_FILE: &str = "ui_state.json";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionMetadata {
    pub schema_version: u32,
    pub session_id: String,
    pub working_directory: PathBuf,
    pub started_at: String,
}

#[derive(Clone, Debug)]
pub struct HistoryStore {
    root: PathBuf,
    session_id: String,
    session_dir: PathBuf,
    events_path: PathBuf,
    artifacts_dir: PathBuf,
}

impl HistoryStore {
    pub fn create(working_directory: &Path) -> Result<Self> {
        let root = working_directory.join(".multiagent");
        let session_id = new_id();
        let session_dir = root.join("sessions").join(&session_id);
        let artifacts_dir = session_dir.join("artifacts");
        let runs_dir = root.join("runs");

        create_private_dir(&artifacts_dir)?;
        create_private_dir(&runs_dir)?;

        let metadata = SessionMetadata {
            schema_version: 1,
            session_id: session_id.clone(),
            working_directory: working_directory.to_path_buf(),
            started_at: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
        };
        let metadata_path = session_dir.join("metadata.json");
        write_private_file(
            &metadata_path,
            serde_json::to_string_pretty(&metadata)?.as_bytes(),
        )?;

        let events_path = session_dir.join("events.jsonl");
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(&events_path)
            .with_context(|| format!("failed to create {}", events_path.display()))?;
        set_private_file_permissions(&events_path)?;

        Ok(Self {
            root,
            session_id,
            session_dir,
            events_path,
            artifacts_dir,
        })
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    pub fn session_dir(&self) -> &Path {
        &self.session_dir
    }

    pub fn append_event(&self, event: &HistoryEvent) -> Result<()> {
        let mut file = OpenOptions::new()
            .append(true)
            .create(true)
            .open(&self.events_path)
            .with_context(|| format!("failed to open {}", self.events_path.display()))?;
        serde_json::to_writer(&mut file, event)?;
        file.write_all(b"\n")?;
        file.flush()?;
        Ok(())
    }

    pub fn append_debug_event(&self, event: &HistoryEvent) -> Result<()> {
        let path = self.root.join("debug.log");
        let existed = path.exists();
        let mut file = OpenOptions::new()
            .append(true)
            .create(true)
            .open(&path)
            .with_context(|| format!("failed to open {}", path.display()))?;
        if !existed {
            set_private_file_permissions(&path)?;
        }
        let record = serde_json::json!({
            "schema_version": 1,
            "timestamp": event.timestamp,
            "event_id": event.event_id,
            "session_id": event.session_id,
            "run_id": event.run_id,
            "group_id": event.group_id,
            "step_id": event.step_id,
            "kind": event.kind,
            "payload": event.payload,
        });
        serde_json::to_writer(&mut file, &record)?;
        file.write_all(b"\n")?;
        file.flush()?;
        Ok(())
    }

    pub fn read_events(&self) -> Result<Vec<HistoryEvent>> {
        read_events_from_path(&self.events_path)
    }

    fn ui_state_path(&self) -> PathBuf {
        self.root.join(UI_STATE_FILE)
    }

    fn read_ui_state(&self) -> PersistentUiState {
        // A missing or unparseable file degrades to defaults rather than failing
        // the run — at worst the show-once explainer shows one extra time.
        fs::read_to_string(self.ui_state_path())
            .ok()
            .and_then(|raw| serde_json::from_str::<PersistentUiState>(&raw).ok())
            .unwrap_or_default()
    }

    /// Whether the first-approval explainer has already been shown once for this
    /// user (cross-session show-once latch, ADR-004).
    pub fn first_approval_explainer_shown(&self) -> bool {
        self.read_ui_state().first_approval_explainer_shown
    }

    /// Idempotently latch the first-approval explainer as shown. A no-op (no
    /// write) when the flag is already set.
    pub fn mark_first_approval_explainer_shown(&self) -> Result<()> {
        let mut state = self.read_ui_state();
        if state.first_approval_explainer_shown {
            return Ok(());
        }
        state.first_approval_explainer_shown = true;
        write_private_file(
            &self.ui_state_path(),
            serde_json::to_string_pretty(&state)?.as_bytes(),
        )?;
        Ok(())
    }

    pub fn write_run_record<T: Serialize>(&self, run_id: &str, record: &T) -> Result<PathBuf> {
        let path = self.root.join("runs").join(format!("{run_id}.json"));
        write_private_file(&path, serde_json::to_string_pretty(record)?.as_bytes())?;
        Ok(path)
    }

    pub fn write_artifact(
        &self,
        extension: &str,
        media_type: &str,
        contents: &[u8],
        redaction_status: &str,
    ) -> Result<ArtifactReference> {
        let artifact_id = new_id();
        let extension = extension.trim_start_matches('.');
        let filename = format!("{artifact_id}.{extension}");
        let path = self.artifacts_dir.join(&filename);
        write_private_file(&path, contents)?;

        let sha256 = Sha256::digest(contents)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let relative_path = path
            .strip_prefix(&self.root)
            .unwrap_or(&path)
            .to_string_lossy()
            .to_string();

        Ok(ArtifactReference {
            artifact_id,
            path: relative_path,
            media_type: media_type.to_string(),
            byte_length: contents.len(),
            sha256,
            redaction_status: redaction_status.to_string(),
        })
    }
}

pub fn read_events_from_path(path: &Path) -> Result<Vec<HistoryEvent>> {
    let file = File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    let reader = BufReader::new(file);
    let mut events = Vec::new();
    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let event: HistoryEvent = serde_json::from_str(&line)
            .with_context(|| format!("failed to parse history event in {}", path.display()))?;
        if event.schema_version != 1 {
            return Err(anyhow!(
                "unsupported history schema_version {} in {}",
                event.schema_version,
                path.display()
            ));
        }
        events.push(event);
    }
    Ok(events)
}

pub fn clean_sessions(working_directory: &Path) -> Result<Vec<PathBuf>> {
    let root = working_directory.join(".multiagent");
    let targets = [root.join("sessions"), root.join("runs")];
    let mut deleted = Vec::new();
    for target in targets {
        if target.exists() {
            fs::remove_dir_all(&target)
                .with_context(|| format!("failed to delete {}", target.display()))?;
            deleted.push(target);
        }
    }
    Ok(deleted)
}

fn create_private_dir(path: &Path) -> Result<()> {
    let existed = path.exists();
    fs::create_dir_all(path).with_context(|| format!("failed to create {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if !existed {
            fs::set_permissions(path, fs::Permissions::from_mode(0o700)).with_context(|| {
                format!("failed to set private permissions on {}", path.display())
            })?;
        }
    }
    Ok(())
}

fn write_private_file(path: &Path, contents: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        create_private_dir(parent)?;
    }
    let mut file =
        File::create(path).with_context(|| format!("failed to create {}", path.display()))?;
    file.write_all(contents)
        .with_context(|| format!("failed to write {}", path.display()))?;
    file.flush()?;
    set_private_file_permissions(path)?;
    Ok(())
}

fn set_private_file_permissions(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .with_context(|| format!("failed to set private permissions on {}", path.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::tempdir;

    #[test]
    fn appends_and_reads_jsonl_events() {
        let dir = tempdir().unwrap();
        let store = HistoryStore::create(dir.path()).unwrap();
        let event = HistoryEvent::new(
            store.session_id().to_string(),
            Some("run".to_string()),
            None,
            "run_started",
            json!({"prompt": "hello"}),
        );
        store.append_event(&event).unwrap();
        let events = store.read_events().unwrap();
        assert_eq!(events, vec![event]);
    }

    #[test]
    fn reads_legacy_jsonl_events_without_group_id() {
        let dir = tempdir().unwrap();
        let store = HistoryStore::create(dir.path()).unwrap();
        let path = store.session_dir().join("events.jsonl");
        fs::write(
            &path,
            r#"{"schema_version":1,"event_id":"event","session_id":"session","run_id":"run","step_id":"step","timestamp":"2026-06-06T00:00:00.000Z","kind":"agent_result","payload":{}}"#,
        )
        .unwrap();

        let events = read_events_from_path(&path).unwrap();

        assert_eq!(events[0].group_id, None);
        assert!(!events[0].payload_truncated);
    }

    #[test]
    fn writes_artifact_with_sha256_metadata() {
        let dir = tempdir().unwrap();
        let store = HistoryStore::create(dir.path()).unwrap();
        let artifact = store
            .write_artifact("txt", "text/plain", b"hello", "contains_user_content")
            .unwrap();
        assert_eq!(artifact.byte_length, 5);
        assert_eq!(
            artifact.sha256,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
        assert!(store.session_dir().join("artifacts").exists());
    }

    #[test]
    fn first_approval_explainer_latch_defaults_unset_then_persists() {
        let dir = tempdir().unwrap();
        let store = HistoryStore::create(dir.path()).unwrap();

        // Fresh install: latch unset.
        assert!(!store.first_approval_explainer_shown());

        store.mark_first_approval_explainer_shown().unwrap();
        assert!(store.first_approval_explainer_shown());

        // Persists across sessions: a new store on the same workspace root (a
        // fresh session dir) still sees the latch — the flag lives at the root.
        let next_session = HistoryStore::create(dir.path()).unwrap();
        assert!(next_session.first_approval_explainer_shown());
        // And it survives session cleanup (the flag is not under sessions/).
        clean_sessions(dir.path()).unwrap();
        let after_clean = HistoryStore::create(dir.path()).unwrap();
        assert!(after_clean.first_approval_explainer_shown());
    }

    #[test]
    fn marking_first_approval_explainer_is_idempotent() {
        let dir = tempdir().unwrap();
        let store = HistoryStore::create(dir.path()).unwrap();
        store.mark_first_approval_explainer_shown().unwrap();
        // Repeat calls are a no-op and keep the latch set.
        store.mark_first_approval_explainer_shown().unwrap();
        assert!(store.first_approval_explainer_shown());
    }

    #[test]
    fn first_approval_latch_degrades_to_unset_on_corrupt_file() {
        let dir = tempdir().unwrap();
        let store = HistoryStore::create(dir.path()).unwrap();
        // Latch once so the flag file exists at its real (root-agnostic) path,
        // then corrupt it in place.
        store.mark_first_approval_explainer_shown().unwrap();
        let flag_path = store.ui_state_path();
        fs::write(&flag_path, "not json").unwrap();
        // A corrupt flag file must not panic or error — it reads as not-yet-shown.
        assert!(!store.first_approval_explainer_shown());
    }

    #[test]
    fn cleanup_deletes_only_sessions_and_runs() {
        let dir = tempdir().unwrap();
        let root = dir.path().join(".multiagent");
        fs::create_dir_all(root.join("sessions/a")).unwrap();
        fs::create_dir_all(root.join("runs")).unwrap();
        fs::write(root.join("debug.log"), "keep").unwrap();

        let deleted = clean_sessions(dir.path()).unwrap();
        assert_eq!(deleted.len(), 2);
        assert!(!root.join("sessions").exists());
        assert!(!root.join("runs").exists());
        assert!(root.join("debug.log").exists());
    }
}
