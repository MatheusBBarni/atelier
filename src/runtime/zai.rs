use super::{
    prompt_envelope_json, Runtime, RuntimeAvailability, RuntimeAvailabilityStatus, RuntimeOutput,
    RuntimeProviderError, RuntimeRequest, RuntimeStepResult, RuntimeStreamDelta,
};
use crate::config::RuntimeConfig;
use crate::orchestrator::{parse_agent_result, parse_contract, parse_orchestrator_decision};
use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use serde_json::Value;
use std::env;
use std::time::Duration;

#[derive(Clone, Debug)]
pub struct ZaiRuntime {
    config: RuntimeConfig,
}

impl ZaiRuntime {
    pub fn new(config: RuntimeConfig) -> Self {
        Self { config }
    }
}

#[async_trait]
impl Runtime for ZaiRuntime {
    async fn check_availability(&self) -> RuntimeAvailability {
        let Some(api_key_env) = &self.config.api_key_env else {
            return RuntimeAvailability {
                runtime_id: self.config.id.clone(),
                status: RuntimeAvailabilityStatus::Unavailable,
                message: "Z.ai api_key_env is not configured".to_string(),
                remediation: Some("Set [runtimes.zai].api_key_env in multiagent.toml.".to_string()),
            };
        };
        match env::var(api_key_env) {
            Ok(value) if !value.trim().is_empty() => RuntimeAvailability {
                runtime_id: self.config.id.clone(),
                status: RuntimeAvailabilityStatus::Unknown,
                message: format!(
                    "{api_key_env} is set; network/API check is deferred until a step runs"
                ),
                remediation: None,
            },
            _ => RuntimeAvailability {
                runtime_id: self.config.id.clone(),
                status: RuntimeAvailabilityStatus::Unavailable,
                message: format!("environment variable {api_key_env} is not set"),
                remediation: Some(format!("Export {api_key_env} with a valid Z.ai API key.")),
            },
        }
    }

    async fn stream_step(&self, request: RuntimeRequest) -> Result<RuntimeStepResult> {
        let api_key_env = self
            .config
            .api_key_env
            .as_ref()
            .context("Z.ai api_key_env is not configured")?;
        let api_key = env::var(api_key_env)
            .with_context(|| format!("environment variable {api_key_env} is not set"))?;
        let base_url = self
            .config
            .base_url
            .as_ref()
            .context("Z.ai base_url is not configured")?;
        let prompt = prompt_envelope_json(&request)?;
        let body = serde_json::json!({
            "model": request.agent_profile.model,
            "messages": [
                {
                    "role": "system",
                    "content": request.agent_profile.instructions
                },
                {
                    "role": "user",
                    "content": prompt
                }
            ],
            "stream": false
        });

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(120))
            .build()?;
        let response = client
            .post(format!("{base_url}/chat/completions"))
            .bearer_auth(api_key)
            .json(&body)
            .send()
            .await
            .map_err(|error| {
                RuntimeProviderError::retryable(format!(
                    "Z.ai chat completions request failed: {error}"
                ))
            })?;

        let status = response.status();
        let text = response
            .text()
            .await
            .context("failed to read Z.ai response body")?;
        if !status.is_success() {
            let body = serde_json::from_str::<Value>(&text)
                .map(|value| redact_response(&value))
                .unwrap_or_else(|_| concise_response_text(&text));
            let message = format!("Z.ai request failed with status {status}: {body}");
            if retryable_status(status) {
                return Err(RuntimeProviderError::retryable(message).into());
            }
            return Err(RuntimeProviderError::non_retryable(message).into());
        }
        let value: Value =
            serde_json::from_str(&text).context("failed to parse Z.ai response JSON")?;

        let content = value
            .get("choices")
            .and_then(Value::as_array)
            .and_then(|choices| choices.first())
            .and_then(|choice| choice.get("message"))
            .and_then(|message| message.get("content"))
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("Z.ai response did not include choices[0].message.content"))?;

        let output = if let Ok(action_request) = parse_contract(content) {
            RuntimeOutput::ActionRequest {
                request: action_request,
            }
        } else if request.agent_profile.id == "orchestrator" {
            match parse_orchestrator_decision(content) {
                Ok(decision) => RuntimeOutput::OrchestratorDecision { decision },
                Err(error) => RuntimeOutput::ParseError {
                    agent: request.agent_profile.id,
                    raw_output: content.to_string(),
                    diagnostic: error.to_string(),
                },
            }
        } else {
            match parse_agent_result(content) {
                Ok(result) => RuntimeOutput::AgentResult { result },
                Err(error) => RuntimeOutput::ParseError {
                    agent: request.agent_profile.id,
                    raw_output: content.to_string(),
                    diagnostic: error.to_string(),
                },
            }
        };

        Ok(
            RuntimeStepResult::new(output).with_delta(RuntimeStreamDelta::final_delta(
                1,
                "message",
                content.to_string(),
            )),
        )
    }
}

fn retryable_status(status: reqwest::StatusCode) -> bool {
    status.as_u16() == 408 || status.as_u16() == 429 || status.is_server_error()
}

fn redact_response(value: &Value) -> String {
    redact_sensitive_text(&value.to_string())
}

fn concise_response_text(text: &str) -> String {
    let text = redact_sensitive_text(&text.split_whitespace().collect::<Vec<_>>().join(" "));
    const MAX_CHARS: usize = 240;
    if text.chars().count() <= MAX_CHARS {
        return text;
    }
    format!(
        "{}...",
        text.chars()
            .take(MAX_CHARS.saturating_sub(3))
            .collect::<String>()
    )
}

fn redact_sensitive_text(text: &str) -> String {
    redact_raw_secret_tokens(&redact_bearer_tokens(text))
}

fn redact_bearer_tokens(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut remaining = text;
    while let Some(auth_start) = remaining.to_ascii_lowercase().find("bearer ") {
        output.push_str(&remaining[..auth_start]);
        output.push_str("Bearer <redacted>");
        let token_start = auth_start + "bearer ".len();
        let token = &remaining[token_start..];
        let token_len = token
            .find(|character: char| {
                character.is_whitespace()
                    || matches!(character, '"' | '\'' | '\\' | ',' | ';' | ')' | ']')
            })
            .unwrap_or(token.len());
        remaining = &token[token_len..];
    }
    output.push_str(remaining);
    output
}

fn redact_raw_secret_tokens(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut remaining = text;

    while let Some((secret_start, _prefix)) = next_raw_secret_prefix(remaining) {
        let absolute_start = text.len() - remaining.len() + secret_start;
        let preceding_character = text[..absolute_start].chars().next_back();
        if preceding_character.is_some_and(is_secret_token_character) {
            output.push_str(&remaining[..secret_start + 1]);
            remaining = &remaining[secret_start + 1..];
            continue;
        }

        output.push_str(&remaining[..secret_start]);
        output.push_str("<redacted secret>");
        let token = &remaining[secret_start..];
        let token_length = token
            .find(|character: char| !is_secret_token_character(character))
            .unwrap_or(token.len());
        remaining = &token[token_length..];
    }

    output.push_str(remaining);
    output
}

fn next_raw_secret_prefix(text: &str) -> Option<(usize, &'static str)> {
    const SECRET_PREFIXES: [&str; 2] = ["sk-", "zai-"];
    let lower = text.to_ascii_lowercase();
    SECRET_PREFIXES
        .into_iter()
        .filter_map(|prefix| lower.find(prefix).map(|index| (index, prefix)))
        .min_by_key(|(index, _prefix)| *index)
}

fn is_secret_token_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        AgentEffort, AgentProfile, AgentPromptMetadata, Capability, Limits, PromptMode, RuntimeKind,
    };
    use crate::orchestrator::{wrap_json_contract, AgentResult};
    use std::net::SocketAddr;
    use tempfile::tempdir;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio::sync::oneshot;

    #[tokio::test]
    async fn zai_adapter_posts_chat_completion_and_parses_agent_result() {
        let dir = tempdir().unwrap();
        let result = AgentResult::completed("oracle", "step", "mocked answer");
        let wrapped = wrap_json_contract(&result).unwrap();
        let (addr, request_rx) = spawn_mock_zai_server(wrapped).await;
        std::env::set_var("MULTIAGENT_TEST_ZAI_KEY", "test-token");

        let runtime = ZaiRuntime::new(RuntimeConfig {
            id: "zai".to_string(),
            kind: RuntimeKind::Zai,
            command: None,
            args: Vec::new(),
            prompt_mode: PromptMode::Stdin,
            base_url: Some(format!("http://{addr}")),
            api_key_env: Some("MULTIAGENT_TEST_ZAI_KEY".to_string()),
        });
        let result = runtime
            .stream_step(runtime_request(dir.path().to_path_buf(), "oracle"))
            .await
            .unwrap();
        assert_eq!(result.stream_deltas.len(), 1);

        match result.output {
            RuntimeOutput::AgentResult { result } => {
                assert_eq!(result.agent, "oracle");
                assert_eq!(result.summary, "mocked answer");
            }
            other => panic!("unexpected runtime output: {other:?}"),
        }

        let request = request_rx.await.unwrap();
        assert!(request.contains("POST /chat/completions HTTP/1.1"));
        assert!(request.contains("authorization: Bearer test-token"));
        assert!(request.contains("\"model\":\"glm-5.1\""));
        assert!(request.contains("\"stream\":false"));
    }

    #[tokio::test]
    async fn zai_availability_reports_missing_credential_reference() {
        let runtime = ZaiRuntime::new(RuntimeConfig {
            id: "zai".to_string(),
            kind: RuntimeKind::Zai,
            command: None,
            args: Vec::new(),
            prompt_mode: PromptMode::Stdin,
            base_url: Some("http://127.0.0.1:1".to_string()),
            api_key_env: Some("MULTIAGENT_TEST_MISSING_ZAI_KEY".to_string()),
        });

        let availability = runtime.check_availability().await;
        assert_eq!(availability.status, RuntimeAvailabilityStatus::Unavailable);
        assert!(availability
            .message
            .contains("MULTIAGENT_TEST_MISSING_ZAI_KEY"));
    }

    #[test]
    fn concise_response_text_redacts_bearer_tokens_in_plain_text_errors() {
        let text = concise_response_text(
            "upstream proxy echoed Authorization: Bearer zai-secret-token while failing",
        );

        assert!(text.contains("Bearer <redacted>"));
        assert!(!text.contains("zai-secret-token"));
    }

    #[test]
    fn redact_response_redacts_bearer_tokens_in_json_errors() {
        let value = serde_json::json!({
            "error": "request included Bearer test-token",
        });

        let text = redact_response(&value);

        assert!(text.contains("Bearer <redacted>"));
        assert!(!text.contains("test-token"));
    }

    #[test]
    fn concise_response_text_redacts_raw_secret_tokens_in_plain_text_errors() {
        let text = concise_response_text(
            "invalid api key zai-secret-token while retrying with sk-test-secret",
        );

        assert!(text.contains("<redacted secret>"));
        assert!(!text.contains("zai-secret-token"));
        assert!(!text.contains("sk-test-secret"));
    }

    #[test]
    fn redact_response_redacts_raw_secret_tokens_in_json_errors() {
        let value = serde_json::json!({
            "error": "invalid credential sk-json-secret",
            "debug": "fallback credential zai-json-secret",
        });

        let text = redact_response(&value);

        assert!(text.contains("<redacted secret>"));
        assert!(!text.contains("sk-json-secret"));
        assert!(!text.contains("zai-json-secret"));
    }

    async fn spawn_mock_zai_server(
        response_content: String,
    ) -> (SocketAddr, oneshot::Receiver<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = oneshot::channel();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let request = read_http_request(&mut socket).await;
            let _ = tx.send(request);
            let response_body = serde_json::json!({
                "choices": [
                    {
                        "message": {
                            "content": response_content
                        }
                    }
                ]
            })
            .to_string();
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                response_body.len(),
                response_body
            );
            socket.write_all(response.as_bytes()).await.unwrap();
        });
        (addr, rx)
    }

    async fn read_http_request(socket: &mut tokio::net::TcpStream) -> String {
        let mut buffer = Vec::new();
        let mut temp = [0_u8; 1024];
        let mut content_length = None;
        loop {
            let read = socket.read(&mut temp).await.unwrap();
            if read == 0 {
                break;
            }
            buffer.extend_from_slice(&temp[..read]);
            if content_length.is_none() {
                if let Some(header_end) = find_header_end(&buffer) {
                    let headers =
                        String::from_utf8_lossy(&buffer[..header_end]).to_ascii_lowercase();
                    content_length = headers
                        .lines()
                        .find_map(|line| line.strip_prefix("content-length: "))
                        .and_then(|value| value.trim().parse::<usize>().ok());
                }
            }
            if let Some(header_end) = find_header_end(&buffer) {
                let body_start = header_end + 4;
                if content_length
                    .map(|length| buffer.len() >= body_start + length)
                    .unwrap_or(false)
                {
                    break;
                }
            }
        }
        String::from_utf8_lossy(&buffer).to_string()
    }

    fn find_header_end(buffer: &[u8]) -> Option<usize> {
        buffer.windows(4).position(|window| window == b"\r\n\r\n")
    }

    fn runtime_request(working_directory: std::path::PathBuf, agent_id: &str) -> RuntimeRequest {
        RuntimeRequest {
            run_id: "run".to_string(),
            step_id: "step".to_string(),
            prompt: "answer this".to_string(),
            session_goal: None,
            working_directory,
            agent_profile: AgentProfile {
                id: agent_id.to_string(),
                name: "Oracle".to_string(),
                runtime: "zai".to_string(),
                model: "glm-5.1".to_string(),
                model_fallbacks: Vec::new(),
                effort: AgentEffort::Medium,
                thinking: true,
                capabilities: vec![Capability::Read, Capability::Answer],
                tools: None,
                instructions: "Answer questions.".to_string(),
                orchestrator_description: None,
                prompt_metadata: AgentPromptMetadata::default(),
                enabled: true,
            },
            session_events: Vec::new(),
            recent_context: crate::runtime::RuntimeRecentContext::default(),
            previous_results: Vec::new(),
            action_results: Vec::new(),
            output_schema: "agent_result".to_string(),
            capability_constraints: vec![Capability::Read, Capability::Answer],
            limits: Limits::default(),
        }
    }
}
