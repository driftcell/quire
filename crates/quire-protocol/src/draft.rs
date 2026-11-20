//! Offline sub-agent draft documents.
//!
//! Drafts are protocol JSON without a server-side revision. They are parsed in-memory by the
//! protocol crate; file I/O lives in the server crate.

use indexmap::IndexMap;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    block::Block,
    ids::{DocId, Revision, SourceId},
    model::{CoverConfig, ReferencesConfig, ReportDocument, SourceEntry, ThemeConfig, TocConfig},
};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ReportDraft {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub doc_id: Option<DocId>,
    /// Convenience title for agents; copied into `cover.title` when that field is empty.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default)]
    pub cover: CoverConfig,
    #[serde(default)]
    pub toc: TocConfig,
    #[serde(default)]
    pub theme: ThemeConfig,
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    #[schemars(with = "std::collections::BTreeMap<String, SourceEntry>")]
    pub sources: IndexMap<SourceId, SourceEntry>,
    #[serde(default)]
    pub blocks: Vec<Block>,
    #[serde(default)]
    pub references: ReferencesConfig,
    #[serde(default)]
    pub lint: crate::LintConfig,
}

#[derive(Debug, Error)]
pub enum DraftError {
    #[error("invalid draft JSON: {source}")]
    InvalidJson {
        #[from]
        source: serde_json::Error,
    },
    #[error("draft has no doc_id and no fallback doc_id was supplied")]
    MissingDocId,
    #[error("draft has no title; supply a root `title` or `cover.title`")]
    MissingTitle,
}

impl ReportDraft {
    pub fn parse(json: &str) -> Result<Self, DraftError> {
        Ok(serde_json::from_str(json)?)
    }

    pub fn into_document(
        mut self,
        fallback_doc_id: Option<&str>,
    ) -> Result<ReportDocument, DraftError> {
        let doc_id = self
            .doc_id
            .take()
            .or_else(|| fallback_doc_id.and_then(|value| DocId::parse(value).ok()))
            .ok_or(DraftError::MissingDocId)?;
        if self.cover.title.trim().is_empty()
            && let Some(title) = self.title.take()
        {
            self.cover.title = title;
        }
        if self.cover.title.trim().is_empty() {
            return Err(DraftError::MissingTitle);
        }
        Ok(ReportDocument {
            doc_id,
            session_id: None,
            revision: Revision::ZERO,
            cover: self.cover,
            toc: self.toc,
            theme: self.theme,
            sources: self.sources,
            blocks: self.blocks,
            references: self.references,
            lint: self.lint,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::*;

    #[test]
    fn draft_parses_full_document_shape() {
        let json = r#"{
            "doc_id": "ch1",
            "title": "Chapter One",
            "revision": 99,
            "blocks": [{"id":"intro","type":"paragraph","text":"Hello"}]
        }"#;
        let draft = ReportDraft::parse(json).unwrap();
        let doc = draft.into_document(None).unwrap();
        assert_eq!(doc.doc_id, "ch1");
        assert_eq!(doc.cover.title, "Chapter One");
        assert_eq!(doc.revision, revision(0));
        assert_eq!(doc.blocks.len(), 1);
    }

    #[test]
    fn draft_requires_doc_id_and_title() {
        let draft = ReportDraft::parse(r#"{"blocks":[]}"#).unwrap();
        assert!(matches!(
            draft.clone().into_document(None),
            Err(DraftError::MissingDocId)
        ));
        assert!(matches!(
            draft.into_document(Some("fallback")),
            Err(DraftError::MissingTitle)
        ));
    }
}
