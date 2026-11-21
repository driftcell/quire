-- Sequence cursor for active-run hydration through the (session_id, seq) primary key.
ALTER TABLE agent_runs ADD COLUMN IF NOT EXISTS start_seq BIGINT;

-- Backfill existing runs from their persisted turn_started event.
UPDATE agent_runs
SET start_seq = starts.min_seq
FROM (
    SELECT run_id, MIN(seq) AS min_seq
    FROM session_events
    WHERE event_type = 'turn_started'
    GROUP BY run_id
) AS starts
WHERE agent_runs.run_id = starts.run_id
  AND agent_runs.start_seq IS NULL;
