# Sources ({{ doc_id }}, rev={{ revision }}, {{ count }})
{% if sources.is_empty() -%}
_No sources registered._
{% else -%}
{% for s in sources -%}
- [^{{ s.id }}] {{ s.title }}{% match s.url %}{% when Some with (u) %} | {{ u }}{% when None %}{% endmatch %}{% match s.date %}{% when Some with (d) %} | {{ d }}{% when None %}{% endmatch %}{% match s.note %}{% when Some with (n) %} | note: {{ n }}{% when None %}{% endmatch %}{% match s.excerpt %}{% when Some with (e) %} | excerpt: {{ e }}{% when None %}{% endmatch %}
{% endfor -%}
{% endif %}
> Next: {{ next_hint }}
