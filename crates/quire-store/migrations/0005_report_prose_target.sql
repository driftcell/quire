-- Optional authoring agreement, not part of published cover text. Existing reports have no target.
ALTER TABLE report_documents ADD COLUMN target_prose_chars BIGINT
    CHECK (target_prose_chars BETWEEN 1 AND 4294967295);
