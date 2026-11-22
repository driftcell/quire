Compact the conversation above into a structured handoff for a fresh context window.
Do not call tools. Populate every schema field with concise, information-dense content:

- task_and_goal: the user's objective and current task.
- confirmed_scope_and_metrics: confirmed scope, data sources, calibers, IDs, numbers and dates.
- document_state: bound doc_id, completed sections, source_ids and current artifact state.
- next_steps: open comments, unresolved questions and the next concrete actions.

Preserve concrete facts and decisions. Do not invent missing information; use an empty string only when a field has no support in the conversation.
