//! Runtime-owned provider runway status model.
//!
//! This module defines the typed contract that describes whether each
//! configured provider is usable right now and what the user should do next,
//! per the Provider Usage Status TechSpec and ADR-001 / ADR-002. It is
//! deliberately a *data* surface only: it holds no provider-probing logic, no
//! terminal/TUI formatting, and no slash-command metadata. Later tasks build
//! the provider status service (probing), the compact renderer, and the
//! `/provider:status` routing on top of these types.
//!
//! Truthfulness rules baked into the shape:
//!
//! - Exact remaining usage is representable *only* through
//!   [`UsageAvailability::Exact`], and the capability-gated
//!   [`UsageAvailability::exact_if_supported`] constructor refuses to produce
//!   it unless an adapter declares the supporting capability. Unsupported,
//!   unavailable, and not-applicable usage are first-class, distinct states —
//!   never errors and never inferred numbers (ADR-001).
//! - Capability support ([`ProviderStatusCapabilities`]) is modeled separately
//!   from a status result so unsupported exact usage is not mistaken for a
//!   failed provider check.
//! - Diagnostics carry only share-safe label/detail text; the type has no
//!   field for secrets, tokens, account identifiers, emails, organization IDs,
//!   or raw provider payloads (redaction itself lives in the service layer).

use crate::config::{EffectiveConfig, RuntimeConfig, RuntimeKind};
use crate::runtime::{check_runtime_availability, RuntimeAvailability, RuntimeAvailabilityStatus};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::{sleep, timeout};

/// Stable identity for a provider whose runway status can be reported.
///
/// Mirrors the configured runtimes and adds [`ProviderId::Local`] for
/// local-only runtime health, which has no provider account behind it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderId {
    Claude,
    Codex,
    Cursor,
    HttpApi,
    Fake,
    Local,
}

impl ProviderId {
    /// Default human-readable display name. Callers may override with a
    /// configured display name on the status result.
    pub fn default_display_name(&self) -> &'static str {
        match self {
            ProviderId::Claude => "Claude",
            ProviderId::Codex => "Codex",
            ProviderId::Cursor => "Cursor",
            ProviderId::HttpApi => "HTTP API",
            ProviderId::Fake => "Fake",
            ProviderId::Local => "Local",
        }
    }
}

impl From<RuntimeKind> for ProviderId {
    fn from(kind: RuntimeKind) -> Self {
        match kind {
            RuntimeKind::Claude => ProviderId::Claude,
            RuntimeKind::Codex => ProviderId::Codex,
            RuntimeKind::Cursor => ProviderId::Cursor,
            RuntimeKind::HttpApi => ProviderId::HttpApi,
            RuntimeKind::Fake => ProviderId::Fake,
        }
    }
}

/// The fixed runway state set from the TechSpec. Each maps to a top-level row
/// label in the compact `/provider:status` output and is matched directly —
/// never parsed from a formatted string.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderRunwayState {
    /// Required local/provider checks passed and no blocking signal is known.
    Ready,
    /// Provider-supplied data says usage is constrained, near limit,
    /// rate-limited, or reset-bound.
    LimitedRunway,
    /// Provider or runtime signal says the provider cannot be used for the
    /// intended session.
    Blocked,
    /// Provider may be usable, but exact remaining usage is unsupported,
    /// unavailable, or not exposed.
    UnavailableUsage,
    /// Credentials, account link, or login state is missing or invalid.
    Unauthenticated,
    /// Local provider configuration is missing, invalid, or points to an
    /// unusable model/provider.
    Misconfigured,
    /// A live check failed due to provider-side or network behavior that
    /// cannot be classified more specifically.
    ProviderError,
    /// Local runtime readiness can be described, but account usage does not
    /// apply.
    LocalOnlyStatus,
}

/// Provider-native usage metric. Kept provider-native rather than normalized
/// into a synthetic percentage (ADR-001). [`UsageMetric::Other`] preserves a
/// metric a provider exposes that does not map onto the common set.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UsageMetric {
    Messages,
    Requests,
    Tokens,
    Credits,
    Other(String),
}

/// A provider-native usage amount with an optional unit label. Stored as a
/// floating amount so fractional credits are represented without loss; this is
/// only ever populated from documented provider data, never inferred.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UsageAmount {
    pub value: f64,
    pub unit: Option<String>,
}

impl UsageAmount {
    pub fn new(value: f64) -> Self {
        Self { value, unit: None }
    }

    pub fn with_unit(value: f64, unit: impl Into<String>) -> Self {
        Self {
            value,
            unit: Some(unit.into()),
        }
    }
}

/// The window a usage metric is measured over, when the provider documents it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UsageWindow {
    Daily,
    Weekly,
    Monthly,
    /// A rolling window of the given number of hours (e.g. a 5-hour window).
    Rolling {
        hours: u32,
    },
    Other(String),
}

/// Exact, documented remaining usage from a supported provider integration.
///
/// This is the *only* representation of exact usage. Constructing it requires
/// an `observed_at` timestamp so callers cannot present exact usage without
/// recording when it was observed. It must never be built from local CLI
/// state, token logs, request counts, or failed-command frequency.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExactUsage {
    pub metric: UsageMetric,
    pub remaining: UsageAmount,
    pub limit: Option<UsageAmount>,
    pub window: Option<UsageWindow>,
    pub reset_at: Option<DateTime<Utc>>,
    pub observed_at: DateTime<Utc>,
}

/// Whether and how exact remaining usage is available for a provider.
///
/// `Unsupported`, `Unavailable`, and `NotApplicable` are deliberately distinct
/// truthful states — they are not errors and not estimates.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UsageAvailability {
    /// Documented exact usage from a capability-supported provider integration.
    Exact(ExactUsage),
    /// The provider may be usable, but exact usage is not exposed by the
    /// integration at all.
    Unsupported { provider_url: Option<String> },
    /// Exact usage is supported in principle but could not be retrieved this
    /// invocation (timeout, transient failure, missing account link, etc.).
    Unavailable {
        reason: String,
        provider_url: Option<String>,
    },
    /// Account usage does not apply to this provider (e.g. a local runtime).
    NotApplicable,
}

impl UsageAvailability {
    /// Capability-gated constructor for exact usage.
    ///
    /// Returns [`UsageAvailability::Exact`] only when `capabilities` declares a
    /// supporting usage capability; otherwise it degrades truthfully to
    /// [`UsageAvailability::Unsupported`]. This is the guard that prevents
    /// exact usage from being represented without supporting provider
    /// capability context.
    pub fn exact_if_supported(
        capabilities: &ProviderStatusCapabilities,
        usage: ExactUsage,
        provider_url: Option<String>,
    ) -> Self {
        if capabilities.supports_exact_usage() {
            UsageAvailability::Exact(usage)
        } else {
            UsageAvailability::Unsupported { provider_url }
        }
    }

    /// True only for [`UsageAvailability::Exact`].
    pub fn is_exact(&self) -> bool {
        matches!(self, UsageAvailability::Exact(_))
    }
}

/// Reset timing for a usage window. `Unknown` is a first-class state: the
/// TechSpec requires saying `unknown` when a provider does not return it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResetAvailability {
    Known { reset_at: DateTime<Utc> },
    Unknown,
    NotApplicable,
}

/// How fresh the status data is. `observed_at` is recorded for live and cached
/// data so older results can be labeled in output.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StatusFreshness {
    Live { observed_at: DateTime<Utc> },
    Cached { observed_at: DateTime<Utc> },
    Unknown,
}

/// Where a status result was derived from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StatusSource {
    /// A documented provider API/account integration.
    ProviderApi,
    /// A local runtime/CLI readiness check.
    LocalRuntimeCheck,
    /// Local provider configuration only.
    LocalConfig,
    Unknown,
}

/// Typed reason code for a status, kept separate from the human-readable
/// summary so consumers can branch without parsing strings.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReasonCode {
    Healthy,
    UsageConstrained,
    UsageBlocked,
    UsageUnsupported,
    UsageUnknown,
    AuthRequired,
    Misconfigured,
    ModelUnavailable,
    ProviderError,
    LocalOnly,
}

/// Short, share-safe reason for a status row. `summary` must not contain
/// secrets or account identifiers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderStatusReason {
    pub code: ReasonCode,
    pub summary: String,
}

impl ProviderStatusReason {
    pub fn new(code: ReasonCode, summary: impl Into<String>) -> Self {
        Self {
            code,
            summary: summary.into(),
        }
    }
}

/// The single recommended next action for a provider row. `provider_url`
/// carries an official account/usage page when checking the provider is the
/// most actionable step.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderNextAction {
    /// Nothing to do; safe to proceed.
    Proceed,
    /// Check the official provider account/usage page.
    CheckProviderUsage { provider_url: Option<String> },
    /// Authenticate or re-link the account.
    Authenticate,
    /// Fix local provider/model configuration.
    FixConfiguration,
    /// Switch to a different provider for this session.
    SwitchProvider,
    /// Reduce session scope to fit constrained runway.
    ReduceScope,
    /// Retry the status check or the run later.
    RetryLater,
    /// Verify local runtime/CLI availability.
    VerifyRuntime,
}

/// Whether a provider adapter supports a given class of status data. Modeled
/// separately from results so unsupported data classes are not treated as
/// failed checks (ADR-001 capability negotiation).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilitySupport {
    Supported,
    Unsupported,
    RequiresAccountLink,
    RequiresConfiguration,
}

/// The status data classes a provider adapter declares support for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderStatusCapabilities {
    pub auth_state: CapabilitySupport,
    pub model_availability: CapabilitySupport,
    pub subscription_usage: CapabilitySupport,
    pub api_billing_usage: CapabilitySupport,
    pub rate_limits: CapabilitySupport,
    pub local_runtime_health: CapabilitySupport,
    pub provider_incident_state: CapabilitySupport,
}

impl ProviderStatusCapabilities {
    /// Capabilities with every class `Unsupported` — a safe default for an
    /// adapter that only reports local readiness.
    pub fn none() -> Self {
        Self {
            auth_state: CapabilitySupport::Unsupported,
            model_availability: CapabilitySupport::Unsupported,
            subscription_usage: CapabilitySupport::Unsupported,
            api_billing_usage: CapabilitySupport::Unsupported,
            rate_limits: CapabilitySupport::Unsupported,
            local_runtime_health: CapabilitySupport::Unsupported,
            provider_incident_state: CapabilitySupport::Unsupported,
        }
    }

    /// True when either subscription or API billing usage is `Supported`, i.e.
    /// the adapter may return [`UsageAvailability::Exact`].
    pub fn supports_exact_usage(&self) -> bool {
        matches!(self.subscription_usage, CapabilitySupport::Supported)
            || matches!(self.api_billing_usage, CapabilitySupport::Supported)
    }
}

/// Severity of a share-safe provider diagnostic.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticSeverity {
    Info,
    Warning,
    Error,
}

/// A single share-safe diagnostic line. Holds only a short `label` and
/// `detail`; there is intentionally no field for secrets, tokens, account
/// identifiers, emails, organization IDs, or raw provider payloads.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderDiagnostic {
    pub severity: DiagnosticSeverity,
    pub label: String,
    pub detail: String,
}

impl ProviderDiagnostic {
    pub fn new(
        severity: DiagnosticSeverity,
        label: impl Into<String>,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            severity,
            label: label.into(),
            detail: detail.into(),
        }
    }

    pub fn info(label: impl Into<String>, detail: impl Into<String>) -> Self {
        Self::new(DiagnosticSeverity::Info, label, detail)
    }

    pub fn warning(label: impl Into<String>, detail: impl Into<String>) -> Self {
        Self::new(DiagnosticSeverity::Warning, label, detail)
    }

    pub fn error(label: impl Into<String>, detail: impl Into<String>) -> Self {
        Self::new(DiagnosticSeverity::Error, label, detail)
    }
}

/// The full typed status for one provider. This is the core API later tasks
/// consume; renderers and routing read these fields rather than re-deriving
/// status from app state.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProviderRunwayStatus {
    pub provider_id: ProviderId,
    pub display_name: String,
    pub selected_model: Option<String>,
    pub state: ProviderRunwayState,
    pub reason: ProviderStatusReason,
    pub next_action: ProviderNextAction,
    pub usage: UsageAvailability,
    pub reset: ResetAvailability,
    pub freshness: StatusFreshness,
    pub source: StatusSource,
    pub diagnostics: Vec<ProviderDiagnostic>,
}

// ---------------------------------------------------------------------------
// Provider status service + adapter boundary
// ---------------------------------------------------------------------------

/// Default per-provider check timeout. Bounded so one slow provider cannot
/// block the whole command and the PRD "median under 15s" metric holds even
/// with several providers checked concurrently.
pub const DEFAULT_PROVIDER_STATUS_TIMEOUT: Duration = Duration::from_secs(10);

/// The raw result a provider adapter returns from a lightweight status check,
/// before the service normalizes it into a [`ProviderRunwayStatus`] (stamps
/// freshness, enforces the capability gate, and redacts diagnostics).
#[derive(Clone, Debug, PartialEq)]
pub struct ProviderProbeResult {
    pub state: ProviderRunwayState,
    pub reason: ProviderStatusReason,
    pub next_action: ProviderNextAction,
    pub usage: UsageAvailability,
    pub reset: ResetAvailability,
    pub selected_model: Option<String>,
    pub source: StatusSource,
    pub diagnostics: Vec<ProviderDiagnostic>,
}

/// A provider adapter's status hook. Implementations live close to the runtime
/// adapters and expose capabilities *separately* from a status check so
/// unsupported data classes are never mistaken for failed checks.
#[async_trait]
pub trait ProviderStatusProbe: Send + Sync {
    fn provider_id(&self) -> ProviderId;
    fn display_name(&self) -> String;
    fn capabilities(&self) -> ProviderStatusCapabilities;
    async fn probe(&self) -> ProviderProbeResult;
}

/// Declared status capabilities for a provider family.
///
/// Centralized here, mirroring how [`check_runtime_availability`] centralizes
/// per-adapter readiness dispatch in the runtime layer. V1 exposes no exact
/// usage for any provider, so `subscription_usage`/`api_billing_usage` are
/// `Unsupported` everywhere and [`UsageAvailability::exact_if_supported`]
/// always degrades to a truthful unsupported state.
pub fn provider_capabilities(provider_id: ProviderId) -> ProviderStatusCapabilities {
    let mut caps = ProviderStatusCapabilities::none();
    match provider_id {
        ProviderId::Claude | ProviderId::Codex | ProviderId::Cursor | ProviderId::Fake => {
            caps.local_runtime_health = CapabilitySupport::Supported;
        }
        ProviderId::HttpApi => {
            // HTTP API readiness is gated on a configured API key env var, so its
            // auth state is checkable even though usage is not exposed.
            caps.auth_state = CapabilitySupport::RequiresConfiguration;
        }
        ProviderId::Local => {
            caps.local_runtime_health = CapabilitySupport::Supported;
        }
    }
    caps
}

/// Official provider-native account/usage page to point users at when exact
/// usage is unavailable. Best-effort V1 URLs; safe to surface (no secrets) and
/// easy to refine as provider account surfaces are confirmed.
pub fn provider_usage_url(provider_id: ProviderId) -> Option<String> {
    let url = match provider_id {
        ProviderId::Claude => "https://claude.ai/settings/usage",
        ProviderId::Codex => "https://platform.openai.com/usage",
        ProviderId::Cursor => "https://www.cursor.com/settings",
        ProviderId::HttpApi => "https://z.ai",
        ProviderId::Fake | ProviderId::Local => return None,
    };
    Some(url.to_string())
}

/// Map an existing [`RuntimeAvailability`] readiness result (produced by the
/// per-adapter `check_availability`) into a typed [`ProviderProbeResult`].
///
/// This is the outcome→runway-state mapping: it never invents remaining quota
/// and always represents unsupported exact usage as a first-class state.
pub fn map_runtime_availability(
    provider_id: ProviderId,
    selected_model: Option<String>,
    availability: &RuntimeAvailability,
) -> ProviderProbeResult {
    let url = provider_usage_url(provider_id);
    match availability.status {
        RuntimeAvailabilityStatus::Available => ProviderProbeResult {
            state: ProviderRunwayState::Ready,
            reason: ProviderStatusReason::new(
                ReasonCode::UsageUnsupported,
                "ready based on auth/config/runtime checks; exact remaining usage is unavailable \
                 from this provider integration",
            ),
            next_action: ProviderNextAction::CheckProviderUsage {
                provider_url: url.clone(),
            },
            usage: UsageAvailability::Unsupported { provider_url: url },
            reset: ResetAvailability::Unknown,
            selected_model,
            source: StatusSource::LocalRuntimeCheck,
            diagnostics: availability_diagnostics(availability),
        },
        RuntimeAvailabilityStatus::Unknown => ProviderProbeResult {
            state: ProviderRunwayState::UnavailableUsage,
            reason: ProviderStatusReason::new(
                ReasonCode::UsageUnsupported,
                "auth/config look ok; exact remaining usage is unsupported by this provider \
                 integration",
            ),
            next_action: ProviderNextAction::CheckProviderUsage {
                provider_url: url.clone(),
            },
            usage: UsageAvailability::Unsupported { provider_url: url },
            reset: ResetAvailability::Unknown,
            selected_model,
            source: StatusSource::LocalRuntimeCheck,
            diagnostics: availability_diagnostics(availability),
        },
        RuntimeAvailabilityStatus::Unavailable => {
            let (state, code, next_action) = classify_unavailable(availability);
            ProviderProbeResult {
                state,
                reason: ProviderStatusReason::new(code, availability.message.clone()),
                next_action,
                usage: UsageAvailability::Unavailable {
                    reason: "provider is unavailable for this session".to_string(),
                    provider_url: url,
                },
                reset: ResetAvailability::NotApplicable,
                selected_model,
                source: StatusSource::LocalRuntimeCheck,
                diagnostics: availability_diagnostics(availability),
            }
        }
    }
}

/// Classify an `Unavailable` readiness result into a typed runway state using
/// the adapter's own message/remediation text (the only signal the existing
/// availability contract carries). Missing/invalid configuration maps to
/// `Misconfigured`; missing credentials map to `Unauthenticated`; anything
/// else is a `ProviderError`.
fn classify_unavailable(
    availability: &RuntimeAvailability,
) -> (ProviderRunwayState, ReasonCode, ProviderNextAction) {
    let text = format!(
        "{} {}",
        availability.message,
        availability.remediation.as_deref().unwrap_or("")
    )
    .to_lowercase();
    let mentions = |needles: &[&str]| needles.iter().any(|needle| text.contains(needle));

    // Check auth/credential signals before configuration signals: an auth error can
    // also contain a generic config word like "not found" (e.g. "api key not found"),
    // which must still classify as Unauthenticated rather than Misconfigured.
    if mentions(&[
        "not set",
        "api key",
        "login",
        "auth",
        "token",
        "credential",
        "export",
    ]) {
        (
            ProviderRunwayState::Unauthenticated,
            ReasonCode::AuthRequired,
            ProviderNextAction::Authenticate,
        )
    } else if mentions(&[
        "not configured",
        "not found",
        "install",
        "path",
        "base_url",
        "command",
    ]) {
        (
            ProviderRunwayState::Misconfigured,
            ReasonCode::Misconfigured,
            ProviderNextAction::FixConfiguration,
        )
    } else {
        (
            ProviderRunwayState::ProviderError,
            ReasonCode::ProviderError,
            ProviderNextAction::RetryLater,
        )
    }
}

/// Build share-safe diagnostics from a readiness result. The message and
/// remediation are run through redaction at the service boundary, so this just
/// captures them at the appropriate severity.
fn availability_diagnostics(availability: &RuntimeAvailability) -> Vec<ProviderDiagnostic> {
    let mut diagnostics = Vec::new();
    if !availability.message.trim().is_empty() {
        let severity = match availability.status {
            RuntimeAvailabilityStatus::Unavailable => DiagnosticSeverity::Error,
            RuntimeAvailabilityStatus::Unknown | RuntimeAvailabilityStatus::Available => {
                DiagnosticSeverity::Info
            }
        };
        diagnostics.push(ProviderDiagnostic::new(
            severity,
            "runtime",
            availability.message.clone(),
        ));
    }
    if let Some(remediation) = &availability.remediation {
        diagnostics.push(ProviderDiagnostic::info("remediation", remediation.clone()));
    }
    diagnostics
}

/// Real provider adapter probe: reuses the existing per-adapter
/// `check_availability` dispatch and maps its result into the typed model.
pub struct RuntimeAvailabilityProbe {
    provider_id: ProviderId,
    display_name: String,
    config: RuntimeConfig,
    selected_model: Option<String>,
}

impl RuntimeAvailabilityProbe {
    pub fn new(config: RuntimeConfig) -> Self {
        let provider_id = ProviderId::from(config.kind.clone());
        Self {
            provider_id,
            display_name: provider_id.default_display_name().to_string(),
            config,
            selected_model: None,
        }
    }
}

#[async_trait]
impl ProviderStatusProbe for RuntimeAvailabilityProbe {
    fn provider_id(&self) -> ProviderId {
        self.provider_id
    }

    fn display_name(&self) -> String {
        self.display_name.clone()
    }

    fn capabilities(&self) -> ProviderStatusCapabilities {
        provider_capabilities(self.provider_id)
    }

    async fn probe(&self) -> ProviderProbeResult {
        let availability = check_runtime_availability(&self.config).await;
        map_runtime_availability(self.provider_id, self.selected_model.clone(), &availability)
    }
}

/// Deterministic probe that returns a fixed result, optionally after a delay.
/// Used for fake-adapter routing/rendering tests and timeout exercises.
pub struct StaticStatusProbe {
    provider_id: ProviderId,
    display_name: String,
    capabilities: ProviderStatusCapabilities,
    result: ProviderProbeResult,
    delay: Option<Duration>,
}

impl StaticStatusProbe {
    pub fn new(
        provider_id: ProviderId,
        display_name: impl Into<String>,
        capabilities: ProviderStatusCapabilities,
        result: ProviderProbeResult,
    ) -> Self {
        Self {
            provider_id,
            display_name: display_name.into(),
            capabilities,
            result,
            delay: None,
        }
    }

    /// Make the probe sleep before returning, to exercise per-provider
    /// timeout handling deterministically.
    pub fn with_delay(mut self, delay: Duration) -> Self {
        self.delay = Some(delay);
        self
    }
}

#[async_trait]
impl ProviderStatusProbe for StaticStatusProbe {
    fn provider_id(&self) -> ProviderId {
        self.provider_id
    }

    fn display_name(&self) -> String {
        self.display_name.clone()
    }

    fn capabilities(&self) -> ProviderStatusCapabilities {
        self.capabilities
    }

    async fn probe(&self) -> ProviderProbeResult {
        if let Some(delay) = self.delay {
            sleep(delay).await;
        }
        self.result.clone()
    }
}

/// Discovers configured providers and collects one typed runway status per
/// provider, concurrently and behind bounded per-provider timeouts.
pub struct ProviderStatusService {
    probes: Vec<Arc<dyn ProviderStatusProbe>>,
    per_provider_timeout: Duration,
}

impl ProviderStatusService {
    /// Build a service from an explicit set of probes (the injection point for
    /// deterministic tests and later command routing).
    pub fn new(probes: Vec<Arc<dyn ProviderStatusProbe>>) -> Self {
        Self {
            probes,
            per_provider_timeout: DEFAULT_PROVIDER_STATUS_TIMEOUT,
        }
    }

    /// Override the per-provider timeout.
    pub fn with_timeout(mut self, per_provider_timeout: Duration) -> Self {
        self.per_provider_timeout = per_provider_timeout;
        self
    }

    /// Discover the *relevant* providers from config: the distinct provider
    /// families actually referenced by enabled agents, one probe each, in
    /// stable agent-id order. This deliberately excludes runtimes that are
    /// merely defined (e.g. built-in defaults the user never references) so the
    /// command reports only the providers a session can actually use and does
    /// not pay to probe unused CLIs.
    pub fn from_config(config: &EffectiveConfig) -> Self {
        let mut seen: HashSet<ProviderId> = HashSet::new();
        let mut probes: Vec<Arc<dyn ProviderStatusProbe>> = Vec::new();
        for agent in config.agents.values() {
            if !agent.enabled {
                continue;
            }
            let Some(runtime) = config.runtimes.get(&agent.runtime) else {
                continue;
            };
            let provider_id = ProviderId::from(runtime.kind.clone());
            if seen.insert(provider_id) {
                probes.push(Arc::new(RuntimeAvailabilityProbe::new(runtime.clone())));
            }
        }
        Self::new(probes)
    }

    /// Number of providers this service will report on.
    pub fn provider_count(&self) -> usize {
        self.probes.len()
    }

    /// The provider families this service will report on, in row order.
    pub fn provider_ids(&self) -> Vec<ProviderId> {
        self.probes
            .iter()
            .map(|probe| probe.provider_id())
            .collect()
    }

    /// Collect one [`ProviderRunwayStatus`] per provider. Probes run
    /// concurrently; a probe that exceeds the timeout (or otherwise fails to
    /// complete) yields a `ProviderError` row for that provider only, so a
    /// single slow/failing provider never blocks the others.
    pub async fn collect_status(&self) -> Vec<ProviderRunwayStatus> {
        let observed_at = Utc::now();
        let mut results: Vec<Option<ProviderProbeResult>> =
            (0..self.probes.len()).map(|_| None).collect();

        let mut set = tokio::task::JoinSet::new();
        for (index, probe) in self.probes.iter().enumerate() {
            let probe = Arc::clone(probe);
            let per_provider_timeout = self.per_provider_timeout;
            set.spawn(async move {
                let outcome = timeout(per_provider_timeout, probe.probe()).await.ok();
                (index, outcome)
            });
        }
        while let Some(joined) = set.join_next().await {
            if let Ok((index, outcome)) = joined {
                results[index] = outcome;
            }
        }

        self.probes
            .iter()
            .enumerate()
            .map(|(index, probe)| match std::mem::take(&mut results[index]) {
                Some(result) => normalize_status(probe.as_ref(), result, observed_at),
                None => unresolved_status(probe.as_ref(), observed_at),
            })
            .collect()
    }
}

/// Normalize a probe result into the typed status: enforce the capability gate
/// on exact usage, stamp live freshness, and redact diagnostics + reason text.
fn normalize_status(
    probe: &dyn ProviderStatusProbe,
    result: ProviderProbeResult,
    observed_at: DateTime<Utc>,
) -> ProviderRunwayStatus {
    let provider_id = probe.provider_id();
    let capabilities = probe.capabilities();

    let mut state = result.state;
    let mut usage = result.usage;
    let mut reason = result.reason;
    // Truthfulness guard at the service boundary: exact usage cannot survive
    // without a supporting capability, regardless of what the adapter returned.
    if usage.is_exact() && !capabilities.supports_exact_usage() {
        usage = UsageAvailability::Unsupported {
            provider_url: provider_usage_url(provider_id),
        };
        if matches!(state, ProviderRunwayState::Ready) {
            state = ProviderRunwayState::UnavailableUsage;
        }
        reason = ProviderStatusReason::new(
            ReasonCode::UsageUnsupported,
            "exact remaining usage is unavailable from this provider integration",
        );
    }

    let reason = ProviderStatusReason {
        code: reason.code,
        summary: redact_secrets(&reason.summary),
    };
    let diagnostics = result
        .diagnostics
        .into_iter()
        .map(redact_diagnostic)
        .collect();

    ProviderRunwayStatus {
        provider_id,
        display_name: probe.display_name(),
        selected_model: result.selected_model,
        state,
        reason,
        next_action: result.next_action,
        usage,
        reset: result.reset,
        freshness: StatusFreshness::Live { observed_at },
        source: result.source,
        diagnostics,
    }
}

/// Status row for a provider whose check did not complete (timed out or
/// panicked). Reported as a `ProviderError` for that provider only.
fn unresolved_status(
    probe: &dyn ProviderStatusProbe,
    observed_at: DateTime<Utc>,
) -> ProviderRunwayStatus {
    let provider_id = probe.provider_id();
    ProviderRunwayStatus {
        provider_id,
        display_name: probe.display_name(),
        selected_model: None,
        state: ProviderRunwayState::ProviderError,
        reason: ProviderStatusReason::new(
            ReasonCode::ProviderError,
            "provider status check did not complete in time",
        ),
        next_action: ProviderNextAction::RetryLater,
        usage: UsageAvailability::Unavailable {
            reason: "status check did not complete".to_string(),
            provider_url: provider_usage_url(provider_id),
        },
        reset: ResetAvailability::Unknown,
        freshness: StatusFreshness::Live { observed_at },
        source: StatusSource::Unknown,
        diagnostics: Vec::new(),
    }
}

/// Redact a diagnostic's label and detail before it leaves the runtime layer.
fn redact_diagnostic(diagnostic: ProviderDiagnostic) -> ProviderDiagnostic {
    ProviderDiagnostic {
        severity: diagnostic.severity,
        label: redact_secrets(&diagnostic.label),
        detail: redact_secrets(&diagnostic.detail),
    }
}

/// Replace secret-like substrings (emails, API keys/tokens, org/account ids,
/// raw high-entropy payload tokens) with placeholders so default output is
/// safe to paste into logs or support requests. Conservative and token-based:
/// it preserves ordinary words and official URLs.
pub fn redact_secrets(text: &str) -> String {
    text.split_whitespace()
        .map(redact_token)
        .collect::<Vec<_>>()
        .join(" ")
}

fn redact_token(token: &str) -> String {
    if looks_like_email(token) {
        return "[redacted-email]".to_string();
    }
    // A sensitive key (e.g. `token=…`, `authorization:…`) means the whole value
    // is secret, even when the value itself looks innocuous.
    for separator in ['=', ':'] {
        if let Some((key, value)) = token.split_once(separator) {
            if !value.is_empty() && key_is_sensitive(key) {
                return format!("{key}{separator}[redacted]");
            }
        }
    }
    // Otherwise redact any secret-like or email-like sub-segment, splitting on
    // structural delimiters so a secret embedded in a URL/query string
    // (e.g. `https://h/p?id=x&token=sk-live…`) cannot slip through the
    // whitespace-only tokenizing in `redact_secrets`.
    redact_segments(token)
}

/// Split a single whitespace token on structural delimiters (query/URL/markup
/// separators) and redact each secret-like or email-like sub-segment,
/// reassembling with the original delimiters preserved.
fn redact_segments(token: &str) -> String {
    let mut out = String::with_capacity(token.len());
    let mut segment_start = 0;
    // Track whether the previous segment was a sensitive key (e.g. `token=`,
    // `authorization:`) so its value is redacted even when the value looks
    // innocuous and follows the first key/value pair in a compound token (e.g.
    // `client_id=ok&token=hunter2`). `redact_token` only inspects the first split.
    let mut prev_key_sensitive = false;
    for (index, ch) in token.char_indices() {
        if is_segment_delimiter(ch) {
            let segment = &token[segment_start..index];
            if prev_key_sensitive && !segment.is_empty() {
                out.push_str("[redacted]");
            } else {
                out.push_str(&redact_segment(segment));
            }
            out.push(ch);
            prev_key_sensitive = matches!(ch, '=' | ':') && key_is_sensitive(segment);
            segment_start = index + ch.len_utf8();
        }
    }
    let last = &token[segment_start..];
    if prev_key_sensitive && !last.is_empty() {
        out.push_str("[redacted]");
    } else {
        out.push_str(&redact_segment(last));
    }
    out
}

fn redact_segment(segment: &str) -> String {
    if looks_like_email(segment) {
        "[redacted-email]".to_string()
    } else if looks_like_secret(segment) {
        "[redacted]".to_string()
    } else {
        segment.to_string()
    }
}

/// Characters that separate logically distinct values inside one token. These
/// are deliberately NOT part of the secret alphabet (`looks_like_secret`), so
/// splitting on them isolates an embedded secret without fragmenting it.
fn is_segment_delimiter(ch: char) -> bool {
    matches!(
        ch,
        '=' | ':'
            | '&'
            | '?'
            | '#'
            | ';'
            | ','
            | '|'
            | '"'
            | '\''
            | '('
            | ')'
            | '['
            | ']'
            | '{'
            | '}'
            | '<'
            | '>'
    )
}

fn looks_like_email(token: &str) -> bool {
    match token.split_once('@') {
        Some((local, domain)) => {
            !local.is_empty() && domain.contains('.') && !domain.ends_with('.')
        }
        None => false,
    }
}

fn key_is_sensitive(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    [
        "key",
        "token",
        "secret",
        "password",
        "passwd",
        "authorization",
        "apikey",
        "api_key",
        "bearer",
        "credential",
    ]
    .iter()
    .any(|needle| key.contains(needle))
}

fn looks_like_secret(token: &str) -> bool {
    const SECRET_PREFIXES: [&str; 9] = [
        "sk-", "sk_", "pk-", "ghp_", "gho_", "xoxb-", "xoxp-", "zai-", "org-",
    ];
    let lowered = token.to_ascii_lowercase();
    if SECRET_PREFIXES
        .iter()
        .any(|prefix| lowered.starts_with(prefix))
    {
        return true;
    }
    // High-entropy-looking token: long, restricted alphabet, mixes letters and
    // digits. Catches raw API keys and opaque payload ids without flagging
    // normal prose or URLs (which lack the letter+digit mix in one token).
    if token.len() >= 20
        && token
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '+' | '/' | '='))
        && token.chars().any(|c| c.is_ascii_digit())
        && token.chars().any(|c| c.is_ascii_alphabetic())
    {
        return true;
    }
    false
}

// ---------------------------------------------------------------------------
// Compact share-safe rendering
// ---------------------------------------------------------------------------

/// Render a list of provider statuses into the compact, share-safe default
/// output: a `Provider status` header followed by exactly one column-aligned
/// row per provider (display name, status label, then a short reason + one
/// next action). This is the small API command routing calls; it needs no
/// provider-specific knowledge.
///
/// Share-safety is structural: diagnostics are omitted entirely from the
/// default view, exact usage is rendered only from `UsageAvailability::Exact`,
/// and the free-text fields (display name, reason) are defensively run through
/// [`redact_secrets`] so a status built outside the service still cannot leak.
pub fn render_provider_status(statuses: &[ProviderRunwayStatus]) -> String {
    if statuses.is_empty() {
        return "Provider status\n\nNo configured providers to report.".to_string();
    }

    let name_width = statuses
        .iter()
        .map(|status| redact_secrets(&status.display_name).chars().count())
        .max()
        .unwrap_or(0);
    let label_width = statuses
        .iter()
        .map(|status| state_label(status.state).chars().count())
        .max()
        .unwrap_or(0);

    let mut out = String::from("Provider status\n\n");
    for status in statuses {
        let name = redact_secrets(&status.display_name);
        let label = state_label(status.state);
        let detail = render_detail(status);
        out.push_str(&format!(
            "{name:name_width$}  {label:label_width$}  {detail}\n"
        ));
    }
    out.trim_end().to_string()
}

/// Stable user-facing label for each runway state.
pub fn state_label(state: ProviderRunwayState) -> &'static str {
    match state {
        ProviderRunwayState::Ready => "ready",
        ProviderRunwayState::LimitedRunway => "limited runway",
        ProviderRunwayState::Blocked => "blocked",
        ProviderRunwayState::UnavailableUsage => "unavailable usage",
        ProviderRunwayState::Unauthenticated => "unauthenticated",
        ProviderRunwayState::Misconfigured => "misconfigured",
        ProviderRunwayState::ProviderError => "provider error",
        ProviderRunwayState::LocalOnlyStatus => "local-only status",
    }
}

/// The detail column for one row: a capitalized reason sentence, the exact
/// usage figures when (and only when) the typed usage is `Exact`, then one
/// next-action sentence.
fn render_detail(status: &ProviderRunwayStatus) -> String {
    let mut segments: Vec<String> = vec![ensure_trailing_period(&capitalize_first(
        &redact_secrets(&status.reason.summary),
    ))];

    if let UsageAvailability::Exact(exact) = &status.usage {
        segments.push(render_exact_usage(exact, &status.reset));
    }

    segments.push(next_action_phrase(status));
    segments.join(" ")
}

/// Render documented exact usage. Only ever called for
/// `UsageAvailability::Exact`, so it never invents numbers.
fn render_exact_usage(exact: &ExactUsage, reset: &ResetAvailability) -> String {
    let remaining = format_amount(&exact.remaining);
    let metric = metric_label(&exact.metric);
    let mut sentence = match &exact.limit {
        Some(limit) => format!("{remaining} of {} {metric} remaining", format_amount(limit)),
        None => format!("{remaining} {metric} remaining"),
    };
    let reset = render_reset(exact, reset);
    if !reset.is_empty() {
        sentence.push_str("; ");
        sentence.push_str(&reset);
    }
    sentence.push('.');
    sentence
}

/// Reset timing for exact usage. Prefers the usage window's own `reset_at`,
/// falls back to the status-level reset, and says `unknown` rather than
/// inferring a time when the provider does not return one.
fn render_reset(exact: &ExactUsage, reset: &ResetAvailability) -> String {
    if let Some(reset_at) = exact.reset_at {
        return format!("resets {}", reset_at.format("%Y-%m-%d %H:%M UTC"));
    }
    match reset {
        ResetAvailability::Known { reset_at } => {
            format!("resets {}", reset_at.format("%Y-%m-%d %H:%M UTC"))
        }
        ResetAvailability::Unknown => "reset unknown".to_string(),
        ResetAvailability::NotApplicable => String::new(),
    }
}

/// Provider-native metric noun. `Other` is provider-supplied text, so it is
/// redacted defensively.
fn metric_label(metric: &UsageMetric) -> String {
    match metric {
        UsageMetric::Messages => "messages".to_string(),
        UsageMetric::Requests => "requests".to_string(),
        UsageMetric::Tokens => "tokens".to_string(),
        UsageMetric::Credits => "credits".to_string(),
        UsageMetric::Other(name) => redact_secrets(name),
    }
}

/// Format a usage amount. `f64`'s `Display` already renders whole numbers
/// without a trailing `.0` and fractional amounts compactly.
fn format_amount(amount: &UsageAmount) -> String {
    format!("{}", amount.value)
}

/// One actionable next-step sentence per typed next action. The provider URL is
/// intentionally not printed in the compact view (kept share-safe and short);
/// a detailed view can surface it later.
fn next_action_phrase(status: &ProviderRunwayStatus) -> String {
    match &status.next_action {
        ProviderNextAction::Proceed => "Safe to proceed.".to_string(),
        ProviderNextAction::CheckProviderUsage { .. } => {
            format!(
                "Check {} account usage.",
                redact_secrets(&status.display_name)
            )
        }
        ProviderNextAction::Authenticate => "Authenticate or re-link the account.".to_string(),
        ProviderNextAction::FixConfiguration => "Update provider configuration.".to_string(),
        ProviderNextAction::SwitchProvider => "Switch providers for this session.".to_string(),
        ProviderNextAction::ReduceScope => "Reduce session scope.".to_string(),
        ProviderNextAction::RetryLater => "Retry later.".to_string(),
        ProviderNextAction::VerifyRuntime => "Verify the local runtime is installed.".to_string(),
    }
}

fn capitalize_first(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

fn ensure_trailing_period(text: &str) -> String {
    let trimmed = text.trim_end();
    if trimmed.ends_with(['.', '!', '?']) {
        trimmed.to_string()
    } else {
        format!("{trimmed}.")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixed_time() -> DateTime<Utc> {
        DateTime::from_timestamp(1_700_000_000, 0).expect("valid fixed timestamp")
    }

    fn sample_exact_usage() -> ExactUsage {
        ExactUsage {
            metric: UsageMetric::Messages,
            remaining: UsageAmount::new(42.0),
            limit: Some(UsageAmount::new(100.0)),
            window: Some(UsageWindow::Rolling { hours: 5 }),
            reset_at: Some(fixed_time()),
            observed_at: fixed_time(),
        }
    }

    #[test]
    fn exact_usage_is_only_reachable_through_the_exact_variant_with_a_timestamp() {
        // The only way to carry exact usage is the Exact variant, and building
        // ExactUsage requires an observed_at timestamp (no Default exists).
        let usage = UsageAvailability::Exact(sample_exact_usage());
        assert!(usage.is_exact());
        match usage {
            UsageAvailability::Exact(exact) => {
                assert_eq!(exact.observed_at, fixed_time());
                assert_eq!(exact.metric, UsageMetric::Messages);
            }
            other => panic!("expected Exact, got {other:?}"),
        }
    }

    #[test]
    fn exact_if_supported_requires_supporting_capability_context() {
        let mut caps = ProviderStatusCapabilities::none();
        // No supporting capability -> degrades to Unsupported, never Exact.
        let degraded = UsageAvailability::exact_if_supported(
            &caps,
            sample_exact_usage(),
            Some("https://example.test/usage".to_string()),
        );
        assert!(!degraded.is_exact());
        assert_eq!(
            degraded,
            UsageAvailability::Unsupported {
                provider_url: Some("https://example.test/usage".to_string()),
            }
        );

        // Declaring subscription usage support unlocks Exact.
        caps.subscription_usage = CapabilitySupport::Supported;
        let exact = UsageAvailability::exact_if_supported(&caps, sample_exact_usage(), None);
        assert!(exact.is_exact());

        // API billing support is the alternative gate.
        let mut billing = ProviderStatusCapabilities::none();
        billing.api_billing_usage = CapabilitySupport::Supported;
        assert!(
            UsageAvailability::exact_if_supported(&billing, sample_exact_usage(), None).is_exact()
        );
    }

    #[test]
    fn unsupported_unavailable_and_not_applicable_usage_are_distinct() {
        let unsupported = UsageAvailability::Unsupported { provider_url: None };
        let unavailable = UsageAvailability::Unavailable {
            reason: "timeout".to_string(),
            provider_url: None,
        };
        let not_applicable = UsageAvailability::NotApplicable;

        assert_ne!(unsupported, unavailable);
        assert_ne!(unsupported, not_applicable);
        assert_ne!(unavailable, not_applicable);
        // None of the non-exact states report as exact.
        for state in [&unsupported, &unavailable, &not_applicable] {
            assert!(!state.is_exact());
        }
    }

    #[test]
    fn every_required_runway_state_can_be_constructed_and_matched() {
        let states = [
            ProviderRunwayState::Ready,
            ProviderRunwayState::LimitedRunway,
            ProviderRunwayState::Blocked,
            ProviderRunwayState::UnavailableUsage,
            ProviderRunwayState::Unauthenticated,
            ProviderRunwayState::Misconfigured,
            ProviderRunwayState::ProviderError,
            ProviderRunwayState::LocalOnlyStatus,
        ];
        assert_eq!(states.len(), 8);
        // Match each variant directly (no string parsing) to prove exhaustive
        // coverage of the fixed set.
        for state in states {
            let matched = match state {
                ProviderRunwayState::Ready
                | ProviderRunwayState::LimitedRunway
                | ProviderRunwayState::Blocked
                | ProviderRunwayState::UnavailableUsage
                | ProviderRunwayState::Unauthenticated
                | ProviderRunwayState::Misconfigured
                | ProviderRunwayState::ProviderError
                | ProviderRunwayState::LocalOnlyStatus => true,
            };
            assert!(matched);
        }
    }

    #[test]
    fn capability_support_covers_all_four_cases() {
        let cases = [
            CapabilitySupport::Supported,
            CapabilitySupport::Unsupported,
            CapabilitySupport::RequiresAccountLink,
            CapabilitySupport::RequiresConfiguration,
        ];
        // Distinct from one another.
        for (i, a) in cases.iter().enumerate() {
            for (j, b) in cases.iter().enumerate() {
                assert_eq!(i == j, a == b);
            }
        }
        // `none()` defaults every class to Unsupported and reports no exact
        // usage support.
        let none = ProviderStatusCapabilities::none();
        assert!(!none.supports_exact_usage());
        assert_eq!(none.subscription_usage, CapabilitySupport::Unsupported);
        assert_eq!(none.api_billing_usage, CapabilitySupport::Unsupported);
    }

    #[test]
    fn diagnostics_carry_only_share_safe_label_and_detail() {
        let diag = ProviderDiagnostic::warning(
            "auth",
            "credentials not found; configure the provider before use",
        );
        assert_eq!(diag.severity, DiagnosticSeverity::Warning);
        assert!(!diag.label.is_empty());
        assert!(!diag.detail.is_empty());
        // The type exposes only label/detail/severity; there is no field that
        // could carry a secret. A constructed diagnostic contains exactly the
        // share-safe text it was given.
        assert_eq!(diag.label, "auth");
        assert!(diag.detail.contains("configure the provider"));
    }

    #[test]
    fn provider_id_maps_from_runtime_kind_and_adds_local() {
        assert_eq!(ProviderId::from(RuntimeKind::Claude), ProviderId::Claude);
        assert_eq!(ProviderId::from(RuntimeKind::Codex), ProviderId::Codex);
        assert_eq!(ProviderId::from(RuntimeKind::Cursor), ProviderId::Cursor);
        assert_eq!(ProviderId::from(RuntimeKind::HttpApi), ProviderId::HttpApi);
        assert_eq!(ProviderId::from(RuntimeKind::Fake), ProviderId::Fake);
        assert_eq!(ProviderId::HttpApi.default_display_name(), "HTTP API");
        assert_eq!(ProviderId::Local.default_display_name(), "Local");
    }

    #[test]
    fn a_full_status_value_composes_the_typed_fields() {
        let status = ProviderRunwayStatus {
            provider_id: ProviderId::Local,
            display_name: ProviderId::Local.default_display_name().to_string(),
            selected_model: None,
            state: ProviderRunwayState::LocalOnlyStatus,
            reason: ProviderStatusReason::new(
                ReasonCode::LocalOnly,
                "runtime reachable; account usage does not apply",
            ),
            next_action: ProviderNextAction::Proceed,
            usage: UsageAvailability::NotApplicable,
            reset: ResetAvailability::NotApplicable,
            freshness: StatusFreshness::Live {
                observed_at: fixed_time(),
            },
            source: StatusSource::LocalRuntimeCheck,
            diagnostics: vec![ProviderDiagnostic::info("runtime", "binary on PATH")],
        };
        assert_eq!(status.state, ProviderRunwayState::LocalOnlyStatus);
        assert_eq!(status.usage, UsageAvailability::NotApplicable);
        assert_eq!(status.reason.code, ReasonCode::LocalOnly);
    }

    fn ready_probe_result() -> ProviderProbeResult {
        ProviderProbeResult {
            state: ProviderRunwayState::Ready,
            reason: ProviderStatusReason::new(
                ReasonCode::UsageUnsupported,
                "ready; exact usage unavailable",
            ),
            next_action: ProviderNextAction::CheckProviderUsage {
                provider_url: Some("https://example.test".to_string()),
            },
            usage: UsageAvailability::Unsupported {
                provider_url: Some("https://example.test".to_string()),
            },
            reset: ResetAvailability::Unknown,
            selected_model: Some("model-x".to_string()),
            source: StatusSource::LocalRuntimeCheck,
            diagnostics: Vec::new(),
        }
    }

    #[tokio::test]
    async fn ready_provider_yields_ready_without_exact_usage() {
        let probe = StaticStatusProbe::new(
            ProviderId::Claude,
            "Claude",
            provider_capabilities(ProviderId::Claude),
            ready_probe_result(),
        );
        let service = ProviderStatusService::new(vec![Arc::new(probe)]);
        let rows = service.collect_status().await;
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].state, ProviderRunwayState::Ready);
        assert!(!rows[0].usage.is_exact());
        assert!(matches!(rows[0].freshness, StatusFreshness::Live { .. }));
    }

    #[tokio::test]
    async fn exact_usage_without_capability_is_downgraded_to_unsupported() {
        // Adapter wrongly returns Exact; the service has no supporting
        // capability, so it must degrade to a truthful unsupported state and
        // never surface invented remaining quota.
        let mut result = ready_probe_result();
        result.usage = UsageAvailability::Exact(sample_exact_usage());
        let probe = StaticStatusProbe::new(
            ProviderId::Codex,
            "Codex",
            provider_capabilities(ProviderId::Codex),
            result,
        );
        let service = ProviderStatusService::new(vec![Arc::new(probe)]);
        let rows = service.collect_status().await;
        assert!(!rows[0].usage.is_exact());
        assert!(matches!(
            rows[0].usage,
            UsageAvailability::Unsupported { .. }
        ));
        assert_eq!(rows[0].state, ProviderRunwayState::UnavailableUsage);
        assert_eq!(rows[0].reason.code, ReasonCode::UsageUnsupported);
    }

    #[tokio::test]
    async fn exact_usage_with_supporting_capability_is_preserved() {
        let mut result = ready_probe_result();
        result.usage = UsageAvailability::Exact(sample_exact_usage());
        let mut caps = provider_capabilities(ProviderId::HttpApi);
        caps.subscription_usage = CapabilitySupport::Supported;
        let probe = StaticStatusProbe::new(ProviderId::HttpApi, "HTTP API", caps, result);
        let service = ProviderStatusService::new(vec![Arc::new(probe)]);
        let rows = service.collect_status().await;
        assert!(rows[0].usage.is_exact());
    }

    #[test]
    fn auth_failure_maps_to_unauthenticated_with_next_action() {
        let availability = RuntimeAvailability {
            runtime_id: "zai".to_string(),
            status: RuntimeAvailabilityStatus::Unavailable,
            message: "environment variable MULTIAGENT_TEST_ZAI_KEY is not set".to_string(),
            remediation: Some("Export MULTIAGENT_TEST_ZAI_KEY with a valid API key.".to_string()),
        };
        let result = map_runtime_availability(ProviderId::HttpApi, None, &availability);
        assert_eq!(result.state, ProviderRunwayState::Unauthenticated);
        assert_eq!(result.reason.code, ReasonCode::AuthRequired);
        assert_eq!(result.next_action, ProviderNextAction::Authenticate);
    }

    #[test]
    fn missing_config_maps_to_misconfigured() {
        let availability = RuntimeAvailability {
            runtime_id: "zai".to_string(),
            status: RuntimeAvailabilityStatus::Unavailable,
            message: "HTTP API api_key_env is not configured".to_string(),
            remediation: Some("Set [runtimes.http_api].api_key_env in atelier.toml.".to_string()),
        };
        let result = map_runtime_availability(ProviderId::HttpApi, None, &availability);
        assert_eq!(result.state, ProviderRunwayState::Misconfigured);
        assert_eq!(result.reason.code, ReasonCode::Misconfigured);
        assert_eq!(result.next_action, ProviderNextAction::FixConfiguration);
    }

    #[test]
    fn available_maps_to_ready_and_unknown_maps_to_unavailable_usage() {
        let available = RuntimeAvailability {
            runtime_id: "codex".to_string(),
            status: RuntimeAvailabilityStatus::Available,
            message: "codex CLI found".to_string(),
            remediation: None,
        };
        let ready = map_runtime_availability(ProviderId::Codex, None, &available);
        assert_eq!(ready.state, ProviderRunwayState::Ready);
        assert!(matches!(ready.usage, UsageAvailability::Unsupported { .. }));
        assert!(!ready.usage.is_exact());

        let unknown = RuntimeAvailability {
            runtime_id: "zai".to_string(),
            status: RuntimeAvailabilityStatus::Unknown,
            message: "key is set; network check deferred".to_string(),
            remediation: None,
        };
        let row = map_runtime_availability(ProviderId::HttpApi, None, &unknown);
        assert_eq!(row.state, ProviderRunwayState::UnavailableUsage);
        assert!(!row.usage.is_exact());
    }

    #[tokio::test]
    async fn slow_provider_times_out_without_blocking_others() {
        let fast = StaticStatusProbe::new(
            ProviderId::Codex,
            "Codex",
            provider_capabilities(ProviderId::Codex),
            ready_probe_result(),
        );
        let slow = StaticStatusProbe::new(
            ProviderId::Claude,
            "Claude",
            provider_capabilities(ProviderId::Claude),
            ready_probe_result(),
        )
        .with_delay(Duration::from_secs(30));
        let service = ProviderStatusService::new(vec![Arc::new(fast), Arc::new(slow)])
            .with_timeout(Duration::from_millis(50));
        let rows = service.collect_status().await;
        assert_eq!(rows.len(), 2);
        // Order is preserved and only the slow provider degrades.
        assert_eq!(rows[0].provider_id, ProviderId::Codex);
        assert_eq!(rows[0].state, ProviderRunwayState::Ready);
        assert_eq!(rows[1].provider_id, ProviderId::Claude);
        assert_eq!(rows[1].state, ProviderRunwayState::ProviderError);
    }

    #[tokio::test]
    async fn diagnostics_and_reasons_are_redacted_before_leaving_the_service() {
        let mut result = ready_probe_result();
        result.reason = ProviderStatusReason::new(
            ReasonCode::ProviderError,
            "login failed for alice@example.com using sk-ABCD1234efgh5678ijkl",
        );
        result.diagnostics = vec![
            ProviderDiagnostic::error("auth", "token=sk-SECRET1234567890abcd org-9988776655AABBCC"),
            ProviderDiagnostic::info(
                "account",
                "email bob@corp.example.org payload 0a1b2c3d4e5f60718293a4b5c6d7e8f9",
            ),
        ];
        let probe = StaticStatusProbe::new(
            ProviderId::Codex,
            "Codex",
            provider_capabilities(ProviderId::Codex),
            result,
        );
        let service = ProviderStatusService::new(vec![Arc::new(probe)]);
        let rows = service.collect_status().await;
        let row = &rows[0];
        let blob = format!("{} {:?}", row.reason.summary, row.diagnostics);
        for secret in [
            "alice@example.com",
            "sk-ABCD1234efgh5678ijkl",
            "sk-SECRET1234567890abcd",
            "org-9988776655AABBCC",
            "bob@corp.example.org",
            "0a1b2c3d4e5f60718293a4b5c6d7e8f9",
        ] {
            assert!(!blob.contains(secret), "secret leaked: {secret} in {blob}");
        }
        assert!(
            blob.contains("[redacted"),
            "expected redaction marker in {blob}"
        );
        // Ordinary words survive redaction.
        assert!(row.reason.summary.contains("login"));
    }

    #[test]
    fn redaction_preserves_official_urls_and_plain_text() {
        let text = "ready; check https://claude.ai/settings/usage before a long run";
        assert_eq!(redact_secrets(text), text);
    }

    #[test]
    fn redaction_catches_secrets_embedded_in_query_strings() {
        // A secret glued into a URL/query string is one whitespace token, so it
        // must be split on structural delimiters (`?`, `&`, `=`) and redacted —
        // it would otherwise slip past whitespace-only tokenizing.
        let text = "request failed: \
             https://api.example.com/v1?client_id=abc&secret=sk-ABCD1234abcd5678ijkl&ok=1";
        let out = redact_secrets(text);
        assert!(
            !out.contains("sk-ABCD1234abcd5678ijkl"),
            "embedded secret leaked: {out}"
        );
        assert!(out.contains("[redacted]"), "{out}");
        // The surrounding non-secret structure survives.
        assert!(out.contains("client_id=abc"), "{out}");
        assert!(out.contains("api.example.com"), "{out}");
    }

    #[test]
    fn redaction_catches_bearer_token_after_separator() {
        // `Authorization:Bearer <secret>` style — the secret is isolated by the
        // `:` segment split even though the key is not the sensitive part.
        let out = redact_secrets("Authorization:Bearer ghp_abcd1234EFGH5678ijkl done");
        assert!(!out.contains("ghp_abcd1234EFGH5678ijkl"), "{out}");
        assert!(out.contains("done"));
    }

    #[test]
    fn redaction_catches_innocuous_value_after_sensitive_key_in_compound_token() {
        // A sensitive key (`token=`) makes its value secret even when the value
        // looks innocuous AND follows a non-sensitive first pair in one glued token
        // (a query string). The first-split check in `redact_token` misses this, so
        // `redact_segments` must redact the value of any sensitive key segment.
        let out = redact_secrets("auth failed at https://h/cb?client_id=ok123&token=hunter2&ok=1");
        assert!(
            !out.contains("hunter2"),
            "sensitive-keyed value leaked: {out}"
        );
        assert!(out.contains("[redacted]"), "{out}");
        // Non-sensitive pairs and surrounding structure survive.
        assert!(out.contains("client_id=ok123"), "{out}");
        assert!(out.contains("ok=1"), "{out}");
    }

    fn status(
        provider_id: ProviderId,
        state: ProviderRunwayState,
        reason: &str,
        next_action: ProviderNextAction,
        usage: UsageAvailability,
    ) -> ProviderRunwayStatus {
        ProviderRunwayStatus {
            provider_id,
            display_name: provider_id.default_display_name().to_string(),
            selected_model: None,
            state,
            reason: ProviderStatusReason::new(ReasonCode::UsageUnsupported, reason),
            next_action,
            usage,
            reset: ResetAvailability::Unknown,
            freshness: StatusFreshness::Unknown,
            source: StatusSource::LocalRuntimeCheck,
            diagnostics: Vec::new(),
        }
    }

    #[test]
    fn ready_with_unsupported_usage_renders_ready_wording() {
        let row = status(
            ProviderId::Claude,
            ProviderRunwayState::Ready,
            "ready based on auth/config/runtime checks; exact remaining usage is unavailable",
            ProviderNextAction::CheckProviderUsage {
                provider_url: Some("https://claude.ai/settings/usage".to_string()),
            },
            UsageAvailability::Unsupported {
                provider_url: Some("https://claude.ai/settings/usage".to_string()),
            },
        );
        let out = render_provider_status(&[row]);
        assert!(out.contains("Claude"));
        assert!(out.contains("ready"));
        assert!(out.contains("exact remaining usage is unavailable"));
        assert!(out.contains("Check Claude account usage."));
        // No exact figures fabricated.
        assert!(!out.contains("remaining of"));
    }

    #[test]
    fn unavailable_usage_renders_unsupported_message_and_verification_action() {
        let row = status(
            ProviderId::Codex,
            ProviderRunwayState::UnavailableUsage,
            "auth/config look ok; exact remaining usage is unsupported by this provider integration",
            ProviderNextAction::CheckProviderUsage { provider_url: None },
            UsageAvailability::Unsupported { provider_url: None },
        );
        let out = render_provider_status(&[row]);
        assert!(out.contains("unavailable usage"));
        assert!(out.contains("unsupported"));
        assert!(out.contains("Check Codex account usage."));
    }

    #[test]
    fn exact_usage_renders_only_when_usage_is_exact() {
        let exact_row = status(
            ProviderId::HttpApi,
            ProviderRunwayState::Ready,
            "ready",
            ProviderNextAction::Proceed,
            UsageAvailability::Exact(ExactUsage {
                metric: UsageMetric::Messages,
                remaining: UsageAmount::new(42.0),
                limit: Some(UsageAmount::new(100.0)),
                window: None,
                reset_at: Some(fixed_time()),
                observed_at: fixed_time(),
            }),
        );
        let exact_out = render_provider_status(&[exact_row]);
        assert!(exact_out.contains("42 of 100 messages remaining"));
        assert!(exact_out.contains("resets 2023-11-14"));

        let unsupported_row = status(
            ProviderId::HttpApi,
            ProviderRunwayState::UnavailableUsage,
            "exact remaining usage is unavailable",
            ProviderNextAction::CheckProviderUsage { provider_url: None },
            UsageAvailability::Unsupported { provider_url: None },
        );
        let unsupported_out = render_provider_status(&[unsupported_row]);
        // No exact-usage figure is rendered (the word "remaining" may still
        // appear inside the truthful "exact remaining usage is unavailable").
        assert!(!unsupported_out.contains("messages remaining"));
        assert!(!unsupported_out.contains(" of "));
    }

    #[test]
    fn local_only_renders_without_account_usage_wording() {
        let row = status(
            ProviderId::Local,
            ProviderRunwayState::LocalOnlyStatus,
            "runtime reachable; account readiness does not require a provider account",
            ProviderNextAction::Proceed,
            UsageAvailability::NotApplicable,
        );
        let out = render_provider_status(&[row]);
        assert!(out.contains("local-only status"));
        assert!(out.contains("Safe to proceed."));
        // Local readiness is never framed as account usage to check.
        assert!(!out.contains("account usage"));
    }

    #[test]
    fn diagnostics_with_sensitive_strings_are_omitted_from_default_output() {
        let mut row = status(
            ProviderId::Codex,
            ProviderRunwayState::ProviderError,
            "a live check failed",
            ProviderNextAction::RetryLater,
            UsageAvailability::Unavailable {
                reason: "transient failure".to_string(),
                provider_url: None,
            },
        );
        row.diagnostics = vec![
            ProviderDiagnostic::error("auth", "token sk-LEAK1234567890abcdef"),
            ProviderDiagnostic::info("account", "user carol@example.com org-AABBCCDDEEFF"),
        ];
        let out = render_provider_status(&[row]);
        for sensitive in [
            "sk-LEAK1234567890abcdef",
            "carol@example.com",
            "org-AABBCCDDEEFF",
        ] {
            assert!(!out.contains(sensitive), "leaked {sensitive} in: {out}");
        }
    }

    #[test]
    fn unknown_reset_renders_as_unknown_not_inferred_text() {
        let row = status(
            ProviderId::HttpApi,
            ProviderRunwayState::Ready,
            "ready",
            ProviderNextAction::Proceed,
            UsageAvailability::Exact(ExactUsage {
                metric: UsageMetric::Credits,
                remaining: UsageAmount::new(12.5),
                limit: None,
                window: None,
                reset_at: None,
                observed_at: fixed_time(),
            }),
        );
        let out = render_provider_status(&[row]);
        assert!(out.contains("12.5 credits remaining"));
        assert!(out.contains("reset unknown"));
    }

    #[test]
    fn empty_status_list_renders_a_friendly_placeholder() {
        let out = render_provider_status(&[]);
        assert!(out.contains("Provider status"));
        assert!(out.contains("No configured providers"));
    }

    const ALL_RUNWAY_STATES: [ProviderRunwayState; 8] = [
        ProviderRunwayState::Ready,
        ProviderRunwayState::LimitedRunway,
        ProviderRunwayState::Blocked,
        ProviderRunwayState::UnavailableUsage,
        ProviderRunwayState::Unauthenticated,
        ProviderRunwayState::Misconfigured,
        ProviderRunwayState::ProviderError,
        ProviderRunwayState::LocalOnlyStatus,
    ];

    #[tokio::test]
    async fn every_runway_state_survives_service_normalization() {
        // With non-exact usage, the capability gate never rewrites the state,
        // so each state must round-trip through the service unchanged.
        for state in ALL_RUNWAY_STATES {
            let mut result = ready_probe_result();
            result.state = state;
            result.usage = UsageAvailability::NotApplicable;
            let probe = StaticStatusProbe::new(
                ProviderId::Fake,
                "Fake",
                provider_capabilities(ProviderId::Fake),
                result,
            );
            let rows = ProviderStatusService::new(vec![Arc::new(probe)])
                .collect_status()
                .await;
            assert_eq!(rows[0].state, state, "state {state:?} must survive");
        }
    }

    #[tokio::test]
    async fn exact_usage_downgrade_rewrites_only_the_ready_state() {
        // A non-Ready state keeps its label even when its (uncapable) exact
        // usage is downgraded — only Ready becomes UnavailableUsage.
        let mut result = ready_probe_result();
        result.state = ProviderRunwayState::LimitedRunway;
        result.usage = UsageAvailability::Exact(sample_exact_usage());
        let probe = StaticStatusProbe::new(
            ProviderId::Codex,
            "Codex",
            provider_capabilities(ProviderId::Codex),
            result,
        );
        let rows = ProviderStatusService::new(vec![Arc::new(probe)])
            .collect_status()
            .await;
        assert!(!rows[0].usage.is_exact(), "uncapable exact must downgrade");
        assert_eq!(
            rows[0].state,
            ProviderRunwayState::LimitedRunway,
            "non-Ready state must be preserved"
        );
    }

    #[test]
    fn every_state_label_is_nonempty_and_distinct() {
        let labels: Vec<&str> = ALL_RUNWAY_STATES.iter().map(|s| state_label(*s)).collect();
        for label in &labels {
            assert!(!label.is_empty());
        }
        let mut deduped = labels.clone();
        deduped.sort_unstable();
        deduped.dedup();
        assert_eq!(deduped.len(), labels.len(), "labels must be distinct");
    }

    #[test]
    fn every_next_action_renders_a_distinct_nonempty_phrase() {
        let actions = [
            ProviderNextAction::Proceed,
            ProviderNextAction::CheckProviderUsage { provider_url: None },
            ProviderNextAction::Authenticate,
            ProviderNextAction::FixConfiguration,
            ProviderNextAction::SwitchProvider,
            ProviderNextAction::ReduceScope,
            ProviderNextAction::RetryLater,
            ProviderNextAction::VerifyRuntime,
        ];
        let mut phrases = Vec::new();
        for action in actions {
            let row = status(
                ProviderId::Claude,
                ProviderRunwayState::Ready,
                "reason",
                action,
                UsageAvailability::NotApplicable,
            );
            let phrase = next_action_phrase(&row);
            assert!(!phrase.trim().is_empty());
            assert!(phrase.ends_with('.'), "phrase must be a sentence: {phrase}");
            phrases.push(phrase);
        }
        let mut deduped = phrases.clone();
        deduped.sort();
        deduped.dedup();
        assert_eq!(
            deduped.len(),
            phrases.len(),
            "each next action must render a distinct phrase"
        );
        // The provider-usage action names the provider for the user.
        let row = status(
            ProviderId::Claude,
            ProviderRunwayState::Ready,
            "reason",
            ProviderNextAction::CheckProviderUsage { provider_url: None },
            UsageAvailability::NotApplicable,
        );
        assert!(next_action_phrase(&row).contains("Claude"));
    }
}
