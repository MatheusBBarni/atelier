use anyhow::{anyhow, bail, Context, Result};
use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

const CODEMAP_FILE: &str = "codemap.md";
const STATE_SCHEMA_VERSION: u32 = 1;

const EXCLUDED_DIRS: &[&str] = &[
    ".git",
    ".hg",
    ".atelier",
    ".multiagent", // legacy data root pre-rename; keep old session logs out of the codemap
    ".next",
    ".pytest_cache",
    ".ruff_cache",
    ".mypy_cache",
    ".turbo",
    ".venv",
    "__pycache__",
    "build",
    "coverage",
    "deps",
    "dist",
    "node_modules",
    "Pods",
    "target",
    "vendor",
    "venv",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
#[value(rename_all = "lower")]
pub enum CodemapCommand {
    Init,
    Changes,
    Update,
}

impl CodemapCommand {
    fn label(self) -> &'static str {
        match self {
            Self::Init => "init",
            Self::Changes => "changes",
            Self::Update => "update",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodemapSummary {
    pub command: CodemapCommand,
    pub state_path: PathBuf,
    pub scanned_files: usize,
    pub stale_maps: Vec<PathBuf>,
    pub maps_written: Vec<PathBuf>,
    pub maps_removed: Vec<PathBuf>,
    pub state_written: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
struct CodemapState {
    schema_version: u32,
    file_hashes: BTreeMap<String, String>,
}

impl CodemapState {
    fn from_snapshot(snapshot: &CodemapSnapshot) -> Self {
        let file_hashes = snapshot
            .files
            .iter()
            .map(|(path, file)| (path.clone(), file.sha256.clone()))
            .collect();
        Self {
            schema_version: STATE_SCHEMA_VERSION,
            file_hashes,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CodemapSnapshot {
    files: BTreeMap<String, CodemapFileEntry>,
    folders: BTreeMap<String, Vec<CodemapFileEntry>>,
}

impl CodemapSnapshot {
    fn from_files(files: BTreeMap<String, CodemapFileEntry>) -> Self {
        let mut folders: BTreeMap<String, Vec<CodemapFileEntry>> = BTreeMap::new();
        for file in files.values() {
            folders
                .entry(file.folder.clone())
                .or_default()
                .push(file.clone());
        }
        Self { files, folders }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CodemapFileEntry {
    relative_path: String,
    folder: String,
    file_name: String,
    byte_len: u64,
    sha256: String,
}

pub fn run_codemap(root: &Path, command: CodemapCommand) -> Result<CodemapSummary> {
    let root = normalize_root(root)?;
    let snapshot = scan_workspace(&root)?;
    let previous_state = read_state(&root)?;
    let stale_maps = stale_maps(&root, previous_state.as_ref(), &snapshot);
    let state_path = state_path(&root);

    match command {
        CodemapCommand::Changes => Ok(CodemapSummary {
            command,
            state_path,
            scanned_files: snapshot.files.len(),
            stale_maps,
            maps_written: Vec::new(),
            maps_removed: Vec::new(),
            state_written: false,
        }),
        CodemapCommand::Init | CodemapCommand::Update => {
            let maps_written = write_maps(&root, &snapshot)?;
            let maps_removed = remove_orphaned_maps(&root, previous_state.as_ref(), &snapshot)?;
            write_state(&state_path, &CodemapState::from_snapshot(&snapshot))?;
            Ok(CodemapSummary {
                command,
                state_path,
                scanned_files: snapshot.files.len(),
                stale_maps,
                maps_written,
                maps_removed,
                state_written: true,
            })
        }
    }
}

pub fn render_summary(summary: &CodemapSummary) -> String {
    let mut output = String::new();
    output.push_str(&format!(
        "codemap {}: scanned {} file(s)\n",
        summary.command.label(),
        summary.scanned_files
    ));
    output.push_str(&format!("state: {}\n", summary.state_path.display()));

    if summary.stale_maps.is_empty() {
        output.push_str("stale maps: none\n");
    } else {
        output.push_str(&format!("stale maps: {}\n", summary.stale_maps.len()));
        for path in &summary.stale_maps {
            output.push_str(&format!("- {}\n", path.display()));
        }
    }

    if !summary.maps_written.is_empty() {
        output.push_str(&format!("maps written: {}\n", summary.maps_written.len()));
        for path in &summary.maps_written {
            output.push_str(&format!("- {}\n", path.display()));
        }
    }

    if !summary.maps_removed.is_empty() {
        output.push_str(&format!("maps removed: {}\n", summary.maps_removed.len()));
        for path in &summary.maps_removed {
            output.push_str(&format!("- {}\n", path.display()));
        }
    }

    output
}

fn normalize_root(root: &Path) -> Result<PathBuf> {
    if !root.exists() {
        bail!("codemap root does not exist: {}", root.display());
    }
    if !root.is_dir() {
        bail!("codemap root is not a directory: {}", root.display());
    }
    root.canonicalize()
        .with_context(|| format!("failed to canonicalize {}", root.display()))
}

fn scan_workspace(root: &Path) -> Result<CodemapSnapshot> {
    let mut files = BTreeMap::new();
    scan_directory(root, root, &mut files)?;
    Ok(CodemapSnapshot::from_files(files))
}

fn scan_directory(
    root: &Path,
    directory: &Path,
    files: &mut BTreeMap<String, CodemapFileEntry>,
) -> Result<()> {
    let mut entries = fs::read_dir(directory)
        .with_context(|| format!("failed to read directory {}", directory.display()))?
        .collect::<std::result::Result<Vec<_>, _>>()
        .with_context(|| format!("failed to read entries in {}", directory.display()))?;
    entries.sort_by_key(|entry| entry.path());

    for entry in entries {
        let path = entry.path();
        let file_type = entry
            .file_type()
            .with_context(|| format!("failed to read file type for {}", path.display()))?;
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            if is_excluded_directory(&path) {
                continue;
            }
            scan_directory(root, &path, files)?;
            continue;
        }
        if file_type.is_file() && !is_excluded_file(root, &path)? {
            let metadata = entry
                .metadata()
                .with_context(|| format!("failed to read metadata for {}", path.display()))?;
            let file = hash_file(root, &path, metadata.len())?;
            files.insert(file.relative_path.clone(), file);
        }
    }
    Ok(())
}

fn is_excluded_directory(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    EXCLUDED_DIRS.contains(&name)
}

fn is_excluded_file(root: &Path, path: &Path) -> Result<bool> {
    if path.file_name().and_then(|name| name.to_str()) == Some(CODEMAP_FILE) {
        return Ok(true);
    }
    let relative = path
        .strip_prefix(root)
        .with_context(|| format!("failed to relativize {}", path.display()))?;
    Ok(relative.components().any(|component| match component {
        Component::Normal(name) => {
            let name = name.to_string_lossy();
            EXCLUDED_DIRS.contains(&name.as_ref())
        }
        _ => false,
    }))
}

fn hash_file(root: &Path, path: &Path, byte_len: u64) -> Result<CodemapFileEntry> {
    let relative = relative_path(root, path)?;
    let contents = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    let sha256 = Sha256::digest(&contents)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let relative_path = relative_to_string(&relative);
    let folder = relative
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .map(relative_to_string)
        .unwrap_or_else(|| ".".to_string());
    let file_name = relative
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| anyhow!("failed to read file name for {}", path.display()))?
        .to_string();

    Ok(CodemapFileEntry {
        relative_path,
        folder,
        file_name,
        byte_len,
        sha256,
    })
}

fn relative_path(root: &Path, path: &Path) -> Result<PathBuf> {
    path.strip_prefix(root)
        .with_context(|| format!("failed to relativize {}", path.display()))
        .map(PathBuf::from)
}

fn relative_to_string(path: &Path) -> String {
    let value = path.to_string_lossy().replace('\\', "/");
    if value.is_empty() {
        ".".to_string()
    } else {
        value
    }
}

fn stale_maps(
    root: &Path,
    previous_state: Option<&CodemapState>,
    snapshot: &CodemapSnapshot,
) -> Vec<PathBuf> {
    let Some(previous_state) = previous_state else {
        return snapshot
            .folders
            .keys()
            .map(|folder| map_path(root, folder))
            .collect();
    };

    let current_state = CodemapState::from_snapshot(snapshot);
    let mut stale_folders = BTreeSet::new();

    for (path, current_hash) in &current_state.file_hashes {
        match previous_state.file_hashes.get(path) {
            Some(previous_hash) if previous_hash == current_hash => {}
            _ => {
                stale_folders.insert(folder_for_relative_path(path));
            }
        }
    }

    for path in previous_state.file_hashes.keys() {
        if !current_state.file_hashes.contains_key(path) {
            stale_folders.insert(folder_for_relative_path(path));
        }
    }

    for folder in snapshot.folders.keys() {
        if !map_path(root, folder).exists() {
            stale_folders.insert(folder.clone());
        }
    }

    stale_folders
        .into_iter()
        .map(|folder| map_path(root, &folder))
        .collect()
}

fn folder_for_relative_path(relative_path: &str) -> String {
    Path::new(relative_path)
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .map(relative_to_string)
        .unwrap_or_else(|| ".".to_string())
}

fn write_maps(root: &Path, snapshot: &CodemapSnapshot) -> Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    for (folder, files) in &snapshot.folders {
        let path = map_path(root, folder);
        let contents = render_folder_map(folder, files);
        fs::write(&path, contents.as_bytes())
            .with_context(|| format!("failed to write {}", path.display()))?;
        paths.push(path);
    }
    Ok(paths)
}

fn remove_orphaned_maps(
    root: &Path,
    previous_state: Option<&CodemapState>,
    snapshot: &CodemapSnapshot,
) -> Result<Vec<PathBuf>> {
    let Some(previous_state) = previous_state else {
        return Ok(Vec::new());
    };
    let mut previous_folders = BTreeSet::new();
    for path in previous_state.file_hashes.keys() {
        previous_folders.insert(folder_for_relative_path(path));
    }

    let mut removed = Vec::new();
    for folder in previous_folders {
        if snapshot.folders.contains_key(&folder) {
            continue;
        }
        let path = map_path(root, &folder);
        if path.exists() {
            fs::remove_file(&path)
                .with_context(|| format!("failed to remove {}", path.display()))?;
            removed.push(path);
        }
    }
    Ok(removed)
}

fn render_folder_map(folder: &str, files: &[CodemapFileEntry]) -> String {
    let mut output = String::new();
    output.push_str(&format!("# Codemap: {folder}\n\n"));
    output.push_str(
        "Generated by `atelier --codemap update`. This is a visible, user-editable repository map; rerunning codemap may regenerate it.\n\n",
    );
    output.push_str("## Files\n\n");
    for file in files {
        output.push_str(&format!(
            "- `{}` - {} bytes - sha256 `{}`\n",
            file.file_name,
            file.byte_len,
            short_hash(&file.sha256)
        ));
    }
    output
}

fn short_hash(hash: &str) -> &str {
    hash.get(..12).unwrap_or(hash)
}

fn map_path(root: &Path, folder: &str) -> PathBuf {
    if folder == "." {
        root.join(CODEMAP_FILE)
    } else {
        root.join(folder).join(CODEMAP_FILE)
    }
}

fn state_path(root: &Path) -> PathBuf {
    root.join(".atelier").join("codemap.json")
}

fn read_state(root: &Path) -> Result<Option<CodemapState>> {
    let path = state_path(root);
    if !path.exists() {
        return Ok(None);
    }
    let contents =
        fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))?;
    let state: CodemapState = serde_json::from_str(&contents)
        .with_context(|| format!("failed to parse {}", path.display()))?;
    if state.schema_version != STATE_SCHEMA_VERSION {
        bail!(
            "unsupported codemap schema_version {} in {}",
            state.schema_version,
            path.display()
        );
    }
    Ok(Some(state))
}

fn write_state(path: &Path, state: &CodemapState) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
        set_private_dir_permissions(parent)?;
    }
    let contents = serde_json::to_vec_pretty(state)?;
    fs::write(path, contents).with_context(|| format!("failed to write {}", path.display()))?;
    set_private_file_permissions(path)
}

fn set_private_dir_permissions(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
            .with_context(|| format!("failed to set private permissions on {}", path.display()))?;
    }
    Ok(())
}

fn set_private_file_permissions(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .with_context(|| format!("failed to set private permissions on {}", path.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn init_writes_state_and_folder_maps() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("README.md"), "readme").unwrap();
        fs::write(dir.path().join("src/lib.rs"), "pub fn demo() {}").unwrap();

        let summary = run_codemap(dir.path(), CodemapCommand::Init).unwrap();

        assert_eq!(summary.scanned_files, 2);
        assert!(dir.path().join(".atelier/codemap.json").exists());
        assert!(dir.path().join("codemap.md").exists());
        assert!(dir.path().join("src/codemap.md").exists());
    }

    #[test]
    fn scan_excludes_generated_and_dependency_paths() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::create_dir_all(dir.path().join(".git")).unwrap();
        fs::create_dir_all(dir.path().join(".atelier")).unwrap();
        fs::create_dir_all(dir.path().join("target/debug")).unwrap();
        fs::create_dir_all(dir.path().join("node_modules/pkg")).unwrap();
        fs::create_dir_all(dir.path().join("vendor/lib")).unwrap();
        fs::write(dir.path().join("src/main.rs"), "fn main() {}").unwrap();
        fs::write(dir.path().join("src/codemap.md"), "manual").unwrap();
        fs::write(dir.path().join(".git/config"), "git").unwrap();
        fs::write(dir.path().join(".atelier/old.json"), "{}").unwrap();
        fs::write(dir.path().join("target/debug/app"), "bin").unwrap();
        fs::write(dir.path().join("node_modules/pkg/index.js"), "dep").unwrap();
        fs::write(dir.path().join("vendor/lib/lib.rs"), "dep").unwrap();

        let summary = run_codemap(dir.path(), CodemapCommand::Init).unwrap();
        let state = read_state(dir.path()).unwrap().unwrap();
        let map = fs::read_to_string(dir.path().join("src/codemap.md")).unwrap();

        assert_eq!(summary.scanned_files, 1);
        assert!(state.file_hashes.contains_key("src/main.rs"));
        assert!(!map.contains("codemap.md"));
    }

    #[test]
    fn changes_reports_stale_map_after_file_edit_without_writing() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("src/lib.rs"), "pub fn one() {}").unwrap();
        run_codemap(dir.path(), CodemapCommand::Init).unwrap();
        fs::write(dir.path().join("src/lib.rs"), "pub fn two() {}").unwrap();

        let summary = run_codemap(dir.path(), CodemapCommand::Changes).unwrap();

        assert_eq!(summary.maps_written.len(), 0);
        assert_eq!(
            summary.stale_maps,
            vec![dir
                .path()
                .join("src/codemap.md")
                .canonicalize()
                .unwrap_or_else(|_| dir.path().join("src/codemap.md"))]
        );
    }

    #[test]
    fn update_refreshes_stale_state() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("src/lib.rs"), "pub fn one() {}").unwrap();
        run_codemap(dir.path(), CodemapCommand::Init).unwrap();
        fs::write(dir.path().join("src/lib.rs"), "pub fn two() {}").unwrap();

        let update = run_codemap(dir.path(), CodemapCommand::Update).unwrap();
        let changes = run_codemap(dir.path(), CodemapCommand::Changes).unwrap();

        assert_eq!(update.stale_maps.len(), 1);
        assert!(changes.stale_maps.is_empty());
    }

    #[test]
    fn update_removes_map_when_folder_no_longer_has_tracked_files() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("src/lib.rs"), "pub fn one() {}").unwrap();
        run_codemap(dir.path(), CodemapCommand::Init).unwrap();
        fs::remove_file(dir.path().join("src/lib.rs")).unwrap();

        let update = run_codemap(dir.path(), CodemapCommand::Update).unwrap();
        let expected_map = dir.path().canonicalize().unwrap().join("src/codemap.md");

        assert_eq!(update.maps_removed, vec![expected_map]);
    }
}
