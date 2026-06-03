use crate::config::{Capability, EffectiveConfig};
use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};

pub const JSON_START: &str = "<<<MULTIAGENT_JSON_START>>>";
pub const JSON_END: &str = "<<<MULTIAGENT_JSON_END>>>";
const JSON_START_PREFIX: &str = "<<<MULTIAGENT_JSON_START";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RunState {
    Idle,
    Planning,
    Running,
    WaitingForUser,
    Interrupted,
    Completed,
    Failed,
    LimitReached,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StepState {
    Queued,
    Starting,
    Running,
    WaitingForAction,
    WaitingForApproval,
    Cancelling,
    Completed,
    Failed,
    Cancelled,
    ParseError,
    LimitReached,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DecisionStatus {
    Continue,
    WaitingForUser,
    Complete,
    Failed,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct OrchestratorDecision {
    pub schema_version: u32,
    pub decision_id: String,
    pub run_id: String,
    pub status: DecisionStatus,
    pub plan: Vec<String>,
    pub next_agent: Option<String>,
    pub reason: String,
    pub required_capabilities: Vec<Capability>,
    pub stop_condition: String,
    pub clarifying_question: Option<String>,
    pub final_summary: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentResultStatus {
    Completed,
    Blocked,
    Failed,
    Cancelled,
    ParseError,
    LimitReached,
    ApprovalDenied,
    NoChanges,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArtifactReference {
    pub artifact_id: String,
    pub path: String,
    pub media_type: String,
    pub byte_length: usize,
    pub sha256: String,
    pub redaction_status: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentResult {
    pub schema_version: u32,
    pub agent: String,
    pub step_id: String,
    pub status: AgentResultStatus,
    pub summary: String,
    pub findings: Vec<String>,
    pub changed_files: Vec<String>,
    pub commands: Vec<String>,
    pub verification: Vec<String>,
    pub blocker: Option<String>,
    pub artifacts: Vec<ArtifactReference>,
}

impl AgentResult {
    pub fn completed(
        agent: impl Into<String>,
        step_id: impl Into<String>,
        summary: impl Into<String>,
    ) -> Self {
        Self {
            schema_version: 1,
            agent: agent.into(),
            step_id: step_id.into(),
            status: AgentResultStatus::Completed,
            summary: summary.into(),
            findings: Vec::new(),
            changed_files: Vec::new(),
            commands: Vec::new(),
            verification: Vec::new(),
            blocker: None,
            artifacts: Vec::new(),
        }
    }
}

pub fn parse_orchestrator_decision(text: &str) -> Result<OrchestratorDecision> {
    parse_contract(text)
}

pub fn parse_agent_result(text: &str) -> Result<AgentResult> {
    parse_contract(text)
}

pub fn wrap_json_contract<T: Serialize>(value: &T) -> Result<String> {
    let json = serde_json::to_string_pretty(value)?;
    Ok(format!("{JSON_START}\n{json}\n{JSON_END}"))
}

pub fn parse_contract<T>(text: &str) -> Result<T>
where
    T: for<'de> Deserialize<'de>,
{
    let payload = extract_json_payload(text).ok_or_else(|| anyhow!("missing JSON contract"))?;
    serde_json::from_str(payload).context("failed to parse JSON contract")
}

pub fn extract_json_payload(text: &str) -> Option<&str> {
    if let Some(payload) = extract_delimited_json_payload(text) {
        return Some(payload);
    }

    let trimmed = text.trim();
    if trimmed.starts_with('{') && trimmed.ends_with('}') {
        return Some(trimmed);
    }

    if let Some(fence_start) = trimmed.find("```json") {
        let rest = &trimmed[fence_start + "```json".len()..];
        let fence_end = rest.find("```")?;
        return Some(rest[..fence_end].trim());
    }

    None
}

fn extract_delimited_json_payload(text: &str) -> Option<&str> {
    let start = text.find(JSON_START_PREFIX)?;
    let after_prefix = start + JSON_START_PREFIX.len();
    let after_marker = consume_angle_marker_suffix(text, after_prefix)?;
    let rest = &text[after_marker..];
    let object_start = after_marker + rest.find('{')?;
    let object_end = find_json_object_end(text, object_start)?;
    Some(text[object_start..object_end].trim())
}

fn consume_angle_marker_suffix(text: &str, index: usize) -> Option<usize> {
    let mut current = index;
    let mut count = 0;
    let bytes = text.as_bytes();
    while current < bytes.len() && bytes[current] == b'>' {
        current += 1;
        count += 1;
    }
    (count >= 2).then_some(current)
}

fn find_json_object_end(text: &str, start: usize) -> Option<usize> {
    if text[start..].chars().next()? != '{' {
        return None;
    }

    let mut depth = 0u32;
    let mut in_string = false;
    let mut escaped = false;
    for (offset, ch) in text[start..].char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }

        match ch {
            '"' => in_string = true,
            '{' => depth += 1,
            '}' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(start + offset + ch.len_utf8());
                }
            }
            _ => {}
        }
    }

    None
}

pub fn validate_orchestrator_decision(
    decision: &OrchestratorDecision,
    config: &EffectiveConfig,
) -> Result<()> {
    if decision.schema_version != 1 {
        bail!(
            "unsupported orchestrator decision schema_version {}",
            decision.schema_version
        );
    }

    match decision.status {
        DecisionStatus::Continue => {
            let next_agent = decision
                .next_agent
                .as_ref()
                .ok_or_else(|| anyhow!("continue decision is missing next_agent"))?;
            let agent = config
                .agents
                .get(next_agent)
                .ok_or_else(|| anyhow!("decision references unknown agent {next_agent}"))?;
            if !agent.enabled {
                bail!("decision references disabled agent {next_agent}");
            }
            for capability in &decision.required_capabilities {
                if !agent.has_capability(capability) {
                    bail!(
                        "agent {next_agent} lacks required capability {:?}",
                        capability
                    );
                }
            }
            if decision.stop_condition.trim().is_empty() {
                bail!("continue decision is missing stop_condition");
            }
        }
        DecisionStatus::WaitingForUser => {
            if decision
                .clarifying_question
                .as_deref()
                .unwrap_or("")
                .trim()
                .is_empty()
            {
                bail!("waiting_for_user decision is missing clarifying_question");
            }
        }
        DecisionStatus::Complete => {
            if decision
                .final_summary
                .as_deref()
                .unwrap_or("")
                .trim()
                .is_empty()
            {
                bail!("complete decision is missing final_summary");
            }
        }
        DecisionStatus::Failed => {
            if decision.reason.trim().is_empty() {
                bail!("failed decision is missing reason");
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{load_effective_config, ConfigLoadOptions};
    use tempfile::tempdir;

    #[test]
    fn parses_delimited_orchestrator_decision() {
        let decision = OrchestratorDecision {
            schema_version: 1,
            decision_id: "01".to_string(),
            run_id: "02".to_string(),
            status: DecisionStatus::Continue,
            plan: vec!["Read context".to_string()],
            next_agent: Some("explorer".to_string()),
            reason: "Need repository context before editing.".to_string(),
            required_capabilities: vec![Capability::Read],
            stop_condition: "Context gathered.".to_string(),
            clarifying_question: None,
            final_summary: None,
        };
        let wrapped = wrap_json_contract(&decision).unwrap();
        assert_eq!(parse_orchestrator_decision(&wrapped).unwrap(), decision);
    }

    #[test]
    fn parses_delimiters_with_missing_angle_bracket() {
        let raw = r#"<<<MULTIAGENT_JSON_START>>{
  "schema_version": 1,
  "decision_id": "01KT7K2ZEMTHX0ZZBN19SW03C7-dec-01",
  "run_id": "01KT7K2ZEMTHX0ZZBN19SW03C7",
  "status": "continue",
  "plan": [
    "Read repository metadata/files before drafting the README."
  ],
  "next_agent": "explorer",
  "reason": "The orchestrator lacks read capability; delegate to a read-capable agent.",
  "required_capabilities": ["read"],
  "stop_condition": "Explorer returns sufficient project context.",
  "clarifying_question": null,
  "final_summary": null
}
<<<MULTIAGENT_JSON_END>>"#;

        let decision = parse_orchestrator_decision(raw).unwrap();

        assert_eq!(decision.next_agent.as_deref(), Some("explorer"));
        assert_eq!(decision.required_capabilities, vec![Capability::Read]);
    }

    #[test]
    fn delimited_extraction_respects_braces_inside_strings() {
        let result = AgentResult {
            schema_version: 1,
            agent: "explorer".to_string(),
            step_id: "03".to_string(),
            status: AgentResultStatus::Completed,
            summary: "Found literals like } and { in project docs.".to_string(),
            findings: Vec::new(),
            changed_files: Vec::new(),
            commands: Vec::new(),
            verification: Vec::new(),
            blocker: None,
            artifacts: Vec::new(),
        };
        let raw = format!(
            "prefix\n<<<MULTIAGENT_JSON_START>>\n{}\n<<<MULTIAGENT_JSON_END>>>>\ntrailing",
            serde_json::to_string_pretty(&result).unwrap()
        );

        assert_eq!(parse_agent_result(&raw).unwrap(), result);
    }

    #[test]
    fn validates_required_capability_against_selected_agent() {
        let dir = tempdir().unwrap();
        let config = load_effective_config(ConfigLoadOptions {
            working_directory: dir.path().to_path_buf(),
            config_path: None,
        })
        .unwrap();
        let decision = OrchestratorDecision {
            schema_version: 1,
            decision_id: "01".to_string(),
            run_id: "02".to_string(),
            status: DecisionStatus::Continue,
            plan: vec![],
            next_agent: Some("explorer".to_string()),
            reason: "Need edit.".to_string(),
            required_capabilities: vec![Capability::Edit],
            stop_condition: "Done.".to_string(),
            clarifying_question: None,
            final_summary: None,
        };
        let error = validate_orchestrator_decision(&decision, &config).unwrap_err();
        assert!(error.to_string().contains("lacks required capability"));
    }

    #[test]
    fn parses_agent_result() {
        let result = AgentResult::completed("explorer", "03", "Found files.");
        let wrapped = wrap_json_contract(&result).unwrap();
        assert_eq!(parse_agent_result(&wrapped).unwrap(), result);
    }
}
