OK — {{ doc_id }} rev={{ revision }} (changed={{ changed }}, deleted={{ deleted_ids.len() }}, remaining={{ source_count }})
{% if !deleted_ids.is_empty() -%}
Deleted: {{ deleted_ids_str }}
{% endif -%}
> Next: {{ next_hint }}
