{% if clean -%}
OK — `rev: {{ rev }}`, 0 diagnostics.
{% else -%}
# Lint Report (`rev: {{ rev }}`, ok={{ ok }})

Errors: {{ errors }} | Warnings: {{ warnings }} | Info: {{ infos }} | Suppressed: {{ suppressed }}

{% if !diagnostics.is_empty() -%}
## Diagnostics
{% for d in diagnostics -%}
- [{{ d.severity }}] `{{ d.code }}` ({{ d.rule }}){% match d.block_id %}{% when Some with (bid) %} @ `{{ bid }}`{% when None %}{% endmatch %}: {{ d.message }}{% if !d.hint.is_empty() %} — *Fix:* {{ d.hint }}{% endif %}{% if !d.valid_values.is_empty() %} (valid: {{ d.valid_values }}){% endif %}
{% endfor -%}
{% endif -%}
{% if !suppressed_blocks.is_empty() -%}
## Suppressed Blocks (`lint_ignore`)
{% for sb in suppressed_blocks -%}
- `{{ sb.block_id }}`: {{ sb.codes }}
{% endfor -%}
{% endif -%}
{% match next_hint %}{% when Some with (hint) %}
> Next: {{ hint }}
{% when None %}{% endmatch -%}
{% endif -%}
