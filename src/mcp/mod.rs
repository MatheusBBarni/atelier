//! Harness-owned Model Context Protocol (MCP) client.
//!
//! atelier brokers every MCP interaction through its own client rather than
//! letting each agent CLI speak MCP natively. This module owns everything that
//! is network- or subprocess-facing; nothing here leaks the underlying
//! [`rmcp`] SDK surface past the [`McpClient`] trait, which is the swap/mock
//! seam that downstream tasks (supervisor, action handlers, trust store) bind
//! to instead of the SDK directly.
//!
//! See `.compozy/tasks/mcp-integration/_techspec.md` and ADR-004 / ADR-008 for
//! the architectural context.

pub mod client;
pub mod fake_server;
pub mod supervisor;
pub mod trust_store;

pub use client::{McpClient, McpResource, McpTool, McpToolResult, RmcpClient};
pub use fake_server::FakeMcpServer;
pub use supervisor::{
    McpHandle, McpSupervisor, ToolCatalog, ToolCatalogServer, DEFAULT_MCP_CALL_TIMEOUT,
};
pub use trust_store::{tool_pin_hash, McpTrustStore, PinStatus, TrustTier};
