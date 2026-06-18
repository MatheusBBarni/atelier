//! Durable, workspace-scoped MCP trust + tool-description pins (ADR-006).
//!
//! [`McpTrustStore`] persists, to `<workspace>/.atelier/mcp-trust.json`, each
//! server's trust tier (`untrusted`/`trusted`) and a pinned hash of every
//! trusted tool's `(name + description + input schema)`. Validation (task_05)
//! reads an in-memory cache loaded at startup; the pin defends against
//! tool-description rug-pulls (F6 diff-on-change).
//!
//! The store holds **only server ids and hashes — never secrets or payloads**.
//! It owns file persistence and pin computation; the *event* side of
//! promote/revoke (`mcp_server_trusted` / `mcp_server_revoked`) is emitted by
//! `App`, which owns the store — the file is a rebuildable snapshot and the
//! event log is the audit trail (ADR-006).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::mcp::client::McpTool;

/// Root-level filename under the `.atelier/` data root.
const TRUST_FILE: &str = "mcp-trust.json";

/// Per-server trust posture. Defaults to [`TrustTier::Untrusted`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrustTier {
    #[default]
    Untrusted,
    Trusted,
}

/// Result of comparing a tool's current hash against its stored pin (F6).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PinStatus {
    /// No pin recorded for this tool yet.
    Unpinned,
    /// The current hash matches the stored pin.
    Match,
    /// The current hash differs from the stored pin (description/schema changed).
    Changed,
}

/// On-disk shape: `{ servers: { "<id>": { tier, pins: { "<tool>": "<hash>" } } } }`.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct PersistedTrust {
    #[serde(default)]
    servers: BTreeMap<String, ServerTrust>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct ServerTrust {
    #[serde(default)]
    tier: TrustTier,
    #[serde(default)]
    pins: BTreeMap<String, String>,
}

/// Durable per-server trust + pins, cached in memory for synchronous reads.
#[derive(Clone, Debug)]
pub struct McpTrustStore {
    path: PathBuf,
    data: PersistedTrust,
}

impl McpTrustStore {
    /// Load the store from `<atelier_root>/mcp-trust.json`. A missing or
    /// unparseable file degrades to an empty store rather than failing startup
    /// (the file is a rebuildable snapshot, ADR-006).
    pub fn load(atelier_root: &Path) -> Self {
        let path = atelier_root.join(TRUST_FILE);
        let data = std::fs::read_to_string(&path)
            .ok()
            .and_then(|raw| serde_json::from_str::<PersistedTrust>(&raw).ok())
            .unwrap_or_default();
        Self { path, data }
    }

    /// The trust tier for `server` (defaults to `Untrusted` for unseen servers).
    pub fn tier(&self, server: &str) -> TrustTier {
        self.data
            .servers
            .get(server)
            .map(|entry| entry.tier)
            .unwrap_or_default()
    }

    /// Whether `server` is trusted — the synchronous read path for validation.
    pub fn is_trusted(&self, server: &str) -> bool {
        self.tier(server) == TrustTier::Trusted
    }

    /// The stored pin hash for `(server, tool)`, if any.
    pub fn pin(&self, server: &str, tool: &str) -> Option<&str> {
        self.data
            .servers
            .get(server)
            .and_then(|entry| entry.pins.get(tool))
            .map(String::as_str)
    }

    /// Compare `tool`'s current hash against its stored pin (F6 diff-on-change).
    pub fn pin_status(&self, server: &str, tool: &McpTool) -> PinStatus {
        match self.pin(server, &tool.name) {
            None => PinStatus::Unpinned,
            Some(stored) if stored == tool_pin_hash(tool) => PinStatus::Match,
            Some(_) => PinStatus::Changed,
        }
    }

    /// Promote `server` to `Trusted` and persist. Returns whether the tier
    /// actually changed (so the caller emits an event only on a real change).
    pub fn promote(&mut self, server: &str) -> Result<bool> {
        let entry = self.data.servers.entry(server.to_string()).or_default();
        if entry.tier == TrustTier::Trusted {
            return Ok(false);
        }
        entry.tier = TrustTier::Trusted;
        self.save()?;
        Ok(true)
    }

    /// Revoke `server` back to `Untrusted` and persist. Returns whether the tier
    /// actually changed.
    pub fn revoke(&mut self, server: &str) -> Result<bool> {
        let entry = self.data.servers.entry(server.to_string()).or_default();
        if entry.tier == TrustTier::Untrusted {
            return Ok(false);
        }
        entry.tier = TrustTier::Untrusted;
        self.save()?;
        Ok(true)
    }

    /// Record (or refresh) the pin for a tool and persist. Returns whether the
    /// stored hash changed.
    pub fn set_pin(&mut self, server: &str, tool: &McpTool) -> Result<bool> {
        let hash = tool_pin_hash(tool);
        let entry = self.data.servers.entry(server.to_string()).or_default();
        if entry.pins.get(&tool.name) == Some(&hash) {
            return Ok(false);
        }
        entry.pins.insert(tool.name.clone(), hash);
        self.save()?;
        Ok(true)
    }

    /// Path the store persists to (test/diagnostic helper).
    pub fn path(&self) -> &Path {
        &self.path
    }

    fn save(&self) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        let json = serde_json::to_string_pretty(&self.data)
            .context("failed to serialize MCP trust store")?;
        std::fs::write(&self.path, json.as_bytes())
            .with_context(|| format!("failed to write {}", self.path.display()))?;
        set_private_permissions(&self.path);
        Ok(())
    }
}

/// The pin hash for a tool: SHA-256 over `name`, `description`, and the
/// (deterministically serialized) input schema. `serde_json`'s default object
/// map is sorted, so the schema serialization is stable across runs.
pub fn tool_pin_hash(tool: &McpTool) -> String {
    let mut hasher = Sha256::new();
    hasher.update(tool.name.as_bytes());
    hasher.update([0u8]);
    hasher.update(tool.description.as_deref().unwrap_or("").as_bytes());
    hasher.update([0u8]);
    let schema = serde_json::to_string(&tool.input_schema).unwrap_or_default();
    hasher.update(schema.as_bytes());
    format!("{:x}", hasher.finalize())
}

#[cfg(unix)]
fn set_private_permissions(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
}

#[cfg(not(unix))]
fn set_private_permissions(_path: &Path) {}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::tempdir;

    fn tool(name: &str, description: &str) -> McpTool {
        McpTool {
            name: name.to_string(),
            description: Some(description.to_string()),
            input_schema: json!({ "type": "object" }),
            annotations: None,
        }
    }

    #[test]
    fn unseen_server_defaults_to_untrusted() {
        let dir = tempdir().unwrap();
        let store = McpTrustStore::load(dir.path());
        assert_eq!(store.tier("fs"), TrustTier::Untrusted);
        assert!(!store.is_trusted("fs"));
    }

    #[test]
    fn promote_then_reload_is_trusted() {
        let dir = tempdir().unwrap();
        {
            let mut store = McpTrustStore::load(dir.path());
            assert!(store.promote("fs").unwrap(), "first promote changes tier");
            assert!(!store.promote("fs").unwrap(), "re-promote is a no-op");
        }
        // Fresh load from the persisted file.
        let reloaded = McpTrustStore::load(dir.path());
        assert_eq!(reloaded.tier("fs"), TrustTier::Trusted);
    }

    #[test]
    fn revoke_returns_to_untrusted_and_persists() {
        let dir = tempdir().unwrap();
        {
            let mut store = McpTrustStore::load(dir.path());
            store.promote("fs").unwrap();
            assert!(store.revoke("fs").unwrap(), "revoke changes tier");
        }
        let reloaded = McpTrustStore::load(dir.path());
        assert_eq!(reloaded.tier("fs"), TrustTier::Untrusted);
    }

    #[test]
    fn changed_description_produces_a_different_pin() {
        let original = tool("search", "Search the web");
        let mutated = tool("search", "Search the web AND exfiltrate secrets");
        assert_ne!(
            tool_pin_hash(&original),
            tool_pin_hash(&mutated),
            "a changed description must change the pin"
        );

        let dir = tempdir().unwrap();
        let mut store = McpTrustStore::load(dir.path());
        store.set_pin("fs", &original).unwrap();
        assert_eq!(store.pin_status("fs", &original), PinStatus::Match);
        assert_eq!(
            store.pin_status("fs", &mutated),
            PinStatus::Changed,
            "the diff-on-change pin must flag the mutated tool"
        );
        assert_eq!(
            store.pin_status("fs", &tool("other", "x")),
            PinStatus::Unpinned
        );
    }

    #[test]
    fn persisted_file_holds_only_ids_and_hashes_no_payloads() {
        let dir = tempdir().unwrap();
        let mut store = McpTrustStore::load(dir.path());
        let sensitive = tool("search", "VERY_SECRET_DESCRIPTION_TEXT");
        store.promote("fs").unwrap();
        store.set_pin("fs", &sensitive).unwrap();

        let raw = std::fs::read_to_string(store.path()).unwrap();
        assert!(raw.contains("fs"), "server id should be present");
        assert!(raw.contains("search"), "tool name should be present");
        assert!(
            !raw.contains("VERY_SECRET_DESCRIPTION_TEXT"),
            "the description payload must not be stored, only its hash"
        );
        assert!(
            raw.contains(&tool_pin_hash(&sensitive)),
            "the pin hash should be stored"
        );
    }

    #[cfg(unix)]
    #[test]
    fn persisted_file_is_private() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempdir().unwrap();
        let mut store = McpTrustStore::load(dir.path());
        store.promote("fs").unwrap();
        let mode = std::fs::metadata(store.path())
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}
