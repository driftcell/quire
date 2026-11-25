OK — {{ doc_id }} rev={{ revision }} (changed={{ changed }}, total={{ source_count }})
{% if !remapped.is_empty() -%}
Mapped: {{ remapped }}
{% endif -%}
{% if !groups.is_empty() -%}
## Warnings
{% for g in groups -%}
{% if g.items.len() == 1 -%}
{% for d in g.items -%}
- [{{ g.severity }}] {{ g.code }} ({{ g.rule }}){% match d.block_id %}{% when Some with (bid) %} @{{ bid }}{% when None %}{% endmatch %}: {{ d.message }}{% if !d.hint.is_empty() %} — Fix: {{ d.hint }}{% endif %}
{% endfor -%}
{% else -%}
- [{{ g.severity }}] {{ g.code }} ({{ g.rule }}) × {{ g.items.len() }}{% if !g.shared_hint.is_empty() %} — Fix: {{ g.shared_hint }}{% endif %}
{% for d in g.items -%}
- {% match d.block_id %}{% when Some with (bid) %}@{{ bid }}: {% when None %}{% endmatch %}{{ d.message }}
{% endfor -%}
{% endif -%}
{% endfor -%}
{% endif -%}
> Next: {{ next_hint }}
