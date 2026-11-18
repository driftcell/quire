-- Schema hardening requested by the runtime contract:
--  * unbounded TEXT ranks with a deferrable UNIQUE ordering constraint
--  * materialized affected_block_ids for Smart CAS conflict detection
--  * remove the redundant revision-time index

ALTER TABLE report_blocks
    ALTER COLUMN rank TYPE TEXT COLLATE "C";

ALTER TABLE report_blocks
    DROP CONSTRAINT IF EXISTS idx_report_blocks_order;
DROP INDEX IF EXISTS idx_report_blocks_order;

-- A UNIQUE constraint (rather than CREATE UNIQUE INDEX) is used because PostgreSQL only
-- permits DEFERRABLE uniqueness through a constraint; it creates the backing unique index with
-- the same name.
ALTER TABLE report_blocks
    ADD CONSTRAINT idx_report_blocks_order
    UNIQUE (doc_id, rank)
    DEFERRABLE INITIALLY DEFERRED;

ALTER TABLE report_revisions
    ADD COLUMN IF NOT EXISTS affected_block_ids TEXT[] NOT NULL DEFAULT '{}';

DROP INDEX IF EXISTS idx_report_revisions_time;
