use super::{
    prompt_envelope_json, Runtime, RuntimeAvailability, RuntimeAvailabilityStatus,
    RuntimeEventSink, RuntimeOutput, RuntimeProviderError, RuntimeRequest,
};
use crate::config::RuntimeConfig;
use crate::orchestrator::{parse_agent_result, parse_contract, parse_orchestrator_decision};
use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use reqwest::header::CONTENT_TYPE;
use serde_json::Value;
use std::env;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

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

    async fn stream_step(
        &self,
        request: RuntimeRequest,
        events: RuntimeEventSink,
        cancellation: CancellationToken,
    ) -> Result<RuntimeOutput> {
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
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(120))
            .build()?;
        let content = self
            .stream_or_fallback(
                &client,
                base_url,
                &api_key,
                &request,
                &events,
                &cancellation,
            )
            .await?;

        parse_runtime_output(&request.agent_profile.id, content)
    }
}

impl ZaiRuntime {
    async fn stream_or_fallback(
        &self,
        client: &reqwest::Client,
        base_url: &str,
        api_key: &str,
        request: &RuntimeRequest,
        events: &RuntimeEventSink,
        cancellation: &CancellationToken,
    ) -> Result<String> {
        let fallback_enabled = streaming_fallback_enabled(&self.config);
        let stream_body = chat_completion_body(request, true)?;
        let response =
            send_chat_completion(client, base_url, api_key, &stream_body, cancellation).await?;
        let status = response.status();
        if !status.is_success() {
            let text = read_response_text(response, cancellation).await?;
            if fallback_enabled && streaming_rejection_status(status) {
                let body = response_error_body(&text);
                events
                    .status(format!(
                        "Z.ai streaming rejected with status {status}; falling back to non-streaming chat completion: {body}"
                    ))
                    .await?;
                return run_non_streaming_completion(
                    client,
                    base_url,
                    api_key,
                    request,
                    events,
                    cancellation,
                )
                .await;
            }
            return Err(zai_status_error(status, &text).into());
        }

        if !response_is_sse(&response) {
            let text = read_response_text(response, cancellation).await?;
            if fallback_enabled {
                events
                    .status("Z.ai streaming response was not SSE; falling back to non-streaming chat completion")
                    .await?;
                return run_non_streaming_completion(
                    client,
                    base_url,
                    api_key,
                    request,
                    events,
                    cancellation,
                )
                .await;
            }
            return Err(RuntimeProviderError::non_retryable(format!(
                "Z.ai streaming response was not SSE: {}",
                concise_response_text(&text)
            ))
            .into());
        }

        read_sse_message_content(response, events, cancellation).await
    }
}

async fn run_non_streaming_completion(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    request: &RuntimeRequest,
    events: &RuntimeEventSink,
    cancellation: &CancellationToken,
) -> Result<String> {
    let body = chat_completion_body(request, false)?;
    let response = send_chat_completion(client, base_url, api_key, &body, cancellation).await?;
    let status = response.status();
    let text = read_response_text(response, cancellation).await?;
    if !status.is_success() {
        return Err(zai_status_error(status, &text).into());
    }
    let content = content_from_non_streaming_response(&text)?;
    events.delta("message", content.clone()).await?;
    Ok(content)
}

fn parse_runtime_output(agent_id: &str, content: String) -> Result<RuntimeOutput> {
    let output = if let Ok(action_request) = parse_contract(&content) {
        RuntimeOutput::ActionRequest {
            request: action_request,
        }
    } else if agent_id == "orchestrator" {
        match parse_orchestrator_decision(&content) {
            Ok(decision) => RuntimeOutput::OrchestratorDecision { decision },
            Err(error) => RuntimeOutput::ParseError {
                agent: agent_id.to_string(),
                raw_output: content,
                diagnostic: error.to_string(),
            },
        }
    } else {
        match parse_agent_result(&content) {
            Ok(result) => RuntimeOutput::AgentResult { result },
            Err(error) => RuntimeOutput::ParseError {
                agent: agent_id.to_string(),
                raw_output: content,
                diagnostic: error.to_string(),
            },
        }
    };

    Ok(output)
}

fn chat_completion_body(request: &RuntimeRequest, stream: bool) -> Result<Value> {
    let prompt = prompt_envelope_json(request)?;
    Ok(serde_json::json!({
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
        "stream": stream
    }))
}

async fn send_chat_completion(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    body: &Value,
    cancellation: &CancellationToken,
) -> Result<reqwest::Response> {
    tokio::select! {
        _ = cancellation.cancelled() => anyhow::bail!("Z.ai runtime cancelled"),
        response = client
            .post(format!("{base_url}/chat/completions"))
            .bearer_auth(api_key)
            .json(body)
            .send() => {
                response.map_err(|error| {
                    RuntimeProviderError::retryable(format!(
                        "Z.ai chat completions request failed: {error}"
                    ))
                }.into())
            }
    }
}

async fn read_response_text(
    response: reqwest::Response,
    cancellation: &CancellationToken,
) -> Result<String> {
    tokio::select! {
        _ = cancellation.cancelled() => anyhow::bail!("Z.ai runtime cancelled"),
        text = response.text() => text.context("failed to read Z.ai response body"),
    }
}

async fn read_sse_message_content(
    mut response: reqwest::Response,
    events: &RuntimeEventSink,
    cancellation: &CancellationToken,
) -> Result<String> {
    let mut buffer = String::new();
    let mut content = String::new();
    loop {
        let chunk = tokio::select! {
            _ = cancellation.cancelled() => anyhow::bail!("Z.ai runtime cancelled"),
            chunk = response.chunk() => chunk.context("failed to read Z.ai streaming response")?,
        };
        let Some(chunk) = chunk else {
            break;
        };
        buffer.push_str(&String::from_utf8_lossy(&chunk));
        normalize_crlf(&mut buffer);
        while let Some(frame_end) = buffer.find("\n\n") {
            let frame = buffer[..frame_end].to_string();
            buffer.drain(..frame_end + 2);
            match parse_sse_frame(&frame)? {
                SseFrame::Content(delta) => {
                    events.delta("message", delta.clone()).await?;
                    content.push_str(&delta);
                }
                SseFrame::Diagnostic(message) => {
                    events.diagnostic("error", message.clone()).await?;
                    return Err(RuntimeProviderError::non_retryable(message).into());
                }
                SseFrame::Done | SseFrame::Empty => {}
            }
        }
    }

    if !buffer.trim().is_empty() {
        match parse_sse_frame(&buffer)? {
            SseFrame::Content(delta) => {
                events.delta("message", delta.clone()).await?;
                content.push_str(&delta);
            }
            SseFrame::Diagnostic(message) => {
                events.diagnostic("error", message.clone()).await?;
                return Err(RuntimeProviderError::non_retryable(message).into());
            }
            SseFrame::Done | SseFrame::Empty => {}
        }
    }

    Ok(content)
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum SseFrame {
    Content(String),
    Diagnostic(String),
    Done,
    Empty,
}

fn parse_sse_frame(frame: &str) -> Result<SseFrame> {
    let data_lines = frame
        .lines()
        .filter_map(|line| {
            let line = line.trim_end_matches('\r');
            if line.is_empty() || line.starts_with(':') {
                return None;
            }
            line.strip_prefix("data:").map(str::trim_start)
        })
        .collect::<Vec<_>>();
    if data_lines.is_empty() {
        return Ok(SseFrame::Empty);
    }
    if data_lines.iter().all(|data| data.trim() == "[DONE]") {
        return Ok(SseFrame::Done);
    }
    let mut content = String::new();
    for data in data_lines {
        if data.trim() == "[DONE]" {
            continue;
        }
        let value: Value = serde_json::from_str(data).with_context(|| {
            format!(
                "malformed Z.ai SSE data frame: {}",
                concise_response_text(data)
            )
        })?;
        if let Some(error) = value.get("error") {
            return Ok(SseFrame::Diagnostic(redact_response(error)));
        }
        content.push_str(
            &value
                .get("choices")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|choice| choice.get("delta"))
                .filter_map(|delta| delta.get("content"))
                .filter_map(Value::as_str)
                .collect::<String>(),
        );
    }
    if content.is_empty() {
        Ok(SseFrame::Empty)
    } else {
        Ok(SseFrame::Content(content))
    }
}

fn content_from_non_streaming_response(text: &str) -> Result<String> {
    let value: Value = serde_json::from_str(text).context("failed to parse Z.ai response JSON")?;
    value
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .and_then(|choice| choice.get("message"))
        .and_then(|message| message.get("content"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| anyhow!("Z.ai response did not include choices[0].message.content"))
}

fn response_is_sse(response: &reqwest::Response) -> bool {
    response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.to_ascii_lowercase().contains("text/event-stream"))
        .unwrap_or(false)
}

fn normalize_crlf(buffer: &mut String) {
    if buffer.contains("\r\n") {
        *buffer = buffer.replace("\r\n", "\n");
    }
}

fn zai_status_error(status: reqwest::StatusCode, text: &str) -> RuntimeProviderError {
    let body = response_error_body(text);
    let message = format!("Z.ai request failed with status {status}: {body}");
    if retryable_status(status) {
        RuntimeProviderError::retryable(message)
    } else {
        RuntimeProviderError::non_retryable(message)
    }
}

fn response_error_body(text: &str) -> String {
    serde_json::from_str::<Value>(text)
        .map(|value| redact_response(&value))
        .unwrap_or_else(|_| concise_response_text(text))
}

fn streaming_rejection_status(status: reqwest::StatusCode) -> bool {
    matches!(status.as_u16(), 400 | 404 | 405 | 406 | 415 | 422)
}

fn streaming_fallback_enabled(config: &RuntimeConfig) -> bool {
    !config.args.iter().any(|arg| {
        matches!(
            arg.as_str(),
            "--no-stream-fallback" | "streaming_fallback=false"
        )
    })
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
    use crate::runtime::collect_runtime_step_result;
    use std::net::SocketAddr;
    use tempfile::tempdir;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio::sync::oneshot;

    #[tokio::test]
    async fn zai_adapter_streams_sse_chunks_and_parses_agent_result() {
        let dir = tempdir().unwrap();
        let result = AgentResult::completed("oracle", "step", "mocked answer");
        let wrapped = wrap_json_contract(&result).unwrap();
        let midpoint = wrapped.len() / 2;
        let (first, second) = wrapped.split_at(midpoint);
        let (addr, request_rx) = spawn_mock_zai_sequence_server(vec![sse_response(&[
            sse_data(&serde_json::json!({
                "choices": [
                    {
                        "delta": {
                            "content": first
                        }
                    }
                ]
            })),
            sse_data(&serde_json::json!({
                "choices": [
                    {
                        "delta": {
                            "content": second
                        }
                    }
                ]
            })),
            "data: [DONE]\n\n".to_string(),
        ])])
        .await;
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
        let request = runtime_request(dir.path().to_path_buf(), "oracle");
        let result = collect_runtime_step_result(|events, cancellation| {
            runtime.stream_step(request, events, cancellation)
        })
        .await
        .unwrap();
        assert!(!result.stream_deltas.is_empty());
        assert!(result
            .stream_deltas
            .iter()
            .any(|delta| { delta.stream == "message" && delta.content.contains("mocked answer") }));

        match result.output {
            RuntimeOutput::AgentResult { result } => {
                assert_eq!(result.agent, "oracle");
                assert_eq!(result.summary, "mocked answer");
            }
            other => panic!("unexpected runtime output: {other:?}"),
        }

        let requests = request_rx.await.unwrap();
        let request = &requests[0];
        assert!(request.contains("POST /chat/completions HTTP/1.1"));
        assert!(request.contains("authorization: Bearer test-token"));
        assert!(request.contains("\"model\":\"glm-5.1\""));
        assert!(request.contains("\"stream\":true"));
    }

    #[tokio::test]
    async fn zai_streaming_rejection_falls_back_explicitly() {
        let dir = tempdir().unwrap();
        let result = AgentResult::completed("oracle", "step", "fallback answer");
        let wrapped = wrap_json_contract(&result).unwrap();
        let (addr, request_rx) = spawn_mock_zai_sequence_server(vec![
            json_response(
                400,
                serde_json::json!({ "error": "streaming is not supported" }).to_string(),
            ),
            json_response(
                200,
                serde_json::json!({
                    "choices": [
                        {
                            "message": {
                                "content": wrapped
                            }
                        }
                    ]
                })
                .to_string(),
            ),
        ])
        .await;
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
        let request = runtime_request(dir.path().to_path_buf(), "oracle");

        let result = collect_runtime_step_result(|events, cancellation| {
            runtime.stream_step(request, events, cancellation)
        })
        .await
        .unwrap();

        assert!(result.stream_deltas.iter().any(|delta| {
            delta.stream == "status" && delta.content.contains("falling back to non-streaming")
        }));
        match result.output {
            RuntimeOutput::AgentResult { result } => {
                assert_eq!(result.summary, "fallback answer");
            }
            other => panic!("unexpected runtime output: {other:?}"),
        }
        let requests = request_rx.await.unwrap();
        assert_eq!(requests.len(), 2);
        assert!(requests[0].contains("\"stream\":true"));
        assert!(requests[1].contains("\"stream\":false"));
    }

    #[tokio::test]
    async fn zai_streaming_rejection_fails_when_fallback_disabled() {
        let dir = tempdir().unwrap();
        let (addr, request_rx) = spawn_mock_zai_sequence_server(vec![json_response(
            400,
            serde_json::json!({ "error": "streaming is not supported" }).to_string(),
        )])
        .await;
        std::env::set_var("MULTIAGENT_TEST_ZAI_KEY", "test-token");
        let runtime = ZaiRuntime::new(RuntimeConfig {
            id: "zai".to_string(),
            kind: RuntimeKind::Zai,
            command: None,
            args: vec!["--no-stream-fallback".to_string()],
            prompt_mode: PromptMode::Stdin,
            base_url: Some(format!("http://{addr}")),
            api_key_env: Some("MULTIAGENT_TEST_ZAI_KEY".to_string()),
        });
        let request = runtime_request(dir.path().to_path_buf(), "oracle");

        let error = runtime
            .stream_step(
                request,
                RuntimeEventSink::channel(1).0,
                CancellationToken::new(),
            )
            .await
            .unwrap_err();

        assert!(error.to_string().contains("status 400"));
        let requests = request_rx.await.unwrap();
        assert_eq!(requests.len(), 1);
        assert!(requests[0].contains("\"stream\":true"));
    }

    #[test]
    fn zai_sse_parser_handles_keepalives_multiline_done_and_errors() {
        assert_eq!(parse_sse_frame(": keepalive\n").unwrap(), SseFrame::Empty);
        assert_eq!(parse_sse_frame("data: [DONE]\n").unwrap(), SseFrame::Done);
        assert_eq!(
            parse_sse_frame(
                "data: {\"choices\":[{\"delta\":{\"content\":\"hel\"}}]}\ndata: {\"choices\":[{\"delta\":{\"content\":\"lo\"}}]}\n"
            )
            .unwrap(),
            SseFrame::Content("hello".to_string())
        );
        assert!(matches!(
            parse_sse_frame("data: {\"error\":\"bad sk-secret\"}\n").unwrap(),
            SseFrame::Diagnostic(message) if message.contains("<redacted secret>")
        ));
        assert!(parse_sse_frame("data: not-json\n").is_err());
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

    async fn spawn_mock_zai_sequence_server(
        responses: Vec<String>,
    ) -> (SocketAddr, oneshot::Receiver<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = oneshot::channel();
        tokio::spawn(async move {
            let mut requests = Vec::new();
            for response in responses {
                let (mut socket, _) = listener.accept().await.unwrap();
                requests.push(read_http_request(&mut socket).await);
                socket.write_all(response.as_bytes()).await.unwrap();
            }
            let _ = tx.send(requests);
        });
        (addr, rx)
    }

    fn sse_response(frames: &[String]) -> String {
        let body = frames.join("");
        format!(
            "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
            body.len(),
            body
        )
    }

    fn sse_data(value: &Value) -> String {
        format!("data: {}\n\n", value)
    }

    fn json_response(status: u16, body: String) -> String {
        format!(
            "HTTP/1.1 {status} Test\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
            body.len(),
            body
        )
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
