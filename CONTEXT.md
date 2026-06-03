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
The installed `multiagent` executable that can be launched from any folder.
_Avoid_: project binary, local script

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
An execution runtime that launches Codex as a child CLI process using the user's subscription authentication.
_Avoid_: OpenAI API runtime

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

**Orchestrator Decision**:
The structured output from the orchestrator that describes the run plan, next agent, reason, required capabilities, and stop condition.
_Avoid_: prose plan, routing text

**Agent Result**:
The typed output envelope returned by a specialized agent after it completes a step.
_Avoid_: message, response

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

**Event Stream**:
The chronological TUI record of prompts, routing decisions, agent activity, and results.
_Avoid_: log, transcript, console output

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
- A **Harness Session** may contain multiple **Runs** over time.
- **Session History** is stored in the project-scoped `.multiagent/` directory.
- **Context Resume** creates a new **Run** rather than continuing an old process exactly.
- A **Run Limit** can be a concrete value or explicitly unlimited in configuration.
- A **Global Command** starts a **Harness Session** in the current **Working Directory**.
- **Harness Configuration** applies across working directories.
- **Local Configuration** can override **Harness Configuration** for the current **Working Directory**.
- `MULTIAGENT_CONFIG` can point the **Global Command** to an alternate **Harness Configuration** file.
- **Effective Configuration** is produced by merging built-in profiles, **Harness Configuration**, **Local Configuration**, and command-line overrides.
- A **Prompt** is first handled by exactly one **Orchestrator**.
- An **Orchestrator** delegates work to one or more **Specialized Agents**.
- An **Orchestrator Decision** drives delegation and run state transitions.
- A **Routing Decision** is displayed in the **Event Stream** before the selected **Specialized Agent** runs.
- The **Agent Roster**, **Event Stream**, and **Input Composer** are the primary TUI surfaces.
- An **Orchestrator** owns the **Run Plan** and decides when to delegate, continue, stop, or ask the user for input.
- A **Specialized Agent** reports results to the **Orchestrator** rather than delegating directly to another **Specialized Agent**.
- A **Specialized Agent** reports an **Agent Result** after completing a step.
- A **Clarifying Question** is asked only by the **Orchestrator**.
- A **Run** pauses while waiting for the user's answer to a **Clarifying Question**.
- The initial **Specialized Agents** are **Orchestrator**, **Explorer**, **Oracle**, **Fixer**, **Reviewer**, and **Consul**.
- A non-trivial code-change **Run Plan** uses **Explorer** before **Fixer**.
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
- An **Agent Profile** uses one **Execution Runtime** when performing work.
- The first supported **Execution Runtimes** are **Codex Runtime** and **Z.ai Runtime**.
- **Runtime Availability** is shown for each **Agent Profile** in the **Agent Roster**.
- **Z.ai Runtime** produces reasoning or structured output, while **Harness Actions** are executed by the **Harness**.
- **Z.ai Runtime** uses a **Credential Reference** to name the environment variable containing its API key.

## Example dialogue

> **Dev:** "When the user presses Enter in the TUI, does the **Prompt** go directly to Explorer?"
> **Domain expert:** "No. Every **Prompt** starts with the **Orchestrator**, and the **Orchestrator** decides which **Specialized Agent** handles it next."
> **Dev:** "Does the user confirm the **Routing Decision** first?"
> **Domain expert:** "No, the **Routing Decision** appears in the **Event Stream**, then the chosen **Specialized Agent** runs automatically."
> **Dev:** "Where does the user see which model Fixer is using?"
> **Domain expert:** "The **Agent Roster** shows each **Specialized Agent** with its **Model Assignment** and **Execution Runtime**."
> **Dev:** "Can the user start a second **Run** while Fixer is still working?"
> **Domain expert:** "No. The **Input Composer** controls the active **Run** until it completes, is interrupted, or asks for input."
> **Dev:** "Is a **Run** the same thing as a **Harness Session**?"
> **Domain expert:** "No. A **Harness Session** is the interactive TUI period, and each submitted **Prompt** creates a separate **Run** recorded in **Session History**."
> **Dev:** "Does resume restore the old Codex child process?"
> **Domain expert:** "No. **Context Resume** starts a new **Run** with previous **Session History** available as context."
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
> **Dev:** "Can a user add a Security agent?"
> **Domain expert:** "Yes. A **Custom Agent** is added as another **Agent Profile** in configuration."
> **Dev:** "What does `--print-config` show?"
> **Domain expert:** "It shows the **Effective Configuration** with secrets redacted."
> **Dev:** "Can Explorer ask the user what file to inspect?"
> **Domain expert:** "No. Explorer reports the blocker to the **Orchestrator**, and the **Orchestrator** asks a **Clarifying Question** if needed."
> **Dev:** "Is the **Orchestrator** just picking the first agent?"
> **Domain expert:** "No. The **Orchestrator** owns the **Run Plan**, which may involve one or many **Specialized Agents** over the life of a **Run**."
> **Dev:** "Does the harness parse the **Orchestrator**'s prose to decide what runs?"
> **Domain expert:** "No. The **Orchestrator** returns an **Orchestrator Decision**, and the **Event Stream** renders it for the user."
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
> **Domain expert:** "No. One **Agent Profile** can use **Codex Runtime** through subscription auth while another uses **Z.ai Runtime** through an API key."
> **Dev:** "What if no runtime is ready on first launch?"
> **Domain expert:** "The **Harness** still opens and shows **Runtime Availability** so the user can see what setup is missing."
> **Dev:** "Does **Codex Runtime** call the OpenAI API directly?"
> **Domain expert:** "No. **Codex Runtime** launches Codex as a child CLI process, while **Z.ai Runtime** uses API-key HTTP requests."
> **Dev:** "Can **Z.ai Runtime** edit files directly?"
> **Domain expert:** "No. **Z.ai Runtime** can propose work, but file edits and command execution are **Harness Actions**."
> **Dev:** "Does `multiagent.toml` store the Z.ai API key?"
> **Domain expert:** "No. It stores a **Credential Reference**, such as the environment variable name that contains the API key."

## Flagged ambiguities

- "multiagent-harness" resolved as **Harness**, the terminal-native coordination system rather than a model provider or agent runtime.
- "agent" resolved as **Specialized Agent**, a role configured by an **Agent Profile** rather than a hardcoded process.
- "session history" resolved as **Session History**, the persisted `.multiagent/` record of harness sessions and runs rather than the active **Run** itself.
