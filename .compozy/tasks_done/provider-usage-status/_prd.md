# Provider Usage Status PRD

Status: Approved
Date: 2026-06-12

## Overview

Atelier should give solo heavy users a fast, truthful provider usage status view before they start or continue a long agent session. The V1 product answers one practical question: can I keep working on this provider right now, and what should I do next if I cannot?

The experience is runway-first. It prioritizes practical readiness states over raw usage accounting or false exact quota. If a provider is ready, Atelier should say so plainly. If usage runway is limited or blocked, Atelier should explain the provider-native signal and the next action. If exact usage is unsupported, Atelier must say that exact remaining usage is unavailable and point the user to the provider-native place to verify it.

When a provider supports exact remaining usage through an explicit supported account connection or documented provider surface, Atelier can show exact remaining messages, credits, requests, or reset timing. When it does not, Atelier must not infer exact quota from local activity, failure patterns, or incomplete signals.

## Problem

Solo heavy users often begin long agentic work without knowing whether their selected provider has enough practical runway. They may discover limits only after a task is already in flight, which wastes time, increases failed runs, and forces reactive provider switching.

The problem is not just missing quota numbers. Subscription quota, paid account usage, rate limits, authentication state, model availability, provider incidents, and local runtime health are different concepts. Collapsing them into one synthetic quota would mislead users. V1 should make the current provider state understandable without pretending Atelier knows more than the provider exposes.

## Goals

- Help solo heavy users decide whether to start, continue, pause, or switch providers before a long Atelier session.
- Provide a provider status answer in under 15 seconds for the median command invocation.
- Avoid false precision by never estimating exact Claude, Codex, or other provider subscription quota from local state, CLI activity, or error patterns.
- Show a clear state for each relevant provider: ready, limited runway, blocked, unavailable usage, unauthenticated, misconfigured, provider error, or local-only status.
- Make unsupported exact quota understandable and actionable by explaining where the user should verify usage in the provider-native account experience.
- Keep the default output compact enough to scan during normal workflow setup.

## User Stories

- As a solo heavy user starting a long task, I want to see whether my preferred provider has practical runway so I can avoid a mid-run interruption.
- As a solo heavy user near a provider limit, I want to know whether I should continue, wait for reset, reduce scope, or switch providers.
- As a user with multiple providers configured, I want to compare readiness states quickly so I can choose the best provider for the next run.
- As a user whose provider does not expose exact usage, I want Atelier to say that clearly so I do not trust invented numbers.
- As a user with an authentication or configuration issue, I want the status output to distinguish that issue from quota exhaustion.
- As a user running a local provider, I want the status output to describe local readiness without pretending local runtime health is provider account usage.

## Core Features

| Priority | Feature | Requirement |
| --- | --- | --- |
| 1 | Runway-first status summary | Show each relevant provider with a concise practical status and one clear next action. |
| 2 | Truthful status states | Use distinct states for ready, limited runway, blocked, unavailable usage, unauthenticated, misconfigured, provider error, and local-only status. |
| 3 | Provider-native reason | Explain the observed reason for each non-ready state without merging unrelated quota, billing, auth, or local-health signals. |
| 4 | Exact usage when supported | Display exact remaining messages, credits, requests, or reset timing only when returned by a supported provider account surface. |
| 5 | Unsupported exact usage state | When exact usage is unavailable, state that limitation directly and guide users to the provider-native usage or account page. |
| 6 | Reset and freshness context | Show reset timing and status freshness when known; otherwise say unknown rather than guessing. |
| 7 | Compact default view | Keep the default output focused on provider, status, reason, and next action. Deeper detail can be available outside the primary summary. |
| 8 | Share-safe output | Avoid exposing secrets, account identifiers, or sensitive usage details in default output. |

## User Experience

The primary flow is a quick status check before a user starts or resumes a long Atelier run. The user invokes the provider usage status experience and sees a compact provider list. Each provider row should answer three questions: can I use this provider now, how confident is Atelier in that answer, and what should I do next?

Ready providers should feel direct and low-friction. The user should be able to proceed without reading a long explanation.

Limited or blocked providers should be explicit. The output should explain whether the issue is remaining usage, reset timing, authentication, configuration, provider availability, or local runtime health.

Unsupported exact usage should not feel like a failure. The product should explain that Atelier cannot retrieve exact remaining quota for that provider and should direct the user to verify in the provider-native account experience.

The experience should be useful in both terminal and UI contexts, but V1 should keep the same product meaning across surfaces. The status labels, reason language, and next actions should not change meaning between surfaces.

## Non-Goals

- Estimating exact subscription quota for providers that do not expose exact remaining usage through a supported account surface.
- Combining subscription quota, paid account usage, rate limits, authentication state, provider incidents, model availability, and local runtime health into one global quota number.
- Building a background monitoring service, notification system, or long-term analytics dashboard in V1.
- Purchasing, upgrading, or managing provider plans from inside Atelier.
- Solving team allocation, organization budgets, or shared account governance.
- Supporting every possible provider in the first release.
- Predicting future usage consumption for arbitrary tasks beyond the current practical runway status.

## High-Level Technical Constraints

- Exact remaining usage must only be shown when a supported provider account surface provides it with clear semantics.
- Atelier must degrade to unavailable usage, provider error, or unknown reset timing rather than inventing a number.
- Local runtime readiness must remain distinct from provider account usage.
- Default output must be safe to paste into logs, issues, and support requests.
- Status language must preserve provider-specific meaning where providers use different quota, billing, reset, or rate-limit concepts.

## Phased Rollout Plan

| Phase | Scope | Outcome |
| --- | --- | --- |
| 1 | MVP runway status | Provide the status command or equivalent status surface for configured providers, with truthful readiness states, reasons, next actions, and exact usage only where supported. |
| 2 | Provider coverage hardening | Improve wording, freshness cues, and provider-specific status handling for the highest-priority providers used by solo heavy users. |
| 3 | Workflow integration | Surface the same runway-first status in provider selection and preflight moments before long runs. |
| 4 | Expanded intelligence | Consider history, alerts, and richer runway guidance only after V1 proves users trust and use the basic status experience. |

## Success Metrics

| Metric | Target |
| --- | --- |
| Time to provider status answer | Median status result appears in under 15 seconds. |
| Runway decision confidence | At least 80% of tested solo heavy users can decide whether to proceed, wait, or switch provider after reading the status output. |
| Unsupported quota comprehension | At least 85% of users shown an unsupported exact-usage state understand that Atelier cannot retrieve exact quota from that provider. |
| False precision rate | Zero known cases where Atelier displays an inferred exact remaining quota for an unsupported provider. |
| Next-action clarity | At least 90% of non-ready states include a clear user action or provider-native verification path. |
| Workflow adoption | At least 50% of active solo heavy users run or view provider status weekly after launch. |
| Limit surprise reduction | Users who check status weekly report fewer mid-run provider-limit surprises than users who do not. |

## Risks and Mitigations

| Risk | Mitigation |
| --- | --- |
| Providers do not expose exact remaining subscription quota. | Treat exact usage as unavailable unless supported; guide users to provider-native verification. |
| Users mistake readiness for a guarantee. | Use practical status and freshness language, not absolute promises. |
| Provider behavior changes or signals become ambiguous. | Fall back to provider error, unavailable usage, or unknown reset timing rather than showing misleading data. |
| Different providers use incompatible usage concepts. | Preserve provider-specific terms in reasons while keeping common runway labels for the top-level status. |
| Output exposes sensitive account details. | Keep default output share-safe with redaction and no secrets, account identifiers, or sensitive values. |
| The command becomes too noisy. | Keep the default view focused on status, reason, and next action; move detailed signals out of the primary summary. |

## Architecture Decision Records

- [ADR-001: Truthful Provider Usage Status Scope](adrs/adr-001.md) - V1 must report provider readiness truthfully and only show exact quota when a supported provider surface exposes it.
- [ADR-002: Runway-First Provider Status](adrs/adr-002.md) - The approved product approach prioritizes practical runway for solo heavy users over raw usage accounting.

## Open Questions

- Which providers should V1 prioritize first for supported exact usage and provider-native verification links?
- What exact runway labels should V1 use in terminal and UI surfaces?
- What minimum freshness threshold should be visible before exact usage is marked stale or unknown?
- What should Atelier show when the provider-native account or usage page is unavailable?
- After the MVP status experience is trusted, where else should runway status appear in the user workflow?
