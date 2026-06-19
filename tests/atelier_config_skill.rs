//! Deterministic (no-LLM) drift/correctness guard for the `atelier-config-setup`
//! skill (ADR-005). Proves the shipped skill is discoverable, its schema doc
//! covers every enum variant and only real ones, every fenced `toml` block loads
//! under the real config loader, and the discovery mirrors match the canonical
//! source.

use multiagent::config::{
    load_effective_config, AgentEffort, ApprovalMode, Capability, ConfigLoadOptions, RuntimeKind,
    ToolName,
};
use multiagent::skills::{discover_skill_metadata, skill_roots_with_home, SKILL_FILE_NAME};
use serde::Serialize;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::tempdir;

const CANONICAL: &str = "skills/atelier-config-setup";

fn repo_path(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(rel)
}

fn read_repo(rel: &str) -> String {
    let path = repo_path(rel);
    fs::read_to_string(&path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
}

/// The serde (TOML) name of a unit enum variant, e.g. `Capability::McpTool` ->
/// `"mcp_tool"`, `AgentEffort::XHigh` -> `"xhigh"`. Derived from the type's own
/// `Serialize` impl so it can never disagree with the loader.
fn serde_name<T: Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .expect("serialize enum variant")
        .as_str()
        .expect("enum variant serializes to a string")
        .to_string()
}

/// Extract the bodies of every fenced code block of the given `lang`
/// (e.g. ` ```toml ` or ` ```text `).
fn fenced_blocks(markdown: &str, lang: &str) -> Vec<String> {
    let opener = format!("```{lang}");
    let mut blocks = Vec::new();
    let mut current: Option<Vec<&str>> = None;
    for line in markdown.lines() {
        match current.as_mut() {
            None => {
                if line.trim_end() == opener {
                    current = Some(Vec::new());
                }
            }
            Some(lines) => {
                if line.trim_end() == "```" {
                    blocks.push(lines.join("\n"));
                    current = None;
                } else {
                    lines.push(line);
                }
            }
        }
    }
    blocks
}

fn list_rel_files(root: &Path) -> Vec<PathBuf> {
    fn walk(dir: &Path, root: &Path, out: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(dir).expect("read_dir") {
            let path = entry.expect("dir entry").path();
            // `metadata` follows symlinks, so a symlinked mirror directory is
            // walked as the real tree it points at.
            if fs::metadata(&path).expect("metadata").is_dir() {
                walk(&path, root, out);
            } else {
                out.push(path.strip_prefix(root).expect("strip prefix").to_path_buf());
            }
        }
    }
    let mut files = Vec::new();
    walk(root, root, &mut files);
    files.sort();
    files
}

#[test]
fn skill_is_discoverable_with_valid_frontmatter() {
    // Mirror the existing discovery test pattern: lay the canonical SKILL.md into
    // a temp `.agents/skills` root and discover it through the real module.
    let skill_md = read_repo(&format!("{CANONICAL}/SKILL.md"));
    let dir = tempdir().unwrap();
    let skill_dir = dir.path().join(".agents/skills/atelier-config-setup");
    fs::create_dir_all(&skill_dir).unwrap();
    fs::write(skill_dir.join(SKILL_FILE_NAME), &skill_md).unwrap();

    let roots = skill_roots_with_home(dir.path(), None);
    let metadata = discover_skill_metadata(&roots).expect("discover skills");
    let found = metadata
        .iter()
        .find(|meta| meta.directory_name == "atelier-config-setup")
        .expect("atelier-config-setup is discoverable");

    assert_eq!(found.display_name, "atelier-config-setup");
    assert!(found
        .aliases
        .iter()
        .any(|alias| alias == "atelier-config-setup"));
    let description = found.description.as_deref().unwrap_or("");
    assert!(
        !description.trim().is_empty(),
        "discovered skill must carry a non-empty description"
    );
}

#[test]
fn schema_doc_covers_all_enum_variants_and_version() {
    let doc = read_repo(&format!("{CANONICAL}/references/config-schema.md"));

    // Every serde name across the five config enums (via the task_04 `all()`s).
    let mut expected: BTreeSet<String> = BTreeSet::new();
    expected.extend(RuntimeKind::all().iter().map(serde_name));
    expected.extend(ApprovalMode::all().iter().map(serde_name));
    expected.extend(AgentEffort::all().iter().map(serde_name));
    expected.extend(Capability::all().iter().map(serde_name));
    expected.extend(ToolName::all().iter().map(serde_name));

    // Variants the doc declares: the tokens inside its dedicated ```text blocks.
    let mut documented: BTreeSet<String> = BTreeSet::new();
    for block in fenced_blocks(&doc, "text") {
        for line in block.lines() {
            let token = line.trim();
            if !token.is_empty() {
                documented.insert(token.to_string());
            }
        }
    }

    let missing: Vec<_> = expected.difference(&documented).cloned().collect();
    let stray: Vec<_> = documented.difference(&expected).cloned().collect();
    assert!(
        missing.is_empty(),
        "enum variants missing from config-schema.md: {missing:?}"
    );
    assert!(
        stray.is_empty(),
        "stray variant strings in config-schema.md enum blocks: {stray:?}"
    );
    // Explicit guard from the mcp-integration packet.
    assert!(
        documented.contains("mcp_tool"),
        "config-schema.md must document the mcp_tool capability"
    );

    assert!(
        doc.contains("schema_version = 1"),
        "config-schema.md must document schema_version = 1"
    );
}

fn assert_block_loads(toml: &str, label: &str) {
    let dir = tempdir().unwrap();
    // A non-default filename so the loader's local `./atelier.toml` pass ignores
    // it; pass it explicitly so the real home config is never merged in.
    let candidate = dir.path().join("candidate.toml");
    fs::write(&candidate, toml).unwrap();
    let result = load_effective_config(ConfigLoadOptions {
        working_directory: dir.path().to_path_buf(),
        config_path: Some(candidate),
    });
    assert!(result.is_ok(), "{label} failed to load: {:?}", result.err());
}

#[test]
fn every_toml_block_loads_under_the_config_loader() {
    let sources = [
        format!("{CANONICAL}/SKILL.md"),
        format!("{CANONICAL}/references/config-schema.md"),
        format!("{CANONICAL}/references/presets.md"),
    ];
    let mut total = 0;
    for src in &sources {
        let markdown = read_repo(src);
        let blocks = fenced_blocks(&markdown, "toml");
        assert!(!blocks.is_empty(), "{src} has no toml blocks");
        for (index, block) in blocks.iter().enumerate() {
            assert_block_loads(block, &format!("{src} block {}", index + 1));
            total += 1;
        }
    }
    assert!(
        total >= 11,
        "expected every documented toml block to load; only saw {total}"
    );
}

#[test]
fn presets_declare_runtime_and_orchestrator_without_secrets() {
    let markdown = read_repo(&format!("{CANONICAL}/references/presets.md"));
    let blocks = fenced_blocks(&markdown, "toml");
    assert_eq!(blocks.len(), 4, "expected the four named presets");
    for (index, block) in blocks.iter().enumerate() {
        let label = format!("preset {}", index + 1);
        assert!(block.contains("type = "), "{label} declares no runtime");
        assert!(
            block.contains("[agents.orchestrator]"),
            "{label} has no orchestrator agent"
        );
        // Credentials are env-var names only — never inlined secret values.
        assert!(
            !block.contains("api_key ="),
            "{label} inlines an api_key value"
        );
        assert!(
            !block.to_ascii_lowercase().contains("sk-"),
            "{label} contains a secret-like literal"
        );
    }
}

#[test]
fn readme_documents_install_and_invocation() {
    // task_08: the README is the human-facing discovery surface; guard that it
    // can't silently drop the install command or the skill name/invocation.
    let readme = read_repo("README.md");
    assert!(
        readme.contains("npx skills add MatheusBBarni/atelier atelier-config-setup"),
        "README must document the name-targeted install command"
    );
    assert!(
        readme.contains("atelier-config-setup"),
        "README must name the skill"
    );
    assert!(
        readme.contains("/skill:atelier-config-setup")
            || readme.contains("set up my atelier config"),
        "README must document how to invoke the skill"
    );
}

#[test]
fn mirrors_equal_canonical_source() {
    let canonical = repo_path(CANONICAL);
    let canonical_files = list_rel_files(&canonical);
    assert!(
        !canonical_files.is_empty(),
        "canonical skill tree is empty: {}",
        canonical.display()
    );

    for mirror in [
        repo_path(".agents/skills/atelier-config-setup"),
        repo_path(".claude/skills/atelier-config-setup"),
    ] {
        assert!(mirror.exists(), "mirror missing: {}", mirror.display());
        assert_eq!(
            canonical_files,
            list_rel_files(&mirror),
            "file set differs for mirror {}",
            mirror.display()
        );
        for rel in &canonical_files {
            let want = fs::read(canonical.join(rel)).unwrap();
            let got = fs::read(mirror.join(rel)).unwrap();
            assert_eq!(
                want,
                got,
                "{} differs in mirror {}",
                rel.display(),
                mirror.display()
            );
        }
    }
}
