use anyhow::{anyhow, bail, Context, Result};
use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
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
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspacePolicy {
    pub extra_read_roots: Vec<PathBuf>,
    pub extra_write_roots: Vec<PathBuf>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeKind {
    Codex,
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

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentProfile {
    pub id: String,
    pub name: String,
    pub runtime: String,
    pub model: String,
    pub effort: AgentEffort,
    pub thinking: bool,
    pub capabilities: Vec<Capability>,
    pub instructions: String,
    pub enabled: bool,
}

impl AgentProfile {
    pub fn has_capability(&self, capability: &Capability) -> bool {
        self.capabilities
            .iter()
            .any(|existing| existing == capability)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EffectiveConfig {
    pub schema_version: u32,
    pub working_directory: PathBuf,
    pub config_sources: Vec<PathBuf>,
    pub approval_mode: ApprovalMode,
    pub workspace: WorkspacePolicy,
    pub limits: Limits,
    pub runtimes: BTreeMap<String, RuntimeConfig>,
    pub agents: BTreeMap<String, AgentProfile>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConfig {
    schema_version: Option<u32>,
    approval_mode: Option<ApprovalMode>,
    workspace: Option<RawWorkspacePolicy>,
    limits: Option<RawLimits>,
    runtimes: Option<BTreeMap<String, RawRuntimeConfig>>,
    agents: Option<BTreeMap<String, RawAgentProfile>>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawWorkspacePolicy {
    extra_read_roots: Option<Vec<PathBuf>>,
    extra_write_roots: Option<Vec<PathBuf>>,
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
    runtime: Option<String>,
    model: Option<String>,
    effort: Option<AgentEffort>,
    thinking: Option<bool>,
    capabilities: Option<Vec<Capability>>,
    instructions: Option<String>,
    instructions_file: Option<PathBuf>,
    enabled: Option<bool>,
}

#[derive(Clone, Debug)]
enum InstructionSource {
    Inline(String),
    File(PathBuf),
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
    effort: Option<AgentEffort>,
    thinking: Option<bool>,
    capabilities: Option<Vec<Capability>>,
    instruction_source: Option<InstructionSource>,
    enabled: Option<bool>,
}

#[derive(Clone, Debug)]
struct MergedConfig {
    working_directory: PathBuf,
    config_sources: Vec<PathBuf>,
    approval_mode: ApprovalMode,
    workspace: WorkspacePolicy,
    limits: Limits,
    runtimes: BTreeMap<String, MergedRuntimeConfig>,
    agents: BTreeMap<String, MergedAgentProfile>,
}

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
                instructions: "Own the run plan, choose specialized agents, ask clarifying questions, and decide when the run is complete.",
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
                instructions: "Read code, documentation, repository state, and session context without changing files.",
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
                instructions: "Answer design or implementation questions from gathered context inside the typed result envelope.",
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
                instructions:
                    "Challenge plans, architecture, and domain decisions before work proceeds.",
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
                instructions: "Apply scoped file changes through harness actions and run targeted verification.",
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
                instructions:
                    "Review changes for bugs, regressions, and missing tests without editing files.",
            },
        );

        Self {
            working_directory,
            config_sources: Vec::new(),
            approval_mode: ApprovalMode::Yolo,
            workspace: WorkspacePolicy::default(),
            limits: Limits::default(),
            runtimes,
            agents,
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
        }

        if let Some(runtimes) = raw.runtimes {
            for (runtime_id, runtime) in runtimes {
                self.apply_runtime(runtime_id, runtime, source_name)?;
            }
        }

        if let Some(agents) = raw.agents {
            for (agent_id, agent) in agents {
                self.apply_agent(agent_id, agent, source_dir, source_name)?;
            }
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

        let entry = self
            .agents
            .entry(agent_id.clone())
            .or_insert(MergedAgentProfile {
                name: None,
                runtime: None,
                model: None,
                effort: None,
                thinking: None,
                capabilities: None,
                instruction_source: None,
                enabled: None,
            });

        if let Some(name) = raw.name {
            entry.name = Some(name);
        }
        if let Some(runtime) = raw.runtime {
            entry.runtime = Some(runtime);
        }
        if let Some(model) = raw.model {
            entry.model = Some(model);
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
        if let Some(instructions) = raw.instructions {
            entry.instruction_source = Some(InstructionSource::Inline(instructions));
        }
        if let Some(instructions_file) = raw.instructions_file {
            entry.instruction_source = Some(InstructionSource::File(resolve_config_path(
                source_dir,
                instructions_file,
            )));
        }
        if let Some(enabled) = raw.enabled {
            entry.enabled = Some(enabled);
        }

        Ok(())
    }

    fn into_effective(self) -> Result<EffectiveConfig> {
        let mut runtimes = BTreeMap::new();
        for (id, runtime) in self.runtimes {
            let kind = runtime
                .kind
                .ok_or_else(|| anyhow!("runtime {id} is missing required field type"))?;

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
        for (id, agent) in self.agents {
            let runtime = agent
                .runtime
                .ok_or_else(|| anyhow!("agent {id} is missing required field runtime"))?;
            if !runtimes.contains_key(&runtime) {
                bail!("agent {id} points at undefined runtime {runtime}");
            }

            let capabilities = agent
                .capabilities
                .ok_or_else(|| anyhow!("agent {id} is missing required field capabilities"))?;
            let instruction_source = agent
                .instruction_source
                .ok_or_else(|| anyhow!("agent {id} is missing instructions"))?;
            let instructions = match instruction_source {
                InstructionSource::Inline(instructions) => instructions,
                InstructionSource::File(path) => fs::read_to_string(&path).with_context(|| {
                    format!("failed to read instructions_file {}", path.display())
                })?,
            };

            agents.insert(
                id.clone(),
                AgentProfile {
                    id: id.clone(),
                    name: agent.name.unwrap_or_else(|| title_case_id(&id)),
                    runtime,
                    model: agent.model.unwrap_or_else(|| "default".to_string()),
                    effort: agent.effort.unwrap_or_default(),
                    thinking: agent.thinking.unwrap_or(false),
                    capabilities,
                    instructions,
                    enabled: agent.enabled.unwrap_or(true),
                },
            );
        }

        Ok(EffectiveConfig {
            schema_version: 1,
            working_directory: self.working_directory,
            config_sources: self.config_sources,
            approval_mode: self.approval_mode,
            workspace: self.workspace,
            limits: self.limits,
            runtimes,
            agents,
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
}

fn insert_builtin_agent(agents: &mut BTreeMap<String, MergedAgentProfile>, agent: BuiltinAgent) {
    agents.insert(
        agent.id.to_string(),
        MergedAgentProfile {
            name: Some(agent.name.to_string()),
            runtime: Some(agent.runtime.to_string()),
            model: Some(agent.model.to_string()),
            effort: Some(agent.effort),
            thinking: Some(agent.thinking),
            capabilities: Some(agent.capabilities),
            instruction_source: Some(InstructionSource::Inline(agent.instructions.to_string())),
            enabled: Some(true),
        },
    );
}

pub fn load_effective_config(options: ConfigLoadOptions) -> Result<EffectiveConfig> {
    let explicit_config_path = options.config_path.is_some();
    let working_directory = options.working_directory.canonicalize().with_context(|| {
        format!(
            "failed to resolve working directory {}",
            options.working_directory.display()
        )
    })?;
    let mut merged = MergedConfig::builtin(working_directory.clone());

    let env_config_path = env::var_os("MULTIAGENT_CONFIG").map(PathBuf::from);
    let config_path = options.config_path.or(env_config_path.clone());
    let home_config = config_path.unwrap_or_else(default_home_config_path);
    let explicit_home_config = explicit_config_path || env_config_path.is_some();

    if home_config.exists() {
        apply_config_file(&mut merged, &home_config)?;
    } else if explicit_home_config {
        bail!(
            "configured harness configuration file does not exist: {}",
            home_config.display()
        );
    }

    let local_config = working_directory.join("multiagent.toml");
    if local_config.exists() {
        apply_config_file(&mut merged, &local_config)?;
    }

    merged.into_effective()
}

pub fn default_home_config_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".config")
        .join(".multiagent")
        .join("multiagent.toml")
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

fn title_case_id(id: &str) -> String {
    let mut chars = id.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => "Agent".to_string(),
    }
}

#[derive(Clone, Debug, Serialize)]
struct PrintableConfig {
    schema_version: u32,
    approval_mode: ApprovalMode,
    workspace: WorkspacePolicy,
    limits: Limits,
    runtimes: BTreeMap<String, PrintableRuntime>,
    agents: BTreeMap<String, PrintableAgent>,
}

#[derive(Clone, Debug, Serialize)]
struct PrintableRuntime {
    #[serde(rename = "type")]
    kind: RuntimeKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    command: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    args: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    prompt_mode: Option<PromptMode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    base_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    api_key_env: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
struct PrintableAgent {
    name: String,
    runtime: String,
    model: String,
    effort: AgentEffort,
    thinking: bool,
    capabilities: Vec<Capability>,
    instructions: String,
    enabled: bool,
}

pub fn to_redacted_toml(config: &EffectiveConfig) -> Result<String> {
    let printable = PrintableConfig {
        schema_version: config.schema_version,
        approval_mode: config.approval_mode.clone(),
        workspace: config.workspace.clone(),
        limits: config.limits.clone(),
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
                            RuntimeKind::Codex => Some(runtime.prompt_mode.clone()),
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
                        name: agent.name.clone(),
                        runtime: agent.runtime.clone(),
                        model: agent.model.clone(),
                        effort: agent.effort.clone(),
                        thinking: agent.thinking,
                        capabilities: agent.capabilities.clone(),
                        instructions: agent.instructions.clone(),
                        enabled: agent.enabled,
                    },
                )
            })
            .collect(),
    };

    toml::to_string_pretty(&printable).context("failed to render effective configuration")
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

[runtimes.codex]
type = "codex"
command = "codex"
args = ["exec", "--skip-git-repo-check", "--color", "never"]
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
"#
    .to_string()
}

fn starter_instruction_files() -> Vec<(&'static str, &'static str)> {
    vec![
        (
            "orchestrator",
            "Own the run plan, choose specialized agents, ask clarifying questions, and return only structured orchestrator decisions.",
        ),
        (
            "explorer",
            "Read repository context without changing files and return structured findings.",
        ),
        (
            "oracle",
            "Answer design and implementation questions using gathered context and return a typed agent result.",
        ),
        (
            "consul",
            "Challenge plans and architecture decisions before implementation proceeds.",
        ),
        (
            "fixer",
            "Apply scoped changes through harness actions and run targeted verification.",
        ),
        (
            "reviewer",
            "Review diffs and verification evidence without editing files.",
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
        let config_path = dir.path().join("multiagent.toml");
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
    }

    #[test]
    fn default_home_config_path_uses_dot_config_multiagent() {
        let path = default_home_config_path();
        assert!(path.ends_with(Path::new(".config/.multiagent/multiagent.toml")));
    }

    #[test]
    fn limit_accepts_unlimited_and_rejects_zero() {
        let ok: RawLimits = toml::from_str("max_agent_steps = \"unlimited\"").unwrap();
        assert_eq!(ok.max_agent_steps, Some(Limit::Unlimited));

        let err = toml::from_str::<RawLimits>("max_agent_steps = 0").unwrap_err();
        assert!(format!("{err:#}").contains("positive"));
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
        assert!(!rendered.contains("Bearer"));
    }
}
