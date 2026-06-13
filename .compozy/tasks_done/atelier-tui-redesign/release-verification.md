# Task 09 — Release Verification & Asset Capture

This file is the human handoff for the parts of task_09 that require an
interactive desktop terminal and a human eye. The agent completed the
automatable docs work (README hero structure, CONTEXT.md surfaces, this
checklist, the website decision); the items below are **blocked on manual
capture/verification** and gate the announcement (ADR-002).

## Why these are manual

The TUI requires an interactive terminal, and this was prepared in a headless
environment with no terminal-capture tooling installed (`vhs`, `termtosvg`,
`asciinema`, `agg` all absent). Real branded screenshots/GIFs and the
three GUI terminal-emulator checks cannot be produced or verified headlessly.
The wordmark visual round also explicitly needs a human eye (ADR-001).

## 1. Assets to capture → `web/public/images/`

Both are referenced by the README hero (see `README.md`, top). Capture against
the **shipped** TUI (tasks 01–08 merged) so the assets match exactly (ADR-002).

- [ ] `atelier-tui-welcome.png` — the welcome screen at **≥80 columns**, a
      **truecolor** terminal, a **fresh session inside a git repo** so the facts
      box shows the repo+branch line and the full wordmark.
- [ ] `atelier-tui-parallel-agents.gif` — a multi-agent run showing **≥2 agents
      running in parallel with visibly distinct accent colors** (the PRD
      differentiator), with the **status footer in frame**.

Convention: PNGs/GIF live alongside the existing 8 assets in
`web/public/images/`; README uses repo-relative paths that render on GitHub.
After placing the files, confirm both images render in the GitHub **rendered**
README view (not just locally), then remove the `RELEASE GATE` comment from
`README.md`.

## 2. Manual compatibility checklist (record PASS/FAIL per cell)

For each terminal, launch a fresh `atelier` session in a git repo and verify the
four surfaces render legibly, then run a short parallel run.

| Surface / Terminal      | Terminal.app (256) | iTerm2 (truecolor) | Alacritty | `NO_COLOR=1` |
|-------------------------|:------------------:|:------------------:|:---------:|:------------:|
| Welcome (wordmark+facts)|                    |                    |           | no wordmark, facts as plain text |
| Status footer           |                    |                    |           |              |
| Dropdowns (`/agent:`, `/skill:`) |           |                    |           |              |
| Parallel run (distinct accents)  |           |                    |           | content present, no color |

- [ ] Terminal.app (256-color): all surfaces legible — recorded PASS.
- [ ] iTerm2 (truecolor): all surfaces legible — recorded PASS.
- [ ] Alacritty: all surfaces legible — recorded PASS.
- [ ] `NO_COLOR=1`: no wordmark, all content present as plain text, no color
      output — recorded PASS.

**Automated partial evidence (already green):** the `NO_COLOR` resolution and
"text identical with/without color" behavior is covered by the theme/welcome/
footer unit + render tests (`tui::tests`, `theme::tests`). The remaining
NO_COLOR row above is the *visual* confirmation in a real terminal.

## 3. Website stale-screenshot decision (req #5)

`web/src/pages/index.astro` (`visualFrames`, ~:132-157) embeds four pre-redesign
TUI screenshots: Run Surface, Skill Picker, Agent Picker, Help Overlay. They now
show superseded chrome (old colors, no welcome, no footer, no per-agent accents).

**Recommendation (owner to confirm):** refresh all four in the **same** human
capture session as the README assets — same terminal setup, low marginal cost —
rather than shipping brand-inconsistent imagery. The PRD scopes website
*changes* out, so if the owner defers, file an explicit follow-up issue:
"Refresh 4 stale TUI screenshots on the marketing site after the visual-identity
release." Record the chosen option in the PR description.

- [ ] Decision recorded in PR: refresh now ⟂ file follow-up (circle one).

## 4. PR description block (paste once the above is done)

```
## Visual identity release (atelier-tui-redesign)

Assets:
- README hero added: welcome screenshot + parallel-agents GIF (web/public/images/).
- Both render in the GitHub view: <yes/no>.

Compatibility (manual):
- Terminal.app (256): PASS/FAIL
- iTerm2 (truecolor): PASS/FAIL
- Alacritty: PASS/FAIL
- NO_COLOR=1: PASS/FAIL (no wordmark, content present, no color)

GIF shows >=2 agents in parallel with distinct accents: yes/no.

Website screenshots: refreshed now / follow-up filed (#____).
CONTEXT.md: Welcome + Status Footer surfaces added.
```
