//! Evidence-based repair planning without implicit host mutation.
//!
//! This module converts observed health into bounded, reviewable proposals.
//! It never launches commands, edits files, restarts services or marks a repair
//! successful. Applying a proposal requires a separate approved executor with
//! backups, postcondition checks and rollback evidence.

use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ComponentObservation {
    pub component_id: String,
    pub state: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RepairFinding {
    pub component_id: String,
    pub severity: String,
    pub condition: String,
    pub observed_state: String,
    pub evidence: String,
    pub recommended_action: String,
    pub requires_operator_approval: bool,
    pub auto_apply_eligible: bool,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RepairPlan {
    pub status: String,
    pub checked_components: usize,
    pub findings: Vec<RepairFinding>,
    pub summary: String,
    pub safety_boundary: String,
}

/// Build deterministic remediation proposals from current component observations.
/// Unsupported states remain visible findings rather than being guessed healthy.
pub fn build_repair_plan(observations: &[ComponentObservation]) -> RepairPlan {
    let mut findings = Vec::new();
    for observation in observations {
        let state = observation.state.trim().to_ascii_lowercase();
        let (severity, condition, action) = match state.as_str() {
            "online" | "available" | "ok" | "healthy" => continue,
            "offline" => (
                "high",
                "component_offline",
                "Inspect the configured endpoint, owning process, service logs and dependencies. Confirm GPU/model/media ownership before any restart.",
            ),
            "degraded" => (
                "medium",
                "component_degraded",
                "Inspect the reported error and recent component logs; compare the endpoint response with its documented health contract before changing configuration.",
            ),
            "redirected" => (
                "medium",
                "unexpected_redirect",
                "Verify the configured endpoint path, reverse-proxy rules and authentication behavior. A redirect is not classified as healthy.",
            ),
            "reachable" => (
                "low",
                "endpoint_reachable_but_unhealthy",
                "Verify the configured API path and authentication requirements; the host responded but the health request was not accepted.",
            ),
            "unknown" | "unavailable" | "not_configured" => (
                "low",
                "health_unverified",
                "Check whether the component is intentionally disabled or whether telemetry/health access is unavailable; do not infer a failure from missing evidence.",
            ),
            _ => (
                "low",
                "unrecognized_health_state",
                "Review the component adapter and its status mapping; preserve the raw status until the contract is understood.",
            ),
        };
        findings.push(RepairFinding {
            component_id: bound_text(&observation.component_id, 160),
            severity: severity.into(),
            condition: condition.into(),
            observed_state: bound_text(&state, 80),
            evidence: bound_text(&observation.detail, 2_048),
            recommended_action: action.into(),
            requires_operator_approval: true,
            auto_apply_eligible: false,
            status: "proposed_not_applied".into(),
        });
    }

    findings.sort_by(|left, right| {
        severity_rank(&left.severity)
            .cmp(&severity_rank(&right.severity))
            .then_with(|| left.component_id.cmp(&right.component_id))
    });
    let high = findings
        .iter()
        .filter(|finding| finding.severity == "high")
        .count();
    let medium = findings
        .iter()
        .filter(|finding| finding.severity == "medium")
        .count();
    let low = findings.len().saturating_sub(high + medium);
    RepairPlan {
        status: "diagnostic_plan_only".into(),
        checked_components: observations.len(),
        summary: format!(
            "{} finding(s): {high} high, {medium} medium, {low} low. No repair action was applied.",
            findings.len()
        ),
        findings,
        safety_boundary: "Read-only diagnosis and proposal generation only. No shell, systemd, filesystem mutation, service restart, model lifecycle operation, GPU lease, plugin execution or credential access. Applying a proposal requires a separately reviewed allowlisted executor, explicit approval, backup where applicable, verified postconditions and rollback.".into(),
    }
}

fn severity_rank(severity: &str) -> u8 {
    match severity {
        "high" => 0,
        "medium" => 1,
        _ => 2,
    }
}

fn bound_text(value: &str, max_chars: usize) -> String {
    let mut output = value.chars().take(max_chars).collect::<String>();
    if value.chars().count() > max_chars {
        output.push('…');
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn healthy_components_do_not_create_repair_findings() {
        let plan = build_repair_plan(&[
            ComponentObservation {
                component_id: "api".into(),
                state: "online".into(),
                detail: "HTTP 200".into(),
            },
            ComponentObservation {
                component_id: "comfyui".into(),
                state: "available".into(),
                detail: "queue and history available".into(),
            },
        ]);
        assert_eq!(plan.status, "diagnostic_plan_only");
        assert_eq!(plan.checked_components, 2);
        assert!(plan.findings.is_empty());
        assert!(plan.summary.contains("0 finding(s)"));
    }

    #[test]
    fn offline_findings_are_prioritized_and_never_auto_applied() {
        let plan = build_repair_plan(&[
            ComponentObservation {
                component_id: "comfyui".into(),
                state: "degraded".into(),
                detail: "history endpoint timed out".into(),
            },
            ComponentObservation {
                component_id: "legacy-hub".into(),
                state: "offline".into(),
                detail: "connection refused".into(),
            },
        ]);
        assert_eq!(plan.findings[0].component_id, "legacy-hub");
        assert_eq!(plan.findings[0].severity, "high");
        assert!(
            plan.findings
                .iter()
                .all(|finding| finding.requires_operator_approval)
        );
        assert!(
            plan.findings
                .iter()
                .all(|finding| !finding.auto_apply_eligible)
        );
        assert!(
            plan.findings
                .iter()
                .all(|finding| finding.status == "proposed_not_applied")
        );
    }

    #[test]
    fn unknown_state_and_evidence_are_preserved_with_bounds() {
        let long = "x".repeat(3_000);
        let plan = build_repair_plan(&[ComponentObservation {
            component_id: "provider".into(),
            state: "new_state".into(),
            detail: long,
        }]);
        assert_eq!(plan.findings.len(), 1);
        assert_eq!(plan.findings[0].condition, "unrecognized_health_state");
        assert!(plan.findings[0].evidence.chars().count() <= 2_049);
    }

    #[test]
    fn empty_evidence_is_retained_as_unverified() {
        let plan = build_repair_plan(&[ComponentObservation {
            component_id: "optional-agent".into(),
            state: "unknown".into(),
            detail: String::new(),
        }]);
        assert_eq!(plan.findings[0].severity, "low");
        assert_eq!(plan.findings[0].observed_state, "unknown");
    }
}
