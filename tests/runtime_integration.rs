use anyhow::{bail, Result};
use multiagent::config::{
    AgentEffort, AgentProfile, Capability, Limits, PromptMode, RuntimeConfig, RuntimeKind,
};
use multiagent::runtime::codex::CodexRuntime;
use multiagent::runtime::zai::ZaiRuntime;
use multiagent::runtime::{Runtime, RuntimeOutput, RuntimeRequest};
use tempfile::tempdir;

#[tokio::test]
#[ignore = "requires MULTIAGENT_RUN_CODEX_INTEGRATION=1 and a working Codex CLI session"]
async fn codex_runtime_executes_real_agent_step() -> Result<()> {
    if std::env::var_os("MULTIAGENT_RUN_CODEX_INTEGRATION").is_none() {
        eprintln!("skipping Codex integration test; set MULTIAGENT_RUN_CODEX_INTEGRATION=1");
        return Ok(());
    }

    let dir = tempdir()?;
    let runtime = CodexRuntime::new(RuntimeConfig {
        id: "codex".to_string(),
        kind: RuntimeKind::Codex,
        command: Some(
            std::env::var("MULTIAGENT_CODEX_INTEGRATION_COMMAND")
                .unwrap_or_else(|_| "codex".to_string()),
        ),
        args: integration_args("MULTIAGENT_CODEX_INTEGRATION_ARGS_JSON")?,
        prompt_mode: PromptMode::Stdin,
        base_url: None,
        api_key_env: None,
    });

    let result = runtime
        .stream_step(agent_request(
            dir.path().to_path_buf(),
            "explorer",
            "codex",
            "default",
        ))
        .await?;

    assert_agent_result(result.output, "explorer")?;
    assert!(!result.stream_deltas.is_empty());
    Ok(())
}

#[tokio::test]
#[ignore = "requires MULTIAGENT_RUN_ZAI_INTEGRATION=1 and a valid Z.ai API key"]
async fn zai_runtime_executes_real_agent_step() -> Result<()> {
    if std::env::var_os("MULTIAGENT_RUN_ZAI_INTEGRATION").is_none() {
        eprintln!("skipping Z.ai integration test; set MULTIAGENT_RUN_ZAI_INTEGRATION=1");
        return Ok(());
    }

    let api_key_env =
        std::env::var("MULTIAGENT_ZAI_API_KEY_ENV").unwrap_or_else(|_| "ZAI_API_KEY".to_string());
    if std::env::var_os(&api_key_env).is_none() {
        bail!("environment variable {api_key_env} is required for Z.ai integration testing");
    }

    let dir = tempdir()?;
    let runtime = ZaiRuntime::new(RuntimeConfig {
        id: "zai".to_string(),
        kind: RuntimeKind::Zai,
        command: None,
        args: Vec::new(),
        prompt_mode: PromptMode::Stdin,
        base_url: Some(
            std::env::var("MULTIAGENT_ZAI_BASE_URL")
                .unwrap_or_else(|_| "https://api.z.ai/api/paas/v4".to_string()),
        ),
        api_key_env: Some(api_key_env),
    });
    let model = std::env::var("MULTIAGENT_ZAI_MODEL").unwrap_or_else(|_| "glm-5.1".to_string());

    let result = runtime
        .stream_step(agent_request(
            dir.path().to_path_buf(),
            "oracle",
            "zai",
            &model,
        ))
        .await?;

    assert_agent_result(result.output, "oracle")?;
    assert!(!result.stream_deltas.is_empty());
    Ok(())
}

fn agent_request(
    working_directory: std::path::PathBuf,
    agent_id: &str,
    runtime_id: &str,
    model: &str,
) -> RuntimeRequest {
    let capabilities = vec![Capability::Read, Capability::Answer];
    RuntimeRequest {
        run_id: "integration-run".to_string(),
        step_id: "integration-step".to_string(),
        prompt: "Return a completed agent_result for this runtime integration smoke test. Do not request actions.".to_string(),
        working_directory,
        agent_profile: AgentProfile {
            id: agent_id.to_string(),
            name: agent_id.to_string(),
            runtime: runtime_id.to_string(),
            model: model.to_string(),
            effort: AgentEffort::Medium,
            thinking: true,
            capabilities: capabilities.clone(),
            instructions: "Produce only the requested structured result inside the JSON contract markers. Do not include prose outside the contract.".to_string(),
            enabled: true,
        },
        session_events: Vec::new(),
        previous_results: Vec::new(),
        action_results: Vec::new(),
        output_schema: "agent_result".to_string(),
        capability_constraints: capabilities,
        limits: Limits::default(),
    }
}

fn assert_agent_result(output: RuntimeOutput, expected_agent: &str) -> Result<()> {
    match output {
        RuntimeOutput::AgentResult { result } => {
            assert_eq!(result.agent, expected_agent);
            Ok(())
        }
        RuntimeOutput::ParseError {
            raw_output,
            diagnostic,
            ..
        } => bail!("runtime returned parse error: {diagnostic}; raw output: {raw_output}"),
        other => bail!("runtime returned unexpected output: {other:?}"),
    }
}

fn integration_args(env_name: &str) -> Result<Vec<String>> {
    match std::env::var(env_name) {
        Ok(value) if !value.trim().is_empty() => Ok(serde_json::from_str(&value)?),
        _ => Ok(Vec::new()),
    }
}
