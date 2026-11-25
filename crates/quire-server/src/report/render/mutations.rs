use askama::Template;
use rmcp::ErrorData;

use crate::report::schema as contracts;

use super::{LintDiagnosticGroup, group_diagnostics, render_template};

#[derive(Template)]
#[template(path = "tools/upsert_sources.md", escape = "none")]
struct UpsertSourcesTemplate<'a> {
    doc_id: &'a str,
    revision: i64,
    changed: bool,
    source_count: usize,
    remapped: String,
    groups: Vec<LintDiagnosticGroup<'a>>,
    next_hint: &'a str,
}

impl contracts::UpsertSourcesReceipt {
    pub fn render_markdown(&self) -> Result<String, ErrorData> {
        let remapped = self
            .canonical_id_map
            .iter()
            .filter(|(from, to)| from != to)
            .map(|(from, to)| format!("{from}->{to}"))
            .collect::<Vec<_>>()
            .join(", ");
        let groups = group_diagnostics(&self.inline_warnings);
        render_template(&UpsertSourcesTemplate {
            doc_id: &self.doc_id,
            revision: self.revision,
            changed: self.changed,
            source_count: self.source_count,
            remapped,
            groups,
            next_hint: &self._next_hint,
        })
    }
}

pub fn render_upsert_sources(
    receipt: &contracts::UpsertSourcesReceipt,
) -> Result<String, ErrorData> {
    receipt.render_markdown()
}

#[derive(Template)]
#[template(path = "tools/delete_sources.md", escape = "none")]
struct DeleteSourcesTemplate<'a> {
    doc_id: &'a str,
    revision: i64,
    changed: bool,
    deleted_ids: &'a [String],
    deleted_ids_str: String,
    source_count: usize,
    next_hint: &'a str,
}

impl contracts::DeleteSourcesReceipt {
    pub fn render_markdown(&self) -> Result<String, ErrorData> {
        render_template(&DeleteSourcesTemplate {
            doc_id: &self.doc_id,
            revision: self.revision,
            changed: self.changed,
            deleted_ids: &self.deleted_ids,
            deleted_ids_str: self.deleted_ids.join(", "),
            source_count: self.source_count,
            next_hint: &self._next_hint,
        })
    }
}

pub fn render_delete_sources(
    receipt: &contracts::DeleteSourcesReceipt,
) -> Result<String, ErrorData> {
    receipt.render_markdown()
}

struct SourceRow<'a> {
    id: &'a str,
    title: &'a str,
    url: Option<&'a str>,
    date: Option<&'a str>,
    note: Option<&'a str>,
    excerpt: Option<&'a str>,
}

#[derive(Template)]
#[template(path = "tools/list_sources.md", escape = "none")]
struct ListSourcesTemplate<'a> {
    doc_id: &'a str,
    revision: i64,
    count: usize,
    sources: Vec<SourceRow<'a>>,
    next_hint: &'a str,
}

impl contracts::ListSourcesReceipt {
    pub fn render_markdown(&self) -> Result<String, ErrorData> {
        let sources = self
            .sources
            .iter()
            .map(|(id, entry)| SourceRow {
                id: id.as_str(),
                title: &entry.title,
                url: entry.url.as_deref(),
                date: entry.date.as_deref(),
                note: entry.note.as_deref(),
                excerpt: entry.excerpt.as_deref(),
            })
            .collect();
        render_template(&ListSourcesTemplate {
            doc_id: &self.doc_id,
            revision: self.revision,
            count: self.count,
            sources,
            next_hint: &self._next_hint,
        })
    }
}

pub fn render_list_sources(receipt: &contracts::ListSourcesReceipt) -> Result<String, ErrorData> {
    receipt.render_markdown()
}

#[derive(Template)]
#[template(path = "tools/apply_operations.md", escape = "none")]
struct ApplyOperationsTemplate<'a> {
    base_revision: i64,
    new_revision: i64,
    rebased: bool,
    rebased_over: usize,
    assigned_ids: String,
    error_groups: Vec<LintDiagnosticGroup<'a>>,
    warning_groups: Vec<LintDiagnosticGroup<'a>>,
    next_hint: &'a str,
}

impl contracts::ApplyOperationsReceipt {
    pub fn render_markdown(&self) -> Result<String, ErrorData> {
        let assigned_ids = self
            .assigned_ids
            .iter()
            .map(|a| {
                format!(
                    "{}->{}",
                    a.requested.as_ref().map_or("<auto>", |id| id.as_str()),
                    a.assigned
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        let error_groups = group_diagnostics(&self.blocking_errors);
        let warning_groups = group_diagnostics(&self.inline_warnings);
        render_template(&ApplyOperationsTemplate {
            base_revision: self.base_revision,
            new_revision: self.new_revision,
            rebased: self.rebased,
            rebased_over: self.rebased_over,
            assigned_ids,
            error_groups,
            warning_groups,
            next_hint: &self._next_hint,
        })
    }
}

pub fn render_apply_operations(
    receipt: &contracts::ApplyOperationsReceipt,
) -> Result<String, ErrorData> {
    receipt.render_markdown()
}
