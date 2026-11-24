# Review Comments (`{{ doc_id }}`, {{ count }})

{% if comments.is_empty() -%}
_No matching review comments._
{% else -%}
{% for thread in comments -%}
- `{{ thread.thread_id }}` [{{ thread.status }}] @ `{{ thread.block_id }}`{% if thread.anchor_detached %} (detached){% endif %}{% match thread.quote %}{% when Some with (q) %} — "{{ q }}"{% when None %}{% endmatch %}: {{ thread.body }}{% if thread.reply_count > 0 %} ({{ thread.reply_count }} replies){% endif %}
{% endfor -%}
{% endif %}
> Next: {{ next_hint }}
