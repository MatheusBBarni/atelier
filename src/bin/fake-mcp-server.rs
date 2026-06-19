//! Test fixture binary: a deterministic stdio MCP server.
//!
//! Integration tests spawn this over real stdio (located via
//! `CARGO_BIN_EXE_fake-mcp-server`) to exercise the real `initialize` handshake
//! and JSON-RPC framing. It is not part of the shipped product — the release
//! pipeline builds only `--bin atelier`. See `src/mcp/fake_server.rs`.

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    multiagent::mcp::fake_server::run_stdio().await
}
