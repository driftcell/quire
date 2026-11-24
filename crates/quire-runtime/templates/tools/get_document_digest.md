# {{ title }} (`rev: {{ current_rev }}`)

- Blocks: {{ block_count }} | Paragraphs: {{ paragraph_count }} ({{ prose_char_count }} chars{% match target_prose_chars %}{% when Some with (target) %} / target {{ target }}{% match prose_char_delta %}{% when Some with (delta) %}, delta {{ delta }}{% when None %}{% endmatch %}{% when None %}{% endmatch %}) | Visuals: {{ visual_block_count }} | Sources: {{ cited_source_count }} cited / {{ source_count }} registered
- Theme preset: `{{ theme_preset }}` | TOC: {% if toc_enabled %}enabled (max_depth={{ toc_max_depth }}){% else %}disabled{% endif %}

{% if blocks.is_empty() -%}
_No matching blocks._
{% else -%}
## Blocks
{% for block in blocks -%}
- `{{ block.id }}` [{{ block.block_type }}{% match block.level %}{% when Some with (lvl) %} H{{ lvl }}{% when None %}{% endmatch %}]{% match block.char_count %}{% when Some with (chars) %} ({{ chars }} chars){% when None %}{% endmatch %}{% match block.preview %}{% when Some with (text) %}: {{ text }}{% when None %}{% endmatch %}{% if !block.source_ids.is_empty() %} [sources: {{ block.source_ids }}]{% endif %}
{% endfor -%}
{% endif %}
> Next: {{ next_hint }}
