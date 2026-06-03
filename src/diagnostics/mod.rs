use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Info,
    Warning,
    Error,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Diagnostic {
    pub severity: Severity,
    pub message: String,
    pub remediation: Option<String>,
    pub context: Option<Value>,
}

impl Diagnostic {
    pub fn error(message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Error,
            message: message.into(),
            remediation: None,
            context: None,
        }
    }

    pub fn warning(message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Warning,
            message: message.into(),
            remediation: None,
            context: None,
        }
    }

    pub fn info(message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Info,
            message: message.into(),
            remediation: None,
            context: None,
        }
    }
}

pub fn redact_authorization_headers(input: &str) -> String {
    input
        .lines()
        .map(|line| {
            let trimmed = line.trim_start();
            if trimmed.to_ascii_lowercase().starts_with("authorization:") {
                let indent = &line[..line.len() - trimmed.len()];
                format!("{indent}Authorization: <redacted>")
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}
