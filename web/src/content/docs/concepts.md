---
title: Concepts
nav_order: 2
---

Atelier is a control plane for agent work. Every prompt you send is not handed
to a single model — it runs through an **orchestrator** that plans the work,
routes each step to a specialized **agent**, and executes that step against a
backing **runtime**, while you keep the ability to see and approve what happens.
This page is the mental model behind that loop: the run lifecycle, how routing
chooses who does the work, the difference between an agent profile and a runtime,
and the control plane that ties them together. It is explanation, not reference —
exact keys and defaults live on [Configuration](../configuration/).

## The orchestrator run loop

When you submit a prompt, Atelier compiles it (injecting any skill or prefix
context) and then drives a loop. The orchestrator is itself an agent: it reads
the prompt and the run so far, and emits a **decision** — a plan, the next agent
to run, the capabilities that step needs, and a stop condition. Atelier executes
that step against a runtime, streams the result back into the run, and asks the
orchestrator for its next decision. The loop repeats until a decision marks the
run complete, fails it, or asks you a question.

Two properties make this a control plane rather than a chat:

- **Agents never touch your machine directly.** A step's work is a set of
  *action requests* (read a file, search, run a command, apply a patch, write a
  file). Each request is mediated and gated before it executes — see
  [Governance &amp; Safety](../governance/).
- **The whole run is recorded as events.** Plans, steps, actions, approvals, and
  results are appended to a durable per-session history, so a run can be replayed
  and audited rather than reconstructed from scrollback.

## What are the run lifecycle states?

A run advances through a small set of states. The orchestrator's decisions — not
a fixed script — are what move it from one to the next:

- **Idle** — no run is in flight; Atelier is waiting for a prompt.
- **Planning** — your prompt has been submitted and the orchestrator is deciding
  the plan and the first step.
- **Running** — a step is executing against a runtime. Most of a run is spent
  here, cycling between the orchestrator's decisions and the steps they schedule.
- **WaitingForUser** — the run is paused on you. The orchestrator asked a
  clarifying question, or a step needs an approval before it can act. The run
  resumes from your answer.
- **Completed** — a decision declared the goal met and the run finished cleanly.
- **Failed** — the run hit an unrecoverable error (for example, a step that
  could not be satisfied even after model fallbacks).
- **Interrupted** — you stopped the run before it reached its own end.
- **LimitReached** — a configured limit (such as a step or turn ceiling) stopped
  the run. Limits are part of the control plane; see
  [Configuration](../configuration/).

`Completed`, `Failed`, `Interrupted`, and `LimitReached` are terminal — the run
is over and a new prompt starts a fresh one. `WaitingForUser` is the only pause
that hands control back to you mid-run.

## How does the orchestrator route a step?

Each decision names *who* does the next step, and routing has three shapes:

- **A single agent.** The common case: the orchestrator picks one specialized
  agent (an explorer to read, a fixer to change code, a reviewer to check it) and
  gives it an instruction and the capabilities that step is allowed to use.
- **A parallel group.** When independent work can proceed at once, the
  orchestrator can schedule several agents together, each scoped to its own set
  of files so their writes cannot collide. The group completes as a unit before
  the run moves on.
- **A council.** A serial review workflow that runs a panel of reviewer agents in
  sequence. The orchestrator routes to a council only for high-risk decisions —
  architecture, security, data integrity, difficult reviews — or when you
  explicitly ask for one. It is a deliberate, heavier path, not the default.

In every case the orchestrator also declares the capabilities the step requires,
which is the first gate the requested actions are checked against.

## Agent profiles vs. runtimes

These are the two concepts people most often conflate, and keeping them apart is
the key to configuring Atelier.

An **agent profile** is a *role*. It declares what an agent is for: its
capabilities (what it is allowed to do — read, plan, answer, write, and so on),
the model it should use, a model fallback chain, its effort level, and the prompt
that briefs it. The built-in profiles — orchestrator, explorer, oracle, consul,
fixer, reviewer — are roles in the run, defined under `[agents.*]`.

A **runtime** is the *engine* a profile runs on: the backing agent CLI or HTTP
service that actually executes a step. Atelier ships five runtime kinds —
`codex`, `claude`, `cursor`, `zai`, and `fake` — defined under `[runtimes.*]`.

A profile *targets* a runtime. The stock orchestrator profile, for instance, is a
planning role that runs on the `zai` runtime with the `glm-5.1` model, while the
default worker profiles (explorer, fixer, reviewer) run on the `codex` runtime.
The same role could be pointed at a different runtime, and the same runtime can
back many roles — which is exactly why the [Quickstart](../quickstart/) can point
every default agent at the zero-setup `fake` runtime to preview the loop without
credentials. If a step fails on a *retryable* provider error, Atelier retries the
same model before falling back to the next model in the profile's chain, and only
the next model — never silently a different role.

The exact profiles, runtimes, models, and fallback chains are documented on
[Configuration](../configuration/).

## The control plane

Routing decides *who* runs; the control plane decides *what they may do and when
you are asked*. Two layers, plus the record, make a run safe to point at a real
repository:

- **Capabilities** declare what an agent is *allowed* to do. A request for an
  action outside a step's granted capabilities is refused before it runs.
- **Approval mode** decides when you are *asked*. The default `yolo` mode
  auto-approves so a run flows uninterrupted; `normal` mode pauses every write or
  command action for your explicit approval (read actions never pause). That
  pause is the `WaitingForUser` state in action.
- **Workspace roots and limits** bound *where* actions can read and write, and
  how far a run can go.
- **The durable record** captures every plan, action, approval, and result, so
  what happened is replayable rather than implied.

Capabilities decide what an agent *may* do; approval mode decides when you are
*asked*. That distinction — and exactly what an agent can and cannot touch — is
the subject of the next page.

## Where to go next

- [Governance &amp; Safety](../governance/) — the trust detail: mediated actions,
  read vs. write roots, capabilities vs. approval mode, limits, and the durable
  replayable record.
- [Configuration](../configuration/) — every `atelier.toml` section: the agent
  profiles, runtimes, models and fallbacks, council, limits, and UI options
  referenced conceptually here.
