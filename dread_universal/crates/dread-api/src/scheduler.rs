//! Opt-in background scheduler (B11). Loops are independently disabled when
//! their env var is unset, invalid, or zero. Intervals are seconds, clamped
//! to [MIN_SECS, MAX_SECS].
//!
//! Safety invariants (do not relax without operator sign-off):
//! - The scheduler NEVER approves or executes repairs. It may only create a
//!   run in `planned`/`awaiting_approval` state with actor "scheduler";
//!   approval and execution remain explicit human actions via the routes.
//! - The autodiscover tick runs the exact same additive pipeline as
//!   `POST /api/models/autoconfigure` (discover, autoconfigure, D-Spine
//!   engine upsert). Nothing is ever deleted or pruned.
//! - The db lock (a std Mutex) is acquired only AFTER every `.await`;
//!   holding the guard across an await makes futures !Send and deadlocks
//!   the single-writer hub pattern.
//! - A panicking tick kills only that loop task (tokio default panic hook
//!   logs it). Accepted for v1: the scheduler is convenience, never
//!   correctness-critical.

use std::time::Duration;

use tokio::time::MissedTickBehavior;

use crate::{AppState, autoconfigure_inventory, db_lock, dspine_engine_roots, now_epoch, repair_now, repair_policy, upsert_dspine_engines};
use dread_platform::repair::build_repair_plan;
use dread_platform::repair_executor::create_run_from_plan;

const MIN_SECS: u64 = 60;
const MAX_SECS: u64 = 86_400;
const SCHEDULER_ACTOR: &str = "scheduler";

/// Any run still awaiting a human counts as open, so the scheduler never
/// piles up duplicate plans. Status strings are the exact serde snake_case
/// DB text from RepairRunStatus: the open set is planned, awaiting_approval,
/// approved, applying; the terminal set is verified, failed, rolled_back,
/// rejected, interrupted (interrupted is recovered at startup, B7).
/// Keep in sync with `RepairRunStatus::as_str` — round-trip asserted below.
const OPEN_RUN_SQL: &str = "SELECT COUNT(*) FROM repair_runs \
     WHERE status IN ('planned', 'awaiting_approval', 'approved', 'applying')";

/// None = disabled. Unset/invalid/zero -> None; otherwise clamped.
pub fn parse_schedule_env(raw: Option<&str>) -> Option<u64> {
    let secs: u64 = raw?.trim().parse().ok()?;
    if secs == 0 {
        return None;
    }
    Some(secs.clamp(MIN_SECS, MAX_SECS))
}

fn env_secs(name: &str) -> Option<u64> {
    parse_schedule_env(std::env::var(name).ok().as_deref())
}

/// Spawn the enabled loops. Call once from main() before serving; AppState
/// is cheap to clone (all fields are Arc/broadcast).
pub fn maybe_spawn_scheduler(state: &AppState) {
    if let Some(secs) = env_secs("DREAD_SCHEDULE_AUTODISCOVER_SECS") {
        let st = state.clone();
        tokio::spawn(async move { autodiscover_loop(st, secs).await });
        eprintln!("scheduler: autodiscover loop armed every {secs}s");
    }
    if let Some(secs) = env_secs("DREAD_SCHEDULE_REPAIR_PLAN_SECS") {
        let st = state.clone();
        tokio::spawn(async move { repair_loop(st, secs).await });
        eprintln!("scheduler: repair-planning loop armed every {secs}s (planning only; human approval required)");
    }
}

async fn autodiscover_loop(state: AppState, secs: u64) {
    let mut tick = tokio::time::interval(Duration::from_secs(secs));
    tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
    tick.tick().await; // first tick completes immediately; first real run is one interval out
    loop {
        tick.tick().await;
        autodiscover_tick(&state).await;
    }
}

async fn autodiscover_tick(state: &AppState) {
    let roots = dspine_engine_roots();
    let scanned = tokio::task::spawn_blocking(move || {
        let inventory = dread_gpu::models::discover_local_models();
        let engines = dread_gpu::dspine::detect_engine_roots(&roots);
        (inventory, engines)
    })
    .await;
    let (inventory, engines) = match scanned {
        Ok(v) => v,
        Err(e) => {
            eprintln!("scheduler: autodiscover scan failed: {e}");
            return;
        }
    };

    // GPU capacities in GiB, mirroring the manual autoconfigure handler.
    let mut vram_gib: Vec<f64> = Vec::new();
    if let Some(sup) = state.supervisor.as_ref() {
        match sup.telemetry().await {
            Ok(t) => {
                vram_gib = t
                    .get("gpus")
                    .and_then(|g| g.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|g| g.get("memory_total_mb"))
                            .filter_map(|m| m.as_f64())
                            .map(|mb| mb / 1024.0)
                            .collect()
                    })
                    .unwrap_or_default();
            }
            Err(e) => eprintln!("scheduler: supervisor telemetry failed: {e}"),
        }
    }

    // INVARIANT: no .await below this point while the guard is alive.
    let now = repair_now();
    let result = {
        let conn = match db_lock(state) {
            Ok(conn) => conn,
            Err(_) => {
                eprintln!("scheduler: db lock unavailable (poisoned or busy); tick skipped");
                return;
            }
        };
        let configured = autoconfigure_inventory(&conn, &inventory, &vram_gib, now);
        let upserted = upsert_dspine_engines(&conn, &engines, now);
        (configured, upserted)
    };
    match result {
        (Ok(summary), Ok(engines_upserted)) => eprintln!(
            "scheduler: autodiscover ok models_registered={} engines_upserted={engines_upserted} gpu_count={gpu_count} epoch={epoch}",
            summary.get("gguf_models_registered").and_then(serde_json::Value::as_u64).unwrap_or(0),
            gpu_count = vram_gib.len(),
            epoch = now_epoch(),
        ),
        (Err(e), _) => eprintln!("scheduler: autoconfigure failed: {e}"),
        (_, Err(e)) => eprintln!("scheduler: dspine upsert failed: {e}"),
    }
}

async fn repair_loop(state: AppState, secs: u64) {
    let mut tick = tokio::time::interval(Duration::from_secs(secs));
    tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
    tick.tick().await;
    loop {
        tick.tick().await;
        repair_tick(&state).await;
    }
}

async fn repair_tick(state: &AppState) {
    let observations = crate::repair_observations(state).await; // all awaits done before the lock
    let plan = build_repair_plan(&observations);
    if plan.findings.is_empty() {
        return;
    }
    let now = repair_now();
    // No .await while holding the guard.
    let outcome = {
        let conn = match db_lock(state) {
            Ok(conn) => conn,
            Err(_) => {
                eprintln!("scheduler: db lock unavailable; planning tick skipped");
                return;
            }
        };
        // repair_runs has no actor column; any open run (human- or
        // scheduler-created) blocks a new plan until it reaches a terminal
        // status. The audit table records who created what.
        let open: i64 = conn
            .query_row(OPEN_RUN_SQL, [], |row| row.get(0))
            .unwrap_or(0);
        if open > 0 {
            None
        } else {
            Some(create_run_from_plan(
                &conn,
                &plan,
                repair_policy(),
                now,
                SCHEDULER_ACTOR,
            ))
        }
    };
    match outcome {
        None => eprintln!("scheduler: {} findings but an open run exists; planning skipped", plan.findings.len()),
        Some(Ok(run)) => eprintln!(
            "scheduler: created run status={} (pending human approval; execution stays manual)",
            run.status.as_str()
        ),
        Some(Err(e)) => eprintln!("scheduler: create_run_from_plan failed: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dread_platform::repair_executor::RepairRunStatus;

    #[test]
    fn parse_disabled() {
        assert_eq!(parse_schedule_env(None), None);
        assert_eq!(parse_schedule_env(Some("0")), None);
        assert_eq!(parse_schedule_env(Some("nope")), None);
        assert_eq!(parse_schedule_env(Some("-3")), None);
        assert_eq!(parse_schedule_env(Some("1e9")), None);
    }

    #[test]
    fn parse_clamps() {
        assert_eq!(parse_schedule_env(Some("5")), Some(60));
        assert_eq!(parse_schedule_env(Some("999999")), Some(86400));
        assert_eq!(parse_schedule_env(Some("120")), Some(120));
        assert_eq!(parse_schedule_env(Some(" 45 ")), Some(60));
    }

    #[test]
    fn open_run_status_round_trip_and_dedupe() {
        // The SQL's status strings must be exactly the DB text the enum
        // writes; any drift here would silently disable dedupe.
        for status in [
            "planned",
            "awaiting_approval",
            "approved",
            "applying",
            "verified",
            "failed",
            "rolled_back",
            "rejected",
            "interrupted",
        ] {
            let parsed = RepairRunStatus::parse(status).unwrap_or_else(|| panic!("{status} must parse"));
            assert_eq!(parsed.as_str(), status);
        }
        let open = ["planned", "awaiting_approval", "approved", "applying"];
        let terminal = ["verified", "failed", "rolled_back", "rejected", "interrupted"];
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        dread_platform::repair_executor::init_repair_executor(&conn).unwrap();
        for status in open {
            conn.execute(
                "INSERT INTO repair_runs (id, status, created_at, updated_at) VALUES (1, ?1, 0, 0)",
                [status],
            )
            .unwrap();
            let count: i64 = conn.query_row(OPEN_RUN_SQL, [], |row| row.get(0)).unwrap();
            assert_eq!(count, 1, "{status} must count as open");
            conn.execute("DELETE FROM repair_runs WHERE id = 1", []).unwrap();
        }
        for status in terminal {
            conn.execute(
                "INSERT INTO repair_runs (id, status, created_at, updated_at) VALUES (1, ?1, 0, 0)",
                [status],
            )
            .unwrap();
            let count: i64 = conn.query_row(OPEN_RUN_SQL, [], |row| row.get(0)).unwrap();
            assert_eq!(count, 0, "{status} must be terminal");
            conn.execute("DELETE FROM repair_runs WHERE id = 1", []).unwrap();
        }
    }
}
