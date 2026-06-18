use super::http_util::{parse_runtime_output, redact_sensitive_text};
use super::{
    prompt_envelope_json, Runtime, RuntimeAvailability, RuntimeAvailabilityStatus,
    RuntimeEventSink, RuntimeOutput, RuntimeProviderError, RuntimeRequest,
};
use crate::config::RuntimeConfig;
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
                remediation: Some("Set [runtimes.zai].api_key_env in atelier.toml.".to_string()),
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
    let mut buffer = Vec::new();
    let mut content = String::new();
    loop {
        let chunk = tokio::select! {
            _ = cancellation.cancelled() => anyhow::bail!("Z.ai runtime cancelled"),
            chunk = response.chunk() => chunk.context("failed to read Z.ai streaming response")?,
        };
        let Some(chunk) = chunk else {
            break;
        };
        buffer.extend_from_slice(&chunk);
        while let Some(frame) = drain_next_sse_frame(&mut buffer)? {
            if apply_sse_frame(frame, events, &mut content).await? {
                return Ok(content);
            }
        }
    }

    if !buffer.iter().all(u8::is_ascii_whitespace) {
        let frame = std::str::from_utf8(&buffer).context("malformed Z.ai SSE UTF-8 frame")?;
        if apply_sse_frame(parse_sse_frame(frame)?, events, &mut content).await? {
            return Ok(content);
        }
    }

    Ok(content)
}

async fn apply_sse_frame(
    frame: SseFrame,
    events: &RuntimeEventSink,
    content: &mut String,
) -> Result<bool> {
    match frame {
        SseFrame::Content(delta) => {
            events.delta("message", delta.clone()).await?;
            content.push_str(&delta);
            Ok(false)
        }
        SseFrame::Diagnostic(message) => {
            events.diagnostic("error", message.clone()).await?;
            Err(RuntimeProviderError::non_retryable(message).into())
        }
        SseFrame::Done => Ok(true),
        SseFrame::Empty => Ok(false),
    }
}

fn drain_next_sse_frame(buffer: &mut Vec<u8>) -> Result<Option<SseFrame>> {
    let Some((frame_end, separator_len)) = find_sse_frame_separator(buffer) else {
        return Ok(None);
    };
    let frame = std::str::from_utf8(&buffer[..frame_end])
        .context("malformed Z.ai SSE UTF-8 frame")
        .and_then(parse_sse_frame)?;
    buffer.drain(..frame_end + separator_len);
    Ok(Some(frame))
}

fn find_sse_frame_separator(buffer: &[u8]) -> Option<(usize, usize)> {
    let lf = buffer
        .windows(2)
        .position(|window| window == b"\n\n")
        .map(|index| (index, 2));
    let crlf = buffer
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|index| (index, 4));
    match (lf, crlf) {
        (Some(left), Some(right)) => Some(if left.0 <= right.0 { left } else { right }),
        (Some(separator), None) | (None, Some(separator)) => Some(separator),
        (None, None) => None,
    }
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
            degrade_not_abandon: false,
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
    async fn zai_adapter_finishes_when_done_frame_keeps_connection_open() {
        let dir = tempdir().unwrap();
        let result = AgentResult::completed("oracle", "step", "done answer");
        let wrapped = wrap_json_contract(&result).unwrap();
        let (addr, request_rx, release_server) = spawn_open_sse_server(vec![
            sse_data(&serde_json::json!({
                "choices": [
                    {
                        "delta": {
                            "content": wrapped
                        }
                    }
                ]
            })),
            "data: [DONE]\n\n".to_string(),
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
            degrade_not_abandon: false,
        });
        let request = runtime_request(dir.path().to_path_buf(), "oracle");
        let result = tokio::time::timeout(Duration::from_secs(2), async {
            collect_runtime_step_result(|events, cancellation| {
                runtime.stream_step(request, events, cancellation)
            })
            .await
        })
        .await
        .unwrap()
        .unwrap();
        let _ = release_server.send(());

        match result.output {
            RuntimeOutput::AgentResult { result } => {
                assert_eq!(result.summary, "done answer");
            }
            other => panic!("unexpected runtime output: {other:?}"),
        }
        let request = request_rx.await.unwrap();
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
            degrade_not_abandon: false,
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
            degrade_not_abandon: false,
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

    #[test]
    fn zai_sse_frame_buffer_preserves_utf8_split_across_chunks() {
        let frame = sse_data(&serde_json::json!({
            "choices": [
                {
                    "delta": {
                        "content": "olá"
                    }
                }
            ]
        }));
        let bytes = frame.as_bytes();
        let split_index = bytes
            .windows("á".len())
            .position(|window| window == "á".as_bytes())
            .unwrap()
            + 1;
        let mut buffer = Vec::new();
        buffer.extend_from_slice(&bytes[..split_index]);

        assert_eq!(drain_next_sse_frame(&mut buffer).unwrap(), None);

        buffer.extend_from_slice(&bytes[split_index..]);

        assert_eq!(
            drain_next_sse_frame(&mut buffer).unwrap(),
            Some(SseFrame::Content("olá".to_string()))
        );
        assert!(buffer.is_empty());
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
            degrade_not_abandon: false,
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

    async fn spawn_open_sse_server(
        frames: Vec<String>,
    ) -> (SocketAddr, oneshot::Receiver<String>, oneshot::Sender<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (request_tx, request_rx) = oneshot::channel();
        let (release_tx, release_rx) = oneshot::channel();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let request = read_http_request(&mut socket).await;
            let _ = request_tx.send(request);
            socket
                .write_all(
                    b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: keep-alive\r\n\r\n",
                )
                .await
                .unwrap();
            socket.write_all(frames.join("").as_bytes()).await.unwrap();
            let _ = release_rx.await;
        });
        (addr, request_rx, release_tx)
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
            parallel_context: None,
            capability_constraints: vec![Capability::Read, Capability::Answer],
            limits: Limits::default(),
        }
    }
}
