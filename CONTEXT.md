# Multiagent Harness

A terminal-native coordination tool where a user prompt is routed through an orchestrator to one or more specialized agents.

## Language

**Harness**:
The interactive system that receives a user prompt, coordinates agent work, and presents progress in a terminal UI.
_Avoid_: framework, platform

**Prompt**:
A user-submitted instruction entered into the harness.
_Avoid_: request, task

**Orchestrator**:
The agent responsible for deciding which specialized agent should handle a prompt or next step.
_Avoid_: router, dispatcher, coordinator

**Explorer**:
The specialized agent responsible for reading code, documentation, and project state without making changes.
_Avoid_: researcher, scout

**Oracle**:
The specialized agent responsible for answering design or implementation questions from gathered context.
_Avoid_: advisor, expert

**Fixer**:
The specialized agent responsible for editing files and running targeted verification.
_Avoid_: implementer, patcher

**Reviewer**:
The specialized agent responsible for reviewing changes for bugs, regressions, and missing tests.
_Avoid_: critic, auditor

**Consul**:
The specialized agent responsible for challenging plans, architecture, and domain decisions before work proceeds.
_Avoid_: council, planner

**Specialized Agent**:
An agent with a named role and bounded responsibility inside the harness.
_Avoid_: worker, subagent, assistant

**Agent Profile**:
The editable definition of a specialized agent's name, responsibility, instructions, runtime preferences, and model choice.
_Avoid_: hardcoded agent, persona

**Instruction Source**:
The inline text or file path that provides an agent profile's role instructions.
_Avoid_: prompt file, system prompt

**Model Assignment**:
The selected model used by an agent profile when it performs work.
_Avoid_: default model, global model

**Agent Capability**:
A permission granted to an agent profile for reading, editing, running commands, or performing verification.
_Avoid_: tool, permission

**Capability Enforcement**:
The harness behavior that blocks an agent from using capabilities not granted by its agent profile.
_Avoid_: prompt guardrail, trust

**Action Approval**:
The user's explicit confirmation before a high-impact harness action proceeds.
_Avoid_: confirmation, permission prompt

**Harness Configuration**:
The home-scoped `~/.config/multiagent/multiagent.toml` file that defines default agent profiles, model assignments, and runtime preferences.
_Avoid_: settings file, config blob

**Local Configuration**:
The optional `multiagent.toml` file in the working directory that overrides project-specific harness behavior.
_Avoid_: project config, local settings

**Built-in Profile**:
An agent profile bundled with the global command so the harness can run before the user creates configuration.
_Avoid_: sample profile, template

**Custom Agent**:
A user-defined specialized agent added through configuration.
_Avoid_: plugin, extension

**Effective Configuration**:
The merged result of built-in profiles, harness configuration, local configuration, and command-line overrides.
_Avoid_: active config, resolved config

**Global Command**:
The installed `atelier` executable that can be launched from any folder.
_Avoid_: project binary, local script

**npm Distribution Package**:
The scoped npm package that installs the global command through the npm registry.
_Avoid_: source package, JavaScript CLI

**Platform Binary Package**:
A platform-specific npm package that contains a prebuilt native `atelier` executable.
_Avoid_: build script, installer

**Working Directory**:
The folder where the user launches the global command and where a run reads, edits, and records project-local history by default.
_Avoid_: project root, installation directory

**Execution Runtime**:
The backend used by an agent profile to perform model work.
_Avoid_: provider, engine, backend

**Runtime Availability**:
Whether an execution runtime can currently perform work with the installed command or configured credentials.
_Avoid_: health check, provider status

**Codex Runtime**:
An execution runtime that launches Codex locally and relies on Codex-owned authentication, commonly ChatGPT subscription login through the Codex CLI.
_Avoid_: OpenAI API runtime

**Claude Runtime**:
An execution runtime that launches Claude locally and relies on Claude-owned authentication through the Claude CLI.
_Avoid_: Anthropic API runtime

**Cursor Runtime**:
An execution runtime that launches Cursor locally and relies on Cursor-owned authentication through the Cursor Agent CLI.
_Avoid_: Cursor API runtime

**Z.ai Runtime**:
An execution runtime that calls Z.ai through API-key authenticated HTTP requests.
_Avoid_: GLM runtime

**Credential Reference**:
The configuration value that names where a runtime credential is read from without storing the secret itself.
_Avoid_: api key, secret value

**Harness Action**:
An operation performed by the harness, such as reading files, editing files, running commands, or recording verification.
_Avoid_: model tool call, provider action

**VCS Action**:
A harness action that changes version-control state, such as committing, branching, or pushing.
_Avoid_: git command, repository mutation

**Routing Decision**:
The orchestrator's choice of which specialized agent should handle the current prompt or next step.
_Avoid_: assignment, dispatch

**Run Plan**:
The orchestrator-owned sequence of steps for satisfying a prompt.
_Avoid_: task list, workflow

**Parallel Step Group**:
A set of specialized agent steps that the orchestrator starts concurrently inside one run.
_Avoid_: parallel run, sub-run, worker pool

**Parallel File Scope**:
The file paths assigned to one specialized agent step inside a parallel step group.
_Avoid_: shared files, unrestricted workspace

**Orchestrator Decision**:
The structured output from the orchestrator that describes the run plan, next step, reason, required capabilities, and stop condition.
_Avoid_: prose plan, routing text

**Agent Result**:
The typed output envelope returned by a specialized agent after it completes a step.
_Avoid_: message, response

**Parallel Group Result**:
The typed output envelope returned after a parallel step group joins and summarizes its child agent results.
_Avoid_: merged agent result, batch response

**Run**:
A single attempt to satisfy one prompt through a run plan and agent activity.
_Avoid_: session, job

**Harness Session**:
The period of interactive TUI use that may contain one or more runs.
_Avoid_: run, job

**Session History**:
The persisted record of harness sessions, runs, plans, agent outputs, diffs, commands, and verification evidence.
_Avoid_: transcript, chat history

**Context Resume**:
Starting a new run with prior session history available as context.
_Avoid_: process resume, continuation

**Run Limit**:
A configured bound that stops a run after too many steps, too much time, or too many review-fix cycles.
_Avoid_: timeout, budget

**Chat**:
The primary TUI surface that presents a readable conversation-style view of prompts, routing decisions, agent activity, harness actions, command results, file edits, diagnostics, and results.
_Avoid_: Event Stream, log, transcript, console output

**Chat Item**:
A curated unit rendered in the Chat, such as a user prompt, routing decision, agent summary, harness action, command result, file edit, diagnostic, diff, or final result.
_Avoid_: message, raw log line

**Agent Roster**:
The TUI surface that shows available specialized agents, their status, model assignment, runtime, and current step.
_Avoid_: sidebar, agent list

**Input Composer**:
The TUI surface where the user enters prompts, answers orchestrator questions, or interrupts a run.
_Avoid_: textbox, prompt bar

**Clarifying Question**:
A targeted question from the orchestrator to the user when a run cannot safely continue.
_Avoid_: prompt, blocker message

## Relationships

- A **Harness** receives one **Prompt** at a time from the user.
- A **Prompt** creates exactly one **Run**.
- A **Run** is guided by exactly one **Run Plan**.
- A **Harness** has at most one active **Run** at a time in the first version.
- A **Run** may contain **Parallel Step Groups** while remaining the only active **Run** in the **Harness Session**.
- A **Harness Session** may contain multiple **Runs** over time.
- **Session History** is stored in the project-scoped `.multiagent/` directory.
- **Session History** records chronological events from **Parallel Step Groups** with group, step, and agent identity.
- **Session History** also records the joined **Parallel Group Result** for each completed **Parallel Step Group**.
- **Context Resume** creates a new **Run** rather than continuing an old process exactly.
- **Context Resume** does not resume Claude CLI sessions by default; it provides prior **Session History** to a new Claude process.
- **Context Resume** does not resume Cursor CLI sessions by default; it provides prior **Session History** to a new Cursor process.
- A **Run Limit** can be a concrete value or explicitly unlimited in configuration.
- A **Global Command** starts a **Harness Session** in the current **Working Directory**.
- An **npm Distribution Package** installs the **Global Command** without requiring the user to build Rust from source.
- An **npm Distribution Package** selects one **Platform Binary Package** for the user's operating system and CPU architecture.
- **Harness Configuration** applies across working directories.
- **Local Configuration** can override **Harness Configuration** for the current **Working Directory**.
- `MULTIAGENT_CONFIG` can point the **Global Command** to an alternate **Harness Configuration** file.
- **Effective Configuration** is produced by merging built-in profiles, **Harness Configuration**, **Local Configuration**, and command-line overrides.
- A **Prompt** is first handled by exactly one **Orchestrator**.
- An **Orchestrator** delegates work to one or more **Specialized Agents**.
- The **Orchestrator** may propose a **Parallel Step Group** when independent file scopes can move forward concurrently.
- The **Orchestrator** may choose sequential steps instead of a **Parallel Step Group** when ordering, safety, review quality, or clarity matters more than throughput.
- **Harness Configuration** bounds how many specialized agent steps may run concurrently.
- Runtime and model fallback happens independently for each step in a **Parallel Step Group**.
- An **Orchestrator Decision** drives delegation and run state transitions.
- An **Orchestrator Decision** may select one **Specialized Agent** step or one **Parallel Step Group** as the next step.
- A **Routing Decision** is displayed in the **Chat** before the selected **Specialized Agent** runs.
- A **Parallel Step Group** contains two or more **Specialized Agent** steps that run concurrently.
- A **Parallel Step Group** may contain multiple concurrent steps that use the same **Agent Profile**.
- A **Parallel Step Group** may use built-in or custom enabled **Agent Profiles**.
- Each **Specialized Agent** step in a **Parallel Step Group** has a **Parallel File Scope**.
- Each **Specialized Agent** step in a **Parallel Step Group** receives shared run context plus a scoped instruction for its assigned work.
- A file path belongs to at most one **Parallel File Scope** in the same **Parallel Step Group**.
- A **Parallel Step Group** runs in the same **Working Directory** as the **Run**.
- **Capability Enforcement** restricts parallel agent file actions to their assigned **Parallel File Scope**.
- Parallel **Fixer** steps may edit files only inside disjoint **Parallel File Scopes**.
- A **Specialized Agent** in a **Parallel Step Group** cannot expand its own **Parallel File Scope**.
- An out-of-scope file action in a **Parallel Step Group** is blocked and reported as an **Agent Result**.
- Parallel agents may run scoped read-only verification commands for their assigned **Parallel File Scope**.
- Mutation-capable or project-wide commands run outside a **Parallel Step Group** after the group joins.
- **Action Approval** requests from a **Parallel Step Group** are handled one at a time by the **Harness**.
- A parallel step waiting for **Action Approval** does not stop unrelated parallel steps by default.
- A blocked or failed step in a **Parallel Step Group** does not cancel other independent parallel steps by default.
- A **Parallel Step Group** joins only after every parallel step finishes, fails, blocks, or is cancelled.
- The **Orchestrator** decides how to continue from one joined **Parallel Step Group** result with per-agent outcomes.
- A **Parallel Group Result** records the joined outcome of one **Parallel Step Group**.
- A **Parallel Group Result** references each child **Agent Result** rather than replacing it.
- The **Orchestrator** may infer **Parallel File Scopes** from **Explorer** findings when the file boundaries are clear.
- The **Orchestrator** asks a **Clarifying Question** before a **Parallel Step Group** starts when file boundaries are missing, overlapping, or high-risk.
- A **Reviewer** in a **Parallel Step Group** reviews only its assigned **Parallel File Scope**.
- A combined-diff **Reviewer** pass happens after a **Parallel Step Group** only when the **Orchestrator** chooses it as the next step.
- A **Chat Item** is curated for user readability and may reference raw output stored in **Session History** or artifacts.
- A **Chat Item** may aggregate multiple related **Session History** events when they describe one visible lifecycle, such as one harness action moving from requested to running to completed.
- **Chat** is a presentation layer derived from active run state and **Session History**; it does not replace the durable **Session History** model.
- During a **Parallel Step Group**, **Chat** shows each active **Specialized Agent** as working.
- The **Agent Roster**, **Chat**, and **Input Composer** are the primary TUI surfaces.
- An **Orchestrator** owns the **Run Plan** and decides when to delegate, continue, stop, or ask the user for input.
- A **Specialized Agent** reports results to the **Orchestrator** rather than delegating directly to another **Specialized Agent**.
- A **Specialized Agent** reports an **Agent Result** after completing a step.
- A **Clarifying Question** is asked only by the **Orchestrator**.
- A **Run** pauses while waiting for the user's answer to a **Clarifying Question**.
- Interrupting a **Run** cancels every active step in the current **Parallel Step Group**.
- The initial **Specialized Agents** are **Orchestrator**, **Explorer**, **Oracle**, **Fixer**, **Reviewer**, and **Consul**.
- A non-trivial code-change **Run Plan** uses **Explorer** before **Fixer**.
- A non-trivial code-change **Parallel Step Group** uses **Explorer** first unless the user provides explicit file scopes.
- An architecture-heavy or ambiguous **Run Plan** uses **Consul** before **Fixer**.
- A normal code-change **Run Plan** follows **Explorer**, **Fixer**, **Reviewer**, optional **Fixer**, then final **Orchestrator** summary.
- A **Specialized Agent** is defined by exactly one **Agent Profile**.
- Each **Agent Profile** has one active **Model Assignment** that can differ from other agent profiles.
- Each **Agent Profile** declares its **Agent Capabilities**.
- Each **Agent Profile** has one **Instruction Source**.
- File-based **Instruction Sources** resolve relative to the configuration file that declares them.
- **Capability Enforcement** prevents a **Specialized Agent** from performing work outside its declared **Agent Capabilities**.
- **VCS Actions** require an explicit user request.
- High-impact **Harness Actions** require **Action Approval** even when the selected **Agent Profile** has the needed **Agent Capability**.
- **Harness Configuration** defines the available **Agent Profiles** and their **Model Assignments**.
- **Built-in Profiles** provide default **Agent Profiles** when configuration is missing or incomplete.
- **Custom Agents** are defined as additional **Agent Profiles** in configuration.
- Configuration can override a **Built-in Profile** by using the same agent name.
- Adding an **Execution Runtime** does not change **Built-in Profiles** unless a profile explicitly selects that runtime.
- An **Agent Profile** uses one **Execution Runtime** when performing work.
- The supported **Execution Runtimes** are **Codex Runtime**, **Claude Runtime**, **Cursor Runtime**, **Z.ai Runtime**, and **Fake Runtime**.
- **Runtime Availability** is shown for each **Agent Profile** in the **Agent Roster**.
- **Runtime Availability** can be unknown when the harness can verify installation but cannot prove authentication without starting model work.
- **Codex Runtime** uses Codex-owned local authentication, while API-keyed runtimes use a **Credential Reference** to name the environment variable containing their API key.
- Execution runtimes produce reasoning or structured output, while **Harness Actions** are executed by the **Harness**.
- **Claude Runtime** uses Claude-owned local authentication, disables Claude Code tools by default, and requests local reads, edits, commands, and VCS operations as **Harness Actions**.
- **Claude Runtime** minimizes ambient Claude project context by default; project-specific Claude settings require explicit harness support.

## Example dialogue

> **Dev:** "When the user presses Enter in the TUI, does the **Prompt** go directly to Explorer?"
> **Domain expert:** "No. Every **Prompt** starts with the **Orchestrator**, and the **Orchestrator** decides which **Specialized Agent** handles it next."
> **Dev:** "Does the user confirm the **Routing Decision** first?"
> **Domain expert:** "No, the **Routing Decision** appears in the **Chat**, then the chosen **Specialized Agent** runs automatically."
> **Dev:** "Where does the user see which model Fixer is using?"
> **Domain expert:** "The **Agent Roster** shows each **Specialized Agent** with its **Model Assignment** and **Execution Runtime**."
> **Dev:** "Can the user start a second **Run** while Fixer is still working?"
> **Domain expert:** "No. The **Input Composer** controls the active **Run** until it completes, is interrupted, or asks for input."
> **Dev:** "Is a **Run** the same thing as a **Harness Session**?"
> **Domain expert:** "No. A **Harness Session** is the interactive TUI period, and each submitted **Prompt** creates a separate **Run** recorded in **Session History**."
> **Dev:** "Does resume restore the old Codex child process?"
> **Domain expert:** "No. **Context Resume** starts a new **Run** with previous **Session History** available as context."
> **Dev:** "Does **Context Resume** pass `--continue` or `--resume` to **Claude Runtime**?"
> **Domain expert:** "No. **Context Resume** uses harness-owned **Session History** and starts a new Claude print-mode process by default."
> **Dev:** "Does **Context Resume** pass `resume` or `--resume` to **Cursor Runtime**?"
> **Domain expert:** "No. **Context Resume** uses harness-owned **Session History** and starts a new Cursor print-mode process by default."
> **Dev:** "What stops a bad plan from looping forever?"
> **Domain expert:** "A **Run Limit** stops the **Run** unless the user explicitly configures that limit as unlimited."
> **Dev:** "Does the user need to install the harness inside each project?"
> **Domain expert:** "No. The **Global Command** can be launched from any **Working Directory**, while **Harness Configuration** lives in the user's home configuration."
> **Dev:** "Where does the home configuration live?"
> **Domain expert:** "By default, **Harness Configuration** lives at `~/.config/multiagent/multiagent.toml`, with `MULTIAGENT_CONFIG` available when a different file is needed."
> **Dev:** "Can one repository use different agent instructions?"
> **Domain expert:** "Yes. **Local Configuration** can override project-specific behavior, while secrets remain outside local configuration."
> **Dev:** "Does first run fail if no `multiagent.toml` exists?"
> **Domain expert:** "No. **Built-in Profiles** let the **Global Command** start, and configuration can customize them later."
> **Dev:** "Does adding **Claude Runtime** move Explorer or Fixer to Claude automatically?"
> **Domain expert:** "No. **Built-in Profiles** stay unchanged, and users opt in through **Harness Configuration**, **Local Configuration**, or a **Custom Agent**."
> **Dev:** "Can a user add a Security agent?"
> **Domain expert:** "Yes. A **Custom Agent** is added as another **Agent Profile** in configuration."
> **Dev:** "What does `--print-config` show?"
> **Domain expert:** "It shows the **Effective Configuration** with secrets redacted."
> **Dev:** "Can Explorer ask the user what file to inspect?"
> **Domain expert:** "No. Explorer reports the blocker to the **Orchestrator**, and the **Orchestrator** asks a **Clarifying Question** if needed."
> **Dev:** "Is the **Orchestrator** just picking the first agent?"
> **Domain expert:** "No. The **Orchestrator** owns the **Run Plan**, which may involve one or many **Specialized Agents** over the life of a **Run**."
> **Dev:** "Does the harness parse the **Orchestrator**'s prose to decide what runs?"
> **Domain expert:** "No. The **Orchestrator** returns an **Orchestrator Decision**, and the **Chat** renders it for the user."
> **Dev:** "Can Reviewer call Fixer directly?"
> **Domain expert:** "No. Reviewer reports its result to the **Orchestrator**, and the **Orchestrator** decides whether Fixer should run next."
> **Dev:** "Can Explorer just send a paragraph back?"
> **Domain expert:** "No. Explorer returns an **Agent Result** with structured findings, though individual fields may contain prose."
> **Dev:** "Should Fixer inspect the whole repository before editing?"
> **Domain expert:** "No. **Explorer** gathers context first when needed; **Fixer** applies changes and verifies them."
> **Dev:** "Can a typo fix go straight to **Fixer**?"
> **Domain expert:** "Yes. The **Orchestrator** can route obvious tiny edits directly to **Fixer**, while non-trivial changes start with **Explorer**."
> **Dev:** "When does **Reviewer** run?"
> **Domain expert:** "For normal code-change work, **Reviewer** runs after **Fixer** and sends findings back to the **Orchestrator**."
> **Dev:** "Can Reviewer edit files if it finds a problem?"
> **Domain expert:** "No. Reviewer's **Agent Capabilities** allow review and verification, while **Fixer** owns file edits."
> **Dev:** "Is that only a prompt instruction?"
> **Domain expert:** "No. **Capability Enforcement** belongs to the **Harness**, so disallowed actions are blocked even if a **Specialized Agent** asks for them."
> **Dev:** "Can the harness commit after a successful review?"
> **Domain expert:** "Only if the user explicitly asks for a **VCS Action** such as commit or push."
> **Dev:** "If Fixer can run commands, can it delete a directory without asking?"
> **Domain expert:** "No. Destructive commands require **Action Approval**."
> **Dev:** "Is Fixer a separate binary?"
> **Domain expert:** "No. Fixer is a **Specialized Agent** defined by an **Agent Profile**, and its **Model Assignment** can differ from the **Orchestrator**."
> **Dev:** "Do long agent instructions need to live inline in TOML?"
> **Domain expert:** "No. The **Instruction Source** can be inline text or a file path."
> **Dev:** "If the home config references `agents/fixer.md`, where is that path resolved?"
> **Domain expert:** "Relative to the home **Harness Configuration** file, not the current **Working Directory**."
> **Dev:** "Where does Fixer's model live?"
> **Domain expert:** "In **Harness Configuration**, so changing Fixer's **Model Assignment** does not require code changes."
> **Dev:** "Does every agent need to use the same provider?"
> **Domain expert:** "No. One **Agent Profile** can use **Codex Runtime** through Codex-owned login while another uses **Z.ai Runtime** through an API key."
> **Dev:** "Does parallel agent work mean starting multiple **Runs**?"
> **Domain expert:** "No. A **Parallel Step Group** runs multiple **Specialized Agent** steps inside one active **Run**."
> **Dev:** "Can two Fixers edit the same file at the same time?"
> **Domain expert:** "No. Each parallel step has a **Parallel File Scope**, and a file can belong to only one scope in the group."
> **Dev:** "Can a parallel Reviewer review the Fixer's changes before the Fixer finishes?"
> **Domain expert:** "No. A parallel **Reviewer** reviews its assigned **Parallel File Scope** as a slice reviewer; the **Orchestrator** can schedule a combined-diff review after the group joins."
> **Dev:** "What happens if one parallel agent gets blocked?"
> **Domain expert:** "Independent steps keep running, then the **Orchestrator** decides from the joined **Parallel Group Result**."
> **Dev:** "What if no runtime is ready on first launch?"
> **Domain expert:** "The **Harness** still opens and shows **Runtime Availability** so the user can see what setup is missing."
> **Dev:** "If `claude --version` works, does that prove **Claude Runtime** is authenticated?"
> **Domain expert:** "No. **Runtime Availability** can remain unknown when authentication cannot be proven without starting model work."
> **Dev:** "Does **Codex Runtime** call the OpenAI API directly?"
> **Domain expert:** "No. **Codex Runtime** launches Codex locally and lets Codex handle its own authentication."
> **Dev:** "Can **Claude Runtime** use Claude Code tools to edit files directly?"
> **Domain expert:** "No. **Claude Runtime** disables Claude Code tools by default, so local reads, edits, commands, and VCS operations remain **Harness Actions**."
> **Dev:** "Does **Claude Runtime** silently inherit project Claude settings and MCP servers?"
> **Domain expert:** "No. **Claude Runtime** minimizes ambient Claude context by default and needs explicit harness support before project-specific Claude settings are used."
> **Dev:** "Can **Z.ai Runtime** edit files directly?"
> **Domain expert:** "No. **Z.ai Runtime** can propose work, but file edits and command execution are **Harness Actions**."
> **Dev:** "Does `multiagent.toml` store the Z.ai API key?"
> **Domain expert:** "No. It stores a **Credential Reference**, such as the environment variable name that contains the API key."

## Flagged ambiguities

- "multiagent-harness" resolved as **Harness**, the terminal-native coordination system rather than a model provider or agent runtime.
- "agent" resolved as **Specialized Agent**, a role configured by an **Agent Profile** rather than a hardcoded process.
- "session history" resolved as **Session History**, the persisted `.multiagent/` record of harness sessions and runs rather than the active **Run** itself.
- "claude integration" resolved as **Claude Runtime**, a CLI-backed execution runtime rather than a direct Anthropic API integration.
- "integrate Claude" does not mean changing **Built-in Profiles** to Claude; Claude adoption is explicit through configuration.
- "multiagent" no longer names the executable; the **Global Command** is `atelier`, while `multiagent.toml` remains the configuration file name.
- "multi agent feature" resolved as **Parallel Step Group** inside one active **Run**, not multiple active **Runs**.
