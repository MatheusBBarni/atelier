# Task Memory: task_04.md

Keep only task-local execution context here. Do not duplicate facts that are obvious from the repository, task file, PRD documents, or git history.

## Objective Snapshot
Authored `web/src/content/docs/quickstart.md` (the activation north star). Lazy/fake-first flow: prerequisites → install + `atelier --doctor` → zero-setup `fake` preview → real runtime + read-only run → approved-write "aha" (approval prompt) → recipes → onward links. Frontmatter `title: Quickstart`, `nav_order: 1` (first in section nav/index). Done + verified (build green, commands accuracy-checked).

## Important Decisions
- `fake` preview config: a local `multiagent.toml` declaring `[runtimes.fake] type="fake"` and overriding all six default-enabled agents (orchestrator/explorer/oracle/consul/fixer/reviewer) to `runtime = "fake"`. Partial agent overrides merge over built-in defaults, so only `runtime` needs setting. Framed explicitly as a simulation/preview, not a real run.
- Approved-write "aha" requires `approval_mode = "normal"` — default is `"yolo"` (auto-approve, no prompt). Page states this explicitly so the prompt is reproducible.
- Did NOT document the fake runtime's internal control phrases ("write action", "approval action", etc.) — they are test scaffolding (`src/runtime/fake.rs`), not product UX. A generic prompt drives the default fake loop.
- Onward links use relative `../concepts/` and `../governance/` (not `assetPath`) — Markdown body can't call the base helper; relative links are base-agnostic and resolve under both `/` (local) and `/atelier` (Pages).

## Learnings
- Markdown content links can't use `assetPath`/`BASE_URL`; relative `../<slug>/` is the base-correct cross-link form for prose pages, relying on the site's directory build format + trailing-slash nav (DocsLayout `docHref` emits `/docs/<id>/`).
- Approval prompt UX (verified): chat title "Action approval required" with `$ write <path>` body (`src/app/chat/projection.rs:577`); approve by typing `y`/`yes`/`approve`, deny otherwise (`src/tui/mod.rs:1178`). Read actions never pause; only writes/commands in `normal` mode.
- `atelier --init-config` writes the starter `multiagent.toml` (+ `agents/*.md`) under `~/.config/.multiagent/`; starter top line is `approval_mode = "yolo"`.

## Files / Surfaces
- New: `web/src/content/docs/quickstart.md`.
- Verified-against (no edits): `src/cli.rs` (flags), `src/config/mod.rs` (approval_mode enum, agent/runtime defaults, init_config template), `src/runtime/fake.rs`, `src/app/chat/projection.rs`, `src/tui/mod.rs`, `README.md`.

## Errors / Corrections
- Initial draft told the reader to edit "the multiagent.toml you created above" for `approval_mode`, but the fake scratch config was deleted by then — corrected to point at the `--init-config` file under `~/.config/.multiagent/`.

## Ready for Next Run
- Cross-link targets `../concepts/` (task_05) and `../governance/` (task_06) do NOT exist yet — the relative links 404 until those pages land in the same wave. A link-check (lychee, task_08) over the full set will only pass once 05/06 ship. Not a defect in task_04.
- task_07 (llms.txt/twins) must include `quickstart` in the corpus.
