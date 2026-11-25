{% if clean -%}
OK — `rev: {{ rev }}`, 0 diagnostics.
{% else -%}
# Lint Report (`rev: {{ rev }}`, ok={{ ok }})

Errors: {{ errors }} | Warnings: {{ warnings }} | Info: {{ infos }} | Suppressed: {{ suppressed }}

{% if !groups.is_empty() -%}
## Diagnostics
{% for g in groups -%}
{% if g.items.len() == 1 -%}
{% for d in g.items -%}
- [{{ g.severity }}] `{{ g.code }}` ({{ g.rule }}){% match d.block_id %}{% when Some with (bid) %} @ `{{ bid }}`{% when None %}{% endmatch %}: {{ d.message }}{% if !d.hint.is_empty() %} — *Fix:* {{ d.hint }}{% endif %}{% if !g.shared_valid_values.is_empty() %} (valid: {{ g.shared_valid_values }}){% else if !d.valid_values.is_empty() %} (valid: {{ d.valid_values }}){% endif %}
{% endfor -%}
{% else -%}
- [{{ g.severity }}] `{{ g.code }}` ({{ g.rule }}) × {{ g.items.len() }}{% if !g.shared_hint.is_empty() %} — *Fix:* {{ g.shared_hint }}{% endif %}{% if !g.shared_valid_values.is_empty() %} (valid: {{ g.shared_valid_values }}){% endif %}
{% for d in g.items -%}
  - {% match d.block_id %}{% when Some with (bid) %}@ `{{ bid }}`: {% when None %}{% endmatch %}{{ d.message }}{% if g.shared_hint.is_empty() && !d.hint.is_empty() %} — *Fix:* {{ d.hint }}{% endif %}
{% endfor -%}
{% endif -%}
{% endfor -%}
{% endif -%}
{% if !suppressed_groups.is_empty() -%}
## Suppressed Rules (`lint_ignore`)
{% for sg in suppressed_groups -%}
- `{{ sg.code }}`{% if !sg.rule.is_empty() %} ({{ sg.rule }}){% endif %} × {{ sg.count }}: {{ sg.block_ids }}
{% endfor -%}
{% endif -%}
{% match next_hint %}{% when Some with (hint) %}
> Next: {{ hint }}
{% when None %}{% endmatch -%}
{% endif -%}
