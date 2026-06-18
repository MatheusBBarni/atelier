# Harness-Owned MCP Tool Access — Task List

## Tasks

| # | Title | Status | Complexity | Dependencies |
|---|-------|--------|------------|--------------|
| 01 | Add rmcp dependency, McpClient trait, and fake stdio server | completed | medium | — |
| 02 | Add mcp.servers config section, mcp_enabled flag, and redaction | completed | medium | — |
| 03 | Build the McpSupervisor actor and McpHandle | completed | high | task_01, task_02 |
| 04 | Add the McpTrustStore with durable trust tiers and pins | completed | medium | task_01 |
| 05 | Add MCP action kinds, capability, validation, and execution | completed | high | task_03, task_04 |
| 06 | Apply record-time redaction at event write | pending | medium | task_05 |
| 07 | Advertise an MCP tool-catalog snapshot to the orchestrator | pending | medium | task_03 |
| 08 | Project MCP tool calls and events into the chat transcript | pending | medium | task_05 |
| 09 | Extend the approval card with MCP description and trust controls | pending | high | task_04, task_05 |
| 10 | Add doctor MCP checks, parity matrix, and local metric | pending | medium | task_03, task_05 |
| 11 | Add the emission repair loop, degrade flag, and spike harness | pending | high | task_01, task_05 |

## Build Waves

- **Wave 0 (no deps):** task_01, task_02
- **Wave 1:** task_03 (←01,02), task_04 (←01)
- **Wave 2:** task_05 (←03,04), task_07 (←03)
- **Wave 3:** task_06, task_08, task_09, task_10, task_11 (all ←05, plus extras)

See `_techspec.md` "Development Sequencing" for the originating build order and `_prd.md` for requirements (CF1–CF8).
