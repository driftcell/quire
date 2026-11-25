# {{ title }} (rev={{ current_rev }})
Blocks: {{ block_count }} | Paragraphs: {{ paragraph_count }} ({{ prose_char_count }}c{% match target_prose_chars %}{% when Some with (target) %} / target {{ target }}c{% match prose_char_delta %}{% when Some with (delta) %}, delta {{ delta }}{% when None %}{% endmatch %}{% when None %}{% endmatch %}) | Visuals: {{ visual_block_count }} | Sources: {{ cited_source_count }}/{{ source_count }}
Theme: {{ theme_preset }} | TOC: {% if toc_enabled %}on(d={{ toc_max_depth }}){% else %}off{% endif %}

{% if blocks.is_empty() -%}
_No matching blocks._
{% else -%}
## Blocks
{% for block in blocks -%}
- {{ block.id }} ({{ block.short_type }}{% match block.char_count %}{% when Some with (chars) %}, {{ chars }}c{% when None %}{% endmatch %}){% match block.preview %}{% when Some with (text) %}: {{ text }}{% when None %}{% endmatch %}{% if !block.source_ids.is_empty() %} {{ block.source_ids }}{% endif %}
{% endfor -%}
{% endif %}
> Next: {{ next_hint }}
