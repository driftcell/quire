OK — rev {{ base_revision }} -> {{ new_revision }}{% if rebased %} (rebased over {{ rebased_over }}){% endif %}
{% if !assigned_ids.is_empty() -%}
Assigned: {{ assigned_ids }}
{% endif -%}
{% if !error_groups.is_empty() -%}
## Blocking Errors
{% for g in error_groups -%}
{% if g.items.len() == 1 -%}
{% for d in g.items -%}
- [{{ g.severity }}] {{ g.code }} ({{ g.rule }}){% match d.block_id %}{% when Some with (bid) %} @{{ bid }}{% when None %}{% endmatch %}: {{ d.message }}{% if !d.hint.is_empty() %} — Fix: {{ d.hint }}{% endif %}{% if !g.shared_valid_values.is_empty() %} (valid: {{ g.shared_valid_values }}){% else if !d.valid_values.is_empty() %} (valid: {{ d.valid_values }}){% endif %}
{% endfor -%}
{% else -%}
- [{{ g.severity }}] {{ g.code }} ({{ g.rule }}) × {{ g.items.len() }}{% if !g.shared_hint.is_empty() %} — Fix: {{ g.shared_hint }}{% endif %}{% if !g.shared_valid_values.is_empty() %} (valid: {{ g.shared_valid_values }}){% endif %}
{% for d in g.items -%}
  - {% match d.block_id %}{% when Some with (bid) %}@{{ bid }}: {% when None %}{% endmatch %}{{ d.message }}{% if g.shared_hint.is_empty() && !d.hint.is_empty() %} — Fix: {{ d.hint }}{% endif %}
{% endfor -%}
{% endif -%}
{% endfor -%}
{% endif -%}
{% if !warning_groups.is_empty() -%}
## Inline Warnings
{% for g in warning_groups -%}
{% if g.items.len() == 1 -%}
{% for d in g.items -%}
- [{{ g.severity }}] {{ g.code }} ({{ g.rule }}){% match d.block_id %}{% when Some with (bid) %} @{{ bid }}{% when None %}{% endmatch %}: {{ d.message }}{% if !d.hint.is_empty() %} — Fix: {{ d.hint }}{% endif %}{% if !g.shared_valid_values.is_empty() %} (valid: {{ g.shared_valid_values }}){% else if !d.valid_values.is_empty() %} (valid: {{ d.valid_values }}){% endif %}
{% endfor -%}
{% else -%}
- [{{ g.severity }}] {{ g.code }} ({{ g.rule }}) × {{ g.items.len() }}{% if !g.shared_hint.is_empty() %} — Fix: {{ g.shared_hint }}{% endif %}{% if !g.shared_valid_values.is_empty() %} (valid: {{ g.shared_valid_values }}){% endif %}
{% for d in g.items -%}
  - {% match d.block_id %}{% when Some with (bid) %}@{{ bid }}: {% when None %}{% endmatch %}{{ d.message }}{% if g.shared_hint.is_empty() && !d.hint.is_empty() %} — Fix: {{ d.hint }}{% endif %}
{% endfor -%}
{% endif -%}
{% endfor -%}
{% endif -%}
> Next: {{ next_hint }}
