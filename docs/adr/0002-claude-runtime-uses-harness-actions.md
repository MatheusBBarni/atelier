# Claude runtime uses harness actions

The first Claude CLI integration runs in strict harness-action mode: Claude can reason and return structured contracts, but local reads, edits, commands, and VCS operations remain Harness Actions executed by the harness. This preserves Capability Enforcement, Action Approval, and Session History even though Claude Code has its own local tool system; direct Claude tool use can be reconsidered later only with an explicit policy mapping.
