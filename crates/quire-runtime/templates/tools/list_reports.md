# Reports ({{ reports.len() }} / {{ total }})

{% if reports.is_empty() -%}
_No matching reports found._
{% else -%}
{% for report in reports -%}
- `{{ report.doc_id }}` — **{{ report.title }}** (rev {{ report.current_rev }}, {{ report.block_count }} blocks, updated {{ report.updated_at }}{% match report.session_id %}{% when Some with (sid) %}, session `{{ sid }}`{% when None %}{% endmatch %})
{% endfor -%}
{% endif %}
> Next: {{ next_hint }}
