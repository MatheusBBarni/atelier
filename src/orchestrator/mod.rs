use crate::config::{AgentProfile, Capability, EffectiveConfig};
use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

pub const JSON_START: &str = "<<<MULTIAGENT_JSON_START>>>";
pub const JSON_END: &str = "<<<MULTIAGENT_JSON_END>>>";
pub const COUNCIL_WORKFLOW_AGENT_ID: &str = "council";
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
    #[serde(default)]
    pub next_agent: Option<String>,
    #[serde(default)]
    pub next_step: Option<DecisionNextStep>,
    pub reason: String,
    pub required_capabilities: Vec<Capability>,
    pub stop_condition: String,
    pub clarifying_question: Option<String>,
    pub final_summary: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum DecisionNextStep {
    SingleAgent(SingleAgentStepPlan),
    ParallelGroup(ParallelGroupPlan),
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SingleAgentStepPlan {
    pub agent: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instruction: Option<String>,
    #[serde(default)]
    pub required_capabilities: Vec<Capability>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ParallelGroupPlan {
    pub group_id: String,
    pub reason: String,
    pub steps: Vec<ParallelChildStepPlan>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ParallelChildStepPlan {
    pub step_label: String,
    pub agent: String,
    pub instruction: String,
    #[serde(default)]
    pub required_capabilities: Vec<Capability>,
    pub file_scope: ParallelFileScope,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct ParallelFileScope {
    pub write_files: Vec<String>,
    pub read_roots: Vec<String>,
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

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum RunStepResult {
    Agent { result: AgentResult },
    ParallelGroup { result: ParallelGroupResult },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ParallelGroupResult {
    pub schema_version: u32,
    pub group_id: String,
    pub run_id: String,
    pub status: ParallelGroupStatus,
    pub summary: String,
    pub children: Vec<ParallelChildResultRef>,
    pub counts: BTreeMap<String, u32>,
    pub changed_files: Vec<String>,
    pub blocked_scopes: Vec<ParallelBlockedScope>,
    pub failed_scopes: Vec<ParallelFailedScope>,
    pub approval_denials: Vec<String>,
    pub started_at: String,
    pub completed_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ParallelGroupStatus {
    Completed,
    CompletedWithIssues,
    Failed,
    Cancelled,
    LimitReached,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ParallelChildResultRef {
    pub step_id: String,
    pub step_label: String,
    pub agent: String,
    pub file_scope: ParallelFileScope,
    pub status: AgentResultStatus,
    pub result_index: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ParallelBlockedScope {
    pub step_id: String,
    pub step_label: String,
    pub agent: String,
    pub file_scope: ParallelFileScope,
    pub blocker: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ParallelFailedScope {
    pub step_id: String,
    pub step_label: String,
    pub agent: String,
    pub file_scope: ParallelFileScope,
    pub diagnostic: String,
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

impl OrchestratorDecision {
    pub fn normalized_next_step(&self) -> Result<Option<DecisionNextStep>> {
        if self.next_agent.is_some() && self.next_step.is_some() {
            bail!("orchestrator decision contains both next_agent and next_step");
        }

        match self.schema_version {
            1 => {
                if let Some(next_step) = &self.next_step {
                    match next_step {
                        DecisionNextStep::SingleAgent(_) => Ok(Some(next_step.clone())),
                        DecisionNextStep::ParallelGroup(_) => {
                            bail!("schema_version 1 cannot select a parallel_group next_step")
                        }
                    }
                } else {
                    Ok(self.next_agent.as_ref().map(|agent| {
                        DecisionNextStep::SingleAgent(SingleAgentStepPlan {
                            agent: agent.clone(),
                            instruction: None,
                            required_capabilities: self.required_capabilities.clone(),
                        })
                    }))
                }
            }
            2 => {
                if self.next_agent.is_some() {
                    bail!("schema_version 2 must use next_step instead of next_agent");
                }
                Ok(self.next_step.clone())
            }
            version => bail!("unsupported orchestrator decision schema_version {version}"),
        }
    }
}

pub fn agent_results(results: &[RunStepResult]) -> impl Iterator<Item = &AgentResult> {
    results.iter().flat_map(|result| match result {
        RunStepResult::Agent { result } => std::slice::from_ref(result),
        RunStepResult::ParallelGroup { .. } => &[],
    })
}

pub fn last_agent_result(results: &[RunStepResult]) -> Option<&AgentResult> {
    agent_results(results).last()
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
    match decision.status {
        DecisionStatus::Continue => {
            if decision.stop_condition.trim().is_empty() {
                bail!("continue decision is missing stop_condition");
            }
            let next_step = decision
                .normalized_next_step()?
                .ok_or_else(|| anyhow!("continue decision is missing next_step"))?;
            validate_decision_next_step(&next_step, config)?;
        }
        DecisionStatus::WaitingForUser => {
            validate_terminal_decision_has_no_next_step(decision)?;
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
            validate_terminal_decision_has_no_next_step(decision)?;
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
            validate_terminal_decision_has_no_next_step(decision)?;
            if decision.reason.trim().is_empty() {
                bail!("failed decision is missing reason");
            }
        }
    }

    Ok(())
}

fn validate_terminal_decision_has_no_next_step(decision: &OrchestratorDecision) -> Result<()> {
    if decision.next_agent.is_some() || decision.next_step.is_some() {
        bail!("non-continue decision must not contain a next step");
    }
    Ok(())
}

fn validate_decision_next_step(
    next_step: &DecisionNextStep,
    config: &EffectiveConfig,
) -> Result<()> {
    match next_step {
        DecisionNextStep::SingleAgent(plan) => validate_single_agent_step_plan(plan, config),
        DecisionNextStep::ParallelGroup(group) => validate_parallel_group_plan(group, config),
    }
}

fn validate_single_agent_step_plan(
    plan: &SingleAgentStepPlan,
    config: &EffectiveConfig,
) -> Result<()> {
    validate_agent_reference(&plan.agent, &plan.required_capabilities, config)
}

pub fn validate_parallel_group_plan(
    group: &ParallelGroupPlan,
    config: &EffectiveConfig,
) -> Result<()> {
    if !config.features.parallel_step_groups {
        bail!("parallel step groups are disabled by features.parallel_step_groups");
    }
    if config.limits.max_parallel_agent_steps == 0 {
        bail!("parallel step groups are disabled by limits.max_parallel_agent_steps = 0");
    }
    if group.group_id.trim().is_empty() {
        bail!("parallel group is missing group_id");
    }
    if group.steps.len() < 2 {
        bail!("parallel group must contain at least two child steps");
    }
    if group.steps.len() > config.limits.max_parallel_agent_steps as usize {
        bail!(
            "parallel group has {} child steps but max_parallel_agent_steps is {}",
            group.steps.len(),
            config.limits.max_parallel_agent_steps
        );
    }

    let mut owners = BTreeMap::<PathBuf, String>::new();
    for child in &group.steps {
        validate_parallel_child_step_plan(child, config)?;
        for write_file in &child.file_scope.write_files {
            let normalized =
                validate_parallel_scope_path(write_file, &config.workspace.extra_write_roots)?;
            if let Some(existing) = owners.insert(normalized, child.step_label.clone()) {
                bail!(
                    "parallel write file {write_file} is assigned to both {existing} and {}",
                    child.step_label
                );
            }
        }
    }

    Ok(())
}

fn validate_parallel_child_step_plan(
    child: &ParallelChildStepPlan,
    config: &EffectiveConfig,
) -> Result<()> {
    if child.step_label.trim().is_empty() {
        bail!("parallel child step is missing step_label");
    }
    if child.instruction.trim().is_empty() {
        bail!(
            "parallel child step {} is missing instruction",
            child.step_label
        );
    }
    validate_agent_reference(&child.agent, &child.required_capabilities, config)?;
    validate_parallel_file_scope(&child.file_scope, config)?;
    let agent = config
        .agents
        .get(&child.agent)
        .ok_or_else(|| anyhow!("decision references unknown agent {}", child.agent))?;
    if agent.has_capability(&Capability::Edit) && child.file_scope.write_files.is_empty() {
        bail!(
            "parallel edit-capable child {} is missing write_files",
            child.step_label
        );
    }
    Ok(())
}

fn validate_parallel_file_scope(scope: &ParallelFileScope, config: &EffectiveConfig) -> Result<()> {
    if scope.write_files.is_empty() && scope.read_roots.is_empty() {
        bail!("parallel file scope must include at least one write_file or read_root");
    }
    let mut seen_write_files = BTreeMap::<PathBuf, ()>::new();
    for write_file in &scope.write_files {
        let normalized =
            validate_parallel_scope_path(write_file, &config.workspace.extra_write_roots)?;
        if seen_write_files.insert(normalized, ()).is_some() {
            bail!("parallel file scope contains duplicate write file {write_file}");
        }
        if parallel_scope_path_is_existing_directory(write_file, config)? {
            bail!("parallel write_files must be exact file paths, not directories: {write_file}");
        }
    }
    for read_root in &scope.read_roots {
        validate_parallel_scope_path(read_root, &config.workspace.extra_read_roots)?;
    }
    Ok(())
}

fn validate_parallel_scope_path(path: &str, extra_roots: &[PathBuf]) -> Result<PathBuf> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        bail!("parallel file scope contains an empty path");
    }
    if trimmed.split('/').any(|component| component == ".") {
        bail!("current-directory components are not allowed in parallel scope: {path}");
    }
    let candidate = Path::new(trimmed);
    if candidate.is_absolute() {
        if extra_roots.iter().any(|root| candidate.starts_with(root)) {
            return Ok(candidate.to_path_buf());
        }
        bail!("absolute paths are not allowed in parallel file scopes: {path}");
    }

    for component in candidate.components() {
        match component {
            Component::ParentDir => {
                bail!("path traversal is not allowed in parallel scope: {path}")
            }
            Component::Prefix(_) | Component::RootDir => {
                bail!("rooted paths are not allowed in parallel scope: {path}")
            }
            Component::CurDir => {
                bail!("current-directory components are not allowed in parallel scope: {path}")
            }
            Component::Normal(_) => {}
        }
    }
    Ok(candidate.to_path_buf())
}

fn parallel_scope_path_is_existing_directory(path: &str, config: &EffectiveConfig) -> Result<bool> {
    let normalized = validate_parallel_scope_path(path, &config.workspace.extra_write_roots)?;
    let resolved = if normalized.is_absolute() {
        normalized
    } else {
        config.working_directory.join(normalized)
    };
    Ok(resolved.is_dir())
}

fn validate_agent_reference(
    agent_id: &str,
    required_capabilities: &[Capability],
    config: &EffectiveConfig,
) -> Result<()> {
    if agent_id == COUNCIL_WORKFLOW_AGENT_ID {
        if config
            .council
            .presets
            .get(&config.council.default_preset)
            .map(BTreeMap::is_empty)
            .unwrap_or(true)
        {
            bail!("council workflow has no configured councillors");
        }
        return Ok(());
    }
    let agent = config
        .agents
        .get(agent_id)
        .ok_or_else(|| anyhow!("decision references unknown agent {agent_id}"))?;
    if !agent.enabled {
        bail!("decision references disabled agent {agent_id}");
    }
    for capability in required_capabilities {
        if !agent.has_capability(capability) {
            bail!(
                "agent {agent_id} lacks required capability {:?}",
                capability
            );
        }
    }
    Ok(())
}

pub fn build_orchestrator_prompt(config: &EffectiveConfig) -> String {
    let base = config
        .agents
        .get("orchestrator")
        .map(|agent| agent.instructions.trim())
        .filter(|instructions| !instructions.is_empty())
        .unwrap_or("Own the run plan and route work through enabled specialized agents.");
    let mut lines = vec![
        base.to_string(),
        String::new(),
        "Enabled specialized agents:".to_string(),
    ];

    let mut enabled_agents = config
        .agents
        .values()
        .filter(|agent| agent.enabled && agent.id != "orchestrator")
        .collect::<Vec<_>>();
    enabled_agents.sort_by(|left, right| left.id.cmp(&right.id));

    if enabled_agents.is_empty() {
        lines.push("- none; ask the user to enable an agent before delegating.".to_string());
    } else {
        for agent in enabled_agents {
            lines.push(agent_routing_line(agent));
        }
    }

    lines.extend([
        String::new(),
        "Available harness workflows:".to_string(),
        format!(
            "- {COUNCIL_WORKFLOW_AGENT_ID}: serial council review using preset `{}`. Use only for architecture, security, data integrity, difficult review, high-risk decisions, or when the user explicitly asks for council review.",
            config.council.default_preset
        ),
    ]);

    lines.extend([
        String::new(),
        "Routing rules:".to_string(),
        "- Delegate only to enabled agents listed above.".to_string(),
        "- Route to council only for high-risk decisions or explicit user council requests."
            .to_string(),
        "- Do not route to disabled or unknown agents.".to_string(),
        "- Keep required_capabilities no broader than the next step needs.".to_string(),
        "- Ask a clarifying question when the next safe step is ambiguous.".to_string(),
        "- Mark the run complete only when the original user request is satisfied.".to_string(),
    ]);
    if config.features.parallel_step_groups && config.limits.max_parallel_agent_steps > 0 {
        lines.extend([
            "- You may return schema_version 2 with next_step.kind = \"parallel_group\" only when child file scopes are exact, disjoint, and safe to run concurrently.".to_string(),
            format!(
                "- Parallel groups may contain at most {} child steps.",
                config.limits.max_parallel_agent_steps
            ),
            "- Every parallel child needs a step_label, agent, instruction, required_capabilities, and file_scope with exact write_files plus read_roots.".to_string(),
            "- Do not put project-wide mutation commands, formatters, dependency installs, code generation, migrations, whole-suite tests, or VCS actions inside a parallel group.".to_string(),
        ]);
    } else {
        lines.push(
            "- Parallel step groups are disabled; return schema_version 1 sequential decisions with next_agent only."
                .to_string(),
        );
    }

    lines.join("\n")
}

fn agent_routing_line(agent: &AgentProfile) -> String {
    let capabilities = agent
        .capabilities
        .iter()
        .map(|capability| format!("{capability:?}").to_ascii_lowercase())
        .collect::<Vec<_>>()
        .join(",");
    let guidance = agent
        .orchestrator_description
        .as_deref()
        .filter(|description| !description.trim().is_empty())
        .map(str::trim)
        .unwrap_or_else(|| default_delegation_guidance(agent));
    format!(
        "- {id} ({name}): capabilities=[{capabilities}], runtime={runtime}, model={model}. Use when: {guidance}",
        id = agent.id,
        name = agent.name,
        runtime = agent.runtime,
        model = agent.model,
    )
}

fn default_delegation_guidance(agent: &AgentProfile) -> &'static str {
    if agent.has_capability(&Capability::Review) {
        "review completed work, verification evidence, and regressions; do not edit files"
    } else if agent.has_capability(&Capability::Edit) {
        "apply scoped implementation changes and verify them through harness-owned actions"
    } else if agent.has_capability(&Capability::Challenge) {
        "challenge architecture, security, data integrity, and high-risk decisions before implementation"
    } else if agent.has_capability(&Capability::Answer) {
        "answer focused questions or research requests from available context"
    } else if agent.has_capability(&Capability::Read) {
        "gather repository context and summarize findings without changing files"
    } else {
        "perform only the explicitly configured capability-bounded work"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{load_effective_config, ConfigLoadOptions};
    use tempfile::tempdir;

    fn parallel_enabled_config(max_parallel_agent_steps: u32) -> crate::config::EffectiveConfig {
        let dir = tempdir().unwrap();
        let mut config = load_effective_config(ConfigLoadOptions {
            working_directory: dir.path().to_path_buf(),
            config_path: None,
        })
        .unwrap();
        config.features.parallel_step_groups = true;
        config.limits.max_parallel_agent_steps = max_parallel_agent_steps;
        config
    }

    fn parallel_child(label: &str, write_file: &str) -> ParallelChildStepPlan {
        ParallelChildStepPlan {
            step_label: label.to_string(),
            agent: "fixer".to_string(),
            instruction: format!("Fix {write_file}."),
            required_capabilities: vec![Capability::Read, Capability::Edit],
            file_scope: ParallelFileScope {
                write_files: vec![write_file.to_string()],
                read_roots: vec!["src".to_string()],
            },
        }
    }

    #[test]
    fn parses_delimited_orchestrator_decision() {
        let decision = OrchestratorDecision {
            schema_version: 1,
            decision_id: "01".to_string(),
            run_id: "02".to_string(),
            status: DecisionStatus::Continue,
            plan: vec!["Read context".to_string()],
            next_agent: Some("explorer".to_string()),
            next_step: None,
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
            next_step: None,
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
    fn normalizes_v1_next_agent_to_single_agent_step() {
        let decision = OrchestratorDecision {
            schema_version: 1,
            decision_id: "01".to_string(),
            run_id: "02".to_string(),
            status: DecisionStatus::Continue,
            plan: Vec::new(),
            next_agent: Some("explorer".to_string()),
            next_step: None,
            reason: "Need context.".to_string(),
            required_capabilities: vec![Capability::Read],
            stop_condition: "Explorer returns context.".to_string(),
            clarifying_question: None,
            final_summary: None,
        };

        let next_step = decision.normalized_next_step().unwrap().unwrap();

        assert!(matches!(
            next_step,
            DecisionNextStep::SingleAgent(SingleAgentStepPlan { agent, .. }) if agent == "explorer"
        ));
    }

    #[test]
    fn rejects_v2_decision_with_both_next_agent_and_next_step() {
        let decision = OrchestratorDecision {
            schema_version: 2,
            decision_id: "01".to_string(),
            run_id: "02".to_string(),
            status: DecisionStatus::Continue,
            plan: Vec::new(),
            next_agent: Some("explorer".to_string()),
            next_step: Some(DecisionNextStep::SingleAgent(SingleAgentStepPlan {
                agent: "explorer".to_string(),
                instruction: None,
                required_capabilities: vec![Capability::Read],
            })),
            reason: "Need context.".to_string(),
            required_capabilities: vec![Capability::Read],
            stop_condition: "Explorer returns context.".to_string(),
            clarifying_question: None,
            final_summary: None,
        };

        let error = decision.normalized_next_step().unwrap_err();

        assert!(error.to_string().contains("both next_agent and next_step"));
    }

    #[test]
    fn rejects_parallel_group_when_feature_disabled() {
        let dir = tempdir().unwrap();
        let config = load_effective_config(ConfigLoadOptions {
            working_directory: dir.path().to_path_buf(),
            config_path: None,
        })
        .unwrap();
        let decision = OrchestratorDecision {
            schema_version: 2,
            decision_id: "01".to_string(),
            run_id: "02".to_string(),
            status: DecisionStatus::Continue,
            plan: Vec::new(),
            next_agent: None,
            next_step: Some(DecisionNextStep::ParallelGroup(ParallelGroupPlan {
                group_id: "group".to_string(),
                reason: "Disjoint scopes.".to_string(),
                steps: vec![
                    ParallelChildStepPlan {
                        step_label: "fix a".to_string(),
                        agent: "fixer".to_string(),
                        instruction: "Fix a.".to_string(),
                        required_capabilities: vec![Capability::Read, Capability::Edit],
                        file_scope: ParallelFileScope {
                            write_files: vec!["src/a.rs".to_string()],
                            read_roots: vec!["src".to_string()],
                        },
                    },
                    ParallelChildStepPlan {
                        step_label: "fix b".to_string(),
                        agent: "fixer".to_string(),
                        instruction: "Fix b.".to_string(),
                        required_capabilities: vec![Capability::Read, Capability::Edit],
                        file_scope: ParallelFileScope {
                            write_files: vec!["src/b.rs".to_string()],
                            read_roots: vec!["src".to_string()],
                        },
                    },
                ],
            })),
            reason: "Use parallel work.".to_string(),
            required_capabilities: Vec::new(),
            stop_condition: "Group joins.".to_string(),
            clarifying_question: None,
            final_summary: None,
        };

        let error = validate_orchestrator_decision(&decision, &config).unwrap_err();

        assert!(error
            .to_string()
            .contains("parallel step groups are disabled"));
    }

    #[test]
    fn rejects_parallel_group_above_configured_limit() {
        let config = parallel_enabled_config(2);
        let group = ParallelGroupPlan {
            group_id: "group".to_string(),
            reason: "Too many parallel children.".to_string(),
            steps: vec![
                parallel_child("fix a", "src/a.rs"),
                parallel_child("fix b", "src/b.rs"),
                parallel_child("fix c", "src/c.rs"),
            ],
        };

        let error = validate_parallel_group_plan(&group, &config).unwrap_err();

        assert!(error.to_string().contains("max_parallel_agent_steps"));
    }

    #[test]
    fn rejects_parallel_group_with_overlapping_write_files() {
        let config = parallel_enabled_config(2);
        let group = ParallelGroupPlan {
            group_id: "group".to_string(),
            reason: "Duplicate write ownership.".to_string(),
            steps: vec![
                parallel_child("fix first", "src/a.rs"),
                parallel_child("fix second", "src/a.rs"),
            ],
        };

        let error = validate_parallel_group_plan(&group, &config).unwrap_err();

        assert!(error.to_string().contains("assigned to both"));
    }

    #[test]
    fn rejects_parallel_group_with_empty_child_scope() {
        let config = parallel_enabled_config(2);
        let group = ParallelGroupPlan {
            group_id: "group".to_string(),
            reason: "Missing scope.".to_string(),
            steps: vec![
                parallel_child("fix first", "src/a.rs"),
                ParallelChildStepPlan {
                    step_label: "review nothing".to_string(),
                    agent: "reviewer".to_string(),
                    instruction: "Review nothing.".to_string(),
                    required_capabilities: vec![Capability::Read, Capability::Review],
                    file_scope: ParallelFileScope {
                        write_files: Vec::new(),
                        read_roots: Vec::new(),
                    },
                },
            ],
        };

        let error = validate_parallel_group_plan(&group, &config).unwrap_err();

        assert!(error
            .to_string()
            .contains("at least one write_file or read_root"));
    }

    #[test]
    fn rejects_parallel_group_with_directory_write_file() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        let mut config = load_effective_config(ConfigLoadOptions {
            working_directory: dir.path().to_path_buf(),
            config_path: None,
        })
        .unwrap();
        config.features.parallel_step_groups = true;
        config.limits.max_parallel_agent_steps = 2;
        let group = ParallelGroupPlan {
            group_id: "group".to_string(),
            reason: "Directory write scope.".to_string(),
            steps: vec![
                parallel_child("fix directory", "src"),
                parallel_child("fix file", "src/a.rs"),
            ],
        };

        let error = validate_parallel_group_plan(&group, &config).unwrap_err();

        assert!(error.to_string().contains("not directories"));
    }

    #[test]
    fn rejects_parallel_group_with_current_dir_scope_component() {
        let config = parallel_enabled_config(2);
        let group = ParallelGroupPlan {
            group_id: "group".to_string(),
            reason: "Ambiguous lexical scope.".to_string(),
            steps: vec![
                parallel_child("fix dotted", "src/./a.rs"),
                parallel_child("fix file", "src/b.rs"),
            ],
        };

        let error = validate_parallel_group_plan(&group, &config).unwrap_err();

        assert!(error.to_string().contains("current-directory components"));
    }

    #[test]
    fn parses_agent_result() {
        let result = AgentResult::completed("explorer", "03", "Found files.");
        let wrapped = wrap_json_contract(&result).unwrap();
        assert_eq!(parse_agent_result(&wrapped).unwrap(), result);
    }

    #[test]
    fn generated_orchestrator_prompt_lists_enabled_custom_agents() {
        let dir = tempdir().unwrap();
        let config_path = dir.path().join("multiagent.toml");
        std::fs::write(
            &config_path,
            r#"
[runtimes.fake]
type = "fake"

[agents.orchestrator]
runtime = "fake"

[agents.explorer]
runtime = "fake"
enabled = false

[agents.docs]
runtime = "fake"
model = "docs-model"
capabilities = ["read", "answer"]
instructions = "Answer documentation questions."
orchestrator_description = "Use for official documentation and API lookup."
"#,
        )
        .unwrap();
        let config = load_effective_config(ConfigLoadOptions {
            working_directory: dir.path().to_path_buf(),
            config_path: Some(config_path),
        })
        .unwrap();

        let prompt = build_orchestrator_prompt(&config);

        assert!(prompt.contains("- docs (Docs):"));
        assert!(prompt.contains("Use for official documentation and API lookup."));
        assert!(!prompt.contains("- explorer (Explorer):"));
    }

    #[test]
    fn generated_orchestrator_prompt_lists_council_workflow_with_routing_limit() {
        let dir = tempdir().unwrap();
        let config = load_effective_config(ConfigLoadOptions {
            working_directory: dir.path().to_path_buf(),
            config_path: None,
        })
        .unwrap();

        let prompt = build_orchestrator_prompt(&config);

        assert!(prompt.contains("- council: serial council review"));
        assert!(prompt.contains("only for high-risk decisions"));
    }

    #[test]
    fn validates_council_as_workflow_target() {
        let dir = tempdir().unwrap();
        let config = load_effective_config(ConfigLoadOptions {
            working_directory: dir.path().to_path_buf(),
            config_path: None,
        })
        .unwrap();
        let decision = OrchestratorDecision {
            schema_version: 1,
            decision_id: "decision".to_string(),
            run_id: "run".to_string(),
            status: DecisionStatus::Continue,
            plan: Vec::new(),
            next_agent: Some(COUNCIL_WORKFLOW_AGENT_ID.to_string()),
            next_step: None,
            reason: "High-risk architecture decision needs council review.".to_string(),
            required_capabilities: vec![Capability::Read, Capability::Challenge],
            stop_condition: "Council returns a recommendation.".to_string(),
            clarifying_question: None,
            final_summary: None,
        };

        validate_orchestrator_decision(&decision, &config).unwrap();
    }

    #[test]
    fn generated_orchestrator_prompt_excludes_disabled_librarian_by_default() {
        let dir = tempdir().unwrap();
        let config = load_effective_config(ConfigLoadOptions {
            working_directory: dir.path().to_path_buf(),
            config_path: None,
        })
        .unwrap();

        let prompt = build_orchestrator_prompt(&config);

        assert!(!prompt.contains("- librarian (Librarian):"));
    }

    #[test]
    fn generated_orchestrator_prompt_excludes_disabled_designer_by_default() {
        let dir = tempdir().unwrap();
        let config = load_effective_config(ConfigLoadOptions {
            working_directory: dir.path().to_path_buf(),
            config_path: None,
        })
        .unwrap();

        let prompt = build_orchestrator_prompt(&config);

        assert!(!prompt.contains("- designer (Designer):"));
    }

    #[test]
    fn generated_orchestrator_prompt_includes_designer_guidance_when_enabled() {
        let dir = tempdir().unwrap();
        let config_path = dir.path().join("multiagent.toml");
        std::fs::write(
            &config_path,
            r#"
[runtimes.fake]
type = "fake"

[agents.designer]
runtime = "fake"
enabled = true
"#,
        )
        .unwrap();
        let config = load_effective_config(ConfigLoadOptions {
            working_directory: dir.path().to_path_buf(),
            config_path: Some(config_path),
        })
        .unwrap();

        let prompt = build_orchestrator_prompt(&config);

        assert!(prompt.contains("- designer (Designer):"));
        assert!(prompt.contains("do not use for backend-only"));
    }
}
