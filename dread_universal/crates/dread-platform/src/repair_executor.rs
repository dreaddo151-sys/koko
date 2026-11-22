//! Self-repair EXECUTOR: state-machine-backed, crash-safe execution of the
//! proposals emitted by [`crate::repair::build_repair_plan`].
//!
//! Safety model (default-deny):
//! * Nothing executes before an explicit operator approval on the run.
//! * A finding is only `Pass`-eligible when it is `auto_apply_eligible`, it
//!   does not `requires_operator_approval`, and its recommended action is in
//!   the configured allowlist. The default allowlist is EMPTY, which makes
//!   every medium finding `Ask` and every high-severity finding `Deny`.
//! * This module never spawns processes and never performs I/O beyond SQLite.
//!   Real remediation commands live behind the [`RepairActionRunner`] trait,
//!   which the hub implements (in v1: a simulated runner).
//! * `Deny` decisions are recorded on the action and can never be overridden
//!   by approving the run.
//!
//! Concurrency note: every public database function opens its own
//! `BEGIN IMMEDIATE` transaction, so none of them may be called from inside
//! an outer transaction. Concurrent approvers serialize on SQLite's write
//! lock and the second one observes the post-approval state (idempotent).
//! Runner contract: `runner.apply` and `checker.observe` run while the
//! caller's database lock may be held (the v1 hub holds its global mutex
//! across an execute, a deliberate and documented tradeoff: repair runs are
//! operator-triggered, rare, and the simulated runner is instant). A real
//! runner MUST be bounded in time and must never block on an unbounded
//! external wait, or hub requests will queue behind it.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use rusqlite::types::{FromSql, FromSqlError, FromSqlResult, ValueRef};
use rusqlite::{Connection, OptionalExtension, Row, params};
use serde::Serialize;

use crate::repair::{ComponentObservation, RepairFinding, RepairPlan};

/// Hard cap on findings that can become actions in one run.
pub const MAX_ACTIONS_PER_RUN: usize = 256;
/// Default per-component cooldown enforced by [`next_eligible_action`].
pub const DEFAULT_COOLDOWN_SECS: i64 = 900;
const MAX_RUNS_LIST: i64 = 100;
const AUDIT_FETCH_LIMIT: i64 = 512;

/// Idempotent per-component cooldown deadline for `now` (saturating: a huge
/// clock can never wrap into a negative deadline).
#[must_use]
pub fn cooldown_deadline(now: i64) -> i64 {
    now.saturating_add(DEFAULT_COOLDOWN_SECS)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RepairRunStatus {
    Planned,
    AwaitingApproval,
    Approved,
    Applying,
    Verified,
    Failed,
    RolledBack,
    Rejected,
    Interrupted,
}

impl RepairRunStatus {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Planned => "planned",
            Self::AwaitingApproval => "awaiting_approval",
            Self::Approved => "approved",
            Self::Applying => "applying",
            Self::Verified => "verified",
            Self::Failed => "failed",
            Self::RolledBack => "rolled_back",
            Self::Rejected => "rejected",
            Self::Interrupted => "interrupted",
        }
    }

    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "planned" => Some(Self::Planned),
            "awaiting_approval" => Some(Self::AwaitingApproval),
            "approved" => Some(Self::Approved),
            "applying" => Some(Self::Applying),
            "verified" => Some(Self::Verified),
            "failed" => Some(Self::Failed),
            "rolled_back" => Some(Self::RolledBack),
            "rejected" => Some(Self::Rejected),
            "interrupted" => Some(Self::Interrupted),
            _ => None,
        }
    }

    /// Terminal runs are replay-safe: re-executing them returns the stored
    /// outcome instead of touching anything.
    #[must_use]
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Verified | Self::Failed | Self::RolledBack | Self::Rejected
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionStatus {
    Pending,
    Denied,
    Applying,
    Verified,
    Failed,
    Skipped,
    Interrupted,
}

impl ActionStatus {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Denied => "denied",
            Self::Applying => "applying",
            Self::Verified => "verified",
            Self::Failed => "failed",
            Self::Skipped => "skipped",
            Self::Interrupted => "interrupted",
        }
    }

    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "pending" => Some(Self::Pending),
            "denied" => Some(Self::Denied),
            "applying" => Some(Self::Applying),
            "verified" => Some(Self::Verified),
            "failed" => Some(Self::Failed),
            "skipped" => Some(Self::Skipped),
            "interrupted" => Some(Self::Interrupted),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    Pass,
    Ask,
    Deny,
}

impl Permission {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Ask => "ask",
            Self::Deny => "deny",
        }
    }

    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "pass" => Some(Self::Pass),
            "ask" => Some(Self::Ask),
            "deny" => Some(Self::Deny),
            _ => None,
        }
    }
}

fn status_transition_allowed(from: RepairRunStatus, to: RepairRunStatus) -> bool {
    use RepairRunStatus as S;
    matches!(
        (from, to),
        (S::Planned, S::AwaitingApproval)
            | (S::Planned, S::Rejected)
            | (S::AwaitingApproval, S::Approved)
            | (S::AwaitingApproval, S::Rejected)
            | (S::Approved, S::Applying)
            | (S::Applying, S::Verified)
            | (S::Applying, S::Failed)
            | (S::Applying, S::Interrupted)
            | (S::Failed, S::RolledBack)
            | (S::Verified, S::RolledBack)
    )
}

/// True only for transitions the executor state machine allows.
#[must_use]
pub fn can_transition(from: RepairRunStatus, to: RepairRunStatus) -> bool {
    status_transition_allowed(from, to)
}

macro_rules! impl_from_sql_status {
    ($ty:ty, $parse:ident, $label:literal) => {
        impl FromSql for $ty {
            fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
                let text = value.as_str()?;
                Self::$parse(text).ok_or_else(|| {
                    FromSqlError::Other(Box::new(InvalidDbValue {
                        expected: $label,
                        found: text.to_owned(),
                    }))
                })
            }
        }
    };
}

#[derive(Debug)]
struct InvalidDbValue {
    expected: &'static str,
    found: String,
}

impl fmt::Display for InvalidDbValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "stored value {:?} is not a valid {}",
            self.found, self.expected
        )
    }
}

impl std::error::Error for InvalidDbValue {}

impl_from_sql_status!(RepairRunStatus, parse, "repair run status");
impl_from_sql_status!(ActionStatus, parse, "repair action status");
impl_from_sql_status!(Permission, parse, "repair permission");

#[derive(Debug, Clone, Serialize)]
pub struct RepairRun {
    pub id: i64,
    pub status: RepairRunStatus,
    pub summary: String,
    pub safety_boundary: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub completed_at: Option<i64>,
    pub result: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RepairAction {
    pub id: i64,
    pub run_id: i64,
    pub position: i64,
    pub component_id: String,
    pub action_id: String,
    pub severity: String,
    pub decision: Permission,
    pub status: ActionStatus,
    pub before_state: String,
    pub after_state: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RunOutcome {
    pub run_id: i64,
    pub status: RepairRunStatus,
    pub verified_actions: i64,
    pub failed_actions: i64,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AuditRow {
    pub id: i64,
    pub ts: i64,
    pub run_id: i64,
    pub action_id: Option<i64>,
    pub component_id: String,
    pub action: String,
    pub decision: String,
    pub actor: String,
    pub before_state: String,
    pub after_state: String,
    pub result: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RunDetail {
    pub run: RepairRun,
    pub actions: Vec<RepairAction>,
    pub audit: Vec<AuditRow>,
    pub audit_truncated: bool,
}

#[derive(Debug, Clone, Default)]
pub struct PermissionPolicy {
    allowlist: BTreeSet<String>,
    denylist: BTreeSet<String>,
}

impl PermissionPolicy {
    /// Both lists are exact action-ID matches (no globs, never trimmed
    /// implicitly beyond what callers pass in).
    #[must_use]
    pub fn new(allowlist: BTreeSet<String>, denylist: BTreeSet<String>) -> Self {
        Self {
            allowlist,
            denylist,
        }
    }

    /// Decides per-action permission with a default-deny posture: denylist
    /// wins outright; high severity without an allowlist entry is Deny;
    /// Pass additionally requires `auto_apply_eligible` AND
    /// `!requires_operator_approval`; everything else is Ask. An empty
    /// allowlist therefore means all medium findings are Ask and all high
    /// findings are Deny.
    #[must_use]
    pub fn decide(&self, finding: &RepairFinding) -> Permission {
        let action = &finding.recommended_action;
        if self.denylist.contains(action) {
            return Permission::Deny;
        }
        // Severity is a free-form string crossing an untrusted boundary:
        // "High", "HIGH", "critical", or anything unrecognized is treated as
        // high (fail closed). Default-deny must not be evadable by casing.
        let severity = finding.severity.to_ascii_lowercase();
        let severity_is_high = !matches!(severity.as_str(), "low" | "info" | "informational" | "medium" | "med");
        if severity_is_high && !self.allowlist.contains(action) {
            return Permission::Deny;
        }
        if finding.auto_apply_eligible
            && !finding.requires_operator_approval
            && self.allowlist.contains(action)
        {
            return Permission::Pass;
        }
        Permission::Ask
    }
}

/// Injected by the hub; the module never runs commands itself.
pub trait RepairActionRunner {
    /// Returns a human-readable detail string on success; `Err(detail)`
    /// marks the action failed.
    fn apply(&self, action: &RepairAction) -> Result<String, String>;
}

/// Injected by the hub; must re-observe the component AFTER the action.
pub trait PostConditionChecker {
    fn observe(&self, action: &RepairAction) -> Result<ComponentObservation, String>;
}

fn is_healthy(state: &str) -> bool {
    matches!(state, "online" | "available" | "ok" | "healthy")
}

#[derive(Debug)]
pub enum RepairExecutorError {
    Database(rusqlite::Error),
    EmptyPlan,
    TooManyActions(usize),
    EmptyActor,
    EmptyActionId,
    InvalidTransition { from: String, to: String },
    RunNotFound(i64),
    CorruptData(String),
    /// A guarded write found the row changed underneath the executor:
    /// concurrent mutation outside the module's serialized path.
    Conflict(String),
}

impl fmt::Display for RepairExecutorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Database(error) => write!(f, "repair executor database error: {error}"),
            Self::EmptyPlan => write!(f, "repair plan contains no findings"),
            Self::TooManyActions(count) => write!(
                f,
                "plan has {count} findings, above the {MAX_ACTIONS_PER_RUN} action limit"
            ),
            Self::EmptyActor => write!(f, "actor must not be empty"),
            Self::EmptyActionId => write!(f, "finding has an empty recommended_action"),
            Self::InvalidTransition { from, to } => {
                write!(f, "illegal repair run transition {from} -> {to}")
            }
            Self::RunNotFound(id) => write!(f, "repair run {id} not found"),
            Self::CorruptData(detail) => write!(f, "corrupt repair state: {detail}"),
            Self::Conflict(detail) => write!(f, "concurrent repair mutation conflict: {detail}"),
        }
    }
}

impl std::error::Error for RepairExecutorError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Database(error) => Some(error),
            _ => None,
        }
    }
}

impl From<rusqlite::Error> for RepairExecutorError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Database(error)
    }
}

pub const REPAIR_EXECUTOR_SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS repair_runs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    status TEXT NOT NULL CHECK (status IN ('planned','awaiting_approval','approved','applying','verified','failed','rolled_back','rejected','interrupted')),
    summary TEXT NOT NULL DEFAULT '',
    safety_boundary TEXT NOT NULL DEFAULT '',
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    completed_at INTEGER,
    result TEXT NOT NULL DEFAULT ''
);
CREATE TABLE IF NOT EXISTS repair_actions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    run_id INTEGER NOT NULL REFERENCES repair_runs(id),
    position INTEGER NOT NULL,
    component_id TEXT NOT NULL,
    action_id TEXT NOT NULL,
    severity TEXT NOT NULL,
    decision TEXT NOT NULL CHECK (decision IN ('pass','ask','deny')),
    status TEXT NOT NULL CHECK (status IN ('pending','denied','applying','verified','failed','skipped','interrupted')),
    before_state TEXT NOT NULL DEFAULT '',
    after_state TEXT NOT NULL DEFAULT '',
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    detail TEXT NOT NULL DEFAULT ''
);
CREATE TABLE IF NOT EXISTS repair_audit (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    ts INTEGER NOT NULL,
    run_id INTEGER NOT NULL,
    action_id INTEGER,
    component_id TEXT NOT NULL DEFAULT '',
    action TEXT NOT NULL DEFAULT '',
    decision TEXT NOT NULL DEFAULT '',
    actor TEXT NOT NULL,
    before_state TEXT NOT NULL DEFAULT '',
    after_state TEXT NOT NULL DEFAULT '',
    result TEXT NOT NULL DEFAULT '',
    detail TEXT NOT NULL DEFAULT ''
);
CREATE INDEX IF NOT EXISTS idx_repair_actions_run ON repair_actions(run_id, position);
CREATE INDEX IF NOT EXISTS idx_repair_audit_run ON repair_audit(run_id, id);
";

/// BEGIN IMMEDIATE guard with rollback-on-drop. Do not nest: every public
/// function in this module opens its own transaction.
struct Tx<'a> {
    conn: &'a Connection,
    active: bool,
}

impl Tx<'_> {
    fn begin(conn: &Connection) -> Result<Tx<'_>, rusqlite::Error> {
        conn.execute_batch("BEGIN IMMEDIATE")?;
        Ok(Tx { conn, active: true })
    }

    fn commit(mut self) -> Result<(), rusqlite::Error> {
        self.conn.execute_batch("COMMIT")?;
        self.active = false;
        Ok(())
    }
}

impl Drop for Tx<'_> {
    fn drop(&mut self) {
        if self.active {
            // Best-effort: an already-broken connection cannot roll back.
            let _ = self.conn.execute_batch("ROLLBACK");
        }
    }
}

struct AuditEvent<'a> {
    ts: i64,
    run_id: i64,
    action_id: Option<i64>,
    component_id: &'a str,
    action: &'a str,
    decision: &'a str,
    actor: &'a str,
    before_state: &'a str,
    after_state: &'a str,
    result: &'a str,
    detail: &'a str,
}

fn insert_audit(conn: &Connection, event: &AuditEvent<'_>) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT INTO repair_audit (ts, run_id, action_id, component_id, action, decision, actor, before_state, after_state, result, detail) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
        params![
            event.ts,
            event.run_id,
            event.action_id,
            event.component_id,
            event.action,
            event.decision,
            event.actor,
            event.before_state,
            event.after_state,
            event.result,
            event.detail,
        ],
    )?;
    Ok(())
}

const RUN_COLUMNS: &str =
    "id, status, summary, safety_boundary, created_at, updated_at, completed_at, result";

fn row_to_run(row: &Row<'_>) -> Result<RepairRun, rusqlite::Error> {
    Ok(RepairRun {
        id: row.get(0)?,
        status: row.get(1)?,
        summary: row.get(2)?,
        safety_boundary: row.get(3)?,
        created_at: row.get(4)?,
        updated_at: row.get(5)?,
        completed_at: row.get(6)?,
        result: row.get(7)?,
    })
}

const ACTION_COLUMNS: &str = "id, run_id, position, component_id, action_id, severity, decision, status, before_state, after_state, created_at, updated_at, detail";

fn row_to_action(row: &Row<'_>) -> Result<RepairAction, rusqlite::Error> {
    Ok(RepairAction {
        id: row.get(0)?,
        run_id: row.get(1)?,
        position: row.get(2)?,
        component_id: row.get(3)?,
        action_id: row.get(4)?,
        severity: row.get(5)?,
        decision: row.get(6)?,
        status: row.get(7)?,
        before_state: row.get(8)?,
        after_state: row.get(9)?,
        created_at: row.get(10)?,
        updated_at: row.get(11)?,
        detail: row.get(12)?,
    })
}

fn load_run(conn: &Connection, run_id: i64) -> Result<Option<RepairRun>, rusqlite::Error> {
    conn.query_row(
        &format!("SELECT {RUN_COLUMNS} FROM repair_runs WHERE id = ?1"),
        params![run_id],
        row_to_run,
    )
    .optional()
}

/// Creates the executor tables. Idempotent; call at startup before routes.
/// Failure mode: SQLite errors only.
pub fn init_repair_executor(conn: &Connection) -> Result<(), RepairExecutorError> {
    conn.execute_batch(REPAIR_EXECUTOR_SCHEMA)?;
    Ok(())
}

/// Turns a planner report into a persisted run of per-action permission
/// decisions. Never executes anything. Failure modes: empty plan, oversized
/// plan, empty actor, empty `recommended_action`, SQLite errors.
pub fn create_run_from_plan(
    conn: &Connection,
    plan: &RepairPlan,
    policy: &PermissionPolicy,
    clock_now: i64,
    actor: &str,
) -> Result<RepairRun, RepairExecutorError> {
    if actor.trim().is_empty() {
        return Err(RepairExecutorError::EmptyActor);
    }
    if plan.findings.is_empty() {
        return Err(RepairExecutorError::EmptyPlan);
    }
    if plan.findings.len() > MAX_ACTIONS_PER_RUN {
        return Err(RepairExecutorError::TooManyActions(plan.findings.len()));
    }
    for finding in &plan.findings {
        if finding.recommended_action.trim().is_empty() {
            return Err(RepairExecutorError::EmptyActionId);
        }
    }

    let tx = Tx::begin(conn)?;
    conn.execute(
        "INSERT INTO repair_runs (status, summary, safety_boundary, created_at, updated_at) VALUES ('planned', ?1, ?2, ?3, ?3)",
        params![plan.summary, plan.safety_boundary, clock_now],
    )?;
    let run_id = conn.last_insert_rowid();
    let created_result = format!("{} findings", plan.findings.len());
    insert_audit(
        conn,
        &AuditEvent {
            ts: clock_now,
            run_id,
            action_id: None,
            component_id: "",
            action: "run_created",
            decision: "",
            actor,
            before_state: "",
            after_state: RepairRunStatus::Planned.as_str(),
            result: &created_result,
            detail: &plan.summary,
        },
    )?;

    let mut executable = 0usize;
    let mut seen_components = std::collections::BTreeSet::new();
    for (index, finding) in plan.findings.iter().enumerate() {
        let decision = policy.decide(finding);
        let position = i64::try_from(index)
            .map_err(|_| RepairExecutorError::TooManyActions(plan.findings.len()))?;
        // At most one executable action per component per run: two findings
        // for the same component must not stack into back-to-back restarts
        // (that is what the cooldown exists to prevent, and a single run
        // would sail straight past it).
        let duplicate_component = decision != Permission::Deny
            && !seen_components.insert(finding.component_id.clone());
        let status = if decision == Permission::Deny {
            "denied"
        } else if duplicate_component {
            "skipped"
        } else {
            executable += 1;
            "pending"
        };
        conn.execute(
            "INSERT INTO repair_actions (run_id, position, component_id, action_id, severity, decision, status, before_state, created_at, updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?9)",
            params![
                run_id,
                position,
                finding.component_id,
                finding.recommended_action,
                finding.severity,
                decision.as_str(),
                status,
                finding.observed_state,
                clock_now,
            ],
        )?;
        let action_id = conn.last_insert_rowid();
        insert_audit(
            conn,
            &AuditEvent {
                ts: clock_now,
                run_id,
                action_id: Some(action_id),
                component_id: &finding.component_id,
                action: &finding.recommended_action,
                decision: decision.as_str(),
                actor,
                before_state: &finding.observed_state,
                after_state: status,
                result: if duplicate_component {
                    "duplicate_component_skipped"
                } else {
                    "permission_decided"
                },
                detail: &finding.evidence,
            },
        )?;
    }

    let final_status = if executable == 0 {
        RepairRunStatus::Rejected
    } else {
        RepairRunStatus::AwaitingApproval
    };
    conn.execute(
        "UPDATE repair_runs SET status = ?1, updated_at = ?2 WHERE id = ?3",
        params![final_status.as_str(), clock_now, run_id],
    )?;
    insert_audit(
        conn,
        &AuditEvent {
            ts: clock_now,
            run_id,
            action_id: None,
            component_id: "",
            action: "status",
            decision: "",
            actor,
            before_state: RepairRunStatus::Planned.as_str(),
            after_state: final_status.as_str(),
            result: if executable == 0 {
                "all actions denied"
            } else {
                "awaiting operator approval"
            },
            detail: "",
        },
    )?;
    tx.commit()?;

    load_run(conn, run_id)?
        .ok_or_else(|| RepairExecutorError::CorruptData(format!("run {run_id} vanished")))
}

/// Operator approves a run. Idempotent: approving an already-approved run
/// returns it unchanged. Failure modes: run missing, illegal transition,
/// SQLite errors.
pub fn approve_run(
    conn: &Connection,
    run_id: i64,
    clock_now: i64,
    actor: &str,
) -> Result<RepairRun, RepairExecutorError> {
    if actor.trim().is_empty() {
        return Err(RepairExecutorError::EmptyActor);
    }
    let run = load_run(conn, run_id)?.ok_or(RepairExecutorError::RunNotFound(run_id))?;
    if run.status == RepairRunStatus::Approved {
        return Ok(run);
    }
    if !status_transition_allowed(run.status, RepairRunStatus::Approved) {
        return Err(RepairExecutorError::InvalidTransition {
            from: run.status.as_str().to_owned(),
            to: RepairRunStatus::Approved.as_str().to_owned(),
        });
    }
    let tx = Tx::begin(conn)?;
    let claimed = conn.execute(
        "UPDATE repair_runs SET status = 'approved', updated_at = ?1 WHERE id = ?2 AND status = 'awaiting_approval'",
        params![clock_now, run_id],
    )?;
    if claimed == 0 {
        // The run changed between our read and this write: the update above
        // touched nothing, so no state or audit is fabricated.
        drop(tx);
        let current = load_run(conn, run_id)?.ok_or(RepairExecutorError::RunNotFound(run_id))?;
        if current.status == RepairRunStatus::Approved {
            return Ok(current);
        }
        return Err(RepairExecutorError::InvalidTransition {
            from: current.status.as_str().to_owned(),
            to: RepairRunStatus::Approved.as_str().to_owned(),
        });
    }
    insert_audit(
        conn,
        &AuditEvent {
            ts: clock_now,
            run_id,
            action_id: None,
            component_id: "",
            action: "run_approved",
            decision: "approved",
            actor,
            before_state: run.status.as_str(),
            after_state: RepairRunStatus::Approved.as_str(),
            result: "",
            detail: "",
        },
    )?;
    tx.commit()?;
    load_run(conn, run_id)?.ok_or(RepairExecutorError::RunNotFound(run_id))
}

/// Operator rejects a pending run. Deny decisions can never be overridden by
/// approval, but a pending run can always be rejected.
pub fn reject_run(
    conn: &Connection,
    run_id: i64,
    clock_now: i64,
    actor: &str,
) -> Result<RepairRun, RepairExecutorError> {
    if actor.trim().is_empty() {
        return Err(RepairExecutorError::EmptyActor);
    }
    let run = load_run(conn, run_id)?.ok_or(RepairExecutorError::RunNotFound(run_id))?;
    if run.status == RepairRunStatus::Rejected {
        return Ok(run);
    }
    if !status_transition_allowed(run.status, RepairRunStatus::Rejected) {
        return Err(RepairExecutorError::InvalidTransition {
            from: run.status.as_str().to_owned(),
            to: RepairRunStatus::Rejected.as_str().to_owned(),
        });
    }
    let tx = Tx::begin(conn)?;
    let claimed = conn.execute(
        "UPDATE repair_runs SET status = 'rejected', completed_at = ?1, updated_at = ?1, result = 'rejected by operator' WHERE id = ?2 AND status IN ('planned', 'awaiting_approval')",
        params![clock_now, run_id],
    )?;
    if claimed == 0 {
        drop(tx);
        let current = load_run(conn, run_id)?.ok_or(RepairExecutorError::RunNotFound(run_id))?;
        if current.status == RepairRunStatus::Rejected {
            return Ok(current);
        }
        return Err(RepairExecutorError::InvalidTransition {
            from: current.status.as_str().to_owned(),
            to: RepairRunStatus::Rejected.as_str().to_owned(),
        });
    }
    insert_audit(
        conn,
        &AuditEvent {
            ts: clock_now,
            run_id,
            action_id: None,
            component_id: "",
            action: "run_rejected",
            decision: "rejected",
            actor,
            before_state: run.status.as_str(),
            after_state: RepairRunStatus::Rejected.as_str(),
            result: "",
            detail: "",
        },
    )?;
    tx.commit()?;
    load_run(conn, run_id)?.ok_or(RepairExecutorError::RunNotFound(run_id))
}

fn count_actions(
    conn: &Connection,
    run_id: i64,
    status: ActionStatus,
) -> Result<i64, rusqlite::Error> {
    conn.query_row(
        "SELECT COUNT(*) FROM repair_actions WHERE run_id = ?1 AND status = ?2",
        params![run_id, status.as_str()],
        |row| row.get(0),
    )
}

fn outcome_for(conn: &Connection, run: &RepairRun) -> Result<RunOutcome, rusqlite::Error> {
    Ok(RunOutcome {
        run_id: run.id,
        status: run.status,
        verified_actions: count_actions(conn, run.id, ActionStatus::Verified)?,
        failed_actions: count_actions(conn, run.id, ActionStatus::Failed)?,
        message: run.result.clone(),
    })
}

/// Executes every pending action of an APPROVED run: apply through the
/// injected runner, then verify through the injected post-condition checker.
/// The first failure marks the run `failed` and skips all remaining pending
/// actions (no retries). Terminal runs replay idempotently: the stored
/// result and action counts are returned and nothing is touched. `Applying`
/// is accepted as a resumption point (same-process re-call before crash
/// recovery). Failure modes: run missing, illegal transition, runner or
/// checker errors recorded as failures (not returned), SQLite errors.
pub fn execute_approved_run(
    conn: &Connection,
    run_id: i64,
    clock_now: i64,
    actor: &str,
    runner: &dyn RepairActionRunner,
    checker: &dyn PostConditionChecker,
) -> Result<RunOutcome, RepairExecutorError> {
    if actor.trim().is_empty() {
        return Err(RepairExecutorError::EmptyActor);
    }
    let run = load_run(conn, run_id)?.ok_or(RepairExecutorError::RunNotFound(run_id))?;
    if run.status.is_terminal() {
        return outcome_for(conn, &run).map_err(RepairExecutorError::from);
    }
    if run.status != RepairRunStatus::Approved && run.status != RepairRunStatus::Applying {
        return Err(RepairExecutorError::InvalidTransition {
            from: run.status.as_str().to_owned(),
            to: RepairRunStatus::Applying.as_str().to_owned(),
        });
    }
    if run.status == RepairRunStatus::Approved {
        let tx = Tx::begin(conn)?;
        let claimed = conn.execute(
            "UPDATE repair_runs SET status = 'applying', updated_at = ?1 WHERE id = ?2 AND status = 'approved'",
            params![clock_now, run_id],
        )?;
        if claimed == 0 {
            // Another writer won the transition; re-dispatch on the truth
            // instead of fabricating a second approved -> applying audit.
            drop(tx);
            let current = load_run(conn, run_id)?.ok_or(RepairExecutorError::RunNotFound(run_id))?;
            if current.status.is_terminal() {
                return outcome_for(conn, &current).map_err(RepairExecutorError::from);
            }
            if current.status != RepairRunStatus::Applying {
                return Err(RepairExecutorError::InvalidTransition {
                    from: current.status.as_str().to_owned(),
                    to: RepairRunStatus::Applying.as_str().to_owned(),
                });
            }
            // Winner moved the run to applying; resume below as an applying run.
        } else {
            insert_audit(
                conn,
                &AuditEvent {
                    ts: clock_now,
                    run_id,
                    action_id: None,
                    component_id: "",
                    action: "status",
                    decision: "",
                    actor,
                    before_state: RepairRunStatus::Approved.as_str(),
                    after_state: RepairRunStatus::Applying.as_str(),
                    result: "",
                    detail: "",
                },
            )?;
            tx.commit()?;
        }
    }

    if run.status == RepairRunStatus::Applying {
        let in_flight: i64 = conn.query_row(
            "SELECT COUNT(*) FROM repair_actions WHERE run_id = ?1 AND status = 'applying'",
            params![run_id],
            |row| row.get(0),
        )?;
        if in_flight > 0 {
            // A dead executor left an action mid-apply. Re-applying it would
            // risk repeating an unknown side effect, and verifying the run
            // would be a lie. Same doctrine as crash recovery: mark the run
            // interrupted, never auto-resume, let an operator decide.
            let tx = Tx::begin(conn)?;
            let demoted = conn.execute(
                "UPDATE repair_runs SET status = 'interrupted', result = 'interrupted: action was still in flight', updated_at = ?1 WHERE id = ?2 AND status = 'applying'",
                params![clock_now, run_id],
            )?;
            if demoted > 0 {
                conn.execute(
                    "UPDATE repair_actions SET status = 'interrupted', detail = 'executor resumed with this action in flight; never blindly reapplied', updated_at = ?1 WHERE run_id = ?2 AND status = 'applying'",
                    params![clock_now, run_id],
                )?;
                insert_audit(
                    conn,
                    &AuditEvent {
                        ts: clock_now,
                        run_id,
                        action_id: None,
                        component_id: "",
                        action: "crash_recovery",
                        decision: "",
                        actor,
                        before_state: RepairRunStatus::Applying.as_str(),
                        after_state: RepairRunStatus::Interrupted.as_str(),
                        result: "never auto-resumed",
                        detail: format!("{in_flight} action(s) still in flight on re-entry").as_str(),
                    },
                )?;
            }
            tx.commit()?;
            let current = load_run(conn, run_id)?
                .ok_or(RepairExecutorError::CorruptData(format!("run {run_id} vanished")))?;
            return outcome_for(conn, &current).map_err(RepairExecutorError::from);
        }
    }

    let pending: Vec<RepairAction> = {
        let mut statement = conn.prepare(
            &format!("SELECT {ACTION_COLUMNS} FROM repair_actions WHERE run_id = ?1 AND status = 'pending' ORDER BY position"),
        )?;
        let rows = statement.query_map(params![run_id], row_to_action)?;
        rows.take(MAX_ACTIONS_PER_RUN)
            .collect::<Result<Vec<_>, _>>()?
    };

    let mut message = String::from("all executable actions verified");
    let mut final_status = RepairRunStatus::Verified;

    for action in &pending {
        let tx = Tx::begin(conn)?;
        let claimed = conn.execute(
            "UPDATE repair_actions SET status = 'applying', updated_at = ?1 WHERE id = ?2 AND status = 'pending'",
            params![clock_now, action.id],
        )?;
        if claimed == 0 {
            // Another writer already claimed this action: never double-apply
            // and never audit a claim we did not win.
            drop(tx);
            continue;
        }
        insert_audit(
            conn,
            &AuditEvent {
                ts: clock_now,
                run_id,
                action_id: Some(action.id),
                component_id: &action.component_id,
                action: &action.action_id,
                decision: action.decision.as_str(),
                actor,
                before_state: &action.before_state,
                after_state: ActionStatus::Applying.as_str(),
                result: "",
                detail: "",
            },
        )?;
        tx.commit()?;

        let (success, after_state, result, detail) = match runner.apply(action) {
            Ok(apply_detail) => match checker.observe(action) {
                Ok(observation) => {
                    if observation.component_id != action.component_id {
                        (
                            false,
                            String::new(),
                            "postcondition_mismatch",
                            format!(
                                "checker observed {} for action on {}",
                                observation.component_id, action.component_id
                            ),
                        )
                    } else if is_healthy(&observation.state) {
                        (
                            true,
                            observation.state.clone(),
                            "verified",
                            format!("applied: {apply_detail}; observed {}", observation.state),
                        )
                    } else {
                        (
                            false,
                            observation.state.clone(),
                            "postcondition_failed",
                            format!(
                                "applied: {apply_detail}; still observed {}",
                                observation.state
                            ),
                        )
                    }
                }
                Err(error) => (
                    false,
                    String::new(),
                    "postcondition_error",
                    format!("applied: {apply_detail}; re-observation failed: {error}"),
                ),
            },
            Err(error) => (false, String::new(), "apply_failed", error),
        };

        let action_status = if success { "verified" } else { "failed" };
        let tx = Tx::begin(conn)?;
        let settled = conn.execute(
            "UPDATE repair_actions SET status = ?1, after_state = ?2, detail = ?3, updated_at = ?4 WHERE id = ?5 AND status = 'applying'",
            params![action_status, after_state, detail, clock_now, action.id],
        )?;
        if settled == 0 {
            // We claimed this action as applying and it no longer is: some
            // other writer settled it underneath us. Report the conflict
            // loudly instead of layering a second terminal write on top.
            return Err(RepairExecutorError::Conflict(format!(
                "repair action {} changed underneath the executor",
                action.id
            )));
        }
        insert_audit(
            conn,
            &AuditEvent {
                ts: clock_now,
                run_id,
                action_id: Some(action.id),
                component_id: &action.component_id,
                action: &action.action_id,
                decision: action.decision.as_str(),
                actor,
                before_state: &action.before_state,
                after_state: action_status,
                result,
                detail: &detail,
            },
        )?;

        if success {
            tx.commit()?;
            continue;
        }

        // First failure stops the run: skip everything still pending.
        let skipped = conn.execute(
            "UPDATE repair_actions SET status = 'skipped', updated_at = ?1 WHERE run_id = ?2 AND status = 'pending'",
            params![clock_now, run_id],
        )? as i64;
        conn.execute(
            "INSERT INTO repair_audit (ts, run_id, action_id, component_id, action, decision, actor, before_state, after_state, result, detail) SELECT ?1, run_id, id, component_id, action_id, decision, '', '', 'skipped', 'skipped_after_failure', '' FROM repair_actions WHERE run_id = ?2 AND status = 'skipped'",
            params![clock_now, run_id],
        )?;
        let run_message = format!(
            "stopped after action {} of {}: {detail}",
            action.action_id, action.component_id
        );
        message.clone_from(&run_message);
        final_status = RepairRunStatus::Failed;
        let finalised = conn.execute(
            "UPDATE repair_runs SET status = 'failed', result = ?1, completed_at = ?2, updated_at = ?2 WHERE id = ?3 AND status = 'applying'",
            params![run_message, clock_now, run_id],
        )?;
        if finalised == 0 {
            // Recovery or another writer already moved this run to a
            // non-applying state; that truth wins over our terminal write.
            drop(tx);
            let current = load_run(conn, run_id)?
                .ok_or(RepairExecutorError::CorruptData(format!("run {run_id} vanished")))?;
            return outcome_for(conn, &current).map_err(RepairExecutorError::from);
        }
        insert_audit(
            conn,
            &AuditEvent {
                ts: clock_now,
                run_id,
                action_id: None,
                component_id: "",
                action: "status",
                decision: "",
                actor,
                before_state: RepairRunStatus::Applying.as_str(),
                after_state: RepairRunStatus::Failed.as_str(),
                result: &run_message,
                detail: format!("{skipped} pending actions skipped").as_str(),
            },
        )?;
        tx.commit()?;
        break;
    }

    if final_status == RepairRunStatus::Verified {
        let tx = Tx::begin(conn)?;
        let finalised = conn.execute(
            "UPDATE repair_runs SET status = 'verified', result = ?1, completed_at = ?2, updated_at = ?2 WHERE id = ?3 AND status = 'applying'",
            params![message, clock_now, run_id],
        )?;
        if finalised == 0 {
            drop(tx);
            let current = load_run(conn, run_id)?
                .ok_or(RepairExecutorError::CorruptData(format!("run {run_id} vanished")))?;
            return outcome_for(conn, &current).map_err(RepairExecutorError::from);
        }
        insert_audit(
            conn,
            &AuditEvent {
                ts: clock_now,
                run_id,
                action_id: None,
                component_id: "",
                action: "status",
                decision: "",
                actor,
                before_state: RepairRunStatus::Applying.as_str(),
                after_state: RepairRunStatus::Verified.as_str(),
                result: &message,
                detail: "",
            },
        )?;
        tx.commit()?;
    }

    let run = load_run(conn, run_id)?.ok_or(RepairExecutorError::CorruptData(format!(
        "run {run_id} vanished"
    )))?;
    outcome_for(conn, &run).map_err(RepairExecutorError::from)
}

/// Startup crash recovery: runs caught mid-execution become `interrupted`
/// and are NEVER auto-resumed; their in-flight actions are marked
/// `interrupted` for operator review. Still-pending actions of the run stay
/// pending so the operator can decide. Returns the number of runs flipped.
/// Failure mode: SQLite errors only.
pub fn recover_interrupted_runs(
    conn: &Connection,
    clock_now: i64,
) -> Result<usize, RepairExecutorError> {
    // Batches are bounded; the loop keeps going until nothing is applying,
    // so no crash victim is silently left behind past the first page. The
    // SELECT lives inside the transaction: the snapshot it reads is the
    // snapshot it acts on. Guarded UPDATEs mean a run a live executor is
    // still settling is never demoted or audited by us.
    const RECOVERY_BATCH: usize = 256;
    let mut total = 0usize;
    loop {
        let tx = Tx::begin(conn)?;
        let ids: Vec<i64> = {
            let mut statement = conn.prepare(
                "SELECT id FROM repair_runs WHERE status = 'applying' ORDER BY id LIMIT ?1",
            )?;
            let rows = statement.query_map(params![RECOVERY_BATCH as i64], |row| row.get(0))?;
            rows.collect::<Result<Vec<i64>, _>>()?
        };
        if ids.is_empty() {
            tx.commit()?;
            return Ok(total);
        }
        for run_id in &ids {
            let demoted = conn.execute(
                "UPDATE repair_runs SET status = 'interrupted', result = 'interrupted by crash recovery', updated_at = ?1 WHERE id = ?2 AND status = 'applying'",
                params![clock_now, run_id],
            )?;
            if demoted == 0 {
                // A live executor settled this run between our SELECT and our
                // UPDATE inside this very transaction: leave its truth alone.
                continue;
            }
            conn.execute(
                "UPDATE repair_actions SET status = 'interrupted', detail = 'crash recovery; never auto-resumed', updated_at = ?1 WHERE run_id = ?2 AND status = 'applying'",
                params![clock_now, run_id],
            )?;
            insert_audit(
                conn,
                &AuditEvent {
                    ts: clock_now,
                    run_id: *run_id,
                    action_id: None,
                    component_id: "",
                    action: "crash_recovery",
                    decision: "",
                    actor: "system",
                    before_state: RepairRunStatus::Applying.as_str(),
                    after_state: RepairRunStatus::Interrupted.as_str(),
                    result: "never auto-resumed",
                    detail: "",
                },
            )?;
            total += 1;
        }
        tx.commit()?;
        if ids.len() < RECOVERY_BATCH {
            return Ok(total);
        }
    }
}

/// Bounded run listing for the UI (default 50, hard cap 100, newest first).
pub fn list_runs(
    conn: &Connection,
    limit: Option<i64>,
) -> Result<Vec<RepairRun>, RepairExecutorError> {
    let clamp = limit.unwrap_or(50).clamp(1, MAX_RUNS_LIST);
    let mut statement = conn.prepare(&format!(
        "SELECT {RUN_COLUMNS} FROM repair_runs ORDER BY created_at DESC, id DESC LIMIT ?1"
    ))?;
    let rows = statement.query_map(params![clamp], row_to_run)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// Full run detail: actions plus the most recent audit trail (bounded, with
/// a truncation flag). Failure mode: run missing or SQLite errors.
pub fn get_run_detail(conn: &Connection, run_id: i64) -> Result<RunDetail, RepairExecutorError> {
    let run = load_run(conn, run_id)?.ok_or(RepairExecutorError::RunNotFound(run_id))?;
    let actions: Vec<RepairAction> = {
        let mut statement = conn.prepare(&format!(
            "SELECT {ACTION_COLUMNS} FROM repair_actions WHERE run_id = ?1 ORDER BY position"
        ))?;
        let rows = statement.query_map(params![run_id], row_to_action)?;
        rows.take(MAX_ACTIONS_PER_RUN)
            .collect::<Result<Vec<_>, _>>()?
    };
    let mut fetched: Vec<AuditRow> = {
        let mut statement = conn.prepare(
            "SELECT id, ts, run_id, action_id, component_id, action, decision, actor, before_state, after_state, result, detail FROM repair_audit WHERE run_id = ?1 ORDER BY id DESC LIMIT ?2",
        )?;
        let rows = statement.query_map(params![run_id, AUDIT_FETCH_LIMIT + 1], |row| {
            Ok(AuditRow {
                id: row.get(0)?,
                ts: row.get(1)?,
                run_id: row.get(2)?,
                action_id: row.get(3)?,
                component_id: row.get(4)?,
                action: row.get(5)?,
                decision: row.get(6)?,
                actor: row.get(7)?,
                before_state: row.get(8)?,
                after_state: row.get(9)?,
                result: row.get(10)?,
                detail: row.get(11)?,
            })
        })?;
        rows.take(usize::try_from(AUDIT_FETCH_LIMIT + 1).unwrap_or(MAX_ACTIONS_PER_RUN))
            .collect::<Result<Vec<_>, _>>()?
    };
    let audit_truncated = fetched.len() as i64 > AUDIT_FETCH_LIMIT;
    if audit_truncated {
        fetched.truncate(usize::try_from(AUDIT_FETCH_LIMIT).unwrap_or(MAX_ACTIONS_PER_RUN));
    }
    Ok(RunDetail {
        run,
        actions,
        audit: fetched,
        audit_truncated,
    })
}

/// Picks the next component worth scheduling: first `auto_apply_eligible`
/// finding whose component cooldown has expired (`cooldowns` maps component
/// id -> not-eligible-until timestamp; missing means never rate-limited).
/// Timestamp comparisons saturate so huge clocks never wrap.
#[must_use]
pub fn next_eligible_action<'a>(
    findings: &'a [RepairFinding],
    cooldowns: &BTreeMap<String, i64>,
    now: i64,
) -> Option<&'a RepairFinding> {
    findings.iter().find(|finding| {
        finding.auto_apply_eligible
            && cooldowns
                .get(&finding.component_id)
                // i64::MAX means the deadline saturated: treat it as
                // permanently in cooldown, never "expired" at clock extremes.
                .is_none_or(|until| *until != i64::MAX && now >= *until)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repair::build_repair_plan;

    fn setup() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        init_repair_executor(&conn).unwrap();
        conn
    }

    fn finding(component: &str, severity: &str, action: &str) -> RepairFinding {
        RepairFinding {
            component_id: component.into(),
            severity: severity.into(),
            condition: format!("{component} is {severity}"),
            observed_state: "offline".into(),
            evidence: "probe failed twice".into(),
            recommended_action: action.into(),
            requires_operator_approval: true,
            auto_apply_eligible: false,
            status: "open".into(),
        }
    }

    fn plan_with(findings: Vec<RepairFinding>) -> RepairPlan {
        RepairPlan {
            status: "attention".into(),
            checked_components: findings.len(),
            findings,
            summary: "test plan".into(),
            safety_boundary: "proposals only".into(),
        }
    }

    struct OkRunner;
    impl RepairActionRunner for OkRunner {
        fn apply(&self, _action: &RepairAction) -> Result<String, String> {
            Ok("simulated restart".into())
        }
    }

    struct FailRunner;
    impl RepairActionRunner for FailRunner {
        fn apply(&self, _action: &RepairAction) -> Result<String, String> {
            Err("runner refused".into())
        }
    }

    struct CheckerTo {
        state: String,
        component: Option<String>,
    }
    impl PostConditionChecker for CheckerTo {
        fn observe(&self, action: &RepairAction) -> Result<ComponentObservation, String> {
            Ok(ComponentObservation {
                component_id: self
                    .component
                    .clone()
                    .unwrap_or_else(|| action.component_id.clone()),
                state: self.state.clone(),
                detail: "re-observed".into(),
            })
        }
    }

    #[test]
    fn serde_variants_match_db_strings() {
        for status in [
            RepairRunStatus::Planned,
            RepairRunStatus::AwaitingApproval,
            RepairRunStatus::Verified,
            RepairRunStatus::RolledBack,
        ] {
            let json = serde_json::to_value(status).unwrap();
            assert_eq!(json.as_str().unwrap(), status.as_str());
            assert_eq!(RepairRunStatus::parse(status.as_str()), Some(status));
        }
        for action in [
            ActionStatus::Pending,
            ActionStatus::Skipped,
            ActionStatus::Denied,
        ] {
            let json = serde_json::to_value(action).unwrap();
            assert_eq!(json.as_str().unwrap(), action.as_str());
        }
        for permission in [Permission::Pass, Permission::Ask, Permission::Deny] {
            let json = serde_json::to_value(permission).unwrap();
            assert_eq!(json.as_str().unwrap(), permission.as_str());
        }
    }

    #[test]
    fn state_machine_table_is_exact() {
        use RepairRunStatus as S;
        assert!(can_transition(S::Planned, S::AwaitingApproval));
        assert!(can_transition(S::Applying, S::Interrupted));
        assert!(can_transition(S::Verified, S::RolledBack));
        // The whole point of the state machine: nothing skips approval.
        assert!(!can_transition(S::Planned, S::Applying));
        assert!(!can_transition(S::AwaitingApproval, S::Applying));
        assert!(!can_transition(S::Verified, S::Applying));
        assert!(!can_transition(S::Interrupted, S::Applying));
        assert!(!can_transition(S::Rejected, S::Approved));
        assert!(!can_transition(S::Failed, S::Verified));
    }

    #[test]
    fn permission_matrix_default_deny() {
        let policy = PermissionPolicy::default();
        let mut medium = finding("gpu", "medium", "restart.supervisor");
        medium.auto_apply_eligible = true;
        medium.requires_operator_approval = false;
        let high = finding("gpu", "high", "reboot.host");
        assert_eq!(policy.decide(&medium), Permission::Ask);
        assert_eq!(policy.decide(&high), Permission::Deny);

        let open = PermissionPolicy::new(
            BTreeSet::from(["restart.supervisor".to_owned(), "reboot.host".to_owned()]),
            BTreeSet::from(["restart.supervisor".to_owned()]),
        );
        // denylist wins over full eligibility + allowlist membership
        assert_eq!(open.decide(&medium), Permission::Deny);
        let mut high_open = finding("gpu", "high", "reboot.host");
        high_open.auto_apply_eligible = true;
        high_open.requires_operator_approval = false;
        // high + allowlisted + eligible = Pass (v1 keeps approval on the RUN)
        assert_eq!(open.decide(&high_open), Permission::Pass);
        // allowlisted but operator approval required by the planner = Ask
        // (uses reboot.host: restart.supervisor sits in the denylist above)
        assert_eq!(
            open.decide(&finding("gpu", "medium", "reboot.host")),
            Permission::Ask
        );
    }

    #[test]
    fn create_run_happy_path_and_rejections() {
        let conn = setup();
        let err = create_run_from_plan(
            &conn,
            &plan_with(vec![]),
            &PermissionPolicy::default(),
            100,
            "operator",
        )
        .unwrap_err();
        assert!(matches!(err, RepairExecutorError::EmptyPlan));
        assert!(matches!(
            create_run_from_plan(
                &conn,
                &plan_with(vec![finding("a", "low", "")]),
                &PermissionPolicy::default(),
                100,
                "operator"
            )
            .unwrap_err(),
            RepairExecutorError::EmptyActionId
        ));
        assert!(matches!(
            create_run_from_plan(
                &conn,
                &plan_with(vec![finding("a", "low", "x")]),
                &PermissionPolicy::default(),
                100,
                "  "
            )
            .unwrap_err(),
            RepairExecutorError::EmptyActor
        ));

        let run = create_run_from_plan(
            &conn,
            &plan_with(vec![
                finding("gpu", "high", "reboot.host"),
                finding("hub", "medium", "restart.hub"),
            ]),
            &PermissionPolicy::default(),
            100,
            "operator",
        )
        .unwrap();
        assert_eq!(run.status, RepairRunStatus::AwaitingApproval);
        let detail = get_run_detail(&conn, run.id).unwrap();
        assert_eq!(detail.actions.len(), 2);
        assert_eq!(detail.actions[0].status, ActionStatus::Denied);
        assert_eq!(detail.actions[1].status, ActionStatus::Pending);

        // All denied -> immediately rejected run.
        let all_denied = create_run_from_plan(
            &conn,
            &plan_with(vec![finding("gpu", "high", "reboot.host")]),
            &PermissionPolicy::default(),
            101,
            "operator",
        )
        .unwrap();
        assert_eq!(all_denied.status, RepairRunStatus::Rejected);
    }

    #[test]
    fn executing_requires_approval_then_verifies() {
        let conn = setup();
        let run = create_run_from_plan(
            &conn,
            &plan_with(vec![finding("hub", "medium", "restart.hub")]),
            &PermissionPolicy::default(),
            100,
            "operator",
        )
        .unwrap();
        // Unapproved execution is illegal.
        let err = execute_approved_run(
            &conn,
            run.id,
            101,
            "operator",
            &OkRunner,
            &CheckerTo {
                state: "online".into(),
                component: None,
            },
        )
        .unwrap_err();
        assert!(matches!(err, RepairExecutorError::InvalidTransition { .. }));

        approve_run(&conn, run.id, 102, "operator").unwrap();
        // Re-approval is idempotent.
        let again = approve_run(&conn, run.id, 103, "operator").unwrap();
        assert_eq!(again.status, RepairRunStatus::Approved);

        let outcome = execute_approved_run(
            &conn,
            run.id,
            104,
            "operator",
            &OkRunner,
            &CheckerTo {
                state: "online".into(),
                component: None,
            },
        )
        .unwrap();
        assert_eq!(outcome.status, RepairRunStatus::Verified);
        assert_eq!(outcome.verified_actions, 1);
        assert_eq!(outcome.failed_actions, 0);

        // Terminal replay is byte-identical and touches nothing.
        let replay = execute_approved_run(
            &conn,
            run.id,
            999,
            "operator",
            &FailRunner,
            &CheckerTo {
                state: "offline".into(),
                component: None,
            },
        )
        .unwrap();
        assert_eq!(replay, outcome);

        let detail = get_run_detail(&conn, run.id).unwrap();
        assert_eq!(detail.actions[0].status, ActionStatus::Verified);
        assert_eq!(detail.actions[0].after_state, "online");
        assert!(!detail.audit.is_empty());
        assert!(!detail.audit_truncated);
    }

    #[test]
    fn postcondition_failure_stops_run_and_skips_rest() {
        let conn = setup();
        let run = create_run_from_plan(
            &conn,
            &plan_with(vec![
                finding("hub", "medium", "restart.hub"),
                finding("gpu", "medium", "restart.supervisor"),
                finding("voice", "medium", "restart.voice"),
            ]),
            &PermissionPolicy::default(),
            100,
            "operator",
        )
        .unwrap();
        approve_run(&conn, run.id, 101, "operator").unwrap();
        let outcome = execute_approved_run(
            &conn,
            run.id,
            102,
            "operator",
            &OkRunner,
            &CheckerTo {
                state: "degraded".into(),
                component: None,
            },
        )
        .unwrap();
        assert_eq!(outcome.status, RepairRunStatus::Failed);
        assert_eq!(outcome.failed_actions, 1);
        assert_eq!(outcome.verified_actions, 0);
        let detail = get_run_detail(&conn, run.id).unwrap();
        assert_eq!(detail.actions[0].status, ActionStatus::Failed);
        assert_eq!(detail.actions[1].status, ActionStatus::Skipped);
        assert_eq!(detail.actions[2].status, ActionStatus::Skipped);
    }

    #[test]
    fn runner_error_and_checker_mismatch_fail_the_run() {
        let conn = setup();
        let run = create_run_from_plan(
            &conn,
            &plan_with(vec![finding("hub", "medium", "restart.hub")]),
            &PermissionPolicy::default(),
            100,
            "operator",
        )
        .unwrap();
        approve_run(&conn, run.id, 101, "operator").unwrap();
        let outcome = execute_approved_run(
            &conn,
            run.id,
            102,
            "operator",
            &FailRunner,
            &CheckerTo {
                state: "online".into(),
                component: None,
            },
        )
        .unwrap();
        assert_eq!(outcome.status, RepairRunStatus::Failed);
        assert!(outcome.message.contains("runner refused"));

        let run2 = create_run_from_plan(
            &conn,
            &plan_with(vec![finding("hub", "medium", "restart.hub")]),
            &PermissionPolicy::default(),
            110,
            "operator",
        )
        .unwrap();
        approve_run(&conn, run2.id, 111, "operator").unwrap();
        let outcome2 = execute_approved_run(
            &conn,
            run2.id,
            112,
            "operator",
            &OkRunner,
            &CheckerTo {
                state: "online".into(),
                component: Some("other.component".into()),
            },
        )
        .unwrap();
        assert_eq!(outcome2.status, RepairRunStatus::Failed);
        assert!(outcome2.message.contains("other.component"));
    }

    #[test]
    fn crash_recovery_flips_applying_never_resumes() {
        let conn = setup();
        let run = create_run_from_plan(
            &conn,
            &plan_with(vec![
                finding("hub", "medium", "restart.hub"),
                finding("gpu", "medium", "restart.supervisor"),
            ]),
            &PermissionPolicy::default(),
            100,
            "operator",
        )
        .unwrap();
        approve_run(&conn, run.id, 101, "operator").unwrap();
        // Simulate a crash mid-execution.
        conn.execute(
            "UPDATE repair_runs SET status = 'applying' WHERE id = ?1",
            params![run.id],
        )
        .unwrap();
        conn.execute(
            "UPDATE repair_actions SET status = 'applying' WHERE run_id = ?1 AND position = 0",
            params![run.id],
        )
        .unwrap();

        assert_eq!(recover_interrupted_runs(&conn, 200).unwrap(), 1);
        assert_eq!(recover_interrupted_runs(&conn, 201).unwrap(), 0);
        let detail = get_run_detail(&conn, run.id).unwrap();
        assert_eq!(detail.run.status, RepairRunStatus::Interrupted);
        assert_eq!(detail.actions[0].status, ActionStatus::Interrupted);
        assert_eq!(detail.actions[1].status, ActionStatus::Pending);
        // Interrupted runs cannot be executed or approved.
        assert!(matches!(
            execute_approved_run(
                &conn,
                run.id,
                202,
                "operator",
                &OkRunner,
                &CheckerTo {
                    state: "online".into(),
                    component: None,
                },
            )
            .unwrap_err(),
            RepairExecutorError::InvalidTransition { .. }
        ));
        assert!(matches!(
            approve_run(&conn, run.id, 203, "operator").unwrap_err(),
            RepairExecutorError::InvalidTransition { .. }
        ));
    }

    #[test]
    fn cooldown_scheduling_picks_next_free_component() {
        let findings = vec![
            finding("hub", "medium", "restart.hub"),
            finding("gpu", "medium", "restart.supervisor"),
        ];
        let mut eligible = findings.clone();
        for f in &mut eligible {
            f.auto_apply_eligible = true;
        }
        let mut cooldowns = BTreeMap::new();
        cooldowns.insert("hub".to_owned(), 1000);
        assert_eq!(
            next_eligible_action(&eligible, &cooldowns, 999)
                .unwrap()
                .component_id,
            "gpu"
        );
        assert_eq!(
            next_eligible_action(&eligible, &cooldowns, 1000)
                .unwrap()
                .component_id,
            "hub"
        );
        assert_eq!(
            next_eligible_action(&findings, &cooldowns, 1000),
            None,
            "non-eligible findings are never scheduled"
        );
        assert_eq!(cooldown_deadline(i64::MAX), i64::MAX);
        // saturating: a far-future cooldown never blocks via overflow
        assert!(next_eligible_action(&eligible, &cooldowns, i64::MAX).is_some());
    }

    #[test]
    fn planner_findings_flow_into_a_verifiable_run() {
        // End-to-end with the REAL planner: an offline observation produces
        // a high finding -> default policy denies it -> run is rejected.
        let conn = setup();
        let plan = build_repair_plan(&[ComponentObservation {
            component_id: "gpu.supervisor".into(),
            state: "offline".into(),
            detail: "no heartbeat".into(),
        }]);
        assert_eq!(plan.findings.len(), 1);
        assert_eq!(plan.findings[0].severity, "high");
        let run = create_run_from_plan(&conn, &plan, &PermissionPolicy::default(), 100, "system")
            .unwrap();
        assert_eq!(run.status, RepairRunStatus::Rejected);
    }

    #[test]
    fn corrupt_status_string_surfaces_as_database_error() {
        let conn = setup();
        conn.execute(
            "INSERT INTO repair_runs (status, created_at, updated_at) VALUES ('bogus', 1, 1)",
            [],
        )
        .unwrap_err(); // CHECK constraint rejects it outright
    }

    struct RecordingRunner {
        applied: std::cell::RefCell<Vec<i64>>,
    }
    impl RepairActionRunner for RecordingRunner {
        fn apply(&self, action: &RepairAction) -> Result<String, String> {
            self.applied.borrow_mut().push(action.id);
            Ok("recorded".into())
        }
    }

    fn healthy_checker() -> CheckerTo {
        CheckerTo {
            state: "online".into(),
            component: None,
        }
    }

    fn approved_run(conn: &Connection, components: &[&str], clock: i64) -> RepairRun {
        let plan = plan_with(
            components
                .iter()
                .enumerate()
                .map(|(i, c)| finding(c, "medium", if i == 0 { "restart.hub" } else { "restart.supervisor" }))
                .collect(),
        );
        let run = create_run_from_plan(conn, &plan, &PermissionPolicy::default(), clock, "operator").unwrap();
        approve_run(conn, run.id, clock + 1, "operator").unwrap()
    }

    #[test]
    fn approve_and_reject_refuse_to_stomp_state_changed_underneath() {
        let conn = setup();
        let run = create_run_from_plan(
            &conn,
            &plan_with(vec![finding("hub", "medium", "restart.hub")]),
            &PermissionPolicy::default(),
            100,
            "operator",
        )
        .unwrap();
        // A concurrent writer already rejected this run; a stale approve
        // that read the old state must not resurrect it (finding F1).
        conn.execute(
            "UPDATE repair_runs SET status = 'rejected' WHERE id = ?1",
            params![run.id],
        )
        .unwrap();
        let err = approve_run(&conn, run.id, 101, "operator").unwrap_err();
        assert!(
            matches!(err, RepairExecutorError::InvalidTransition { .. }),
            "{err}"
        );
        assert_eq!(
            load_run(&conn, run.id).unwrap().unwrap().status,
            RepairRunStatus::Rejected
        );

        // Symmetrically: a reject racing an approve cannot demote applying.
        let run2 = approved_run(&conn, &["gpu"], 200);
        conn.execute(
            "UPDATE repair_runs SET status = 'applying' WHERE id = ?1",
            params![run2.id],
        )
        .unwrap();
        let err = reject_run(&conn, run2.id, 202, "operator").unwrap_err();
        assert!(
            matches!(err, RepairExecutorError::InvalidTransition { .. }),
            "{err}"
        );
    }

    #[test]
    fn actions_claimed_by_another_writer_are_never_reapplied() {
        let conn = setup();
        let run = approved_run(&conn, &["hub", "gpu"], 100);
        // Another writer already applied and verified the first action (F2).
        conn.execute(
            "UPDATE repair_actions SET status = 'verified' WHERE run_id = ?1 AND position = 0",
            params![run.id],
        )
        .unwrap();
        let runner = RecordingRunner {
            applied: std::cell::RefCell::new(Vec::new()),
        };
        let outcome = execute_approved_run(&conn, run.id, 102, "operator", &runner, &healthy_checker())
            .unwrap();
        assert_eq!(outcome.status, RepairRunStatus::Verified);
        assert_eq!(runner.applied.borrow().len(), 1);
    }

    #[test]
    fn re_entering_an_applying_run_with_in_flight_action_interrupts_it() {
        let conn = setup();
        let run = approved_run(&conn, &["hub", "gpu"], 100);
        // Simulate an executor that died mid-apply without committing (F4):
        // the run is applying with an action still in flight.
        conn.execute(
            "UPDATE repair_runs SET status = 'applying' WHERE id = ?1",
            params![run.id],
        )
        .unwrap();
        conn.execute(
            "UPDATE repair_actions SET status = 'applying' WHERE run_id = ?1 AND position = 0",
            params![run.id],
        )
        .unwrap();
        let runner = RecordingRunner {
            applied: std::cell::RefCell::new(Vec::new()),
        };
        let outcome = execute_approved_run(&conn, run.id, 102, "operator", &runner, &healthy_checker())
            .unwrap();
        assert_eq!(outcome.status, RepairRunStatus::Interrupted);
        assert!(runner.applied.borrow().is_empty(), "never blindly reapplied");
        let detail = get_run_detail(&conn, run.id).unwrap();
        assert_eq!(detail.run.status, RepairRunStatus::Interrupted);
        assert_eq!(detail.actions[0].status, ActionStatus::Interrupted);
        // A third execute cannot resume it either: interrupted is operator-
        // territory by doctrine (never auto-resumed).
        let err = execute_approved_run(&conn, run.id, 103, "operator", &runner, &healthy_checker())
            .unwrap_err();
        assert!(
            matches!(err, RepairExecutorError::InvalidTransition { .. }),
            "{err}"
        );
    }

    #[test]
    fn severity_casing_or_gibberish_cannot_evade_default_deny() {
        let policy = PermissionPolicy::new(BTreeSet::new(), BTreeSet::new());
        for severity in ["High", "HIGH", "critical", "🔥", ""] {
            assert_eq!(
                policy.decide(&finding("hub", severity, "restart.hub")),
                Permission::Deny,
                "severity {severity:?} must fail closed"
            );
        }
        assert_eq!(
            policy.decide(&finding("hub", "medium", "restart.hub")),
            Permission::Ask
        );
    }

    #[test]
    fn duplicate_component_findings_collapse_to_one_action_per_run() {
        let conn = setup();
        let plan = plan_with(vec![
            finding("hub", "medium", "restart.hub"),
            finding("hub", "medium", "restart.hub"),
            finding("gpu", "medium", "restart.supervisor"),
        ]);
        let run = create_run_from_plan(&conn, &plan, &PermissionPolicy::default(), 100, "operator")
            .unwrap();
        let detail = get_run_detail(&conn, run.id).unwrap();
        let mut statuses: Vec<(String, &'static str)> = detail
            .actions
            .iter()
            .map(|a| (a.component_id.clone(), a.status.as_str()))
            .collect();
        statuses.sort();
        assert_eq!(
            statuses,
            vec![
                ("gpu".to_string(), "pending"),
                ("hub".to_string(), "pending"),
                ("hub".to_string(), "skipped"),
            ]
        );
        // And execution honours it: the hub is touched exactly once.
        approve_run(&conn, run.id, 101, "operator").unwrap();
        let runner = RecordingRunner {
            applied: std::cell::RefCell::new(Vec::new()),
        };
        execute_approved_run(&conn, run.id, 102, "operator", &runner, &healthy_checker()).unwrap();
        assert_eq!(runner.applied.borrow().len(), 2);
    }

    #[test]
    fn saturated_cooldown_deadline_is_permanently_ineligible() {
        let mut f = finding("hub", "medium", "restart.hub");
        f.auto_apply_eligible = true;
        let saturated = BTreeMap::from([("hub".to_string(), i64::MAX)]);
        assert!(next_eligible_action(&[f.clone()], &saturated, i64::MAX).is_none());
        let expired = BTreeMap::from([("hub".to_string(), 500)]);
        assert!(next_eligible_action(&[f], &expired, 500).is_some());
    }

    #[test]
    fn recovery_returns_truthful_counts_across_runs() {
        let conn = setup();
        let mut ids = Vec::new();
        for i in 0..3 {
            let run = approved_run(&conn, &["hub", "gpu", "comfyui"][..=i], 100 + i as i64 * 10);
            conn.execute(
                "UPDATE repair_runs SET status = 'applying' WHERE id = ?1",
                params![run.id],
            )
            .unwrap();
            ids.push(run.id);
        }
        assert_eq!(recover_interrupted_runs(&conn, 200).unwrap(), 3);
        assert_eq!(recover_interrupted_runs(&conn, 201).unwrap(), 0);
        for id in ids {
            assert_eq!(
                load_run(&conn, id).unwrap().unwrap().status,
                RepairRunStatus::Interrupted
            );
        }
    }
}
