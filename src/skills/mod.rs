use serde::{Deserialize, Serialize};
use std::error::Error;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

pub const SKILL_FILE_NAME: &str = "SKILL.md";
pub const SKILL_DISCOVERY_MAX_DEPTH: usize = 4;
pub const SKILL_SUGGESTION_CACHE_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SkillSourceTag {
    Project,
    Personal,
}

impl SkillSourceTag {
    pub fn label(self) -> &'static str {
        match self {
            SkillSourceTag::Project => "Project",
            SkillSourceTag::Personal => "Personal",
        }
    }

    pub fn scope_rank(self) -> u8 {
        match self {
            SkillSourceTag::Project => 0,
            SkillSourceTag::Personal => 1,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SkillRootFamily {
    Agents,
    Claude,
}

impl SkillRootFamily {
    pub fn family_rank(self) -> u8 {
        match self {
            SkillRootFamily::Agents => 0,
            SkillRootFamily::Claude => 1,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillRoot {
    pub path: PathBuf,
    pub source_tag: SkillSourceTag,
    pub family: SkillRootFamily,
    pub scope_rank: u8,
    pub family_rank: u8,
    pub precedence: usize,
    pub source_origin: String,
}

impl SkillRoot {
    fn project(
        project_root: &Path,
        family: SkillRootFamily,
        relative_path: &str,
        precedence: usize,
    ) -> Self {
        Self::new(
            project_root.join(relative_path),
            SkillSourceTag::Project,
            family,
            relative_path.to_string(),
            precedence,
        )
    }

    fn personal(
        home_root: &Path,
        family: SkillRootFamily,
        relative_path: &str,
        source_origin: &str,
        precedence: usize,
    ) -> Self {
        Self::new(
            home_root.join(relative_path),
            SkillSourceTag::Personal,
            family,
            source_origin.to_string(),
            precedence,
        )
    }

    fn new(
        path: PathBuf,
        source_tag: SkillSourceTag,
        family: SkillRootFamily,
        source_origin: String,
        precedence: usize,
    ) -> Self {
        Self {
            path,
            source_tag,
            family,
            scope_rank: source_tag.scope_rank(),
            family_rank: family.family_rank(),
            precedence,
            source_origin,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillManifest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillIdentity {
    pub canonical_id: String,
    pub source_origin: String,
    pub source_path: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillMetadata {
    pub identity: SkillIdentity,
    pub display_name: String,
    pub description: Option<String>,
    pub aliases: Vec<String>,
    pub directory_name: String,
    pub source_tag: SkillSourceTag,
    pub source_origin: String,
    pub root_precedence: usize,
    pub skill_dir: PathBuf,
    pub skill_file: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillSuggestion {
    pub alias: String,
    pub display_name: String,
    pub description: Option<String>,
    pub source_tag: SkillSourceTag,
    pub source_origin: String,
    pub canonical_id: String,
    pub skill_dir: PathBuf,
    pub source_path: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillSuggestionCache {
    pub schema_version: u32,
    pub suggestions: Vec<SkillSuggestion>,
}

impl SkillSuggestionCache {
    pub fn new(suggestions: Vec<SkillSuggestion>) -> Self {
        Self {
            schema_version: SKILL_SUGGESTION_CACHE_SCHEMA_VERSION,
            suggestions,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SkillDiscoveryError {
    InvalidFrontmatter { path: PathBuf, message: String },
    ReadSkillFile { path: PathBuf, message: String },
    MissingDirectoryName { path: PathBuf },
    MissingAlias { path: PathBuf },
}

impl fmt::Display for SkillDiscoveryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SkillDiscoveryError::InvalidFrontmatter { path, message } => write!(
                formatter,
                "invalid YAML frontmatter in {}: {}",
                path.display(),
                message
            ),
            SkillDiscoveryError::ReadSkillFile { path, message } => {
                write!(formatter, "failed to read {}: {}", path.display(), message)
            }
            SkillDiscoveryError::MissingDirectoryName { path } => {
                write!(
                    formatter,
                    "skill directory has no valid UTF-8 name: {}",
                    path.display()
                )
            }
            SkillDiscoveryError::MissingAlias { path } => write!(
                formatter,
                "skill has no usable alias from frontmatter name or directory name: {}",
                path.display()
            ),
        }
    }
}

impl Error for SkillDiscoveryError {}

pub fn skill_roots(project_root: &Path) -> Vec<SkillRoot> {
    let home_root = dirs::home_dir();
    skill_roots_with_home(project_root, home_root.as_deref())
}

pub fn skill_roots_with_home(project_root: &Path, home_root: Option<&Path>) -> Vec<SkillRoot> {
    let mut roots = vec![
        SkillRoot::project(project_root, SkillRootFamily::Agents, ".agents/skills", 0),
        SkillRoot::project(project_root, SkillRootFamily::Claude, ".claude/skills", 1),
    ];

    if let Some(home_root) = home_root {
        roots.extend([
            SkillRoot::personal(
                home_root,
                SkillRootFamily::Agents,
                ".agents/skills",
                "~/.agents/skills",
                2,
            ),
            SkillRoot::personal(
                home_root,
                SkillRootFamily::Claude,
                ".claude/skills",
                "~/.claude/skills",
                3,
            ),
        ]);
    }

    roots
}

pub fn parse_skill_manifest(
    contents: &str,
    skill_file: &Path,
) -> Result<Option<SkillManifest>, SkillDiscoveryError> {
    let Some(yaml) = frontmatter_yaml(contents, skill_file)? else {
        return Ok(None);
    };
    if yaml.trim().is_empty() {
        return Ok(Some(SkillManifest::default()));
    }

    let mut manifest: SkillManifest =
        serde_norway::from_str(&yaml).map_err(|error| SkillDiscoveryError::InvalidFrontmatter {
            path: skill_file.to_path_buf(),
            message: error.to_string(),
        })?;
    manifest.name = clean_optional_string(manifest.name);
    manifest.description = clean_optional_string(manifest.description);
    Ok(Some(manifest))
}

pub fn skill_metadata_from_dir(
    skill_dir: &Path,
    root: &SkillRoot,
) -> Result<Option<SkillMetadata>, SkillDiscoveryError> {
    if !skill_dir.is_dir() {
        return Ok(None);
    }

    let skill_file = skill_dir.join(SKILL_FILE_NAME);
    if !skill_file.is_file() {
        return Ok(None);
    }

    let contents =
        fs::read_to_string(&skill_file).map_err(|error| SkillDiscoveryError::ReadSkillFile {
            path: skill_file.clone(),
            message: error.to_string(),
        })?;
    let manifest = parse_skill_manifest(&contents, &skill_file)?.unwrap_or_default();
    let directory_name = skill_dir
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::to_string)
        .ok_or_else(|| SkillDiscoveryError::MissingDirectoryName {
            path: skill_dir.to_path_buf(),
        })?;

    let display_name = manifest
        .name
        .clone()
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| directory_name.clone());
    let aliases = skill_aliases(manifest.name.as_deref(), &directory_name);
    if aliases.is_empty() {
        return Err(SkillDiscoveryError::MissingAlias {
            path: skill_file.clone(),
        });
    }

    let identity = SkillIdentity::new(root, &skill_file);
    Ok(Some(SkillMetadata {
        identity,
        display_name,
        description: manifest.description,
        aliases,
        directory_name,
        source_tag: root.source_tag,
        source_origin: root.source_origin.clone(),
        root_precedence: root.precedence,
        skill_dir: skill_dir.to_path_buf(),
        skill_file,
    }))
}

pub fn discover_skill_metadata(
    roots: &[SkillRoot],
) -> Result<Vec<SkillMetadata>, SkillDiscoveryError> {
    let mut skills = Vec::new();
    for root in roots {
        collect_skill_metadata(&root.path, root, 0, &mut skills)?;
    }
    skills.sort_by(|left, right| {
        (
            left.root_precedence,
            left.identity.canonical_id.as_str(),
            left.display_name.as_str(),
        )
            .cmp(&(
                right.root_precedence,
                right.identity.canonical_id.as_str(),
                right.display_name.as_str(),
            ))
    });
    Ok(skills)
}

pub fn skill_suggestions_from_metadata(skills: &[SkillMetadata]) -> Vec<SkillSuggestion> {
    let mut suggestions = skills
        .iter()
        .flat_map(|skill| {
            skill.aliases.iter().map(move |alias| SkillSuggestion {
                alias: alias.clone(),
                display_name: skill.display_name.clone(),
                description: skill.description.clone(),
                source_tag: skill.source_tag,
                source_origin: skill.source_origin.clone(),
                canonical_id: skill.identity.canonical_id.clone(),
                skill_dir: skill.skill_dir.clone(),
                source_path: skill.skill_file.clone(),
            })
        })
        .collect::<Vec<_>>();

    suggestions.sort_by(|left, right| {
        (
            left.source_tag.scope_rank(),
            left.alias.as_str(),
            left.source_origin.as_str(),
            left.canonical_id.as_str(),
        )
            .cmp(&(
                right.source_tag.scope_rank(),
                right.alias.as_str(),
                right.source_origin.as_str(),
                right.canonical_id.as_str(),
            ))
    });
    suggestions
}

pub fn is_valid_skill_alias(value: &str) -> bool {
    !value.trim().is_empty() && !value.chars().any(char::is_whitespace)
}

impl SkillIdentity {
    pub fn new(root: &SkillRoot, skill_file: &Path) -> Self {
        let relative_path = skill_file.strip_prefix(&root.path).unwrap_or(skill_file);
        let relative_path = path_to_slash(relative_path)
            .trim_start_matches('/')
            .to_string();
        let canonical_id = if relative_path.is_empty() {
            root.source_origin.clone()
        } else {
            format!("{}/{}", root.source_origin, relative_path)
        };

        Self {
            canonical_id,
            source_origin: root.source_origin.clone(),
            source_path: skill_file.to_path_buf(),
        }
    }
}

fn collect_skill_metadata(
    directory: &Path,
    root: &SkillRoot,
    depth: usize,
    skills: &mut Vec<SkillMetadata>,
) -> Result<(), SkillDiscoveryError> {
    if depth > SKILL_DISCOVERY_MAX_DEPTH {
        return Ok(());
    }

    let Ok(entries) = fs::read_dir(directory) else {
        return Ok(());
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        if let Some(metadata) = skill_metadata_from_dir(&path, root)? {
            skills.push(metadata);
        }
        collect_skill_metadata(&path, root, depth + 1, skills)?;
    }

    Ok(())
}

fn frontmatter_yaml(
    contents: &str,
    skill_file: &Path,
) -> Result<Option<String>, SkillDiscoveryError> {
    let mut lines = contents.lines();
    if lines.next().map(str::trim) != Some("---") {
        return Ok(None);
    }

    let mut yaml = String::new();
    for line in lines {
        if line.trim() == "---" {
            return Ok(Some(yaml));
        }
        yaml.push_str(line);
        yaml.push('\n');
    }

    Err(SkillDiscoveryError::InvalidFrontmatter {
        path: skill_file.to_path_buf(),
        message: "missing closing frontmatter delimiter".to_string(),
    })
}

fn skill_aliases(frontmatter_name: Option<&str>, directory_name: &str) -> Vec<String> {
    let mut aliases = Vec::new();
    push_alias(&mut aliases, frontmatter_name);
    push_alias(&mut aliases, Some(directory_name));
    aliases
}

fn push_alias(aliases: &mut Vec<String>, candidate: Option<&str>) {
    let Some(candidate) = candidate
        .map(str::trim)
        .filter(|value| is_valid_skill_alias(value))
    else {
        return;
    };
    if !aliases.iter().any(|alias| alias == candidate) {
        aliases.push(candidate.to_string());
    }
}

fn clean_optional_string(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn path_to_slash(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use tempfile::tempdir;

    #[test]
    fn root_discovery_returns_exact_precedence_order_with_injected_paths() {
        let dir = tempdir().unwrap();
        let project = dir.path().join("project");
        let home = dir.path().join("home");

        let roots = skill_roots_with_home(&project, Some(&home));

        let paths = roots
            .iter()
            .map(|root| root.path.clone())
            .collect::<Vec<_>>();
        assert_eq!(
            paths,
            vec![
                project.join(".agents/skills"),
                project.join(".claude/skills"),
                home.join(".agents/skills"),
                home.join(".claude/skills"),
            ]
        );
        assert_eq!(
            roots
                .iter()
                .map(|root| root.source_origin.as_str())
                .collect::<Vec<_>>(),
            vec![
                ".agents/skills",
                ".claude/skills",
                "~/.agents/skills",
                "~/.claude/skills",
            ]
        );
        assert_eq!(
            roots
                .iter()
                .map(|root| (root.scope_rank, root.family_rank, root.precedence))
                .collect::<Vec<_>>(),
            vec![(0, 0, 0), (0, 1, 1), (1, 0, 2), (1, 1, 3)]
        );
    }

    #[test]
    fn root_discovery_can_avoid_real_home_directory() {
        let dir = tempdir().unwrap();
        let project = dir.path().join("project");

        let roots = skill_roots_with_home(&project, None);

        assert_eq!(
            roots.iter().map(|root| &root.path).collect::<Vec<_>>(),
            vec![
                &project.join(".agents/skills"),
                &project.join(".claude/skills"),
            ]
        );
    }

    #[test]
    fn valid_yaml_frontmatter_parses_name_and_description() {
        let manifest = parse_skill_manifest(
            "---\nname: reviewer\ndescription: Review code changes\n---\n# Body\n",
            Path::new("SKILL.md"),
        )
        .unwrap()
        .unwrap();

        assert_eq!(manifest.name.as_deref(), Some("reviewer"));
        assert_eq!(manifest.description.as_deref(), Some("Review code changes"));
    }

    #[test]
    fn quoted_yaml_names_parse_correctly() {
        let manifest = parse_skill_manifest(
            "---\nname: \"quoted-skill\"\ndescription: 'quoted description'\n---\n",
            Path::new("SKILL.md"),
        )
        .unwrap()
        .unwrap();

        assert_eq!(manifest.name.as_deref(), Some("quoted-skill"));
        assert_eq!(manifest.description.as_deref(), Some("quoted description"));
    }

    #[test]
    fn missing_frontmatter_falls_back_to_directory_name_alias() {
        let dir = tempdir().unwrap();
        let root = SkillRoot::project(dir.path(), SkillRootFamily::Agents, ".agents/skills", 0);
        let skill_dir = root.path.join("plain-dir");
        write_skill_file(&skill_dir, "# Skill body\n");

        let metadata = skill_metadata_from_dir(&skill_dir, &root).unwrap().unwrap();

        assert_eq!(metadata.display_name, "plain-dir");
        assert_eq!(metadata.aliases, vec!["plain-dir"]);
        assert_eq!(
            metadata.identity.canonical_id,
            ".agents/skills/plain-dir/SKILL.md"
        );
    }

    #[test]
    fn invalid_yaml_frontmatter_fails_with_descriptive_module_error() {
        let error = parse_skill_manifest(
            "---\nname: [unterminated\n---\n",
            Path::new("/tmp/project/.agents/skills/bad/SKILL.md"),
        )
        .unwrap_err();

        let message = error.to_string();
        assert!(message.contains("invalid YAML frontmatter"));
        assert!(message.contains("/tmp/project/.agents/skills/bad/SKILL.md"));
        assert!(message.contains("name"));
    }

    #[test]
    fn directory_name_and_frontmatter_name_are_aliases_for_one_identity() {
        let dir = tempdir().unwrap();
        let root = SkillRoot::project(dir.path(), SkillRootFamily::Agents, ".agents/skills", 0);
        let skill_dir = root.path.join("directory-alias");
        write_skill_file(
            &skill_dir,
            "---\nname: frontmatter-alias\ndescription: shared identity\n---\nBody\n",
        );

        let metadata = skill_metadata_from_dir(&skill_dir, &root).unwrap().unwrap();

        assert_eq!(metadata.display_name, "frontmatter-alias");
        assert_eq!(
            metadata.aliases,
            vec!["frontmatter-alias", "directory-alias"]
        );
        assert_eq!(
            metadata.identity,
            SkillIdentity {
                canonical_id: ".agents/skills/directory-alias/SKILL.md".to_string(),
                source_origin: ".agents/skills".to_string(),
                source_path: skill_dir.join(SKILL_FILE_NAME),
            }
        );
    }

    #[test]
    fn suggestion_metadata_serializes_without_skill_body_content() {
        let dir = tempdir().unwrap();
        let root = SkillRoot::project(dir.path(), SkillRootFamily::Agents, ".agents/skills", 0);
        let skill_dir = root.path.join("reviewer");
        write_skill_file(
            &skill_dir,
            "---\nname: review\ndescription: Review safely\n---\nSECRET_SKILL_BODY\n",
        );
        let metadata = skill_metadata_from_dir(&skill_dir, &root).unwrap().unwrap();

        let suggestions = skill_suggestions_from_metadata(&[metadata]);
        let cache = SkillSuggestionCache::new(suggestions);
        let serialized = serde_json::to_string(&cache).unwrap();
        let value: Value = serde_json::from_str(&serialized).unwrap();

        assert!(serialized.contains("\"alias\":\"review\""));
        assert!(serialized.contains("\"display_name\":\"review\""));
        assert!(serialized.contains("\"source_tag\":\"Project\""));
        assert!(serialized.contains("\"source_origin\":\".agents/skills\""));
        assert!(serialized.contains("reviewer/SKILL.md"));
        assert!(!serialized.contains("SECRET_SKILL_BODY"));
        assert!(value["suggestions"][0].get("content").is_none());
        assert!(value["suggestions"][0].get("body").is_none());
    }

    #[test]
    fn discover_metadata_represents_project_and_personal_roots() {
        let dir = tempdir().unwrap();
        let project = dir.path().join("project");
        let home = dir.path().join("home");
        let roots = skill_roots_with_home(&project, Some(&home));
        write_skill_file(
            &project.join(".agents/skills/project-agent"),
            "---\nname: project-frontmatter\ndescription: Project agents skill\n---\n",
        );
        write_skill_file(
            &project.join(".claude/skills/project-claude"),
            "---\ndescription: Project claude skill\n---\n",
        );
        write_skill_file(
            &home.join(".agents/skills/personal-agent"),
            "---\nname: personal-frontmatter\n---\n",
        );
        write_skill_file(
            &home.join(".claude/skills/personal-claude"),
            "# no frontmatter\n",
        );

        let metadata = discover_skill_metadata(&roots).unwrap();
        let suggestions = skill_suggestions_from_metadata(&metadata);

        assert_eq!(
            metadata
                .iter()
                .map(|skill| (
                    skill.display_name.as_str(),
                    skill.source_tag,
                    skill.source_origin.as_str()
                ))
                .collect::<Vec<_>>(),
            vec![
                (
                    "project-frontmatter",
                    SkillSourceTag::Project,
                    ".agents/skills"
                ),
                ("project-claude", SkillSourceTag::Project, ".claude/skills"),
                (
                    "personal-frontmatter",
                    SkillSourceTag::Personal,
                    "~/.agents/skills"
                ),
                (
                    "personal-claude",
                    SkillSourceTag::Personal,
                    "~/.claude/skills"
                ),
            ]
        );
        assert!(suggestions.iter().any(|suggestion| {
            suggestion.alias == "project-agent"
                && suggestion.display_name == "project-frontmatter"
                && suggestion.source_origin == ".agents/skills"
        }));
        assert!(suggestions.iter().any(|suggestion| {
            suggestion.alias == "personal-claude"
                && suggestion.source_tag == SkillSourceTag::Personal
                && suggestion.source_origin == "~/.claude/skills"
        }));
    }

    fn write_skill_file(skill_dir: &Path, contents: &str) {
        fs::create_dir_all(skill_dir).unwrap();
        fs::write(skill_dir.join(SKILL_FILE_NAME), contents).unwrap();
    }
}
