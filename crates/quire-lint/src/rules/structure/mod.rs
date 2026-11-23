//! Structure and text-quality rules: placeholders, heading flow, visual captions and references.

use std::collections::{BTreeMap, BTreeSet};

use quire_protocol::{BlockKind, NumberingKind, ReportDocument};

use crate::{
    context::LintContext,
    engine::{Engine, Finding},
    registry::RuleId,
};

mod heading_text;
mod headings;
mod notes_syntax;
mod number_prefix;
mod placeholders;
mod visual_refs;

pub(crate) use heading_text::{action_or_closing_section_exempt, dense_prose_section_exempt};
use number_prefix::*;

pub(crate) fn check(context: &LintContext<'_>) -> Vec<Finding> {
    let mut engine = Engine::new();
    let document = context.document;
    if document.blocks.is_empty() {
        engine.report(
            RuleId::EmptyDocument,
            "document has no blocks",
            "use apply_operations with Append or InsertAfter to add content",
            None,
            Some("blocks".into()),
        );
    }
    headings::check_headings(document, &mut engine);
    placeholders::check_empty_text(document, &mut engine);
    placeholders::check_placeholders(document, &mut engine);
    visual_refs::check_visual_titles(document, &mut engine);
    visual_refs::check_visual_references(document, &mut engine);
    notes_syntax::check_inline_syntax(document, &mut engine);
    visual_refs::check_consecutive_visuals(document, &mut engine);
    heading_text::check_boring_headings(document, &mut engine);
    notes_syntax::check_duplicate_note_prefix(document, &mut engine);
    notes_syntax::check_duplicate_source_prefix(document, &mut engine);
    engine.into_findings()
}
