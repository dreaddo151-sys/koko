"""
PROJECT DREAD — persistent autonomous build engine foundation.

This module owns mission state and history. It does not execute shell commands
or modify project files. Execution is delegated to the existing agent adapter.
"""
from __future__ import annotations

import json
import os
import re
import sqlite3
import uuid
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

WORKSPACE = Path(__file__).resolve().parent
DATA_DIR = WORKSPACE / ".dread_build"
DB_PATH = DATA_DIR / "engine.sqlite3"

STATUSES = {
    "queued", "planning", "ready", "building", "testing",
    "repairing", "verifying", "completed", "blocked", "cancelled",
}


def _now() -> str:
    return datetime.now(timezone.utc).isoformat(timespec="seconds")


def _connect() -> sqlite3.Connection:
    DATA_DIR.mkdir(parents=True, exist_ok=True)
    db = sqlite3.connect(DB_PATH, timeout=20)
    db.row_factory = sqlite3.Row
    db.execute("PRAGMA foreign_keys=ON")
    db.execute("PRAGMA journal_mode=WAL")
    db.execute("PRAGMA busy_timeout=20000")
    return db


def _migrate(db: sqlite3.Connection) -> None:
    # Ensure the base table exists before attempting an ALTER (fresh DBs).
    db.execute(
        """CREATE TABLE IF NOT EXISTS missions (
            id TEXT PRIMARY KEY,
            objective TEXT NOT NULL,
            workspace TEXT NOT NULL,
            status TEXT NOT NULL,
            plan_json TEXT NOT NULL DEFAULT '[]',
            acceptance_json TEXT NOT NULL DEFAULT '[]',
            current_step TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            started_at TEXT,
            finished_at TEXT,
            last_error TEXT
        )""")
    cols = {r[1] for r in db.execute("PRAGMA table_info(missions)")}
    if "completion_report_json" not in cols:
        db.execute("ALTER TABLE missions ADD COLUMN completion_report_json TEXT")
    db.execute(
        """CREATE TABLE IF NOT EXISTS mission_checkpoints (
            mission_id TEXT PRIMARY KEY,
            phase TEXT NOT NULL,
            step_id TEXT,
            attempt INTEGER NOT NULL DEFAULT 0,
            detail_json TEXT NOT NULL DEFAULT '{}',
            updated_at TEXT NOT NULL
        )""")
    db.execute(
        """CREATE TABLE IF NOT EXISTS temporal_executions (
            idempotency_key TEXT PRIMARY KEY,
            mission_id TEXT NOT NULL,
            step_id TEXT,
            status TEXT NOT NULL,
            result_json TEXT NOT NULL DEFAULT '{}',
            error TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        )""")


def initialize() -> dict:
    with _connect() as db:
        _migrate(db)
        db.executescript("""
        CREATE TABLE IF NOT EXISTS missions (
            id TEXT PRIMARY KEY,
            objective TEXT NOT NULL,
            workspace TEXT NOT NULL,
            status TEXT NOT NULL,
            plan_json TEXT NOT NULL DEFAULT '[]',
            acceptance_json TEXT NOT NULL DEFAULT '[]',
            current_step TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            started_at TEXT,
            finished_at TEXT,
            last_error TEXT
        );

        CREATE TABLE IF NOT EXISTS mission_steps (
            id TEXT PRIMARY KEY,
            mission_id TEXT NOT NULL REFERENCES missions(id) ON DELETE CASCADE,
            position INTEGER NOT NULL,
            title TEXT NOT NULL,
            detail TEXT NOT NULL DEFAULT '',
            status TEXT NOT NULL DEFAULT 'pending',
            attempts INTEGER NOT NULL DEFAULT 0,
            max_repairs INTEGER NOT NULL DEFAULT 2,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            UNIQUE(mission_id, position)
        );

        CREATE TABLE IF NOT EXISTS mission_events (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            mission_id TEXT NOT NULL REFERENCES missions(id) ON DELETE CASCADE,
            event_type TEXT NOT NULL,
            message TEXT NOT NULL,
            data_json TEXT NOT NULL DEFAULT '{}',
            created_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS test_runs (
            id TEXT PRIMARY KEY,
            mission_id TEXT NOT NULL REFERENCES missions(id) ON DELETE CASCADE,
            step_id TEXT REFERENCES mission_steps(id) ON DELETE SET NULL,
            test_key TEXT NOT NULL,
            passed INTEGER NOT NULL,
            output TEXT NOT NULL DEFAULT '',
            repair_number INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_missions_updated
            ON missions(updated_at DESC);
        CREATE INDEX IF NOT EXISTS idx_events_mission
            ON mission_events(mission_id, id);
        CREATE INDEX IF NOT EXISTS idx_tests_mission_key
            ON test_runs(mission_id, test_key, created_at);
        """)
    return {"ok": True, "database": str(DB_PATH)}


def _event(db, mission_id: str, event_type: str, message: str,
           data: dict | None = None) -> None:
    db.execute(
        """INSERT INTO mission_events
           (mission_id,event_type,message,data_json,created_at)
           VALUES (?,?,?,?,?)""",
        (mission_id, event_type, message,
         json.dumps(data or {}, ensure_ascii=False), _now()),
    )


def create_mission(objective: str, workspace: str | None = None,
                   acceptance: list | None = None) -> dict:
    objective = (objective or "").strip()
    if not objective:
        raise ValueError("Mission objective is required")
    if len(objective) > 12000:
        raise ValueError("Mission objective is too long")

    root = Path(workspace or WORKSPACE).expanduser().resolve()
    if not root.exists() or not root.is_dir():
        raise ValueError("Workspace must be an existing directory")

    mission_id = "mis_" + uuid.uuid4().hex[:16]
    now = _now()
    acceptance = acceptance or []

    with _connect() as db:
        db.execute(
            """INSERT INTO missions
               (id,objective,workspace,status,acceptance_json,created_at,updated_at)
               VALUES (?,?,?,?,?,?,?)""",
            (mission_id, objective, str(root), "queued",
             json.dumps(acceptance, ensure_ascii=False), now, now),
        )
        _event(db, mission_id, "created", "Mission created", {
            "workspace": str(root)
        })

    return get_mission(mission_id)


def add_step(mission_id: str, title: str, detail: str = "",
             position: int | None = None, max_repairs: int = 2) -> dict:
    title = (title or "").strip()
    if not title:
        raise ValueError("Step title is required")
    if max_repairs < 0 or max_repairs > 2:
        raise ValueError("max_repairs must be between 0 and 2")

    step_id = "stp_" + uuid.uuid4().hex[:12]
    now = _now()

    with _connect() as db:
        if not db.execute("SELECT 1 FROM missions WHERE id=?", (mission_id,)).fetchone():
            raise KeyError("Mission not found")
        if position is None:
            position = db.execute(
                "SELECT COALESCE(MAX(position),0)+1 FROM mission_steps WHERE mission_id=?",
                (mission_id,),
            ).fetchone()[0]
        db.execute(
            """INSERT INTO mission_steps
               (id,mission_id,position,title,detail,max_repairs,created_at,updated_at)
               VALUES (?,?,?,?,?,?,?,?)""",
            (step_id, mission_id, position, title, detail,
             max_repairs, now, now),
        )
        db.execute("UPDATE missions SET updated_at=? WHERE id=?", (now, mission_id))
        _event(db, mission_id, "step_added", title, {"step_id": step_id})

    return get_step(step_id)


def set_status(mission_id: str, status: str, message: str = "",
               error: str | None = None) -> dict:
    if status not in STATUSES:
        raise ValueError("Invalid mission status")
    now = _now()

    with _connect() as db:
        row = db.execute("SELECT status FROM missions WHERE id=?", (mission_id,)).fetchone()
        if not row:
            raise KeyError("Mission not found")

        started = now if status in {"planning", "building"} else None
        finished = now if status in {"completed", "cancelled"} else None
        db.execute(
            """UPDATE missions SET status=?,updated_at=?,
               started_at=COALESCE(started_at,?),
               finished_at=?,last_error=? WHERE id=?""",
            (status, now, started, finished, error, mission_id),
        )
        _event(db, mission_id, "status", message or f"Status: {status}", {
            "from": row["status"], "to": status, "error": error
        })

    return get_mission(mission_id)


def record_test(mission_id: str, test_key: str, passed: bool,
                output: str = "", step_id: str | None = None) -> dict:
    """Record a test. A failing test permits at most two repair attempts."""
    if not test_key or len(test_key) > 300:
        raise ValueError("A test_key of 1–300 characters is required")

    test_id = "tst_" + uuid.uuid4().hex[:12]
    now = _now()

    with _connect() as db:
        if not db.execute("SELECT 1 FROM missions WHERE id=?", (mission_id,)).fetchone():
            raise KeyError("Mission not found")

        if step_id and not db.execute(
            "SELECT 1 FROM mission_steps WHERE id=? AND mission_id=?",
            (step_id, mission_id),
        ).fetchone():
            raise ValueError("Step does not belong to this mission")

        failures = db.execute(
            """SELECT COUNT(*) FROM test_runs
               WHERE mission_id=? AND test_key=? AND passed=0""",
            (mission_id, test_key),
        ).fetchone()[0]

        repair_number = failures if not passed else 0
        repair_allowed = bool(not passed and repair_number < 2)

        db.execute(
            """INSERT INTO test_runs
               (id,mission_id,step_id,test_key,passed,output,repair_number,created_at)
               VALUES (?,?,?,?,?,?,?,?)""",
            (test_id, mission_id, step_id, test_key, int(bool(passed)),
             (output or "")[-30000:], repair_number, now),
        )

        if step_id:
            db.execute(
                "UPDATE mission_steps SET updated_at=? WHERE id=?",
                (now, step_id),
            )

        _event(db, mission_id, "test_passed" if passed else "test_failed",
               f"{test_key}: {'passed' if passed else 'failed'}", {
                   "test_id": test_id, "step_id": step_id,
                   "repair_number": repair_number,
                   "repair_allowed": repair_allowed,
                   "output_tail": (output or "")[-2000:],
               })
        db.execute("UPDATE missions SET updated_at=? WHERE id=?", (now, mission_id))

    return {
        "id": test_id, "test_key": test_key, "passed": bool(passed),
        "repair_number": repair_number, "repair_allowed": repair_allowed,
        "max_repairs": 2,
    }


COMPLETION_REPORT_SCHEMA = {
    "version": 1,
    "required": {
        "schema_version": int,
        "mission_id": str,
        "status": str,
        "generated_at": str,
        "acceptance": list,
        "tests": list,
        "repairs": list,
        "evidence_verified": bool,
    },
    "statuses": {"completed", "blocked"},
}


def build_completion_report(mission_id: str) -> dict:
    """Build the structured JSON completion report for a mission.

    The report is derived ONLY from engine state (tests, acceptance,
    repair counts) — never from the agent's free-text response.
    """
    mission = get_mission(mission_id)
    if not mission:
        raise KeyError("Mission not found")

    evidence = verify_acceptance_evidence(mission_id)
    tests = mission.get("tests") or []

    def _repair_allowed(t: dict) -> bool:
        # repair_allowed is derived from the persisted failure counter:
        # a failing test may be repaired while it has < 2 prior failures.
        if t.get("passed"):
            return False
        return int(t.get("repair_number", 0)) < 2

    repairs = [
        {
            "test_key": t["test_key"],
            "repair_number": t["repair_number"],
            "repair_allowed": _repair_allowed(t),
            "passed": bool(t["passed"]),
        }
        for t in tests
        if not t["passed"]
    ]
    report = {
        "schema_version": COMPLETION_REPORT_SCHEMA["version"],
        "mission_id": mission_id,
        "status": "completed" if (
            mission["status"] == "completed" and evidence["verified"]
        ) else "blocked",
        "generated_at": _now(),
        "acceptance": evidence["criteria"],
        "tests": [
            {
                "id": t["id"],
                "test_key": t["test_key"],
                "passed": bool(t["passed"]),
                "repair_number": t["repair_number"],
                "repair_allowed": _repair_allowed(t),
            }
            for t in tests
        ],
        "repairs": repairs,
        "evidence_verified": evidence["verified"],
    }
    validate_completion_report(report, mission_id=mission_id)
    return report


def validate_completion_report(report: dict, mission_id: str | None = None) -> dict:
    """Strictly validate a completion report. Raises ValueError on any
    violation. Returns the report when valid (fail closed)."""
    if not isinstance(report, dict):
        raise ValueError("Completion report must be a JSON object")

    schema = COMPLETION_REPORT_SCHEMA
    if report.get("schema_version") != schema["version"]:
        raise ValueError(
            f"Unsupported schema_version: {report.get('schema_version')!r} "
            f"(expected {schema['version']})"
        )

    for field, ftype in schema["required"].items():
        if field not in report:
            raise ValueError(f"Missing required field: {field}")
        value = report[field]
        if ftype is bool:
            if not isinstance(value, bool):
                raise ValueError(f"Field {field} must be a boolean")
        elif not isinstance(value, ftype) or isinstance(value, bool) and ftype is not bool:
            raise ValueError(f"Field {field} must be of type {ftype.__name__}")

    if report["status"] not in schema["statuses"]:
        raise ValueError(f"Invalid status: {report['status']!r}")

    if mission_id is not None and report["mission_id"] != mission_id:
        raise ValueError(
            f"Report mission_id {report['mission_id']!r} does not match "
            f"{mission_id!r}"
        )

    for i, crit in enumerate(report["acceptance"]):
        if not isinstance(crit, dict) or "criterion" not in crit                 or "satisfied" not in crit:
            raise ValueError(f"acceptance[{i}] must carry criterion+satisfied")
        if not isinstance(crit["satisfied"], bool):
            raise ValueError(f"acceptance[{i}].satisfied must be a boolean")

    for i, t in enumerate(report["tests"]):
        for key in ("id", "test_key", "passed", "repair_number", "repair_allowed"):
            if key not in t:
                raise ValueError(f"tests[{i}] missing {key}")
        if not isinstance(t["passed"], bool) or not isinstance(t["repair_allowed"], bool):
            raise ValueError(f"tests[{i}] booleans must be real booleans")
        if not isinstance(t["repair_number"], int):
            raise ValueError(f"tests[{i}].repair_number must be an integer")

    # Cross-field consistency: completed status requires verified evidence
    if report["status"] == "completed" and not report["evidence_verified"]:
        raise ValueError("status=completed requires evidence_verified=true")
    if report["evidence_verified"] and any(
            not c["satisfied"] for c in report["acceptance"]):
        raise ValueError("evidence_verified=true requires all criteria satisfied")

    return report


def save_completion_report(mission_id: str) -> dict:
    """Build, strictly validate and persist the completion report."""
    report = build_completion_report(mission_id)
    now = _now()
    with _connect() as db:
        if not db.execute("SELECT 1 FROM missions WHERE id=?", (mission_id,)).fetchone():
            raise KeyError("Mission not found")
        db.execute(
            "UPDATE missions SET completion_report_json=?, updated_at=? WHERE id=?",
            (json.dumps(report, ensure_ascii=False), now, mission_id),
        )
        _event(db, mission_id, "completion_report",
               f"Completion report saved (status={report['status']})",
               {"evidence_verified": report["evidence_verified"]})
    return report


def get_completion_report(mission_id: str) -> dict | None:
    with _connect() as db:
        row = db.execute(
            "SELECT completion_report_json FROM missions WHERE id=?",
            (mission_id,),
        ).fetchone()
    if not row or not row["completion_report_json"]:
        return None
    report = json.loads(row["completion_report_json"])
    validate_completion_report(report, mission_id=mission_id)
    return report


def verify_acceptance_evidence(mission_id: str) -> dict:
    """Independently verify that every acceptance criterion has recorded
    passing test evidence.

    This does NOT trust the agent's self-reported completion marker. It only
    inspects the test_runs table: a criterion is satisfied only when a
    recorded test whose key references the criterion (or the criterion text
    references the test key) passed.

    Returns:
        {
          "verified": bool,
          "criteria": [
             {"criterion": str, "satisfied": bool, "evidence_test_id": str|None,
              "evidence_test_key": str|None}
          ],
          "unsatisfied": [str, ...]
        }
    """
    mission = get_mission(mission_id)
    if not mission:
        raise KeyError("Mission not found")

    criteria = mission.get("acceptance") or []
    tests = mission.get("tests") or []
    passed_tests = [t for t in tests if t.get("passed")]

    def _norm(s: str) -> str:
        return re.sub(r"[^a-z0-9]+", " ", (s or "").lower()).strip()

    def _tokens(s: str) -> set:
        return {t for t in _norm(s).split() if len(t) >= 3}

    def _match(criterion: str, test: dict) -> bool:
        key = test.get("test_key") or ""
        c_norm, k_norm = _norm(criterion), _norm(key)
        if not c_norm or not k_norm:
            return False
        if c_norm in k_norm or k_norm in c_norm:
            return True
        c_tok, k_tok = _tokens(criterion), _tokens(key)
        if not c_tok:
            return False
        overlap = c_tok & k_tok
        return len(overlap) >= max(2, int(0.5 * len(c_tok)))

    result_criteria = []
    unsatisfied = []
    for criterion in criteria:
        evidence = None
        for test in passed_tests:
            if _match(criterion, test):
                evidence = test
                break
        satisfied = evidence is not None
        result_criteria.append({
            "criterion": criterion,
            "satisfied": satisfied,
            "evidence_test_id": evidence["id"] if evidence else None,
            "evidence_test_key": evidence["test_key"] if evidence else None,
        })
        if not satisfied:
            unsatisfied.append(criterion)

    verified = bool(criteria) and not unsatisfied
    return {
        "verified": verified,
        "criteria": result_criteria,
        "unsatisfied": unsatisfied,
    }


def get_step(step_id: str) -> dict | None:
    with _connect() as db:
        row = db.execute("SELECT * FROM mission_steps WHERE id=?", (step_id,)).fetchone()
        return dict(row) if row else None


def get_mission(mission_id: str) -> dict | None:
    with _connect() as db:
        row = db.execute("SELECT * FROM missions WHERE id=?", (mission_id,)).fetchone()
        if not row:
            return None
        result = dict(row)
        result["plan"] = json.loads(result.pop("plan_json"))
        result["acceptance"] = json.loads(result.pop("acceptance_json"))
        result["steps"] = [
            dict(x) for x in db.execute(
                "SELECT * FROM mission_steps WHERE mission_id=? ORDER BY position",
                (mission_id,),
            ).fetchall()
        ]
        result["tests"] = [
            dict(x) for x in db.execute(
                "SELECT * FROM test_runs WHERE mission_id=? ORDER BY created_at",
                (mission_id,),
            ).fetchall()
        ]
        return result


def list_missions(limit: int = 50) -> list[dict]:
    limit = max(1, min(int(limit), 200))
    with _connect() as db:
        rows = db.execute(
            """SELECT id,objective,workspace,status,current_step,
                      created_at,updated_at,started_at,finished_at,last_error
               FROM missions ORDER BY updated_at DESC LIMIT ?""",
            (limit,),
        ).fetchall()
        return [dict(row) for row in rows]


def save_checkpoint(mission_id: str, phase: str, step_id: str | None = None,
                    attempt: int = 0, detail: dict | None = None) -> dict:
    """Point 2 — persist the mission checkpoint so a crashed inference
    request can be resumed without re-running completed work."""
    if not phase or len(phase) > 100:
        raise ValueError("A phase of 1-100 characters is required")
    with _connect() as db:
        row = db.execute("SELECT id FROM missions WHERE id=?", (mission_id,)).fetchone()
        if not row:
            raise KeyError("Mission not found")
        db.execute(
            """INSERT INTO mission_checkpoints
               (mission_id, phase, step_id, attempt, detail_json, updated_at)
               VALUES (?,?,?,?,?,?)
               ON CONFLICT(mission_id) DO UPDATE SET
                 phase=excluded.phase, step_id=excluded.step_id,
                 attempt=excluded.attempt, detail_json=excluded.detail_json,
                 updated_at=excluded.updated_at""",
            (mission_id, phase, step_id, int(attempt),
             json.dumps(detail or {}, ensure_ascii=False), _now()),
        )
    return get_checkpoint(mission_id)


def get_checkpoint(mission_id: str) -> dict | None:
    with _connect() as db:
        row = db.execute(
            "SELECT * FROM mission_checkpoints WHERE mission_id=?",
            (mission_id,)).fetchone()
    if not row:
        return None
    data = dict(row)
    data["detail"] = json.loads(data.pop("detail_json") or "{}")
    return data


def clear_checkpoint(mission_id: str) -> None:
    with _connect() as db:
        db.execute("DELETE FROM mission_checkpoints WHERE mission_id=?",
                   (mission_id,))


def record_execution_start(idempotency_key: str, mission_id: str,
                           step_id: str | None = None) -> dict:
    """Durable record that a bounded step execution has begun.
    Idempotent: a second call with the same key is a no-op (returns existing)."""
    now = _now()
    with _connect() as db:
        existing = db.execute(
            "SELECT * FROM temporal_executions WHERE idempotency_key=?",
            (idempotency_key,)).fetchone()
        if existing:
            return dict(existing)
        db.execute(
            """INSERT INTO temporal_executions
               (idempotency_key, mission_id, step_id, status, created_at, updated_at)
               VALUES (?,?,?,?,?,?)""",
            (idempotency_key, mission_id, step_id, "running", now, now),
        )
    return get_execution(idempotency_key)


def record_execution_complete(idempotency_key: str, result: dict | None = None) -> dict:
    now = _now()
    with _connect() as db:
        row = db.execute(
            "SELECT 1 FROM temporal_executions WHERE idempotency_key=?",
            (idempotency_key,)).fetchone()
        if not row:
            raise KeyError("Execution not found: " + idempotency_key)
        db.execute(
            """UPDATE temporal_executions
               SET status='completed', result_json=?, updated_at=?
               WHERE idempotency_key=?""",
            (json.dumps(result or {}, ensure_ascii=False), now, idempotency_key),
        )
    return get_execution(idempotency_key)


def record_execution_failed(idempotency_key: str, error: str) -> dict:
    now = _now()
    with _connect() as db:
        row = db.execute(
            "SELECT 1 FROM temporal_executions WHERE idempotency_key=?",
            (idempotency_key,)).fetchone()
        if not row:
            raise KeyError("Execution not found: " + idempotency_key)
        db.execute(
            """UPDATE temporal_executions
               SET status='failed', error=?, updated_at=?
               WHERE idempotency_key=?""",
            (error[:3000], now, idempotency_key),
        )
    return get_execution(idempotency_key)


def get_execution(idempotency_key: str) -> dict | None:
    with _connect() as db:
        row = db.execute(
            "SELECT * FROM temporal_executions WHERE idempotency_key=?",
            (idempotency_key,)).fetchone()
    if not row:
        return None
    data = dict(row)
    data["result"] = json.loads(data.pop("result_json") or "{}")
    return data


def get_events(mission_id: str, after: int = 0, limit: int = 200) -> list[dict]:
    limit = max(1, min(int(limit), 1000))
    with _connect() as db:
        if not db.execute("SELECT 1 FROM missions WHERE id=?", (mission_id,)).fetchone():
            raise KeyError("Mission not found")
        rows = db.execute(
            """SELECT * FROM mission_events
               WHERE mission_id=? AND id>? ORDER BY id LIMIT ?""",
            (mission_id, max(0, int(after)), limit),
        ).fetchall()
        result = []
        for row in rows:
            item = dict(row)
            item["data"] = json.loads(item.pop("data_json"))
            result.append(item)
        return result


if __name__ == "__main__":
    print(json.dumps(initialize(), indent=2))
