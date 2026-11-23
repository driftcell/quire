Register or incrementally update citation entries in `doc_id` with automatic URL and normalized-title deduplication.
- First registration of a new `id` requires a non-empty `title` and should include a verified `excerpt` (verbatim facts/numbers fetched from the primary source or database), `publisher`, `date`, and `url`.
- Incremental update: To enrich an existing source `id` with additional `excerpt` data or `note`, pass `id` and the updated fields (`title` may be omitted for existing IDs).
- Receipt: Returns `canonical_id_map` (always cite the mapped canonical `source_id` in prose as `[^source_id]` and in visual block `source_ids`), `revision`, and `changed`.
