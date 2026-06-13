# Provider Usage Status

## Overview

Provider Usage Status adds an Atelier status command that tells software engineers whether their Claude and Codex provider setup is ready for a long coding session. V1 reports local provider configuration, account connection state, selected model, command availability, recent provider errors, and documented exact quota when a supported provider integration exposes it.

The feature uses a hybrid scope. It keeps the user's desired outcome, checking remaining Claude and Codex usage inside Atelier, but avoids false precision. If exact remaining subscription quota is unavailable because the provider does not expose it through a supported integration, Atelier shows an explicit unavailable state and guidance to the official provider account or usage page.

## Problem

Atelier users often start long agentic coding sessions without knowing whether their Claude or Codex subscriptions can support the work. Today, a user must leave Atelier, inspect provider dashboards, interpret provider-specific plan and quota language, then return to the coding flow. That breaks concentration and still may not answer whether failures are caused by local configuration, authentication, provider rate limits, billing, subscription quota, or provider outages.

Market context: modern AI developer workflows increasingly depend on external model providers with separate accounts, plans, rate limits, billing controls, and model availability. Heavy users operate near provider limits more often than occasional users, so readiness and quota visibility become part of the daily development loop. The repository exploration found no existing Atelier quota, subscription, remaining-credit, or usage telemetry path, which means this feature creates a new provider-status capability rather than extending an existing quota screen.

The exact-quota requirement has a feasibility constraint. Claude and Codex subscription quota appears to be provider-owned account state, not something Atelier can compute from local CLI sessions or in-repo runtime data. V1 must display exact remaining quota only when a documented provider API or explicit account-linking integration returns it.

## Summary / Differentiator

The differentiator is truthful provider readiness, not a guessed quota number. Atelier should make the next action obvious: continue working, reconnect a provider account, fix local configuration, check the provider dashboard, or treat exact quota as unsupported by that provider.

## Core Features

| Priority | Feature | Description |
| --- | --- | --- |
| 1 | Provider usage status command | Add an Atelier command that shows Claude and Codex readiness in one place. |
| 2 | Capability-based provider adapters | Each provider declares which status classes it supports: subscription quota, API billing usage, rate limits, auth state, model availability, command availability, and local runtime health. |
| 3 | Exact quota when supported | Display exact remaining quota only when returned by a documented provider integration or explicit account-linking surface. |
| 4 | Unsupported quota state | Show exact quota as unavailable when a provider does not expose it through a supported integration, with clear copy and official-dashboard guidance. |
| 5 | Provider diagnostics | Separate local config failures, missing auth, provider errors, billing or rate-limit problems, model unavailability, and unsupported quota fields. |
| 6 | Freshness and source labels | Show whether data is live, cached, local-only, provider-returned, or unavailable, including fetch time where relevant. |
| 7 | Privacy-safe account display | Identify the connected account or organization only enough for the user to disambiguate, without leaking sensitive identifiers into logs or telemetry. |

## Integration With Existing Features

Atelier already models command execution status and chat item lifecycle status, but repository discovery found no quota or subscription telemetry. Provider Usage Status should therefore introduce a new provider-status domain instead of overloading existing command-status concepts.

The command should fit the existing Atelier command workflow: users invoke a status-style command from inside Atelier, receive a concise provider readiness result, and can inspect details when a provider reports a problem.

## KPIs

| KPI | Target |
| --- | --- |
| Time to answer provider readiness | Median under 15 seconds from command invocation to actionable status. |
| Clear status coverage | At least 90% of runs return one clear provider state: exact quota, unsupported quota, unauthenticated, misconfigured, provider error, or local-only status. |
| Ambiguous wording rate | Fewer than 2% of status results use ambiguous language such as maybe, inferred, or estimated for quota. |
| Repeat usage | At least 80% of connected-account users run the status command again within 14 days. |
| Diagnostic usefulness | At least 95% of known auth, permission, billing, rate-limit, and model-availability failures receive actionable messages. |
| Unsupported quota comprehension | At least 85% of users shown an unsupported exact-quota state correctly understand that Atelier cannot retrieve exact quota from that provider. |

## Feature Assessment

| Criteria | Score | Rationale |
| --- | --- | --- |
| Impact | Strong | Reduces provider uncertainty before high-cost coding sessions and helps users recover from provider failures faster. |
| Reach | Strong | Applies to most Atelier users who rely on Claude, Codex, or multiple model providers. |
| Frequency | Strong | Heavy users may check provider readiness before or during daily work sessions. |
| Differentiation | Strong | A truthful provider-readiness surface inside the coding workflow is more useful than external dashboard links alone. |
| Defensibility | Maybe | Basic status checks can be copied, but provider capability modeling and diagnostics can compound as more providers are added. |
| Feasibility | Strong for readiness; Maybe for exact quota | Local readiness and diagnostics are feasible; exact subscription quota depends on documented provider support. |

## Council Insights

The council recommendation was to avoid positioning V1 as an exact-quota feature. Exact remaining quota is too strong unless Claude and Codex expose documented quota surfaces. The accepted V1 is a provider usage status command that reports readiness, account connection, selected provider/model config, recent provider errors, and exact quota only when supported.

Key trade-offs:

| Area | Decision |
| --- | --- |
| Scope | Ship readiness/status first; keep exact quota behind provider capability checks. |
| Trust | Never infer subscription quota from local CLI state, session activity, or error patterns. |
| Provider semantics | Preserve provider-native meanings instead of forcing all usage into one percentage. |
| Security | Account linking requires explicit consent, least privilege, encrypted local storage, revocation, and token redaction. |
| Product clarity | Separate subscription quota, API billing usage, rate limits, auth state, selected model, and local runtime health. |

The strongest product framing is a preflight readiness check for long Atelier sessions: provider auth, model availability, quota support, recent failures, and likely blockers in one command.

## Out Of Scope (V1)

| Exclusion | Justification |
| --- | --- |
| Dashboard scraping | Fragile, privacy-sensitive, and not trustworthy enough for quota reporting. |
| Undocumented provider endpoints | Creates maintenance, reliability, and trust risks. |
| Inferred exact subscription quota | Local sessions and CLI state cannot prove remaining provider quota. |
| Combined Claude plus Codex quota total | Provider quotas have different semantics and should not be normalized into a single number. |
| Team admin billing dashboard | Larger account-management scope than a V1 user-facing readiness command. |
| Quota forecasting | Requires reliable historical usage and quota semantics that V1 does not yet have. |
| Automatic plan recommendations | Commercial and provider-specific guidance is outside the immediate readiness problem. |

## Architecture Decision Records

| ADR | Status | Summary |
| --- | --- | --- |
| `.compozy/tasks/provider-usage-status/adrs/adr-001.md` | Accepted | Proceed with a provider-integration status command scoped to truthful provider status and documented quota capabilities. Exact remaining quota is displayed only when returned by a supported provider integration. |

## Open Questions

| Question | Why It Matters |
| --- | --- |
| Do Anthropic/Claude and OpenAI/Codex expose documented subscription quota APIs or account-linking surfaces for the relevant plans? | Determines whether exact quota can appear for either provider in V1. |
| Which command surface should host this: a dedicated status command, an existing command palette action, or both? | Affects discoverability and implementation scope. |
| What is the minimum account identifier that helps users distinguish accounts without exposing sensitive details? | Balances usability and privacy. |
| Should cached provider status be shown when live provider fetch fails? | Affects trust, freshness, and failure handling. |
| What official provider account or usage pages should Atelier link to when exact quota is unsupported? | Ensures fallback guidance is actionable. |
| Should V1 include only Claude and Codex, or define the adapter model for future providers immediately? | Affects extensibility and initial scope. |

## Recommended Next Step

Proceed to PRD creation with the Hybrid V1 scope: a truthful provider usage/readiness status command, with exact quota gated by documented provider capability support.
