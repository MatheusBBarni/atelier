//! Shared HTTP-runtime utilities: secret redaction and runtime-output parsing.
//!
//! These functions are the single source of truth used by [`super`] (re-exported
//! for the hooks/history modules), [`super::codex`], and the HTTP API runtime.
//! Consolidated per ADR-003 so secret-redaction logic cannot silently diverge
//! across runtime modules. The functions are pure (no side effects), so the
//! extraction is behavior-preserving.

use super::RuntimeOutput;
use anyhow::Result;

/// Parse a runtime's raw stdout into a [`RuntimeOutput`].
///
/// Tries the action contract first; otherwise interprets the payload as an
/// orchestrator decision (for the `orchestrator` agent) or an agent result,
/// surfacing a [`RuntimeOutput::ParseError`] when parsing fails.
pub(crate) fn parse_runtime_output(agent_id: &str, raw_output: String) -> Result<RuntimeOutput> {
    if let Ok(request) = crate::orchestrator::parse_contract(&raw_output) {
        return Ok(RuntimeOutput::ActionRequest { request });
    }

    if agent_id == "orchestrator" {
        match crate::orchestrator::parse_orchestrator_decision(&raw_output) {
            Ok(decision) => Ok(RuntimeOutput::OrchestratorDecision { decision }),
            Err(error) => Ok(RuntimeOutput::ParseError {
                agent: agent_id.to_string(),
                raw_output,
                diagnostic: error.to_string(),
            }),
        }
    } else {
        match crate::orchestrator::parse_agent_result(&raw_output) {
            Ok(result) => Ok(RuntimeOutput::AgentResult { result }),
            Err(error) => Ok(RuntimeOutput::ParseError {
                agent: agent_id.to_string(),
                raw_output,
                diagnostic: error.to_string(),
            }),
        }
    }
}

/// Redact both `Bearer` tokens and raw secret-prefixed tokens from `text`.
pub(crate) fn redact_sensitive_text(text: &str) -> String {
    redact_raw_secret_tokens(&redact_bearer_tokens(text))
}

/// Redact `Bearer <token>` occurrences (case-insensitive match on the keyword)
/// to `Bearer <redacted>`.
pub(crate) fn redact_bearer_tokens(text: &str) -> String {
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

/// Redact raw secret tokens (those starting with a known provider prefix) to
/// `<redacted secret>`. Tokens embedded mid-identifier are left untouched.
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

/// Find the earliest occurrence of any known secret prefix in `text`.
fn next_raw_secret_prefix(text: &str) -> Option<(usize, &'static str)> {
    const SECRET_PREFIXES: [&str; 4] = ["sk-", "zai-", "or-", "ov-"];
    let lower = text.to_ascii_lowercase();
    SECRET_PREFIXES
        .into_iter()
        .filter_map(|prefix| lower.find(prefix).map(|index| (index, prefix)))
        .min_by_key(|(index, _prefix)| *index)
}

/// Whether `character` can appear inside a secret token body.
fn is_secret_token_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orchestrator::{wrap_json_contract, AgentResult};

    #[test]
    fn redact_sensitive_text_redacts_all_known_provider_prefixes() {
        let text = "keys sk-abc123 zai-def456 or-ghi789 ov-jkl012 end";
        let redacted = redact_sensitive_text(text);
        assert_eq!(
            redacted,
            "keys <redacted secret> <redacted secret> <redacted secret> <redacted secret> end"
        );
        for token in ["sk-abc123", "zai-def456", "or-ghi789", "ov-jkl012"] {
            assert!(!redacted.contains(token), "leaked secret token: {token}");
        }
    }

    #[test]
    fn redact_bearer_tokens_redacts_authorization_header_pattern() {
        let text = "Authorization: Bearer sk-secret-value\nnext line";
        let redacted = redact_bearer_tokens(text);
        assert_eq!(redacted, "Authorization: Bearer <redacted>\nnext line");
        assert!(!redacted.contains("sk-secret-value"));
    }

    #[test]
    fn parse_runtime_output_parses_valid_agent_result() {
        let result = AgentResult::completed("explorer", "step", "done");
        let wrapped = wrap_json_contract(&result).unwrap();
        match parse_runtime_output("explorer", wrapped).unwrap() {
            RuntimeOutput::AgentResult { result: parsed } => {
                assert_eq!(parsed, result);
            }
            other => panic!("unexpected runtime output: {other:?}"),
        }
    }

    #[test]
    fn parse_runtime_output_returns_parse_error_for_invalid_json() {
        match parse_runtime_output("explorer", "plain prose".to_string()).unwrap() {
            RuntimeOutput::ParseError {
                agent,
                raw_output,
                diagnostic,
            } => {
                assert_eq!(agent, "explorer");
                assert_eq!(raw_output, "plain prose");
                assert!(diagnostic.contains("missing JSON contract"));
            }
            other => panic!("unexpected runtime output: {other:?}"),
        }
    }

    #[test]
    fn next_raw_secret_prefix_finds_each_prefix_in_sequence() {
        for prefix in ["sk-", "zai-", "or-", "ov-"] {
            let text = format!("leading {prefix}tokenbody trailing");
            let (index, found) = next_raw_secret_prefix(&text).expect("prefix should be found");
            assert_eq!(found, prefix);
            assert_eq!(&text[index..index + prefix.len()], prefix);
        }
        assert!(next_raw_secret_prefix("no secrets here").is_none());
    }

    #[test]
    fn is_secret_token_character_identifies_token_characters() {
        for character in ['a', 'Z', '9', '-', '_', '.'] {
            assert!(
                is_secret_token_character(character),
                "{character} should be a token character"
            );
        }
        for character in [' ', '\n', '"', ',', ';', '/'] {
            assert!(
                !is_secret_token_character(character),
                "{character} should not be a token character"
            );
        }
    }
}
