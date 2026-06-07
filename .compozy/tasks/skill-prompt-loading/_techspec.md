# Skill Prompt Loading TechSpec

## Executive Summary

Implement Skill Prompt Loading as an app-owned prompt compilation path backed by a shared `src/skills/mod.rs` module. The resolver will parse every `/skill:<id>` occurrence, resolve aliases from frontmatter `name` and directory names, apply the selected precedence rules, load each skill once, record metadata in history, and render full skill sections only when building `RuntimeRequest` values.

The primary trade-off is adding a shared skills module and YAML frontmatter dependency to avoid storing full skill bodies in history or teaching runtime adapters new `/skill:` semantics. Runtime adapters remain generic, while the app owns resolution, audit metadata, and derived prompt propagation.

## System Architecture

### Component Overview

| Component | Responsibility | Boundary |
| --- | --- | --- |
| `src/skills/mod.rs` | Parse skill references, discover roots, parse frontmatter, resolve aliases, load content, dedupe skills, render V1 prompt sections | No runtime adapter logic |
| `App::submit_prompt` | Resolve skill references before run creation and fail closed on invalid input | Normal prompts only; pending clarification does not load new skills |
| `/subtask` handling | Resolve skill references inside subtask task text before starting the subtask run | Subtask prompt gets its own compiled skill context |
| `RunDriveContext` | Carry submitted prompt, normalized user prompt, and `SkillPromptContext` | Full skill content stays in memory only |
| `runtime_request` | Render full skill sections around the current prompt before constructing `RuntimeRequest` | Runtime adapters receive plain prompt text |
| History/chat projection | Record and display `skills_loaded` metadata | No full `SKILL.md` body persistence |
| TUI skill dropdown | Source suggestions from shared skill discovery | Cache remains advisory only |

Data flow:

1. User submits prompt.
2. App calls `skills::compile_prompt`.
3. On failure, app returns an error before `run_started`.
4. On success, app records `run_started`, `prompt_submitted`, and `skills_loaded`.
5. App stores normalized prompt and `SkillPromptContext` on `RunDriveContext`.
6. Every `runtime_request` call renders skill sections exactly once for that runtime prompt.

## Implementation Design

### Core Interfaces

```rust
pub struct CompiledPrompt {
    pub submitted_prompt: String,
    pub user_prompt: String,
    pub skill_context: Option<SkillPromptContext>,
}

pub struct SkillPromptContext {
    pub loaded: Vec<LoadedSkill>,
}

pub struct LoadedSkill {
    pub metadata: LoadedSkillMetadata,
    pub content: String,
}
```

```rust
pub struct LoadedSkillMetadata {
    pub requested_names: Vec<String>,
    pub display_name: String,
    pub canonical_id: String,
    pub source_origin: String,
    pub source_path: String,
    pub load_reason: String,
}

pub struct SkillLoadError {
    pub requested_name: String,
    pub kind: SkillLoadErrorKind,
    pub suggestions: Vec<String>,
}
```

```rust
pub fn compile_prompt(
    working_directory: &Path,
    submitted_prompt: &str,
) -> Result<CompiledPrompt>;

pub fn render_runtime_prompt(
    skill_context: Option<&SkillPromptContext>,
    prompt: &str,
) -> String;
```

### Data Models

- `SkillRoot`: path, scope rank, family rank, and display origin.
- `SkillManifest`: deserialized YAML frontmatter with `name` and `description`.
- `SkillIdentity`: canonical path plus root origin.
- `ResolvedSkill`: identity, alias set, metadata, and file content.
- `CompiledPrompt`: original submitted prompt, normalized user prompt, and optional loaded skill context.
- `skills_loaded` history event payload:

```json
{
  "skills": [
    {
      "requested_names": ["reviewer"],
      "display_name": "reviewer",
      "canonical_id": ".agents/skills/reviewer/SKILL.md",
      "source_origin": ".agents/skills",
      "source_path": ".agents/skills/reviewer/SKILL.md",
      "load_reason": "explicit"
    }
  ]
}
```

No full `SKILL.md` content is written to history, debug logs, or run records.

### Parsing And Resolution

- Match every `/skill:<id>` occurrence.
- `<id>` is one or more identifier characters and ends at whitespace or common punctuation.
- Empty `/skill:` is invalid and fails before run creation.
- Frontmatter parsing uses a Serde-compatible YAML parser dependency, preferably `serde_norway`, because the selected behavior requires YAML compatibility.
- Directory name and frontmatter `name` both become aliases for the same skill.
- Root precedence order:
  1. project `.agents/skills`
  2. project `.claude/skills`
  3. personal `~/.agents/skills`
  4. personal `~/.claude/skills`
- The first matching precedence tier wins.
- Duplicate resolved identities are loaded once, preserving first-use order.
- Same-alias ambiguity inside the same root family fails with conflicting source metadata.

### Prompt Rendering

Runtime prompt shape:

```text
<System Prompt>
{existing runtime/harness prompt context}
</System Prompt>

<Skill: reviewer source=".agents/skills/reviewer/SKILL.md">
{skill body}
</Skill>

<User Prompt>
{normalized user or derived prompt}
</User Prompt>
```

The app strips skill references from the normalized user prompt after loading them. Derived prompts receive normalized prompt text and reuse the same `SkillPromptContext`; they do not reparse or duplicate rendered sections.

### API Endpoints

No API endpoints are introduced. This feature changes local app prompt compilation, local history events, and local TUI discovery.

## Integration Points

No external service integration is required.

External dependency:

- Add a Serde-compatible YAML parser for `SKILL.md` frontmatter, preferably [`serde_norway`](https://docs.rs/serde_norway/latest/serde_norway/).
- Avoid YAML crates with active maintenance or soundness advisories; [`serde_yml` has a RustSec advisory](https://rustsec.org/advisories/RUSTSEC-2025-0068.html).

## Impact Analysis

| Component | Impact Type | Description and Risk | Required Action |
| --- | --- | --- | --- |
| `src/skills/mod.rs` | New | Central resolver and renderer; medium risk because it defines prompt semantics | Add module and unit tests |
| `src/tui/mod.rs` | Modified | Dropdown discovery should use shared suggestions; low risk if UI behavior is preserved | Replace local discovery helpers |
| `src/app/mod.rs` | Modified | Run creation and derived prompts need compiled prompt context; high-risk integration point | Resolve before run creation and thread context through run |
| `src/runtime/mod.rs` | Modified | Runtime envelope remains generic, but prompt input is rendered text; low risk | Add snapshot coverage |
| `src/history/mod.rs` | Modified | New `skills_loaded` event kind; low risk | Persist metadata-only event |
| `src/app/chat/projection.rs` | Modified | Show concise loaded-skills note; low risk | Add projection handler |
| App tests in `src/app/mod.rs` | Modified | Raw `/skill:` behavior test must change | Replace with loading/failure tests |
| `Cargo.toml` | Modified | YAML parser dependency added | Run dependency review and locked tests |

## Testing Approach

### Unit Tests

- `src/skills` parser:
  - all `/skill:name` occurrences are detected;
  - IDs stop at whitespace and punctuation;
  - empty `/skill:` fails;
  - duplicate references dedupe by canonical identity.
- Resolver:
  - frontmatter `name` and directory name aliases both work;
  - project roots beat personal roots;
  - `.agents/skills` beats `.claude/skills`;
  - same-root ambiguity fails with source details;
  - unreadable or invalid `SKILL.md` fails.
- Renderer:
  - System/Skill/User sections render in order;
  - loaded skills render once;
  - normalized user prompt excludes skill directives.

### Integration Tests

- Valid skill prompt creates run and records `skills_loaded`.
- Unknown skill returns an error and records no `run_started` or `prompt_submitted`.
- `prompt_submitted`, run records, and debug events do not contain full skill bodies.
- `/subtask explorer /skill:x inspect`, council prompts, and parallel child prompts carry skill context once.
- `prompt_envelope_json` contains rendered skill sections through the existing `prompt` field.
- TUI dropdown still discovers project and personal skills from the shared module.

## Development Sequencing

### Build Order

1. Add `src/skills/mod.rs` with parser, data models, and unit tests - no dependencies.
2. Add YAML frontmatter parsing and root discovery - depends on step 1.
3. Add resolver precedence, alias indexing, content loading, and dedupe - depends on step 2.
4. Add prompt rendering helpers - depends on step 3.
5. Extend `RunDriveContext` with submitted prompt and `SkillPromptContext` - depends on step 4.
6. Integrate normal `submit_prompt` skill compilation and fail-closed errors - depends on step 5.
7. Integrate `/subtask` compilation and derived prompt propagation - depends on step 6.
8. Add `skills_loaded` history event and chat projection rendering - depends on step 6.
9. Move TUI suggestions to shared discovery while preserving cache behavior - depends on step 3.
10. Add runtime envelope and app integration tests - depends on steps 6 through 9.
11. Run full verification and update docs/help text if wording still implies raw prefix behavior - depends on step 10.

### Technical Dependencies

- Add one YAML parser dependency to `Cargo.toml`.
- Update `Cargo.lock`.
- Verify dependency health before implementation, with specific attention to RustSec advisories.

## Monitoring and Observability

- `skills_loaded` history event fields:
  - `requested_names`
  - `display_name`
  - `canonical_id`
  - `source_origin`
  - `source_path`
  - `load_reason`
- Chat projection shows a concise "Skills loaded" item.
- Diagnostics for failures include requested name, failure kind, and suggested matches.
- Debug logs must not include full skill bodies unless debug behavior is explicitly expanded later.

## Technical Considerations

### Key Decisions

- **Decision:** Add `src/skills/mod.rs`.
  **Rationale:** TUI and app behavior need one source of truth.
  **Trade-off:** More shared surface area, less duplicated discovery logic.
  **Alternatives rejected:** Keep discovery in TUI or compile directly in app.

- **Decision:** Store `SkillPromptContext` on `RunDriveContext`.
  **Rationale:** Derived prompts need the same loaded skill context without reparsing.
  **Trade-off:** Run context grows, but runtime adapters stay unchanged.
  **Alternatives rejected:** Reparse each derived prompt or copy skill sections into strings.

- **Decision:** Render full skill text only in `runtime_request`.
  **Rationale:** Prevent full skill body leakage into history and run records.
  **Trade-off:** Runtime request construction becomes responsible for final prompt text.
  **Alternatives rejected:** Replace `run.prompt` with full rendered prompt.

- **Decision:** Use a YAML parser dependency for frontmatter.
  **Rationale:** V1 requires compatible frontmatter parsing beyond the current first-line `name:` behavior.
  **Trade-off:** New dependency and dependency-review obligation.
  **Alternatives rejected:** Ad hoc parsing or frontmatter-name-only support.

### Known Risks

- **Dependency risk:** YAML parser ecosystem has known maintenance issues. Mitigate with a maintained parser and dependency review.
- **History leakage:** Full skill contents may accidentally be persisted. Mitigate with metadata-only event tests.
- **Prompt injection:** Skill bodies may contain hostile instructions. Mitigate by preserving Harness Actions as enforcement and framing skills as workflow guidance.
- **Double rendering:** Derived prompts may receive duplicate skill sections. Mitigate by rendering only inside `runtime_request`.
- **Behavioral churn:** Existing `/skill:` raw pass-through test will fail. Mitigate by replacing it with explicit success and fail-closed tests.

## Architecture Decision Records

- [ADR-001: Scope Skill Prompt Loading V1](adrs/adr-001.md) - App-owned skill prompt loading with generic runtime delivery.
- [ADR-002: Select Deterministic Common Flow For PRD](adrs/adr-002.md) - PRD scope uses the confirmed deterministic common-flow behavior.
- [ADR-003: Shared Skill Resolver With Runtime-Time Prompt Rendering](adrs/adr-003.md) - Technical design uses a shared skills module, run-context metadata, metadata-only audit events, and runtime-request-time rendering.
