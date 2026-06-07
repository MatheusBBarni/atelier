use multiagent::skills::{
    discover_skill_metadata, skill_roots_with_home, skill_suggestions_from_metadata,
    SkillSourceTag, SKILL_FILE_NAME,
};
use std::fs;
use std::path::Path;
use tempfile::tempdir;

#[test]
fn public_skills_module_imports_without_tui_state() {
    let dir = tempdir().unwrap();
    let roots = skill_roots_with_home(dir.path(), None);

    assert_eq!(roots.len(), 2);
    assert_eq!(roots[0].source_tag, SkillSourceTag::Project);
    assert_eq!(roots[0].source_origin, ".agents/skills");
}

#[test]
fn shared_suggestions_represent_project_and_personal_skill_roots() {
    let dir = tempdir().unwrap();
    let project = dir.path().join("project");
    let home = dir.path().join("home");
    write_skill(
        &project.join(".agents/skills/project-agent"),
        "---\nname: project-tool\ndescription: Project skill\n---\nPROJECT_BODY\n",
    );
    write_skill(
        &home.join(".claude/skills/personal-claude"),
        "---\nname: personal-tool\ndescription: Personal skill\n---\nPERSONAL_BODY\n",
    );

    let roots = skill_roots_with_home(&project, Some(&home));
    let metadata = discover_skill_metadata(&roots).unwrap();
    let suggestions = skill_suggestions_from_metadata(&metadata);

    assert!(suggestions.iter().any(|suggestion| {
        suggestion.alias == "project-tool"
            && suggestion.display_name == "project-tool"
            && suggestion.source_tag == SkillSourceTag::Project
            && suggestion.source_origin == ".agents/skills"
            && suggestion.source_path.ends_with("project-agent/SKILL.md")
    }));
    assert!(suggestions.iter().any(|suggestion| {
        suggestion.alias == "personal-tool"
            && suggestion.display_name == "personal-tool"
            && suggestion.source_tag == SkillSourceTag::Personal
            && suggestion.source_origin == "~/.claude/skills"
            && suggestion.source_path.ends_with("personal-claude/SKILL.md")
    }));
}

fn write_skill(skill_dir: &Path, contents: &str) {
    fs::create_dir_all(skill_dir).unwrap();
    fs::write(skill_dir.join(SKILL_FILE_NAME), contents).unwrap();
}
