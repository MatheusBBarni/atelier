---
title: Quickstart
nav_order: 1
---

Atelier routes every prompt through an orchestrator that plans the work, picks
specialized agents, and runs each step against a real agent-CLI runtime. This
Quickstart takes you from install to a real orchestrated run — and, on the way,
to the moment Atelier asks your permission before it writes. That approval
prompt is the whole point: agents propose actions, you stay in control.

You will preview the loop with **zero setup** first, then connect a real runtime
and watch a read-only run, then make a single approved write.

## Before you begin

- A terminal and **Node.js 20+ / npm 10+** (the recommended install path ships a
  prebuilt native binary).
- A **project directory** to point Atelier at — any repository you are happy to
  let an agent read. Atelier opens in the current directory, or in `--cwd <path>`.
- **One** of the following, depending on how far you want to go:
  - *Nothing* — the credential-free `fake` runtime previews the loop with no
    account or key (it simulates; it does not do real work).
  - A **`ZAI_API_KEY`** for a real run — the default orchestrator runs on the
    `zai` runtime (model `glm-5.1`) and fails without it.
  - A **`codex` login** (`codex login`) — the default worker agents (explorer,
    fixer, reviewer) run on the `codex` runtime out of the box.

## Install Atelier and check your setup

Install the global command and confirm what's wired up:

```bash
npm install -g @matheusbbarni/atelier
atelier --doctor
```

`atelier --doctor` reports which runtimes and credentials are ready without
starting the TUI; add `--json` for a machine-readable report
(`atelier --doctor --json`). It will flag a missing `ZAI_API_KEY` or `codex`
login — that's expected if you haven't set them up yet. You can still preview the
loop without either, next.

Building from source instead? `cargo install --path .` produces the same
`atelier` binary.

## Preview the loop with zero setup (`fake` runtime)

Before touching any credential, see the orchestration loop run end to end. The
`fake` runtime is a **zero-setup preview**: it streams the same plan → route →
step → complete loop the real runtimes drive, but it simulates the work instead
of calling a provider. It needs no key and no login.

In a scratch directory, create a local `atelier.toml` that points every
default agent at the `fake` runtime:

```toml
[runtimes.fake]
type = "fake"

[agents.orchestrator]
runtime = "fake"

[agents.explorer]
runtime = "fake"

[agents.oracle]
runtime = "fake"

[agents.consul]
runtime = "fake"

[agents.fixer]
runtime = "fake"

[agents.reviewer]
runtime = "fake"
```

Then launch the TUI in that directory and send any prompt:

```bash
atelier
```

Type something like `summarize this project` and submit. Watch the orchestrator
open a plan, route a step to an agent, stream progress, and finish the run — the
shape of every real run, with nothing to configure. This is a preview, not a
real result; the `fake` runtime never reads your files or calls a model.

When you've seen the loop, delete the scratch `atelier.toml` (or leave the
directory) and move on to a real run.

## Connect a real runtime and make your first run

Generate a starter configuration and instruction files:

```bash
atelier --init-config
```

This writes `atelier.toml` (and `agents/*.md` prompts) under
`~/.config/.atelier/`. Open that file once to see the stock defaults — the
orchestrator on `zai`, the workers on `codex`.

Provide the credentials the defaults expect:

```bash
export ZAI_API_KEY="your-key"   # default orchestrator (zai / glm-5.1)
codex login                      # default workers (explorer, fixer, reviewer)
```

Re-run `atelier --doctor` until it reports everything ready, then start the TUI
in a real project:

```bash
cd path/to/your/project
atelier
```

Send a **read-only** prompt:

```text
summarize this project
```

The orchestrator routes to read-capable agents (the explorer holds only the
`read` capability), so the run requests `read` and `search` actions and never
asks to change anything. A first win, with no risk to your files.

## Your first approved write (the safety "aha")

Atelier ships in `approval_mode = "yolo"` — it auto-approves actions so a run
flows without interruption. To *see* the control plane at the moment it acts,
switch to `normal` mode, where every write or command action pauses for your
explicit approval.

Edit the `atelier.toml` that `atelier --init-config` created (under
`~/.config/.atelier/`) and set:

```toml
approval_mode = "normal"
```

Restart `atelier`, then send a prompt that needs a write:

```text
add a short NOTES.md file describing what this project does
```

When the orchestrator routes the change to a write-capable agent (the fixer),
the run pauses on an **approval prompt** before the file is touched:

```text
Action approval required
$ write NOTES.md
```

Type `y` (or `yes` / `approve`) to allow it, or `n` to deny — denied actions are
recorded just like approved ones. Read actions never pause; only writes and
commands do, and only the capabilities you granted are ever requested. That
gate — capabilities decide what an agent *may* do, approval mode decides when
you're *asked* — is Atelier's control plane, and you just used it.

## Recipes

Two prompts to try once you're set up. Both assume a real run; switch
`approval_mode` to `normal` first if you want to approve each write.

- **Read-only review** — `review the error handling in src/ and list the
  riskiest gaps` routes to read/review agents and returns findings without
  editing anything.
- **Scoped write** — `add a CONTRIBUTING.md with a short build-and-test section`
  routes to the fixer, which proposes a single `write` action you approve before
  it lands.

## Where to go next

- [Concepts](../concepts/) — the mental model: the orchestrator run loop, run
  lifecycle, and how agents differ from runtimes.
- [Governance &amp; Safety](../governance/) — exactly what an agent can and can't
  touch: mediated actions, read vs write roots, capabilities vs approval mode,
  limits, and the durable replayable record.
