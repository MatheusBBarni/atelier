use multiagent::config::{AgentEffort, AgentProfile, AgentPromptMetadata, Capability, Limits};
use multiagent::runtime::{prompt_envelope_json, RuntimeRecentContext, RuntimeRequest};
use multiagent::skills::{compile_prompt_with_home, render_runtime_prompt, SKILL_FILE_NAME};
use serde_json::Value;
use std::fs;
use std::path::Path;
use tempfile::tempdir;

#[test]
fn rendered_skill_prompt_fits_existing_runtime_request_prompt_field() {
    let dir = tempdir().unwrap();
    let project = dir.path().join("project");
    write_skill(
        &project.join(".agents/skills/reviewer"),
        "---\nname: reviewer\n---\nReview with care.\n",
    );
    let compiled =
        compile_prompt_with_home(&project, None, "/skill:reviewer inspect README").unwrap();
    let rendered = render_runtime_prompt(compiled.skill_context.as_ref(), &compiled.user_prompt);
    let request = runtime_request(project, rendered);

    let envelope: Value = serde_json::from_str(&prompt_envelope_json(&request).unwrap()).unwrap();

    assert_eq!(envelope["prompt"], request.prompt);
    assert!(envelope.get("skills").is_none());
    assert!(envelope["prompt"]
        .as_str()
        .unwrap()
        .contains("<Skill: reviewer"));
    assert_eq!(
        count_occurrences(envelope["prompt"].as_str().unwrap(), "<Skill: reviewer"),
        1
    );
    assert!(envelope["prompt"]
        .as_str()
        .unwrap()
        .contains("<User Prompt>"));
    let mut envelope_without_prompt = envelope.clone();
    envelope_without_prompt
        .as_object_mut()
        .unwrap()
        .remove("prompt");
    let non_prompt_fields = serde_json::to_string(&envelope_without_prompt).unwrap();
    assert!(!non_prompt_fields.contains("<Skill: reviewer"));
    assert!(!non_prompt_fields.contains("Review with care."));
}

#[test]
fn duplicate_skill_references_render_once_per_runtime_request() {
    let dir = tempdir().unwrap();
    let project = dir.path().join("project");
    write_skill(
        &project.join(".agents/skills/reviewer"),
        "---\nname: reviewer\n---\nReview with care.\n",
    );
    let compiled = compile_prompt_with_home(
        &project,
        None,
        "/skill:reviewer inspect README /skill:reviewer",
    )
    .unwrap();
    let rendered = render_runtime_prompt(compiled.skill_context.as_ref(), &compiled.user_prompt);
    let request = runtime_request(project, rendered);

    assert_eq!(request.prompt.matches("<Skill: reviewer").count(), 1);
    assert_eq!(request.prompt.matches("Review with care.").count(), 1);
    assert!(request
        .prompt
        .contains("<User Prompt>\ninspect README\n</User Prompt>"));
}

#[test]
fn compiled_skill_metadata_serializes_without_skill_body_content() {
    let dir = tempdir().unwrap();
    let project = dir.path().join("project");
    write_skill(
        &project.join(".agents/skills/reviewer"),
        "---\nname: reviewer\n---\nSECRET_SKILL_BODY\n",
    );

    let compiled =
        compile_prompt_with_home(&project, None, "/skill:reviewer inspect README").unwrap();
    let context = compiled.skill_context.unwrap();
    let metadata_json = serde_json::to_string(&context.metadata()).unwrap();

    assert!(metadata_json.contains("reviewer"));
    assert!(metadata_json.contains(".agents/skills/reviewer/SKILL.md"));
    assert!(!metadata_json.contains("SECRET_SKILL_BODY"));
}

fn runtime_request(working_directory: std::path::PathBuf, prompt: String) -> RuntimeRequest {
    RuntimeRequest {
        run_id: "run".to_string(),
        step_id: "step".to_string(),
        prompt,
        session_goal: None,
        working_directory,
        agent_profile: AgentProfile {
            id: "explorer".to_string(),
            name: "Explorer".to_string(),
            runtime: "fake".to_string(),
            model: "default".to_string(),
            model_fallbacks: Vec::new(),
            effort: AgentEffort::Medium,
            thinking: false,
            capabilities: vec![Capability::Read],
            tools: None,
            instructions: "Read files.".to_string(),
            orchestrator_description: None,
            prompt_metadata: AgentPromptMetadata::default(),
            enabled: true,
        },
        session_events: Vec::new(),
        recent_context: RuntimeRecentContext::default(),
        previous_results: Vec::new(),
        action_results: Vec::new(),
        output_schema: "agent_result".to_string(),
        parallel_context: None,
        capability_constraints: vec![Capability::Read],
        limits: Limits::default(),
    }
}

fn write_skill(skill_dir: &Path, contents: &str) {
    fs::create_dir_all(skill_dir).unwrap();
    fs::write(skill_dir.join(SKILL_FILE_NAME), contents).unwrap();
}

fn count_occurrences(haystack: &str, needle: &str) -> usize {
    haystack.match_indices(needle).count()
}
