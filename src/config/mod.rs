use anyhow::{anyhow, bail, Context, Result};
use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::borrow::Cow;
use std::collections::BTreeMap;
use std::env;
use std::fmt;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct ConfigLoadOptions {
    pub working_directory: PathBuf,
    pub config_path: Option<PathBuf>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalMode {
    #[default]
    Yolo,
    Normal,
}

/// Posture for the gray-area floor (ADR-002's phased-rollout lever). `Warn`
/// surfaces the risk and records an audit annotation but still runs under `Yolo`;
/// `Enforce` re-prompts instead. This controls ONLY the gray-area tier — the
/// catastrophic core is non-bypassable and not configurable off.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FloorPolicy {
    #[default]
    Warn,
    Enforce,
}

/// Approval-system posture. Today it carries only the gray-area `floor`; the
/// `approval_mode` (`Yolo`/`Normal`) stays a top-level field for back-compat.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ApprovalConfig {
    pub floor: FloorPolicy,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    Plan,
    Read,
    Answer,
    Challenge,
    Edit,
    Command,
    Verify,
    Review,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum ToolName {
    ReadFile,
    ListFiles,
    SearchText,
    RunCommand,
    ApplyPatch,
    WriteFile,
    RecordNote,
}

impl ToolName {
    pub fn all() -> Vec<Self> {
        vec![
            Self::ReadFile,
            Self::ListFiles,
            Self::SearchText,
            Self::RunCommand,
            Self::ApplyPatch,
            Self::WriteFile,
            Self::RecordNote,
        ]
    }

    pub fn required_capability(&self) -> Option<Capability> {
        match self {
            Self::ReadFile | Self::ListFiles | Self::SearchText => Some(Capability::Read),
            Self::RunCommand => Some(Capability::Command),
            Self::ApplyPatch | Self::WriteFile => Some(Capability::Edit),
            Self::RecordNote => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Limit {
    Value(u32),
    Unlimited,
}

impl Limit {
    pub fn default_agent_steps() -> Self {
        Self::Value(12)
    }

    pub fn default_step_actions() -> Self {
        Self::Value(20)
    }

    pub fn default_minutes(value: u32) -> Self {
        Self::Value(value)
    }

    pub fn is_reached_by(&self, value: u32) -> bool {
        match self {
            Self::Value(limit) => value >= *limit,
            Self::Unlimited => false,
        }
    }
}

impl Serialize for Limit {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Value(value) => serializer.serialize_u32(*value),
            Self::Unlimited => serializer.serialize_str("unlimited"),
        }
    }
}

impl<'de> Deserialize<'de> for Limit {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct LimitVisitor;

        impl<'de> Visitor<'de> for LimitVisitor {
            type Value = Limit;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a positive integer or the string \"unlimited\"")
            }

            fn visit_u64<E>(self, value: u64) -> std::result::Result<Self::Value, E>
            where
                E: de::Error,
            {
                if value == 0 {
                    return Err(E::custom(
                        "limit value 0 is invalid; use \"unlimited\" explicitly",
                    ));
                }
                let value =
                    u32::try_from(value).map_err(|_| E::custom("limit value is too large"))?;
                Ok(Limit::Value(value))
            }

            fn visit_i64<E>(self, value: i64) -> std::result::Result<Self::Value, E>
            where
                E: de::Error,
            {
                if value <= 0 {
                    return Err(E::custom("limit values must be positive"));
                }
                let value =
                    u32::try_from(value).map_err(|_| E::custom("limit value is too large"))?;
                Ok(Limit::Value(value))
            }

            fn visit_str<E>(self, value: &str) -> std::result::Result<Self::Value, E>
            where
                E: de::Error,
            {
                if value.eq_ignore_ascii_case("unlimited") {
                    Ok(Limit::Unlimited)
                } else {
                    Err(E::custom(
                        "only \"unlimited\" is accepted as a string limit",
                    ))
                }
            }
        }

        deserializer.deserialize_any(LimitVisitor)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Limits {
    pub max_agent_steps: Limit,
    pub max_step_actions: Limit,
    pub max_wall_clock_minutes: Limit,
    pub max_step_minutes: Limit,
    pub max_command_minutes: Limit,
    pub max_review_fix_cycles: Limit,
    pub max_parallel_agent_steps: u32,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_agent_steps: Limit::default_agent_steps(),
            max_step_actions: Limit::default_step_actions(),
            max_wall_clock_minutes: Limit::default_minutes(30),
            max_step_minutes: Limit::default_minutes(10),
            max_command_minutes: Limit::default_minutes(10),
            max_review_fix_cycles: Limit::Value(2),
            max_parallel_agent_steps: 2,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Features {
    pub parallel_step_groups: bool,
    /// Pause a first-turn single-agent run that intends to write, so the user
    /// can confirm the interpreted intent before any edit (governance spine,
    /// ADR-004). Off by default.
    pub governance_early_abort: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct UiConfig {
    pub hide_banner: bool,
    /// Enable shell-style ↑/↓ recall of this project's past prompts. On by
    /// default (ADR-002); set `[ui] prompt_history_enabled = false` to disable
    /// the background loader and recall entirely.
    pub prompt_history_enabled: bool,
    /// Upper bound on how many past prompts are retained for recall (ADR-004's
    /// bounded projection). Defaults to 200.
    pub prompt_history_max: usize,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            hide_banner: false,
            prompt_history_enabled: true,
            prompt_history_max: 200,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspacePolicy {
    pub extra_read_roots: Vec<PathBuf>,
    pub extra_write_roots: Vec<PathBuf>,
    /// Opt-in (`[workspace] allow_unrestricted_reads = true`): let the model
    /// read any absolute path on the machine. Reads only — writes still require
    /// `extra_write_roots`.
    #[serde(default)]
    pub allow_unrestricted_reads: bool,
}

impl WorkspacePolicy {
    /// Read roots the model may target. When `allow_unrestricted_reads` is set,
    /// the filesystem root is returned so every absolute path is a valid read
    /// root; otherwise only the configured `extra_read_roots` apply. Writes are
    /// unaffected — they always gate on `extra_write_roots`.
    pub fn read_roots(&self) -> Cow<'_, [PathBuf]> {
        if self.allow_unrestricted_reads {
            Cow::Owned(vec![PathBuf::from(std::path::MAIN_SEPARATOR_STR)])
        } else {
            Cow::Borrowed(&self.extra_read_roots)
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeKind {
    Codex,
    Claude,
    Cursor,
    Zai,
    Fake,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PromptMode {
    #[default]
    Stdin,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentEffort {
    Minimal,
    Low,
    #[default]
    Medium,
    High,
    #[serde(rename = "xhigh")]
    XHigh,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CouncilExecutionMode {
    #[default]
    Serial,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeConfig {
    pub id: String,
    pub kind: RuntimeKind,
    pub command: Option<String>,
    pub args: Vec<String>,
    pub prompt_mode: PromptMode,
    pub base_url: Option<String>,
    pub api_key_env: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentPromptMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instructions_file: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instructions_append_file: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub orchestrator_description_file: Option<PathBuf>,
}

impl AgentPromptMetadata {
    pub fn is_empty(&self) -> bool {
        self.instructions_file.is_none()
            && self.instructions_append_file.is_none()
            && self.orchestrator_description_file.is_none()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentProfile {
    pub id: String,
    pub name: String,
    pub runtime: String,
    pub model: String,
    pub model_fallbacks: Vec<String>,
    pub effort: AgentEffort,
    pub thinking: bool,
    pub capabilities: Vec<Capability>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<ToolName>>,
    pub instructions: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub orchestrator_description: Option<String>,
    #[serde(default, skip_serializing_if = "AgentPromptMetadata::is_empty")]
    pub prompt_metadata: AgentPromptMetadata,
    pub enabled: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct CouncilPromptMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_file: Option<PathBuf>,
}

impl CouncilPromptMetadata {
    pub fn is_empty(&self) -> bool {
        self.prompt_file.is_none()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct CouncilMemberProfile {
    pub id: String,
    pub runtime: String,
    pub model: String,
    pub model_fallbacks: Vec<String>,
    pub effort: AgentEffort,
    pub thinking: bool,
    pub prompt: String,
    #[serde(default, skip_serializing_if = "CouncilPromptMetadata::is_empty")]
    pub prompt_metadata: CouncilPromptMetadata,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct CouncilConfig {
    pub default_preset: String,
    pub timeout_seconds: u64,
    pub execution_mode: CouncilExecutionMode,
    pub presets: BTreeMap<String, BTreeMap<String, CouncilMemberProfile>>,
}

impl AgentProfile {
    pub fn has_capability(&self, capability: &Capability) -> bool {
        self.capabilities
            .iter()
            .any(|existing| existing == capability)
    }

    pub fn has_tool(&self, tool: &ToolName) -> bool {
        self.tools
            .as_ref()
            .map(|tools| tools.iter().any(|existing| existing == tool))
            .unwrap_or(true)
    }

    pub fn effective_tools(&self) -> Vec<ToolName> {
        let tools = self.tools.clone().unwrap_or_else(ToolName::all);
        tools
            .into_iter()
            .filter(|tool| {
                tool.required_capability()
                    .map(|capability| self.has_capability(&capability))
                    .unwrap_or(true)
            })
            .collect()
    }

    pub fn model_chain(&self) -> Vec<String> {
        let mut models = Vec::with_capacity(self.model_fallbacks.len() + 1);
        models.push(self.model.clone());
        models.extend(self.model_fallbacks.iter().cloned());
        models
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EffectiveConfig {
    pub schema_version: u32,
    pub working_directory: PathBuf,
    pub config_sources: Vec<PathBuf>,
    pub active_preset: Option<String>,
    pub approval_mode: ApprovalMode,
    pub approval: ApprovalConfig,
    pub workspace: WorkspacePolicy,
    pub features: Features,
    pub ui: UiConfig,
    pub limits: Limits,
    pub council: CouncilConfig,
    pub runtimes: BTreeMap<String, RuntimeConfig>,
    pub agents: BTreeMap<String, AgentProfile>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConfig {
    schema_version: Option<u32>,
    preset: Option<String>,
    approval_mode: Option<ApprovalMode>,
    approval: Option<RawApprovalConfig>,
    workspace: Option<RawWorkspacePolicy>,
    features: Option<RawFeatures>,
    ui: Option<RawUiConfig>,
    limits: Option<RawLimits>,
    council: Option<RawCouncilConfig>,
    runtimes: Option<BTreeMap<String, RawRuntimeConfig>>,
    presets: Option<BTreeMap<String, RawPreset>>,
    agents: Option<BTreeMap<String, RawAgentProfile>>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPreset {
    agents: Option<BTreeMap<String, RawAgentProfile>>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawWorkspacePolicy {
    extra_read_roots: Option<Vec<PathBuf>>,
    extra_write_roots: Option<Vec<PathBuf>>,
    allow_unrestricted_reads: Option<bool>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawApprovalConfig {
    floor: Option<FloorPolicy>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawFeatures {
    parallel_step_groups: Option<bool>,
    governance_early_abort: Option<bool>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawUiConfig {
    hide_banner: Option<bool>,
    prompt_history_enabled: Option<bool>,
    prompt_history_max: Option<usize>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawLimits {
    max_agent_steps: Option<Limit>,
    max_step_actions: Option<Limit>,
    max_wall_clock_minutes: Option<Limit>,
    max_step_minutes: Option<Limit>,
    max_command_minutes: Option<Limit>,
    max_review_fix_cycles: Option<Limit>,
    max_parallel_agent_steps: Option<u32>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCouncilConfig {
    default_preset: Option<String>,
    timeout_seconds: Option<u64>,
    execution_mode: Option<CouncilExecutionMode>,
    presets: Option<BTreeMap<String, BTreeMap<String, RawCouncilMember>>>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCouncilMember {
    runtime: Option<String>,
    model: Option<String>,
    model_fallbacks: Option<Vec<String>>,
    effort: Option<AgentEffort>,
    thinking: Option<bool>,
    prompt: Option<String>,
    prompt_file: Option<PathBuf>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRuntimeConfig {
    #[serde(rename = "type")]
    runtime_type: Option<RuntimeKind>,
    command: Option<String>,
    args: Option<Vec<String>>,
    prompt_mode: Option<PromptMode>,
    base_url: Option<String>,
    api_key_env: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawAgentProfile {
    name: Option<String>,
    display_name: Option<String>,
    runtime: Option<String>,
    model: Option<String>,
    model_fallbacks: Option<Vec<String>>,
    effort: Option<AgentEffort>,
    thinking: Option<bool>,
    capabilities: Option<Vec<Capability>>,
    tools: Option<Vec<ToolName>>,
    instructions: Option<String>,
    instructions_file: Option<PathBuf>,
    instructions_append_file: Option<PathBuf>,
    orchestrator_description: Option<String>,
    orchestrator_description_file: Option<PathBuf>,
    enabled: Option<bool>,
}

#[derive(Clone, Debug)]
enum InstructionSource {
    Inline(String),
    File(PathBuf),
}

impl InstructionSource {
    fn path(&self) -> Option<PathBuf> {
        match self {
            Self::Inline(_) => None,
            Self::File(path) => Some(path.clone()),
        }
    }
}

#[derive(Clone, Debug)]
struct MergedRuntimeConfig {
    kind: Option<RuntimeKind>,
    command: Option<String>,
    args: Option<Vec<String>>,
    prompt_mode: Option<PromptMode>,
    base_url: Option<String>,
    api_key_env: Option<String>,
}

#[derive(Clone, Debug)]
struct MergedAgentProfile {
    name: Option<String>,
    runtime: Option<String>,
    model: Option<String>,
    model_fallbacks: Option<Vec<String>>,
    effort: Option<AgentEffort>,
    thinking: Option<bool>,
    capabilities: Option<Vec<Capability>>,
    tools: Option<Vec<ToolName>>,
    instruction_source: Option<InstructionSource>,
    instruction_append_source: Option<InstructionSource>,
    orchestrator_description_source: Option<InstructionSource>,
    enabled: Option<bool>,
}

#[derive(Clone, Debug)]
struct MergedCouncilConfig {
    default_preset: String,
    timeout_seconds: u64,
    execution_mode: CouncilExecutionMode,
    presets: BTreeMap<String, BTreeMap<String, MergedCouncilMember>>,
}

#[derive(Clone, Debug)]
struct MergedCouncilMember {
    runtime: Option<String>,
    model: Option<String>,
    model_fallbacks: Option<Vec<String>>,
    effort: Option<AgentEffort>,
    thinking: Option<bool>,
    prompt_source: Option<InstructionSource>,
}

#[derive(Clone, Debug)]
struct RawPresetDefinition {
    source_dir: PathBuf,
    source_name: String,
    preset: RawPreset,
}

#[derive(Clone, Debug)]
struct PendingPresetSelection {
    name: String,
    agent_layer_index: usize,
}

#[derive(Clone, Debug)]
struct PendingAgentLayer {
    source_dir: PathBuf,
    source_name: String,
    agents: BTreeMap<String, RawAgentProfile>,
}

#[derive(Clone, Debug)]
struct MergedConfig {
    working_directory: PathBuf,
    config_sources: Vec<PathBuf>,
    active_preset: Option<PendingPresetSelection>,
    approval_mode: ApprovalMode,
    approval: ApprovalConfig,
    workspace: WorkspacePolicy,
    features: Features,
    ui: UiConfig,
    limits: Limits,
    council: MergedCouncilConfig,
    runtimes: BTreeMap<String, MergedRuntimeConfig>,
    presets: BTreeMap<String, RawPresetDefinition>,
    agents: BTreeMap<String, MergedAgentProfile>,
    agent_layers: Vec<PendingAgentLayer>,
}

/// Canonical default system prompts for the six core structured-runtime agents.
///
/// These constants are the single source of truth for the built-in agent defaults
/// (`insert_builtin_agent`) and the generated starter instruction files
/// (`starter_instruction_files`). Keeping both consumers pointed at these constants
/// prevents prompt drift between implicit built-ins and freshly initialized projects.
/// They intentionally do not change any runtime schema, action contract, capability,
/// model default, or permission behavior — only the instruction text.
const DEFAULT_ORCHESTRATOR_INSTRUCTIONS: &str = "\
You are the Orchestrator. You own run planning, agent routing, clarification, and delegation for the structured runtime.\n\
\n\
Contract: obey the runtime-requested structured output contract exactly. Return only the requested orchestrator decision and emit no prose outside the requested JSON envelope.\n\
\n\
Do: choose the next specialist agent from the available capabilities based on the current stop condition; define the next step, required capabilities, reason, and stop condition clearly; ask one targeted clarifying question only when the decision cannot be made safely.\n\
\n\
Do not: edit files, run commands, inspect the repository, or perform specialist work directly; embed action descriptors inside decisions; route to unavailable agents or capabilities.\n\
\n\
Harness actions: you never run them yourself. Route any file, command, edit, or verification work to a specialist agent that requests those harness actions.\n\
\n\
Blockers: report a blocker that names the missing user decision, capability, or context. Stop once the next step is decided, the run is complete, or you are blocked.";

const DEFAULT_EXPLORER_INSTRUCTIONS: &str = "\
You are the Explorer. You own read-only repository and context discovery.\n\
\n\
Contract: obey the runtime-requested structured output contract exactly. Return only the requested result and emit no prose outside the requested JSON envelope.\n\
\n\
Do: request read, list, search, or safe inspection harness actions when repository data is needed; return factual findings with file paths and observed behavior; label uncertainty; identify the next useful files, commands, or decisions when discovery is incomplete.\n\
\n\
Do not: edit files, run modifying commands, or present unverified conclusions as facts.\n\
\n\
Harness actions: you have no direct tool access. Obtain any file read, search, or inspection result by requesting the matching harness action; never claim to have run it yourself.\n\
\n\
Blockers: report a blocker that names the missing file, permission, action result, or context. Stop when discovery for the assigned step is complete or blocked.";

const DEFAULT_FIXER_INSTRUCTIONS: &str = "\
You are the Fixer. You own scoped implementation changes and targeted verification.\n\
\n\
Contract: obey the runtime-requested structured output contract exactly. Return only the requested result and emit no prose outside the requested JSON envelope.\n\
\n\
Do: request reads before editing when file context is missing; request edits through harness actions; request commands for formatting, tests, or verification; report changed files, commands run, verification evidence, and residual blockers in the proper result fields.\n\
\n\
Do not: claim direct tool access; perform unrelated refactors; mark completion without either verification evidence or a specific blocker explaining why verification could not run.\n\
\n\
Harness actions: every file, command, edit, or verification operation must be requested as a harness action; you cannot touch the filesystem or shell directly.\n\
\n\
Blockers: report a blocker that names the missing action result, command output, permission, or decision. Stop when the assigned change is verified, blocked, or failed.";

const DEFAULT_REVIEWER_INSTRUCTIONS: &str = "\
You are the Reviewer. You own risk-first review of completed work.\n\
\n\
Contract: obey the runtime-requested structured output contract exactly. Return only the requested result and emit no prose outside the requested JSON envelope.\n\
\n\
Do: review for bugs, regressions, missing tests, incomplete requirements, and verification gaps; lead with findings ordered by severity; reference files and evidence; state explicitly when no issues are found and name residual risk or test gaps.\n\
\n\
Do not: edit files, take over implementation, or raise vague concerns without concrete evidence or an explicit uncertainty label.\n\
\n\
Harness actions: request any file, command, or verification you need as a harness action; you have no direct tool access and do not modify the work under review.\n\
\n\
Blockers: report a blocker that names the missing diff, file, command result, or context. Stop when the review of the assigned work is complete or blocked.";

const DEFAULT_ORACLE_INSTRUCTIONS: &str = "\
You are the Oracle. You own focused advisory answers drawn from available evidence.\n\
\n\
Contract: obey the runtime-requested structured output contract exactly. Return only the requested result and emit no prose outside the requested JSON envelope.\n\
\n\
Do: answer the narrow question using provided context and any requested reads or searches; label uncertainty and name missing evidence; keep the answer scoped to the question.\n\
\n\
Do not: pretend to have unseen repository, web, or command data; edit files or execute implementation steps; overrule runtime constraints.\n\
\n\
Harness actions: obtain any file or search result you rely on by requesting the matching harness action rather than assuming it; you have no direct tool access.\n\
\n\
Blockers: report a blocker that names the missing evidence, context, or decision required to answer. Stop when the question is answered or blocked.";

const DEFAULT_CONSUL_INSTRUCTIONS: &str = "\
You are the Consul. You own adversarial critique, trade-off analysis, and assumption testing.\n\
\n\
Contract: obey the runtime-requested structured output contract exactly. Return only the requested result and emit no prose outside the requested JSON envelope.\n\
\n\
Do: challenge plans, risks, and assumptions; identify decision trade-offs and failure modes; recommend adjustments while leaving execution ownership with the responsible agent.\n\
\n\
Do not: edit files, execute the plan, or add process overhead when the path is already clear and low risk.\n\
\n\
Harness actions: request any file or search result you need to ground a critique as a harness action; you have no direct tool access and do not implement the work.\n\
\n\
Blockers: report a blocker that names the missing plan detail, evidence, or decision required to critique. Stop when the critique of the assigned plan is complete or blocked.";

impl MergedConfig {
    fn builtin(working_directory: PathBuf) -> Self {
        let mut runtimes = BTreeMap::new();
        runtimes.insert(
            "codex".to_string(),
            MergedRuntimeConfig {
                kind: Some(RuntimeKind::Codex),
                command: Some("codex".to_string()),
                args: Some(Vec::new()),
                prompt_mode: Some(PromptMode::Stdin),
                base_url: None,
                api_key_env: None,
            },
        );
        runtimes.insert(
            "claude".to_string(),
            MergedRuntimeConfig {
                kind: Some(RuntimeKind::Claude),
                command: Some("claude".to_string()),
                args: Some(Vec::new()),
                prompt_mode: Some(PromptMode::Stdin),
                base_url: None,
                api_key_env: None,
            },
        );
        runtimes.insert(
            "cursor".to_string(),
            MergedRuntimeConfig {
                kind: Some(RuntimeKind::Cursor),
                command: Some("cursor-agent".to_string()),
                args: Some(Vec::new()),
                prompt_mode: Some(PromptMode::Stdin),
                base_url: None,
                api_key_env: None,
            },
        );
        runtimes.insert(
            "zai".to_string(),
            MergedRuntimeConfig {
                kind: Some(RuntimeKind::Zai),
                command: None,
                args: None,
                prompt_mode: None,
                base_url: Some("https://api.z.ai/api/paas/v4".to_string()),
                api_key_env: Some("ZAI_API_KEY".to_string()),
            },
        );

        let mut agents = BTreeMap::new();
        insert_builtin_agent(
            &mut agents,
            BuiltinAgent {
                id: "orchestrator",
                name: "Orchestrator",
                runtime: "zai",
                model: "glm-5.1",
                effort: AgentEffort::High,
                thinking: true,
                capabilities: vec![Capability::Plan],
                instructions: DEFAULT_ORCHESTRATOR_INSTRUCTIONS,
                orchestrator_description: None,
                enabled: true,
            },
        );
        insert_builtin_agent(
            &mut agents,
            BuiltinAgent {
                id: "explorer",
                name: "Explorer",
                runtime: "codex",
                model: "default",
                effort: AgentEffort::Medium,
                thinking: false,
                capabilities: vec![Capability::Read],
                instructions: DEFAULT_EXPLORER_INSTRUCTIONS,
                orchestrator_description: None,
                enabled: true,
            },
        );
        insert_builtin_agent(
            &mut agents,
            BuiltinAgent {
                id: "oracle",
                name: "Oracle",
                runtime: "zai",
                model: "glm-5.1",
                effort: AgentEffort::Medium,
                thinking: true,
                capabilities: vec![Capability::Read, Capability::Answer],
                instructions: DEFAULT_ORACLE_INSTRUCTIONS,
                orchestrator_description: None,
                enabled: true,
            },
        );
        insert_builtin_agent(
            &mut agents,
            BuiltinAgent {
                id: "consul",
                name: "Consul",
                runtime: "zai",
                model: "glm-5.1",
                effort: AgentEffort::High,
                thinking: true,
                capabilities: vec![Capability::Read, Capability::Challenge],
                instructions: DEFAULT_CONSUL_INSTRUCTIONS,
                orchestrator_description: None,
                enabled: true,
            },
        );
        insert_builtin_agent(
            &mut agents,
            BuiltinAgent {
                id: "fixer",
                name: "Fixer",
                runtime: "codex",
                model: "default",
                effort: AgentEffort::High,
                thinking: false,
                capabilities: vec![
                    Capability::Read,
                    Capability::Edit,
                    Capability::Command,
                    Capability::Verify,
                ],
                instructions: DEFAULT_FIXER_INSTRUCTIONS,
                orchestrator_description: None,
                enabled: true,
            },
        );
        insert_builtin_agent(
            &mut agents,
            BuiltinAgent {
                id: "reviewer",
                name: "Reviewer",
                runtime: "codex",
                model: "default",
                effort: AgentEffort::High,
                thinking: false,
                capabilities: vec![
                    Capability::Read,
                    Capability::Command,
                    Capability::Verify,
                    Capability::Review,
                ],
                instructions: DEFAULT_REVIEWER_INSTRUCTIONS,
                orchestrator_description: None,
                enabled: true,
            },
        );
        insert_builtin_agent(
            &mut agents,
            BuiltinAgent {
                id: "librarian",
                name: "Librarian",
                runtime: "zai",
                model: "glm-5.1",
                effort: AgentEffort::Medium,
                thinking: true,
                capabilities: vec![Capability::Read, Capability::Answer],
                instructions: "Research current official documentation and APIs. Return cited answers without editing files or running commands.",
                orchestrator_description: Some(
                    "Use for current official documentation, API lookup, and library research; do not use for edits or shell commands.",
                ),
                enabled: false,
            },
        );
        insert_builtin_agent(
            &mut agents,
            BuiltinAgent {
                id: "designer",
                name: "Designer",
                runtime: "codex",
                model: "default",
                effort: AgentEffort::High,
                thinking: false,
                capabilities: vec![Capability::Read, Capability::Edit, Capability::Verify],
                instructions: "Work on user-facing UI and TUI changes through harness actions, preserving accessibility and verification evidence.",
                orchestrator_description: Some(
                    "Use for user-facing UI/TUI implementation and polish; do not use for backend-only or non-visual changes.",
                ),
                enabled: false,
            },
        );

        Self {
            working_directory,
            config_sources: Vec::new(),
            active_preset: None,
            approval_mode: ApprovalMode::Yolo,
            approval: ApprovalConfig::default(),
            workspace: WorkspacePolicy::default(),
            features: Features::default(),
            ui: UiConfig::default(),
            limits: Limits::default(),
            council: builtin_council_config(),
            runtimes,
            presets: BTreeMap::new(),
            agents,
            agent_layers: Vec::new(),
        }
    }

    fn apply_raw(&mut self, raw: RawConfig, source_dir: &Path, source_name: &str) -> Result<()> {
        if let Some(version) = raw.schema_version {
            if version != 1 {
                bail!("unsupported schema_version {version} in {source_name}; expected 1");
            }
        }

        if let Some(approval_mode) = raw.approval_mode {
            self.approval_mode = approval_mode;
        }

        if let Some(approval) = raw.approval {
            if let Some(floor) = approval.floor {
                self.approval.floor = floor;
            }
        }

        if let Some(workspace) = raw.workspace {
            if let Some(extra_read_roots) = workspace.extra_read_roots {
                self.workspace.extra_read_roots = extra_read_roots
                    .into_iter()
                    .map(|path| resolve_config_path(source_dir, path))
                    .collect();
            }
            if let Some(extra_write_roots) = workspace.extra_write_roots {
                self.workspace.extra_write_roots = extra_write_roots
                    .into_iter()
                    .map(|path| resolve_config_path(source_dir, path))
                    .collect();
            }
            if let Some(allow_unrestricted_reads) = workspace.allow_unrestricted_reads {
                self.workspace.allow_unrestricted_reads = allow_unrestricted_reads;
            }
        }

        if let Some(features) = raw.features {
            if let Some(value) = features.parallel_step_groups {
                self.features.parallel_step_groups = value;
            }
            if let Some(value) = features.governance_early_abort {
                self.features.governance_early_abort = value;
            }
        }

        if let Some(ui) = raw.ui {
            if let Some(value) = ui.hide_banner {
                self.ui.hide_banner = value;
            }
            if let Some(value) = ui.prompt_history_enabled {
                self.ui.prompt_history_enabled = value;
            }
            if let Some(value) = ui.prompt_history_max {
                self.ui.prompt_history_max = value;
            }
        }

        if let Some(limits) = raw.limits {
            if let Some(value) = limits.max_agent_steps {
                self.limits.max_agent_steps = value;
            }
            if let Some(value) = limits.max_step_actions {
                self.limits.max_step_actions = value;
            }
            if let Some(value) = limits.max_wall_clock_minutes {
                self.limits.max_wall_clock_minutes = value;
            }
            if let Some(value) = limits.max_step_minutes {
                self.limits.max_step_minutes = value;
            }
            if let Some(value) = limits.max_command_minutes {
                self.limits.max_command_minutes = value;
            }
            if let Some(value) = limits.max_review_fix_cycles {
                self.limits.max_review_fix_cycles = value;
            }
            if let Some(value) = limits.max_parallel_agent_steps {
                self.limits.max_parallel_agent_steps = value;
            }
        }

        if let Some(council) = raw.council {
            self.apply_council(council, source_dir, source_name)?;
        }

        if let Some(runtimes) = raw.runtimes {
            for (runtime_id, runtime) in runtimes {
                self.apply_runtime(runtime_id, runtime, source_name)?;
            }
        }

        if let Some(presets) = raw.presets {
            for (preset_name, preset) in presets {
                self.presets.insert(
                    preset_name,
                    RawPresetDefinition {
                        source_dir: source_dir.to_path_buf(),
                        source_name: source_name.to_string(),
                        preset,
                    },
                );
            }
        }

        if let Some(preset) = raw.preset {
            self.active_preset = Some(PendingPresetSelection {
                name: preset,
                agent_layer_index: self.agent_layers.len(),
            });
        }

        if let Some(agents) = raw.agents {
            self.agent_layers.push(PendingAgentLayer {
                source_dir: source_dir.to_path_buf(),
                source_name: source_name.to_string(),
                agents,
            });
        }

        Ok(())
    }

    fn apply_pending_agent_layers(&mut self) -> Result<()> {
        let active_preset = self.active_preset.clone();
        let agent_layers = std::mem::take(&mut self.agent_layers);
        let agent_layer_count = agent_layers.len();

        for (layer_index, layer) in agent_layers.into_iter().enumerate() {
            if let Some(preset) = active_preset
                .as_ref()
                .filter(|preset| preset.agent_layer_index == layer_index)
            {
                self.apply_preset(&preset.name)?;
            }
            self.apply_agent_layer(layer)?;
        }

        if let Some(preset) = active_preset
            .as_ref()
            .filter(|preset| preset.agent_layer_index == agent_layer_count)
        {
            self.apply_preset(&preset.name)?;
        }

        Ok(())
    }

    fn apply_agent_layer(&mut self, layer: PendingAgentLayer) -> Result<()> {
        for (agent_id, agent) in layer.agents {
            self.apply_agent(agent_id, agent, &layer.source_dir, &layer.source_name)?;
        }
        Ok(())
    }

    fn apply_preset(&mut self, preset_name: &str) -> Result<()> {
        let definition = self
            .presets
            .get(preset_name)
            .cloned()
            .ok_or_else(|| anyhow!("selected preset {preset_name} is not defined"))?;
        let source_name = format!("preset {preset_name} in {}", definition.source_name);
        if let Some(agents) = definition.preset.agents {
            for (agent_id, agent) in agents {
                self.apply_agent(agent_id, agent, &definition.source_dir, &source_name)?;
            }
        }
        Ok(())
    }

    fn apply_council(
        &mut self,
        raw: RawCouncilConfig,
        source_dir: &Path,
        source_name: &str,
    ) -> Result<()> {
        if let Some(default_preset) = raw.default_preset {
            if default_preset.trim().is_empty() {
                bail!("council default_preset in {source_name} cannot be empty");
            }
            self.council.default_preset = default_preset;
        }
        if let Some(timeout_seconds) = raw.timeout_seconds {
            if timeout_seconds == 0 {
                bail!("council timeout_seconds in {source_name} must be positive");
            }
            self.council.timeout_seconds = timeout_seconds;
        }
        if let Some(execution_mode) = raw.execution_mode {
            self.council.execution_mode = execution_mode;
        }
        if let Some(presets) = raw.presets {
            for (preset_name, members) in presets {
                for (member_id, member) in members {
                    self.apply_council_member(
                        &preset_name,
                        member_id,
                        member,
                        source_dir,
                        source_name,
                    )?;
                }
            }
        }
        Ok(())
    }

    fn apply_council_member(
        &mut self,
        preset_name: &str,
        member_id: String,
        raw: RawCouncilMember,
        source_dir: &Path,
        source_name: &str,
    ) -> Result<()> {
        if raw.prompt.is_some() && raw.prompt_file.is_some() {
            bail!(
                "council member {member_id} in preset {preset_name} in {source_name} sets both prompt and prompt_file"
            );
        }
        let entry = self
            .council
            .presets
            .entry(preset_name.to_string())
            .or_default()
            .entry(member_id)
            .or_insert(MergedCouncilMember {
                runtime: None,
                model: None,
                model_fallbacks: None,
                effort: None,
                thinking: None,
                prompt_source: None,
            });

        if let Some(runtime) = raw.runtime {
            entry.runtime = Some(runtime);
        }
        if let Some(model) = raw.model {
            entry.model = Some(model);
        }
        if let Some(model_fallbacks) = raw.model_fallbacks {
            entry.model_fallbacks = Some(model_fallbacks);
        }
        if let Some(effort) = raw.effort {
            entry.effort = Some(effort);
        }
        if let Some(thinking) = raw.thinking {
            entry.thinking = Some(thinking);
        }
        if let Some(prompt) = raw.prompt {
            entry.prompt_source = Some(InstructionSource::Inline(prompt));
        }
        if let Some(prompt_file) = raw.prompt_file {
            entry.prompt_source = Some(InstructionSource::File(resolve_config_path(
                source_dir,
                prompt_file,
            )));
        }

        Ok(())
    }

    fn apply_runtime(
        &mut self,
        runtime_id: String,
        raw: RawRuntimeConfig,
        source_name: &str,
    ) -> Result<()> {
        let entry = self
            .runtimes
            .entry(runtime_id.clone())
            .or_insert(MergedRuntimeConfig {
                kind: None,
                command: None,
                args: None,
                prompt_mode: None,
                base_url: None,
                api_key_env: None,
            });

        if let Some(kind) = raw.runtime_type {
            if let Some(existing) = &entry.kind {
                if existing != &kind {
                    bail!(
                        "runtime {runtime_id} changes type from {:?} to {:?} in {source_name}",
                        existing,
                        kind
                    );
                }
            }
            entry.kind = Some(kind);
        }
        if let Some(command) = raw.command {
            entry.command = Some(command);
        }
        if let Some(args) = raw.args {
            entry.args = Some(args);
        }
        if let Some(prompt_mode) = raw.prompt_mode {
            entry.prompt_mode = Some(prompt_mode);
        }
        if let Some(base_url) = raw.base_url {
            entry.base_url = Some(base_url.trim_end_matches('/').to_string());
        }
        if let Some(api_key_env) = raw.api_key_env {
            entry.api_key_env = Some(api_key_env);
        }

        Ok(())
    }

    fn apply_agent(
        &mut self,
        agent_id: String,
        raw: RawAgentProfile,
        source_dir: &Path,
        source_name: &str,
    ) -> Result<()> {
        if raw.instructions.is_some() && raw.instructions_file.is_some() {
            bail!("agent {agent_id} in {source_name} sets both instructions and instructions_file");
        }
        if raw.name.is_some() && raw.display_name.is_some() {
            bail!("agent {agent_id} in {source_name} sets both name and display_name");
        }
        if raw.orchestrator_description.is_some() && raw.orchestrator_description_file.is_some() {
            bail!(
                "agent {agent_id} in {source_name} sets both orchestrator_description and orchestrator_description_file"
            );
        }

        let entry = self
            .agents
            .entry(agent_id.clone())
            .or_insert(MergedAgentProfile {
                name: None,
                runtime: None,
                model: None,
                model_fallbacks: None,
                effort: None,
                thinking: None,
                capabilities: None,
                tools: None,
                instruction_source: None,
                instruction_append_source: None,
                orchestrator_description_source: None,
                enabled: None,
            });

        if let Some(name) = raw.display_name.or(raw.name) {
            entry.name = Some(name);
        }
        if let Some(runtime) = raw.runtime {
            entry.runtime = Some(runtime);
        }
        if let Some(model) = raw.model {
            entry.model = Some(model);
        }
        if let Some(model_fallbacks) = raw.model_fallbacks {
            entry.model_fallbacks = Some(model_fallbacks);
        }
        if let Some(effort) = raw.effort {
            entry.effort = Some(effort);
        }
        if let Some(thinking) = raw.thinking {
            entry.thinking = Some(thinking);
        }
        if let Some(capabilities) = raw.capabilities {
            entry.capabilities = Some(capabilities);
        }
        if let Some(tools) = raw.tools {
            entry.tools = Some(tools);
        }
        if let Some(instructions) = raw.instructions {
            entry.instruction_source = Some(InstructionSource::Inline(instructions));
        }
        if let Some(instructions_file) = raw.instructions_file {
            entry.instruction_source = Some(InstructionSource::File(resolve_config_path(
                source_dir,
                instructions_file,
            )));
        }
        if let Some(instructions_append_file) = raw.instructions_append_file {
            entry.instruction_append_source = Some(InstructionSource::File(resolve_config_path(
                source_dir,
                instructions_append_file,
            )));
        }
        if let Some(orchestrator_description) = raw.orchestrator_description {
            entry.orchestrator_description_source =
                Some(InstructionSource::Inline(orchestrator_description));
        }
        if let Some(orchestrator_description_file) = raw.orchestrator_description_file {
            entry.orchestrator_description_source = Some(InstructionSource::File(
                resolve_config_path(source_dir, orchestrator_description_file),
            ));
        }
        if let Some(enabled) = raw.enabled {
            entry.enabled = Some(enabled);
        }

        Ok(())
    }

    fn into_effective(mut self) -> Result<EffectiveConfig> {
        self.apply_pending_agent_layers()?;
        let active_preset = self
            .active_preset
            .as_ref()
            .map(|preset| preset.name.clone());

        let mut runtimes = BTreeMap::new();
        // Sibling runtime ids for near-miss "did you mean?" hints (ADR-004),
        // captured before the map is consumed by the loop below.
        let runtime_ids: Vec<String> = self.runtimes.keys().cloned().collect();
        for (id, runtime) in self.runtimes {
            let kind = runtime.kind.ok_or_else(|| {
                anyhow!(
                    "runtime {id} is missing required field type{}",
                    did_you_mean(&id, runtime_ids.iter().map(String::as_str))
                )
            })?;

            let config = match kind {
                RuntimeKind::Codex => RuntimeConfig {
                    id: id.clone(),
                    kind,
                    command: Some(runtime.command.unwrap_or_else(|| "codex".to_string())),
                    args: runtime.args.unwrap_or_default(),
                    prompt_mode: runtime.prompt_mode.unwrap_or_default(),
                    base_url: None,
                    api_key_env: None,
                },
                RuntimeKind::Claude => {
                    if runtime.api_key_env.is_some() {
                        bail!(
                            "claude runtime {id} cannot set api_key_env; Claude credentials are owned by the Claude CLI"
                        );
                    }
                    let args = runtime.args.unwrap_or_default();
                    validate_claude_runtime_args(&id, &args)?;
                    let prompt_mode = runtime.prompt_mode.unwrap_or_default();
                    match prompt_mode {
                        PromptMode::Stdin => {}
                    }
                    RuntimeConfig {
                        id: id.clone(),
                        kind,
                        command: Some(runtime.command.unwrap_or_else(|| "claude".to_string())),
                        args,
                        prompt_mode,
                        base_url: None,
                        api_key_env: None,
                    }
                }
                RuntimeKind::Cursor => {
                    if runtime.api_key_env.is_some() {
                        bail!(
                            "cursor runtime {id} cannot set api_key_env; Cursor credentials are owned by the Cursor CLI or environment"
                        );
                    }
                    let args = runtime.args.unwrap_or_default();
                    validate_cursor_runtime_args(&id, &args)?;
                    let prompt_mode = runtime.prompt_mode.unwrap_or_default();
                    match prompt_mode {
                        PromptMode::Stdin => {}
                    }
                    RuntimeConfig {
                        id: id.clone(),
                        kind,
                        command: Some(
                            runtime
                                .command
                                .unwrap_or_else(|| "cursor-agent".to_string()),
                        ),
                        args,
                        prompt_mode,
                        base_url: None,
                        api_key_env: None,
                    }
                }
                RuntimeKind::Zai => {
                    let api_key_env = runtime
                        .api_key_env
                        .ok_or_else(|| anyhow!("zai runtime {id} is missing api_key_env"))?;
                    validate_env_reference(&api_key_env)
                        .with_context(|| format!("invalid api_key_env for runtime {id}"))?;
                    RuntimeConfig {
                        id: id.clone(),
                        kind,
                        command: None,
                        args: Vec::new(),
                        prompt_mode: PromptMode::Stdin,
                        base_url: Some(
                            runtime
                                .base_url
                                .unwrap_or_else(|| "https://api.z.ai/api/paas/v4".to_string()),
                        ),
                        api_key_env: Some(api_key_env),
                    }
                }
                RuntimeKind::Fake => RuntimeConfig {
                    id: id.clone(),
                    kind,
                    command: None,
                    args: Vec::new(),
                    prompt_mode: PromptMode::Stdin,
                    base_url: None,
                    api_key_env: None,
                },
            };
            runtimes.insert(id, config);
        }

        let mut agents = BTreeMap::new();
        // Sibling agent ids for near-miss hints, captured before the loop consumes
        // the map (ADR-004).
        let agent_ids: Vec<String> = self.agents.keys().cloned().collect();
        for (id, agent) in self.agents {
            let runtime = agent.runtime.ok_or_else(|| {
                anyhow!(
                    "agent {id} is missing required field runtime{}",
                    did_you_mean(&id, agent_ids.iter().map(String::as_str))
                )
            })?;
            if !runtimes.contains_key(&runtime) {
                bail!(
                    "agent {id} points at undefined runtime {runtime}{}",
                    did_you_mean(&runtime, runtimes.keys().map(String::as_str))
                );
            }

            let model = agent.model.unwrap_or_else(|| "default".to_string());
            validate_model_name(&model).with_context(|| format!("invalid model for agent {id}"))?;
            let model_fallbacks = agent.model_fallbacks.unwrap_or_default();
            validate_model_fallbacks(&id, &model, &model_fallbacks)?;
            let capabilities = agent.capabilities.ok_or_else(|| {
                anyhow!(
                    "agent {id} is missing required field capabilities{}",
                    did_you_mean(&id, agent_ids.iter().map(String::as_str))
                )
            })?;
            let instruction_source = agent
                .instruction_source
                .ok_or_else(|| anyhow!("agent {id} is missing instructions"))?;
            let mut prompt_metadata = AgentPromptMetadata {
                instructions_file: instruction_source.path(),
                instructions_append_file: None,
                orchestrator_description_file: None,
            };
            let mut instructions = read_prompt_source(&instruction_source, "instructions_file")?;
            if let Some(append_source) = agent.instruction_append_source {
                prompt_metadata.instructions_append_file = append_source.path();
                let appended = read_prompt_source(&append_source, "instructions_append_file")?;
                append_instruction_text(&mut instructions, &appended);
            }
            let orchestrator_description = match agent.orchestrator_description_source {
                Some(source) => {
                    prompt_metadata.orchestrator_description_file = source.path();
                    Some(read_prompt_source(
                        &source,
                        "orchestrator_description_file",
                    )?)
                }
                None => None,
            };

            agents.insert(
                id.clone(),
                AgentProfile {
                    id: id.clone(),
                    name: agent.name.unwrap_or_else(|| title_case_id(&id)),
                    runtime,
                    model,
                    model_fallbacks,
                    effort: agent.effort.unwrap_or_default(),
                    thinking: agent.thinking.unwrap_or(false),
                    capabilities,
                    tools: agent.tools,
                    instructions,
                    orchestrator_description,
                    prompt_metadata,
                    enabled: agent.enabled.unwrap_or(true),
                },
            );
        }

        let council = Self::into_effective_council(self.council, &runtimes)?;

        Ok(EffectiveConfig {
            schema_version: 1,
            working_directory: self.working_directory,
            config_sources: self.config_sources,
            active_preset,
            approval_mode: self.approval_mode,
            approval: self.approval,
            workspace: self.workspace,
            features: self.features,
            ui: self.ui,
            limits: self.limits,
            council,
            runtimes,
            agents,
        })
    }

    fn into_effective_council(
        council: MergedCouncilConfig,
        runtimes: &BTreeMap<String, RuntimeConfig>,
    ) -> Result<CouncilConfig> {
        if !council.presets.contains_key(&council.default_preset) {
            bail!(
                "council default_preset {} is not defined",
                council.default_preset
            );
        }

        let mut presets = BTreeMap::new();
        for (preset_name, members) in council.presets {
            let mut effective_members = BTreeMap::new();
            for (member_id, member) in members {
                let runtime = member.runtime.ok_or_else(|| {
                    anyhow!("council member {member_id} in preset {preset_name} is missing runtime")
                })?;
                if !runtimes.contains_key(&runtime) {
                    bail!(
                        "council member {member_id} in preset {preset_name} points at undefined runtime {runtime}"
                    );
                }
                let model = member.model.unwrap_or_else(|| "default".to_string());
                validate_model_name(&model).with_context(|| {
                    format!("invalid model for council member {member_id} in preset {preset_name}")
                })?;
                let model_fallbacks = member.model_fallbacks.unwrap_or_default();
                validate_model_fallbacks(
                    &format!("council.{preset_name}.{member_id}"),
                    &model,
                    &model_fallbacks,
                )?;
                let prompt_source = member.prompt_source.ok_or_else(|| {
                    anyhow!("council member {member_id} in preset {preset_name} is missing prompt")
                })?;
                let prompt_metadata = CouncilPromptMetadata {
                    prompt_file: prompt_source.path(),
                };
                let prompt = read_prompt_source(&prompt_source, "council prompt_file")?;
                effective_members.insert(
                    member_id.clone(),
                    CouncilMemberProfile {
                        id: member_id,
                        runtime,
                        model,
                        model_fallbacks,
                        effort: member.effort.unwrap_or_default(),
                        thinking: member.thinking.unwrap_or(false),
                        prompt,
                        prompt_metadata,
                    },
                );
            }
            presets.insert(preset_name, effective_members);
        }

        Ok(CouncilConfig {
            default_preset: council.default_preset,
            timeout_seconds: council.timeout_seconds,
            execution_mode: council.execution_mode,
            presets,
        })
    }
}

struct BuiltinAgent {
    id: &'static str,
    name: &'static str,
    runtime: &'static str,
    model: &'static str,
    effort: AgentEffort,
    thinking: bool,
    capabilities: Vec<Capability>,
    instructions: &'static str,
    orchestrator_description: Option<&'static str>,
    enabled: bool,
}

struct BuiltinCouncilMember {
    id: &'static str,
    runtime: &'static str,
    model: &'static str,
    effort: AgentEffort,
    thinking: bool,
    prompt: &'static str,
}

fn builtin_council_config() -> MergedCouncilConfig {
    let mut default_members = BTreeMap::new();
    for member in [
        BuiltinCouncilMember {
            id: "architect",
            runtime: "zai",
            model: "glm-5.1",
            effort: AgentEffort::High,
            thinking: true,
            prompt: "Evaluate architecture, maintainability, coupling, and migration risks. Return a concrete recommendation with dissent if important.",
        },
        BuiltinCouncilMember {
            id: "security",
            runtime: "zai",
            model: "glm-5.1",
            effort: AgentEffort::High,
            thinking: true,
            prompt: "Evaluate security, data exposure, dependency, and abuse-case risks. Return concrete blockers and mitigations.",
        },
        BuiltinCouncilMember {
            id: "reviewer",
            runtime: "zai",
            model: "glm-5.1",
            effort: AgentEffort::Medium,
            thinking: true,
            prompt: "Evaluate correctness, testability, rollout risk, and user impact. Return the safest next action.",
        },
    ] {
        default_members.insert(
            member.id.to_string(),
            MergedCouncilMember {
                runtime: Some(member.runtime.to_string()),
                model: Some(member.model.to_string()),
                model_fallbacks: Some(Vec::new()),
                effort: Some(member.effort),
                thinking: Some(member.thinking),
                prompt_source: Some(InstructionSource::Inline(member.prompt.to_string())),
            },
        );
    }

    let mut presets = BTreeMap::new();
    presets.insert("default".to_string(), default_members);

    MergedCouncilConfig {
        default_preset: "default".to_string(),
        timeout_seconds: 900,
        execution_mode: CouncilExecutionMode::Serial,
        presets,
    }
}

fn insert_builtin_agent(agents: &mut BTreeMap<String, MergedAgentProfile>, agent: BuiltinAgent) {
    agents.insert(
        agent.id.to_string(),
        MergedAgentProfile {
            name: Some(agent.name.to_string()),
            runtime: Some(agent.runtime.to_string()),
            model: Some(agent.model.to_string()),
            model_fallbacks: Some(Vec::new()),
            effort: Some(agent.effort),
            thinking: Some(agent.thinking),
            capabilities: Some(agent.capabilities),
            tools: None,
            instruction_source: Some(InstructionSource::Inline(agent.instructions.to_string())),
            instruction_append_source: None,
            orchestrator_description_source: agent
                .orchestrator_description
                .map(|description| InstructionSource::Inline(description.to_string())),
            enabled: Some(agent.enabled),
        },
    );
}

pub fn load_effective_config(options: ConfigLoadOptions) -> Result<EffectiveConfig> {
    let working_directory = options.working_directory.canonicalize().with_context(|| {
        format!(
            "failed to resolve working directory {}",
            options.working_directory.display()
        )
    })?;
    let mut merged = MergedConfig::builtin(working_directory.clone());

    let env_config_path = env::var_os("ATELIER_CONFIG")
        .or_else(|| env::var_os("MULTIAGENT_CONFIG")) // back-compat
        .map(PathBuf::from);
    let config_path = options.config_path.or(env_config_path);

    if let Some(home_config) = config_path {
        // Explicit path (CLI flag or env var): it must exist.
        if home_config.exists() {
            apply_config_file(&mut merged, &home_config)?;
        } else {
            bail!(
                "configured harness configuration file does not exist: {}",
                home_config.display()
            );
        }
    } else if let Some(home_config) =
        first_existing([default_home_config_path(), legacy_home_config_path()])
    {
        // No explicit path: prefer ~/.config/.atelier/atelier.toml, but still
        // load the legacy ~/.config/.multiagent/multiagent.toml if that's all
        // that exists.
        apply_config_file(&mut merged, &home_config)?;
    }

    // Local override: prefer ./atelier.toml, fall back to legacy ./multiagent.toml.
    if let Some(local_config) = first_existing([
        working_directory.join("atelier.toml"),
        working_directory.join("multiagent.toml"),
    ]) {
        apply_config_file(&mut merged, &local_config)?;
    }

    merged.into_effective()
}

/// Legacy home config location, kept so configs written before the `.atelier`
/// rename keep loading without a manual move.
fn legacy_home_config_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".config")
        .join(".multiagent")
        .join("multiagent.toml")
}

fn first_existing<I: IntoIterator<Item = PathBuf>>(candidates: I) -> Option<PathBuf> {
    candidates.into_iter().find(|path| path.exists())
}

/// Format a "; did you mean `x`?" fragment for an unknown config name, or an
/// empty string when no sibling key is within the edit-distance threshold
/// (ADR-004). The unknown name is excluded from the candidates so a typo'd key
/// that is itself present in the merged map can't suggest itself (distance 0).
/// Purely additive: callers append this to errors that already fire.
fn did_you_mean<'a>(unknown: &str, siblings: impl IntoIterator<Item = &'a str>) -> String {
    let candidates = siblings.into_iter().filter(|name| *name != unknown);
    match crate::util::suggest_nearby_name(unknown, candidates) {
        Some(name) => format!("; did you mean `{name}`?"),
        None => String::new(),
    }
}

pub fn default_home_config_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".config")
        .join(".atelier")
        .join("atelier.toml")
}

fn apply_config_file(merged: &mut MergedConfig, path: &Path) -> Result<()> {
    let contents = fs::read_to_string(path)
        .with_context(|| format!("failed to read configuration file {}", path.display()))?;
    let raw: RawConfig = toml::from_str(&contents)
        .with_context(|| format!("failed to parse configuration file {}", path.display()))?;
    let source_dir = path.parent().unwrap_or_else(|| Path::new("."));
    merged.apply_raw(raw, source_dir, &path.display().to_string())?;
    merged
        .config_sources
        .push(path.canonicalize().unwrap_or_else(|_| path.to_path_buf()));
    Ok(())
}

fn resolve_config_path(source_dir: &Path, path: PathBuf) -> PathBuf {
    if path.is_absolute() {
        path
    } else {
        source_dir.join(path)
    }
}

fn read_prompt_source(source: &InstructionSource, file_label: &str) -> Result<String> {
    match source {
        InstructionSource::Inline(value) => Ok(value.clone()),
        InstructionSource::File(path) => fs::read_to_string(path)
            .with_context(|| format!("failed to read {file_label} {}", path.display())),
    }
}

fn append_instruction_text(base: &mut String, appended: &str) {
    if !base.ends_with('\n') {
        base.push('\n');
    }
    base.push_str(appended);
}

fn validate_model_name(model: &str) -> Result<()> {
    if model.trim().is_empty() {
        bail!("model cannot be empty");
    }
    Ok(())
}

fn validate_model_fallbacks(agent_id: &str, model: &str, fallbacks: &[String]) -> Result<()> {
    for fallback in fallbacks {
        validate_model_name(fallback)
            .with_context(|| format!("agent {agent_id} has an invalid fallback model"))?;
        if fallback == model {
            bail!("agent {agent_id} repeats its primary model in model_fallbacks");
        }
    }
    Ok(())
}

fn validate_env_reference(value: &str) -> Result<()> {
    if value.is_empty() {
        bail!("credential reference cannot be empty");
    }
    if value.contains('=') || value.starts_with("sk-") || value.starts_with("zai-") {
        bail!("credential reference looks like a raw secret");
    }
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        bail!("credential reference cannot be empty");
    };
    if !(first == '_' || first.is_ascii_alphabetic()) {
        bail!("credential reference must start with a letter or underscore");
    }
    if !chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric()) {
        bail!("credential reference must be an environment variable name");
    }
    Ok(())
}

pub(crate) fn validate_claude_runtime_args(runtime_id: &str, args: &[String]) -> Result<()> {
    for arg in args {
        if let Some(flag) = claude_protected_arg(arg) {
            bail!(
                "claude runtime {runtime_id} args include protected flag {flag}; the Claude Runtime owns tool, session, prompt, model, budget, and output protocol flags"
            );
        }
    }
    Ok(())
}

pub(crate) fn validate_cursor_runtime_args(runtime_id: &str, args: &[String]) -> Result<()> {
    for arg in args {
        if let Some(flag) = cursor_protected_arg(arg) {
            bail!(
                "cursor runtime {runtime_id} args include protected flag {flag}; the Cursor Runtime owns policy, auth-secret, session, prompt, model, and output protocol flags"
            );
        }
    }
    Ok(())
}

fn claude_protected_arg(arg: &str) -> Option<String> {
    let flag = arg
        .split_once('=')
        .map(|(flag, _value)| flag)
        .unwrap_or(arg);
    if CLAUDE_PROTECTED_ARG_NAMES.contains(&flag)
        || CLAUDE_PROTECTED_ARG_PREFIXES
            .iter()
            .any(|prefix| flag.starts_with(prefix))
    {
        Some(flag.to_string())
    } else {
        None
    }
}

fn cursor_protected_arg(arg: &str) -> Option<String> {
    let flag = arg
        .split_once('=')
        .map(|(flag, _value)| flag)
        .unwrap_or(arg);
    if CURSOR_PROTECTED_ARG_NAMES.contains(&flag)
        || CURSOR_PROTECTED_ARG_PREFIXES
            .iter()
            .any(|prefix| flag.starts_with(prefix))
    {
        Some(flag.to_string())
    } else {
        None
    }
}

const CLAUDE_PROTECTED_ARG_NAMES: &[&str] = &[
    "-p",
    "--print",
    "--output-format",
    "--include-partial-messages",
    "--no-session-persistence",
    "--tools",
    "--allowedTools",
    "--allowed-tools",
    "--disallowedTools",
    "--disallowed-tools",
    "--system-prompt",
    "--system-prompt-file",
    "--append-system-prompt",
    "--append-system-prompt-file",
    "--exclude-dynamic-system-prompt-sections",
    "--model",
    "-m",
    "--fallback-model",
    "--max-turns",
    "--max-budget-usd",
    "--continue",
    "-c",
    "--resume",
    "-r",
    "--session-id",
    "--fork-session",
    "--from-pr",
    "--teleport",
    "--mcp-config",
    "--strict-mcp-config",
    "--plugin-dir",
    "--plugin-url",
    "--channels",
    "--setting-sources",
    "--settings",
    "--add-dir",
    "--worktree",
    "-w",
    "--exec",
    "--bg",
    "--remote",
    "--remote-control",
    "--rc",
    "--chrome",
    "--no-chrome",
    "--browser",
    "--permission-mode",
    "--permission-prompt-tool",
    "--dangerously-skip-permissions",
    "--init",
    "--init-only",
    "--maintenance",
    "--include-hook-events",
    "--agent",
    "--agents",
    "--input-format",
    "--prompt",
    "--prompt-file",
    "--replay-user-messages",
    "--prompt-suggestions",
    "--json-schema",
    "--debug-file",
];

const CLAUDE_PROTECTED_ARG_PREFIXES: &[&str] = &[
    "--plugin-",
    "--mcp-",
    "--tool-",
    "--allowed-tool",
    "--disallowed-tool",
    "--permission-",
    "--session-",
    "--system-prompt",
    "--append-system-prompt",
];

const CURSOR_PROTECTED_ARG_NAMES: &[&str] = &[
    "-p",
    "--print",
    "--output-format",
    "--model",
    "-m",
    "--api-key",
    "-a",
    "--force",
    "-f",
    "resume",
    "--resume",
    "ls",
    "-b",
    "--background",
    "--fullscreen",
];

const CURSOR_PROTECTED_ARG_PREFIXES: &[&str] = &[
    "--api-key",
    "--resume",
    "--model",
    "--output-format",
    "--force",
];

fn title_case_id(id: &str) -> String {
    let mut chars = id.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => "Agent".to_string(),
    }
}

/// Redacted, serializable projection of [`EffectiveConfig`] used by `--print-config`
/// (via [`to_redacted_toml`]) and the docs generator (`src/docgen`). Field visibility is
/// `pub(crate)` so `docgen` can read the same redacted view it renders to Markdown.
#[derive(Clone, Debug, Serialize)]
pub(crate) struct PrintableConfig {
    pub(crate) schema_version: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) preset: Option<String>,
    pub(crate) approval_mode: ApprovalMode,
    pub(crate) workspace: WorkspacePolicy,
    pub(crate) features: Features,
    pub(crate) approval: ApprovalConfig,
    pub(crate) ui: UiConfig,
    pub(crate) limits: Limits,
    pub(crate) council: PrintableCouncilConfig,
    pub(crate) runtimes: BTreeMap<String, PrintableRuntime>,
    pub(crate) agents: BTreeMap<String, PrintableAgent>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct PrintableRuntime {
    #[serde(rename = "type")]
    pub(crate) kind: RuntimeKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) command: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) args: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) prompt_mode: Option<PromptMode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) base_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) api_key_env: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct PrintableAgent {
    pub(crate) display_name: String,
    pub(crate) runtime: String,
    pub(crate) model: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) model_fallbacks: Vec<String>,
    pub(crate) effort: AgentEffort,
    pub(crate) thinking: bool,
    pub(crate) capabilities: Vec<Capability>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) tools: Option<Vec<ToolName>>,
    pub(crate) prompt_source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) instructions_file: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) instructions_append_file: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) orchestrator_description_file: Option<PathBuf>,
    pub(crate) enabled: bool,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct PrintableCouncilConfig {
    pub(crate) default_preset: String,
    pub(crate) timeout_seconds: u64,
    pub(crate) execution_mode: CouncilExecutionMode,
    pub(crate) presets: BTreeMap<String, BTreeMap<String, PrintableCouncilMember>>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct PrintableCouncilMember {
    pub(crate) runtime: String,
    pub(crate) model: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) model_fallbacks: Vec<String>,
    pub(crate) effort: AgentEffort,
    pub(crate) thinking: bool,
    pub(crate) prompt_source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) prompt_file: Option<PathBuf>,
}

/// Builds the redacted [`PrintableConfig`] projection of `config`. This is the single
/// place the `EffectiveConfig → PrintableConfig` mapping lives; both [`to_redacted_toml`]
/// (for `--print-config`) and the docs generator reuse it so the redacted view stays
/// identical across surfaces. Preserves every redaction invariant: env-var names (not
/// secrets), `prompt_source` labels, authored-args-only runtimes, and prompt-file paths
/// without bodies.
pub(crate) fn build_printable_config(config: &EffectiveConfig) -> PrintableConfig {
    PrintableConfig {
        schema_version: config.schema_version,
        preset: config.active_preset.clone(),
        approval_mode: config.approval_mode.clone(),
        workspace: config.workspace.clone(),
        features: config.features.clone(),
        approval: config.approval.clone(),
        ui: config.ui.clone(),
        limits: config.limits.clone(),
        council: PrintableCouncilConfig {
            default_preset: config.council.default_preset.clone(),
            timeout_seconds: config.council.timeout_seconds,
            execution_mode: config.council.execution_mode.clone(),
            presets: config
                .council
                .presets
                .iter()
                .map(|(preset, members)| {
                    (
                        preset.clone(),
                        members
                            .iter()
                            .map(|(id, member)| {
                                (
                                    id.clone(),
                                    PrintableCouncilMember {
                                        runtime: member.runtime.clone(),
                                        model: member.model.clone(),
                                        model_fallbacks: member.model_fallbacks.clone(),
                                        effort: member.effort.clone(),
                                        thinking: member.thinking,
                                        prompt_source: if member
                                            .prompt_metadata
                                            .prompt_file
                                            .is_some()
                                        {
                                            "file".to_string()
                                        } else {
                                            "inline_redacted".to_string()
                                        },
                                        prompt_file: member.prompt_metadata.prompt_file.clone(),
                                    },
                                )
                            })
                            .collect(),
                    )
                })
                .collect(),
        },
        runtimes: config
            .runtimes
            .iter()
            .map(|(id, runtime)| {
                (
                    id.clone(),
                    PrintableRuntime {
                        kind: runtime.kind.clone(),
                        command: runtime.command.clone(),
                        args: runtime.args.clone(),
                        prompt_mode: match runtime.kind {
                            RuntimeKind::Codex | RuntimeKind::Claude | RuntimeKind::Cursor => {
                                Some(runtime.prompt_mode.clone())
                            }
                            _ => None,
                        },
                        base_url: runtime.base_url.clone(),
                        api_key_env: runtime.api_key_env.clone(),
                    },
                )
            })
            .collect(),
        agents: config
            .agents
            .iter()
            .map(|(id, agent)| {
                (
                    id.clone(),
                    PrintableAgent {
                        display_name: agent.name.clone(),
                        runtime: agent.runtime.clone(),
                        model: agent.model.clone(),
                        model_fallbacks: agent.model_fallbacks.clone(),
                        effort: agent.effort.clone(),
                        thinking: agent.thinking,
                        capabilities: agent.capabilities.clone(),
                        tools: agent.tools.clone(),
                        prompt_source: prompt_source_label(&agent.prompt_metadata),
                        instructions_file: agent.prompt_metadata.instructions_file.clone(),
                        instructions_append_file: agent
                            .prompt_metadata
                            .instructions_append_file
                            .clone(),
                        orchestrator_description_file: agent
                            .prompt_metadata
                            .orchestrator_description_file
                            .clone(),
                        enabled: agent.enabled,
                    },
                )
            })
            .collect(),
    }
}

/// Renders the redacted effective configuration as TOML for `--print-config`. Thin
/// serialize-only wrapper over [`build_printable_config`]; output is byte-identical to the
/// previous inlined implementation.
pub fn to_redacted_toml(config: &EffectiveConfig) -> Result<String> {
    let printable = build_printable_config(config);
    toml::to_string_pretty(&printable).context("failed to render effective configuration")
}

fn prompt_source_label(metadata: &AgentPromptMetadata) -> String {
    if metadata.instructions_file.is_some() {
        "file".to_string()
    } else {
        "inline_redacted".to_string()
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct InitConfigSummary {
    pub config_path: PathBuf,
    pub created: Vec<PathBuf>,
    pub skipped: Vec<PathBuf>,
}

pub fn init_config(config_path: Option<PathBuf>) -> Result<InitConfigSummary> {
    let config_path = config_path.unwrap_or_else(default_home_config_path);
    let config_dir = config_path
        .parent()
        .ok_or_else(|| anyhow!("config path has no parent: {}", config_path.display()))?;
    create_private_dir(config_dir)?;

    let mut created = Vec::new();
    let mut skipped = Vec::new();

    let agents_dir = config_dir.join("agents");
    create_private_dir(&agents_dir)?;

    let starter_config = starter_config_text();
    write_private_file_if_missing(
        &config_path,
        starter_config.as_bytes(),
        &mut created,
        &mut skipped,
    )?;

    for (name, instructions) in starter_instruction_files() {
        let path = agents_dir.join(format!("{name}.md"));
        write_private_file_if_missing(&path, instructions.as_bytes(), &mut created, &mut skipped)?;
    }

    Ok(InitConfigSummary {
        config_path,
        created,
        skipped,
    })
}

fn starter_config_text() -> String {
    r#"schema_version = 1
approval_mode = "yolo"

[features]
parallel_step_groups = false

# Approval floor posture for gray-area actions (ADR-002). "warn" (default)
# surfaces the risk but still auto-runs under yolo; "enforce" re-prompts
# instead. The catastrophic core always prompts and cannot be disabled here.
# [approval]
# floor = "warn"

# Optional UI tweaks. Uncomment to adjust the input experience.
# [ui]
# hide_banner = true             # suppress the welcome banner
# prompt_history_enabled = true  # ↑/↓ recall of past prompts (on by default)
# prompt_history_max = 200       # how many past prompts to keep for recall

[runtimes.codex]
type = "codex"
command = "codex"
args = ["exec", "--skip-git-repo-check", "--color", "never"]
prompt_mode = "stdin"

[runtimes.claude]
type = "claude"
command = "claude"
args = []
prompt_mode = "stdin"

[runtimes.cursor]
type = "cursor"
command = "cursor-agent"
args = []
prompt_mode = "stdin"

[runtimes.zai]
type = "zai"
base_url = "https://api.z.ai/api/paas/v4"
api_key_env = "ZAI_API_KEY"

[limits]
max_agent_steps = 12
max_step_actions = 20
max_wall_clock_minutes = 30
max_step_minutes = 10
max_command_minutes = 10
max_review_fix_cycles = 2
max_parallel_agent_steps = 2

[council]
default_preset = "default"
timeout_seconds = 900
execution_mode = "serial"

[council.presets.default.architect]
runtime = "zai"
model = "glm-5.1"
effort = "high"
thinking = true
prompt_file = "agents/council-architect.md"

[council.presets.default.security]
runtime = "zai"
model = "glm-5.1"
effort = "high"
thinking = true
prompt_file = "agents/council-security.md"

[council.presets.default.reviewer]
runtime = "zai"
model = "glm-5.1"
effort = "medium"
thinking = true
prompt_file = "agents/council-reviewer.md"

[agents.orchestrator]
runtime = "zai"
model = "glm-5.1"
effort = "high"
thinking = true
capabilities = ["plan"]
instructions_file = "agents/orchestrator.md"

[agents.explorer]
runtime = "codex"
model = "default"
effort = "medium"
thinking = false
capabilities = ["read"]
instructions_file = "agents/explorer.md"

[agents.oracle]
runtime = "zai"
model = "glm-5.1"
effort = "medium"
thinking = true
capabilities = ["read", "answer"]
instructions_file = "agents/oracle.md"

[agents.consul]
runtime = "zai"
model = "glm-5.1"
effort = "high"
thinking = true
capabilities = ["read", "challenge"]
instructions_file = "agents/consul.md"

[agents.fixer]
runtime = "codex"
model = "default"
effort = "high"
thinking = false
capabilities = ["read", "edit", "command", "verify"]
instructions_file = "agents/fixer.md"

[agents.reviewer]
runtime = "codex"
model = "default"
effort = "high"
thinking = false
capabilities = ["read", "command", "verify", "review"]
instructions_file = "agents/reviewer.md"

[agents.librarian]
runtime = "zai"
model = "glm-5.1"
effort = "medium"
thinking = true
capabilities = ["read", "answer"]
instructions_file = "agents/librarian.md"
enabled = false

[agents.designer]
runtime = "codex"
model = "default"
effort = "high"
thinking = false
capabilities = ["read", "edit", "verify"]
instructions_file = "agents/designer.md"
orchestrator_description = "Use for user-facing UI/TUI implementation and polish; do not use for backend-only or non-visual changes."
enabled = false
"#
    .to_string()
}

fn starter_instruction_files() -> Vec<(&'static str, &'static str)> {
    vec![
        ("orchestrator", DEFAULT_ORCHESTRATOR_INSTRUCTIONS),
        ("explorer", DEFAULT_EXPLORER_INSTRUCTIONS),
        ("oracle", DEFAULT_ORACLE_INSTRUCTIONS),
        ("consul", DEFAULT_CONSUL_INSTRUCTIONS),
        ("fixer", DEFAULT_FIXER_INSTRUCTIONS),
        ("reviewer", DEFAULT_REVIEWER_INSTRUCTIONS),
        (
            "librarian",
            "Research current official documentation and APIs. Return cited answers without editing files or running commands.",
        ),
        (
            "designer",
            "Work on user-facing UI and TUI changes through harness actions, preserving accessibility and verification evidence.",
        ),
        (
            "council-architect",
            "Evaluate architecture, maintainability, coupling, and migration risks. Return a concrete recommendation with dissent if important.",
        ),
        (
            "council-security",
            "Evaluate security, data exposure, dependency, and abuse-case risks. Return concrete blockers and mitigations.",
        ),
        (
            "council-reviewer",
            "Evaluate correctness, testability, rollout risk, and user impact. Return the safest next action.",
        ),
    ]
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

fn write_private_file_if_missing(
    path: &Path,
    contents: &[u8],
    created: &mut Vec<PathBuf>,
    skipped: &mut Vec<PathBuf>,
) -> Result<()> {
    if path.exists() {
        skipped.push(path.to_path_buf());
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        create_private_dir(parent)?;
    }
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    let mut file = options
        .open(path)
        .with_context(|| format!("failed to create {}", path.display()))?;
    file.write_all(contents)
        .with_context(|| format!("failed to write {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .with_context(|| format!("failed to set private permissions on {}", path.display()))?;
    }
    created.push(path.to_path_buf());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn load_from_temp(contents: &str) -> Result<EffectiveConfig> {
        let dir = tempdir()?;
        let config_path = dir.path().join("atelier.toml");
        fs::write(&config_path, contents)?;
        load_effective_config(ConfigLoadOptions {
            working_directory: dir.path().to_path_buf(),
            config_path: Some(config_path),
        })
    }

    #[test]
    fn builtin_config_resolves_without_files() {
        let dir = tempdir().unwrap();
        let config_path = dir.path().join("empty-home.toml");
        fs::write(&config_path, "schema_version = 1\n").unwrap();
        let config = load_effective_config(ConfigLoadOptions {
            working_directory: dir.path().to_path_buf(),
            config_path: Some(config_path),
        })
        .unwrap();
        assert!(config.agents.contains_key("orchestrator"));
        assert_eq!(config.limits.max_agent_steps, Limit::Value(12));
        assert_eq!(config.approval_mode, ApprovalMode::Yolo);
        assert_eq!(config.agents["orchestrator"].effort, AgentEffort::High);
        assert!(config.agents["orchestrator"].thinking);
        assert_eq!(config.council.default_preset, "default");
        assert_eq!(config.council.execution_mode, CouncilExecutionMode::Serial);
        assert!(config.council.presets["default"].contains_key("architect"));
    }

    #[test]
    fn builtin_config_includes_opt_in_claude_runtime() {
        let config = load_from_temp("schema_version = 1\n").unwrap();
        let claude = config.runtimes.get("claude").unwrap();

        assert_eq!(claude.kind, RuntimeKind::Claude);
        assert_eq!(claude.command.as_deref(), Some("claude"));
        assert!(claude.args.is_empty());
        assert_eq!(claude.prompt_mode, PromptMode::Stdin);
        assert!(claude.api_key_env.is_none());
        assert!(config
            .agents
            .values()
            .all(|agent| agent.runtime != "claude"));
    }

    #[test]
    fn builtin_config_includes_opt_in_cursor_runtime() {
        let config = load_from_temp("schema_version = 1\n").unwrap();
        let cursor = config.runtimes.get("cursor").unwrap();

        assert_eq!(cursor.kind, RuntimeKind::Cursor);
        assert_eq!(cursor.command.as_deref(), Some("cursor-agent"));
        assert!(cursor.args.is_empty());
        assert_eq!(cursor.prompt_mode, PromptMode::Stdin);
        assert!(cursor.api_key_env.is_none());
        assert!(config
            .agents
            .values()
            .all(|agent| agent.runtime != "cursor"));
    }

    #[test]
    fn default_home_config_path_uses_dot_config_multiagent() {
        let path = default_home_config_path();
        assert!(path.ends_with(Path::new(".config/.atelier/atelier.toml")));
    }

    #[test]
    fn limit_accepts_unlimited_and_rejects_zero() {
        let ok: RawLimits = toml::from_str("max_agent_steps = \"unlimited\"").unwrap();
        assert_eq!(ok.max_agent_steps, Some(Limit::Unlimited));

        let err = toml::from_str::<RawLimits>("max_agent_steps = 0").unwrap_err();
        assert!(format!("{err:#}").contains("positive"));
    }

    #[test]
    fn parallel_feature_defaults_disabled_with_conservative_limit() {
        let config = load_from_temp("schema_version = 1\n").unwrap();

        assert!(!config.features.parallel_step_groups);
        assert_eq!(config.limits.max_parallel_agent_steps, 2);
    }

    #[test]
    fn max_parallel_agent_steps_accepts_zero_as_disable_signal() {
        let config = load_from_temp(
            r#"
[features]
parallel_step_groups = true

[limits]
max_parallel_agent_steps = 0
"#,
        )
        .unwrap();

        assert!(config.features.parallel_step_groups);
        assert_eq!(config.limits.max_parallel_agent_steps, 0);
    }

    #[test]
    fn governance_early_abort_defaults_off_and_parses_true() {
        let default = load_from_temp("schema_version = 1\n").unwrap();
        assert!(!default.features.governance_early_abort);

        let enabled = load_from_temp(
            r#"
[features]
governance_early_abort = true
"#,
        )
        .unwrap();
        assert!(enabled.features.governance_early_abort);
    }

    #[test]
    fn ui_section_absent_defaults_hide_banner_false() {
        let config = load_from_temp("schema_version = 1\n").unwrap();

        assert!(!config.ui.hide_banner);
    }

    #[test]
    fn ui_hide_banner_true_parses() {
        let config = load_from_temp(
            r#"
[ui]
hide_banner = true
"#,
        )
        .unwrap();

        assert!(config.ui.hide_banner);
    }

    #[test]
    fn ui_prompt_history_defaults_on_with_cap_200() {
        // Omitted keys → recall on by default, capped at 200 (ADR-002 / ADR-004).
        let config = load_from_temp("schema_version = 1\n").unwrap();

        assert!(config.ui.prompt_history_enabled);
        assert_eq!(config.ui.prompt_history_max, 200);
    }

    #[test]
    fn ui_prompt_history_can_be_disabled() {
        let config = load_from_temp(
            r#"
[ui]
prompt_history_enabled = false
"#,
        )
        .unwrap();

        assert!(!config.ui.prompt_history_enabled);
        // The cap keeps its default when only the toggle is set.
        assert_eq!(config.ui.prompt_history_max, 200);
    }

    #[test]
    fn ui_prompt_history_max_override() {
        let config = load_from_temp(
            r#"
[ui]
prompt_history_max = 50
"#,
        )
        .unwrap();

        assert_eq!(config.ui.prompt_history_max, 50);
        // The toggle keeps its default when only the cap is set.
        assert!(config.ui.prompt_history_enabled);
    }

    #[test]
    fn ui_prompt_history_local_overrides_home() {
        // Layer precedence (built-in → home → local): home disables recall, the
        // project file re-enables it; local wins.
        let home_dir = tempdir().unwrap();
        let home_path = home_dir.path().join("home.toml");
        fs::write(
            &home_path,
            "schema_version = 1\n[ui]\nprompt_history_enabled = false\n",
        )
        .unwrap();

        let local_dir = tempdir().unwrap();
        fs::write(
            local_dir.path().join("atelier.toml"),
            "[ui]\nprompt_history_enabled = true\n",
        )
        .unwrap();

        let config = load_effective_config(ConfigLoadOptions {
            working_directory: local_dir.path().to_path_buf(),
            config_path: Some(home_path),
        })
        .unwrap();

        assert!(config.ui.prompt_history_enabled);
    }

    #[test]
    fn approval_floor_defaults_to_warn_when_section_absent() {
        // Omitting [approval] preserves today's gray-area behavior (ADR-002).
        let config = load_from_temp("schema_version = 1\n").unwrap();
        assert_eq!(config.approval.floor, FloorPolicy::Warn);
    }

    #[test]
    fn approval_floor_can_be_set_to_enforce() {
        let config =
            load_from_temp("schema_version = 1\n[approval]\nfloor = \"enforce\"\n").unwrap();
        assert_eq!(config.approval.floor, FloorPolicy::Enforce);
    }

    #[test]
    fn approval_floor_local_enforce_overrides_home_warn() {
        // Layer precedence (built-in → home → local): home warns, the project file
        // opts into enforce; local wins.
        let home_dir = tempdir().unwrap();
        let home_path = home_dir.path().join("home.toml");
        fs::write(
            &home_path,
            "schema_version = 1\n[approval]\nfloor = \"warn\"\n",
        )
        .unwrap();

        let local_dir = tempdir().unwrap();
        fs::write(
            local_dir.path().join("atelier.toml"),
            "[approval]\nfloor = \"enforce\"\n",
        )
        .unwrap();

        let config = load_effective_config(ConfigLoadOptions {
            working_directory: local_dir.path().to_path_buf(),
            config_path: Some(home_path),
        })
        .unwrap();

        assert_eq!(config.approval.floor, FloorPolicy::Enforce);
    }

    #[test]
    fn approval_invalid_floor_value_names_the_field() {
        let error = load_from_temp("schema_version = 1\n[approval]\nfloor = \"block\"\n")
            .expect_err("invalid floor value should fail to load");
        let message = format!("{error:#}");
        assert!(
            message.contains("floor"),
            "error should name the floor field, got: {message}"
        );
    }

    #[test]
    fn redacted_toml_includes_approval_floor() {
        let config =
            load_from_temp("schema_version = 1\n[approval]\nfloor = \"enforce\"\n").unwrap();
        let rendered = to_redacted_toml(&config).unwrap();
        assert!(rendered.contains("[approval]"));
        assert!(rendered.contains("floor = \"enforce\""));
    }

    // ---- Near-miss "did you mean?" config hints (task_02) -------------

    #[test]
    fn runtime_missing_type_suggests_nearby_runtime() {
        let error = load_from_temp("schema_version = 1\n[runtimes.codx]\n")
            .expect_err("runtime missing type should fail");
        let message = format!("{error:#}");
        assert!(
            message.contains("did you mean `codex`?"),
            "expected codex hint, got: {message}"
        );
    }

    #[test]
    fn agent_undefined_runtime_suggests_nearby_runtime() {
        let error = load_from_temp("schema_version = 1\n[agents.fixer]\nruntime = \"codx\"\n")
            .expect_err("agent pointing at an undefined runtime should fail");
        let message = format!("{error:#}");
        assert!(
            message.contains("undefined runtime codx"),
            "expected the base error, got: {message}"
        );
        assert!(
            message.contains("did you mean `codex`?"),
            "expected codex hint, got: {message}"
        );
    }

    #[test]
    fn agent_missing_field_suggests_nearby_agent() {
        let error = load_from_temp("schema_version = 1\n[agents.fixr]\n")
            .expect_err("agent missing required fields should fail");
        let message = format!("{error:#}");
        assert!(
            message.contains("did you mean `fixer`?"),
            "expected fixer hint, got: {message}"
        );
    }

    #[test]
    fn valid_custom_runtime_name_loads_with_no_hint() {
        // False-positive lock: an unconventional but valid custom runtime loads
        // cleanly and is present — no error, so no near-miss hint can fire.
        let config =
            load_from_temp("schema_version = 1\n[runtimes.my_custom_thing]\ntype = \"codex\"\n")
                .expect("a valid custom runtime should load");
        assert!(config.runtimes.contains_key("my_custom_thing"));
    }

    #[test]
    fn wild_typo_runtime_gets_no_suggestion() {
        let error = load_from_temp("schema_version = 1\n[runtimes.zzzzzz]\n")
            .expect_err("runtime missing type should fail");
        let message = format!("{error:#}");
        assert!(
            message.contains("runtime zzzzzz is missing required field type"),
            "expected the base error, got: {message}"
        );
        assert!(
            !message.contains("did you mean"),
            "a wild typo must not get a suggestion, got: {message}"
        );
    }

    #[test]
    fn approval_section_cannot_disable_catastrophic_core() {
        // The catastrophic core is intentionally not configurable; any attempt to
        // add such a key is rejected by deny_unknown_fields.
        let error = load_from_temp("schema_version = 1\n[approval]\ncatastrophic = false\n")
            .expect_err("unknown approval keys must be rejected");
        let message = format!("{error:#}");
        assert!(
            message.contains("catastrophic") || message.contains("unknown"),
            "unexpected error: {message}"
        );
    }

    #[test]
    fn ui_prompt_history_max_flows_from_project_toml() {
        // Integration: a project-level multiagent.toml [ui] override reaches the
        // effective config. A hermetic empty home config keeps the developer's
        // real ~/.config out of the result.
        let home_dir = tempdir().unwrap();
        let home_path = home_dir.path().join("home.toml");
        fs::write(&home_path, "schema_version = 1\n").unwrap();

        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("multiagent.toml"),
            "[ui]\nprompt_history_max = 10\n",
        )
        .unwrap();

        let config = load_effective_config(ConfigLoadOptions {
            working_directory: dir.path().to_path_buf(),
            config_path: Some(home_path),
        })
        .unwrap();

        assert_eq!(config.ui.prompt_history_max, 10);
    }

    #[test]
    fn ui_unknown_key_rejected() {
        let error = load_from_temp(
            r#"
[ui]
unknown_key = 1
"#,
        )
        .unwrap_err();

        assert!(format!("{error:#}").contains("unknown_key"));
    }

    #[test]
    fn redacted_toml_includes_ui_section_with_effective_value() {
        let config = load_from_temp(
            r#"
[ui]
hide_banner = true
"#,
        )
        .unwrap();

        let rendered = to_redacted_toml(&config).unwrap();
        assert!(rendered.contains("[ui]"));
        assert!(rendered.contains("hide_banner = true"));
    }

    #[test]
    fn ui_section_merges_over_preset_from_project_file() {
        let config = load_from_temp(
            r#"
preset = "fast"

[ui]
hide_banner = true

[presets.fast.agents.fixer]
model = "preset-model"
"#,
        )
        .unwrap();

        assert_eq!(config.active_preset.as_deref(), Some("fast"));
        assert_eq!(config.agents.get("fixer").unwrap().model, "preset-model");
        assert!(config.ui.hide_banner);
    }

    #[test]
    fn arrays_replace_during_agent_merge() {
        let config = load_from_temp(
            r#"
[agents.fixer]
capabilities = ["read"]
"#,
        )
        .unwrap();
        let fixer = config.agents.get("fixer").unwrap();
        assert_eq!(fixer.capabilities, vec![Capability::Read]);
    }

    #[test]
    fn selected_preset_applies_before_local_agent_overrides() {
        let dir = tempdir().unwrap();
        let home_config = dir.path().join("home.toml");
        fs::write(
            &home_config,
            r#"
preset = "fast"

[presets.fast.agents.fixer]
model = "preset-model"
model_fallbacks = ["preset-fallback"]
effort = "minimal"
"#,
        )
        .unwrap();
        fs::write(
            dir.path().join("atelier.toml"),
            r#"
[agents.fixer]
model = "local-model"
"#,
        )
        .unwrap();

        let config = load_effective_config(ConfigLoadOptions {
            working_directory: dir.path().to_path_buf(),
            config_path: Some(home_config),
        })
        .unwrap();

        let fixer = config.agents.get("fixer").unwrap();
        assert_eq!(config.active_preset.as_deref(), Some("fast"));
        assert_eq!(fixer.model, "local-model");
        assert_eq!(fixer.model_fallbacks, vec!["preset-fallback"]);
        assert_eq!(fixer.effort, AgentEffort::Minimal);
    }

    #[test]
    fn workspace_allow_unrestricted_reads_parses_and_widens_read_roots() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("atelier.toml"),
            r#"
[workspace]
allow_unrestricted_reads = true
"#,
        )
        .unwrap();

        let config = load_effective_config(ConfigLoadOptions {
            working_directory: dir.path().to_path_buf(),
            config_path: None,
        })
        .unwrap();

        assert!(config.workspace.allow_unrestricted_reads);
        // The flag turns the filesystem root into the read root; writes untouched.
        assert_eq!(
            config.workspace.read_roots().to_vec(),
            vec![PathBuf::from(std::path::MAIN_SEPARATOR_STR)]
        );
        assert!(config.workspace.extra_write_roots.is_empty());
    }

    #[test]
    fn legacy_local_multiagent_toml_is_still_loaded() {
        // Configs written before the `.atelier` rename keep working without a
        // manual move: ./multiagent.toml is still discovered as a local override.
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("multiagent.toml"),
            "[workspace]\nallow_unrestricted_reads = true\n",
        )
        .unwrap();

        let config = load_effective_config(ConfigLoadOptions {
            working_directory: dir.path().to_path_buf(),
            config_path: None,
        })
        .unwrap();

        assert!(config.workspace.allow_unrestricted_reads);
    }

    #[test]
    fn workspace_read_roots_default_to_configured_extra_roots() {
        let default = WorkspacePolicy::default();
        assert!(!default.allow_unrestricted_reads);
        assert!(default.read_roots().is_empty());

        let scoped = WorkspacePolicy {
            extra_read_roots: vec![PathBuf::from("/tmp/refs")],
            ..Default::default()
        };
        assert_eq!(
            scoped.read_roots().to_vec(),
            vec![PathBuf::from("/tmp/refs")]
        );
    }

    #[test]
    fn later_preset_selection_does_not_leak_earlier_preset_agent_fields() {
        let dir = tempdir().unwrap();
        let home_config = dir.path().join("home.toml");
        fs::write(
            &home_config,
            r#"
preset = "fast"

[presets.fast.agents.fixer]
model = "fast-model"
model_fallbacks = ["fast-fallback"]
effort = "minimal"

[presets.accurate.agents.fixer]
effort = "high"
"#,
        )
        .unwrap();
        fs::write(
            dir.path().join("atelier.toml"),
            r#"
preset = "accurate"
"#,
        )
        .unwrap();

        let config = load_effective_config(ConfigLoadOptions {
            working_directory: dir.path().to_path_buf(),
            config_path: Some(home_config),
        })
        .unwrap();

        let fixer = config.agents.get("fixer").unwrap();
        assert_eq!(config.active_preset.as_deref(), Some("accurate"));
        assert_eq!(fixer.model, "default");
        assert!(fixer.model_fallbacks.is_empty());
        assert_eq!(fixer.effort, AgentEffort::High);
    }

    #[test]
    fn prompt_append_and_orchestrator_description_files_resolve_from_config_dir() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("agents")).unwrap();
        fs::write(dir.path().join("agents/base.md"), "base instructions").unwrap();
        fs::write(dir.path().join("agents/append.md"), "append instructions").unwrap();
        fs::write(
            dir.path().join("agents/route.md"),
            "route to fixer for edits",
        )
        .unwrap();
        let config_path = dir.path().join("custom.toml");
        fs::write(
            &config_path,
            r#"
[agents.fixer]
instructions_file = "agents/base.md"
instructions_append_file = "agents/append.md"
orchestrator_description_file = "agents/route.md"
"#,
        )
        .unwrap();

        let config = load_effective_config(ConfigLoadOptions {
            working_directory: dir.path().to_path_buf(),
            config_path: Some(config_path),
        })
        .unwrap();

        let fixer = config.agents.get("fixer").unwrap();
        assert_eq!(fixer.instructions, "base instructions\nappend instructions");
        assert_eq!(
            fixer.orchestrator_description.as_deref(),
            Some("route to fixer for edits")
        );
        assert!(fixer
            .prompt_metadata
            .instructions_file
            .as_ref()
            .unwrap()
            .ends_with("agents/base.md"));
        assert!(fixer
            .prompt_metadata
            .instructions_append_file
            .as_ref()
            .unwrap()
            .ends_with("agents/append.md"));
    }

    #[test]
    fn council_prompt_files_resolve_and_print_config_redacts_prompt_bodies() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("council")).unwrap();
        fs::write(
            dir.path().join("council/architect.md"),
            "private council prompt",
        )
        .unwrap();
        let config_path = dir.path().join("custom.toml");
        fs::write(
            &config_path,
            r#"
[runtimes.fake]
type = "fake"

[council]
default_preset = "local"
timeout_seconds = 5
execution_mode = "serial"

[council.presets.local.architect]
runtime = "fake"
model = "default"
prompt_file = "council/architect.md"
"#,
        )
        .unwrap();

        let config = load_effective_config(ConfigLoadOptions {
            working_directory: dir.path().to_path_buf(),
            config_path: Some(config_path),
        })
        .unwrap();
        let architect = &config.council.presets["local"]["architect"];
        let rendered = to_redacted_toml(&config).unwrap();

        assert_eq!(architect.prompt, "private council prompt");
        assert!(rendered.contains("prompt_file"));
        assert!(rendered.contains("council/architect.md"));
        assert!(!rendered.contains("private council prompt"));
    }

    #[test]
    fn runtime_type_cannot_change() {
        let error = load_from_temp(
            r#"
[runtimes.codex]
type = "zai"
api_key_env = "ZAI_API_KEY"
"#,
        )
        .unwrap_err();
        assert!(error.to_string().contains("changes type"));
    }

    #[test]
    fn claude_runtime_type_deserializes() {
        let config = load_from_temp(
            r#"
[runtimes.local_claude]
type = "claude"

[agents.explorer]
runtime = "local_claude"
"#,
        )
        .unwrap();

        assert_eq!(config.runtimes["local_claude"].kind, RuntimeKind::Claude);
        assert_eq!(
            config.runtimes["local_claude"].command.as_deref(),
            Some("claude")
        );
    }

    #[test]
    fn cursor_runtime_type_deserializes() {
        let config = load_from_temp(
            r#"
[runtimes.local_cursor]
type = "cursor"

[agents.explorer]
runtime = "local_cursor"
"#,
        )
        .unwrap();

        assert_eq!(config.runtimes["local_cursor"].kind, RuntimeKind::Cursor);
        assert_eq!(
            config.runtimes["local_cursor"].command.as_deref(),
            Some("cursor-agent")
        );
    }

    #[test]
    fn claude_runtime_rejects_api_key_env() {
        let error = load_from_temp(
            r#"
[runtimes.claude]
api_key_env = "CLAUDE_API_KEY"
"#,
        )
        .unwrap_err();

        assert!(error.to_string().contains("cannot set api_key_env"));
    }

    #[test]
    fn cursor_runtime_rejects_api_key_env() {
        let error = load_from_temp(
            r#"
[runtimes.cursor]
api_key_env = "CURSOR_API_KEY"
"#,
        )
        .unwrap_err();

        assert!(error.to_string().contains("cannot set api_key_env"));
    }

    #[test]
    fn claude_runtime_rejects_protected_args() {
        for flag in [
            "-p",
            "--output-format",
            "--tools",
            "--allowedTools",
            "--system-prompt",
            "--model",
            "--fallback-model",
            "--max-turns",
            "--max-budget-usd",
            "--continue",
            "--resume",
            "--session-id",
            "--mcp-config",
            "--plugin-dir",
            "--setting-sources",
            "--add-dir",
            "--worktree",
            "--permission-mode",
            "--dangerously-skip-permissions",
            "--settings",
            "--include-hook-events",
            "--input-format",
            "--json-schema",
            "--debug-file",
            "--from-pr",
        ] {
            let error = load_from_temp(&format!(
                r#"
[runtimes.claude]
args = ["{flag}"]
"#
            ))
            .unwrap_err();
            assert!(
                error.to_string().contains("protected flag"),
                "flag {flag} produced {error:#}"
            );
        }
    }

    #[test]
    fn claude_runtime_rejects_protected_args_with_values() {
        let error = load_from_temp(
            r#"
[runtimes.claude]
args = ["--tools=Bash"]
"#,
        )
        .unwrap_err();

        assert!(error.to_string().contains("protected flag --tools"));
    }

    #[test]
    fn cursor_runtime_rejects_protected_args() {
        for flag in [
            "-p",
            "--print",
            "--output-format",
            "--model",
            "-m",
            "--api-key",
            "-a",
            "--force",
            "-f",
            "resume",
            "--resume",
            "ls",
            "--background",
            "--fullscreen",
        ] {
            let error = load_from_temp(&format!(
                r#"
[runtimes.cursor]
args = ["{flag}"]
"#
            ))
            .unwrap_err();
            assert!(
                error.to_string().contains("protected flag"),
                "flag {flag} produced {error:#}"
            );
        }
    }

    #[test]
    fn cursor_runtime_rejects_protected_args_with_values() {
        let error = load_from_temp(
            r#"
[runtimes.cursor]
args = ["--api-key=secret"]
"#,
        )
        .unwrap_err();

        assert!(error.to_string().contains("protected flag --api-key"));
    }

    #[test]
    fn instruction_file_resolves_relative_to_declaring_config() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("agents")).unwrap();
        fs::write(
            dir.path().join("agents/fixer.md"),
            "local fixer instructions",
        )
        .unwrap();
        let config_path = dir.path().join("custom.toml");
        fs::write(
            &config_path,
            r#"
[agents.fixer]
instructions_file = "agents/fixer.md"
"#,
        )
        .unwrap();
        let config = load_effective_config(ConfigLoadOptions {
            working_directory: dir.path().to_path_buf(),
            config_path: Some(config_path),
        })
        .unwrap();
        assert_eq!(
            config.agents["fixer"].instructions,
            "local fixer instructions"
        );
    }

    #[test]
    fn raw_secret_in_credential_reference_is_invalid() {
        let error = load_from_temp(
            r#"
[runtimes.zai]
api_key_env = "sk-secret"
"#,
        )
        .unwrap_err();
        assert!(format!("{error:#}").contains("raw secret"));
    }

    #[test]
    fn redacted_toml_contains_env_reference_not_secret() {
        let dir = tempdir().unwrap();
        let config_path = dir.path().join("empty-home.toml");
        fs::write(&config_path, "schema_version = 1\n").unwrap();
        let config = load_effective_config(ConfigLoadOptions {
            working_directory: dir.path().to_path_buf(),
            config_path: Some(config_path),
        })
        .unwrap();
        let rendered = to_redacted_toml(&config).unwrap();
        assert!(rendered.contains("api_key_env = \"ZAI_API_KEY\""));
        assert!(rendered.contains("effort = \"high\""));
        assert!(rendered.contains("thinking = true"));
        assert!(rendered.contains("prompt_source = \"inline_redacted\""));
        // Sentinel: the inline prompt body must never leak into the redacted output.
        // Key it to the live orchestrator default (not a hard-coded phrase) so a future
        // prompt rewrite cannot silently make this assertion vacuous.
        let orchestrator_body = config.agents["orchestrator"].instructions.as_str();
        assert!(!orchestrator_body.is_empty());
        assert!(!rendered.contains(orchestrator_body));
        assert!(!rendered.contains("Bearer"));
    }

    #[test]
    fn build_printable_config_matches_to_redacted_toml() {
        let dir = tempdir().unwrap();
        let config_path = dir.path().join("empty-home.toml");
        fs::write(&config_path, "schema_version = 1\n").unwrap();
        let config = load_effective_config(ConfigLoadOptions {
            working_directory: dir.path().to_path_buf(),
            config_path: Some(config_path),
        })
        .unwrap();

        let via_builder = toml::to_string_pretty(&build_printable_config(&config)).unwrap();
        let via_wrapper = to_redacted_toml(&config).unwrap();

        assert_eq!(via_builder, via_wrapper);
    }

    #[test]
    fn build_printable_config_exposes_all_sections() {
        let dir = tempdir().unwrap();
        let config_path = dir.path().join("empty-home.toml");
        fs::write(&config_path, "schema_version = 1\n").unwrap();
        let config = load_effective_config(ConfigLoadOptions {
            working_directory: dir.path().to_path_buf(),
            config_path: Some(config_path),
        })
        .unwrap();

        let printable = build_printable_config(&config);

        // Agents, runtimes, and council presets are all reachable from the reusable
        // builder for the docs generator (task_10).
        assert!(!printable.agents.is_empty());
        assert!(!printable.runtimes.is_empty());
        assert!(!printable.council.presets.is_empty());
        // The scalar limits/ui/workspace sections are reachable by construction.
        assert!(printable.limits.max_parallel_agent_steps > 0);
        let _ = printable.workspace.extra_read_roots.len();
        let _ = printable.ui.hide_banner;
    }

    #[test]
    fn redacted_toml_prints_claude_authored_args_only() {
        let config = load_from_temp(
            r#"
[runtimes.claude]
args = ["--safe-compat"]
"#,
        )
        .unwrap();

        let rendered = to_redacted_toml(&config).unwrap();

        assert!(rendered.contains("[runtimes.claude]"));
        assert!(rendered.contains("type = \"claude\""));
        assert!(rendered.contains("args = [\"--safe-compat\"]"));
        assert!(rendered.contains("prompt_mode = \"stdin\""));
        assert!(!rendered.contains("stream-json"));
        assert!(!rendered.contains("--include-partial-messages"));
        assert!(!rendered.contains("--no-session-persistence"));
    }

    #[test]
    fn redacted_toml_prints_cursor_authored_args_only() {
        let config = load_from_temp(
            r#"
[runtimes.cursor]
args = ["--safe-compat"]
"#,
        )
        .unwrap();

        let rendered = to_redacted_toml(&config).unwrap();

        assert!(rendered.contains("[runtimes.cursor]"));
        assert!(rendered.contains("type = \"cursor\""));
        assert!(rendered.contains("command = \"cursor-agent\""));
        assert!(rendered.contains("args = [\"--safe-compat\"]"));
        assert!(rendered.contains("prompt_mode = \"stdin\""));
        assert!(!rendered.contains("CURSOR_API_KEY"));
        assert!(!rendered.contains("stream-json"));
        assert!(!rendered.contains("--print"));
    }

    #[test]
    fn starter_config_includes_claude_runtime_without_protected_flags() {
        let starter = starter_config_text();

        assert!(starter.contains("[runtimes.claude]"));
        assert!(starter.contains("type = \"claude\""));
        assert!(starter.contains("command = \"claude\""));
        assert!(starter.contains("args = []"));
        assert!(starter.contains("prompt_mode = \"stdin\""));
        assert!(!starter.contains("stream-json"));
        assert!(!starter.contains("--include-partial-messages"));
    }

    #[test]
    fn starter_config_includes_cursor_runtime_without_protected_flags() {
        let starter = starter_config_text();

        assert!(starter.contains("[runtimes.cursor]"));
        assert!(starter.contains("type = \"cursor\""));
        assert!(starter.contains("command = \"cursor-agent\""));
        assert!(starter.contains("args = []"));
        assert!(starter.contains("prompt_mode = \"stdin\""));
        assert!(!starter.contains("CURSOR_API_KEY"));
        assert!(!starter.contains("--output-format"));
    }

    #[test]
    fn redacted_toml_shows_prompt_file_paths_without_prompt_bodies() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("agents")).unwrap();
        fs::write(dir.path().join("agents/fixer.md"), "secret prompt body").unwrap();
        let config_path = dir.path().join("custom.toml");
        fs::write(
            &config_path,
            r#"
[agents.fixer]
instructions_file = "agents/fixer.md"
"#,
        )
        .unwrap();
        let config = load_effective_config(ConfigLoadOptions {
            working_directory: dir.path().to_path_buf(),
            config_path: Some(config_path),
        })
        .unwrap();

        let rendered = to_redacted_toml(&config).unwrap();

        assert!(rendered.contains("instructions_file"));
        assert!(rendered.contains("agents/fixer.md"));
        assert!(!rendered.contains("secret prompt body"));
    }

    #[test]
    fn librarian_is_available_but_disabled_by_default() {
        let dir = tempdir().unwrap();
        let config_path = dir.path().join("empty-home.toml");
        fs::write(&config_path, "schema_version = 1\n").unwrap();
        let config = load_effective_config(ConfigLoadOptions {
            working_directory: dir.path().to_path_buf(),
            config_path: Some(config_path),
        })
        .unwrap();
        let librarian = config.agents.get("librarian").unwrap();

        assert!(!librarian.enabled);
        assert_eq!(
            librarian.capabilities,
            vec![Capability::Read, Capability::Answer]
        );
    }

    #[test]
    fn designer_is_available_but_disabled_by_default() {
        let dir = tempdir().unwrap();
        let config_path = dir.path().join("empty-home.toml");
        fs::write(&config_path, "schema_version = 1\n").unwrap();
        let config = load_effective_config(ConfigLoadOptions {
            working_directory: dir.path().to_path_buf(),
            config_path: Some(config_path),
        })
        .unwrap();
        let designer = config.agents.get("designer").unwrap();

        assert!(!designer.enabled);
        assert_eq!(
            designer.capabilities,
            vec![Capability::Read, Capability::Edit, Capability::Verify]
        );
        assert!(designer
            .orchestrator_description
            .as_deref()
            .unwrap()
            .contains("do not use for backend-only"));
    }

    /// The six core structured-runtime roles whose default prompts must stay
    /// aligned between built-in defaults and generated starter instruction files.
    /// Listing them explicitly means adding or removing a core role requires an
    /// intentional update to these drift tests.
    const CORE_PROMPT_ROLES: [&str; 6] = [
        "orchestrator",
        "explorer",
        "fixer",
        "reviewer",
        "oracle",
        "consul",
    ];

    fn builtin_defaults() -> EffectiveConfig {
        load_from_temp("schema_version = 1\n").unwrap()
    }

    fn starter_instruction(role: &str) -> &'static str {
        starter_instruction_files()
            .into_iter()
            .find(|(name, _)| *name == role)
            .unwrap_or_else(|| panic!("missing starter instruction file for {role}"))
            .1
    }

    #[test]
    fn core_builtin_and_starter_prompts_stay_aligned() {
        let config = builtin_defaults();
        for role in CORE_PROMPT_ROLES {
            let builtin = config
                .agents
                .get(role)
                .unwrap_or_else(|| panic!("missing built-in agent {role}"))
                .instructions
                .as_str();
            assert_eq!(
                builtin,
                starter_instruction(role),
                "built-in default and generated starter prompts drifted for `{role}`"
            );
        }
    }

    #[test]
    fn core_prompts_contain_contract_first_language() {
        let config = builtin_defaults();
        for role in CORE_PROMPT_ROLES {
            let prompt = config.agents[role].instructions.as_str();
            for phrase in [
                "structured output contract",
                "JSON envelope",
                "harness action",
                "blocker",
                "Stop ",
            ] {
                assert!(
                    prompt.contains(phrase),
                    "`{role}` prompt is missing required contract phrase {phrase:?}"
                );
            }
        }
    }

    #[test]
    fn core_prompts_assert_role_boundaries() {
        let config = builtin_defaults();
        let boundaries: [(&str, &[&str]); 6] = [
            ("orchestrator", &["inspect the repository", "edit files"]),
            ("explorer", &["read-only", "edit files"]),
            ("fixer", &["verification evidence or a specific blocker"]),
            ("reviewer", &["take over implementation"]),
            ("oracle", &["pretend to have unseen"]),
            ("consul", &["execute the plan"]),
        ];
        // Every core role must carry an explicit boundary case.
        assert_eq!(boundaries.len(), CORE_PROMPT_ROLES.len());
        for (role, phrases) in boundaries {
            let prompt = config.agents[role].instructions.as_str();
            for phrase in phrases {
                assert!(
                    prompt.contains(phrase),
                    "`{role}` prompt is missing role-boundary phrase {phrase:?}"
                );
            }
        }
    }
}
