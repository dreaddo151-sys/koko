-- Each API boot advances a generation. Recovery attachment is valid only
-- for the current boot, so a stale timestamp from a previous process cannot
-- accidentally open the startup dispatch barrier.
ALTER TABLE dread_task_attempts
    ADD COLUMN recovery_generation INTEGER NOT NULL DEFAULT 0
    CHECK (recovery_generation >= 0);

INSERT OR IGNORE INTO dread_execution_runtime_state(state_key, state_value, updated_at)
VALUES ('startup_recovery_generation', '0', 0);
