//! Validation and model projection for structured report-comment references.

use std::collections::BTreeSet;

use quire_session_protocol::{SessionCommentRef, SessionMessage};

use crate::error::{RuntimeError, RuntimeResult};

pub fn validate_comment_refs(comment_refs: &[SessionCommentRef]) -> RuntimeResult<()> {
    let mut thread_ids = BTreeSet::new();
    for comment_ref in comment_refs {
        for (field, value) in [
            ("thread_id", &comment_ref.thread_id),
            ("block_id", &comment_ref.block_id),
            ("comment", &comment_ref.comment),
        ] {
            if value.trim().is_empty() {
                return Err(RuntimeError::InvalidRequest(format!(
                    "comment reference {field} must not be empty"
                )));
            }
        }
        if !thread_ids.insert(comment_ref.thread_id.as_str()) {
            return Err(RuntimeError::InvalidRequest(format!(
                "comment reference thread_id `{}` is duplicated",
                comment_ref.thread_id
            )));
        }
    }
    Ok(())
}

pub fn validate_user_payload(
    content: &str,
    comment_refs: &[SessionCommentRef],
    attachment_ids: &[String],
) -> RuntimeResult<()> {
    if content.trim().is_empty() && comment_refs.is_empty() && attachment_ids.is_empty() {
        return Err(RuntimeError::InvalidRequest(
            "message content, comment_refs and attachment_ids must not all be empty".into(),
        ));
    }
    validate_comment_refs(comment_refs)
}

/// Project one stored user message into the single text input accepted by model providers.
/// Ordinary messages remain byte-for-byte unchanged; metadata uses a JSON envelope.
pub fn render_model_user_message(
    content: &str,
    comment_refs: &[SessionCommentRef],
    attachment_ids: &[String],
) -> String {
    if comment_refs.is_empty() && attachment_ids.is_empty() {
        return content.to_string();
    }
    let mut envelope = serde_json::Map::new();
    envelope.insert("instruction".into(), serde_json::json!(content));
    if !comment_refs.is_empty() {
        envelope.insert("comment_refs".into(), serde_json::json!(comment_refs));
    }
    if !attachment_ids.is_empty() {
        envelope.insert("attachment_ids".into(), serde_json::json!(attachment_ids));
    }
    serde_json::Value::Object(envelope).to_string()
}

pub fn render_stored_user_message(message: &SessionMessage) -> String {
    render_model_user_message(
        &message.content,
        &message.comment_refs,
        &message.attachment_ids,
    )
}

#[cfg(test)]
mod tests {
    use quire_session_protocol::SessionCommentRef;

    use super::{render_model_user_message, validate_comment_refs, validate_user_payload};

    fn comment_ref(thread_id: &str) -> SessionCommentRef {
        SessionCommentRef {
            thread_id: thread_id.into(),
            block_id: "block_1".into(),
            end_block_id: Some("block_2".into()),
            anchor_blocks: vec!["block_1".into(), "block_2".into()],
            status: "open".into(),
            quote: Some("<source> & \"quote\"".into()),
            end_quote: None,
            anchor_snapshot: Some("source … [表格: Metrics]".into()),
            comment: "Fix it's <scope>".into(),
        }
    }

    #[test]
    fn ordinary_content_is_unchanged() {
        assert_eq!(
            render_model_user_message("hello\nworld", &[], &[]),
            "hello\nworld"
        );
    }

    #[test]
    fn references_use_one_json_envelope() {
        let rendered =
            render_model_user_message("Handle <this> & that", &[comment_ref("ct_1")], &[]);
        let value: serde_json::Value = serde_json::from_str(&rendered).unwrap_or_default();
        assert_eq!(value["instruction"], "Handle <this> & that");
        assert_eq!(value["comment_refs"][0]["thread_id"], "ct_1");
        assert_eq!(value["comment_refs"][0]["quote"], "<source> & \"quote\"");
    }

    #[test]
    fn attachments_and_comments_share_one_envelope() {
        let attachment_only = render_model_user_message("", &[], &["att_1".into()]);
        let attachment_only: serde_json::Value =
            serde_json::from_str(&attachment_only).unwrap_or_default();
        assert_eq!(attachment_only["instruction"], "");
        assert_eq!(attachment_only["attachment_ids"][0], "att_1");
        assert!(attachment_only.get("comment_refs").is_none());

        let combined = render_model_user_message(
            "Review both",
            &[comment_ref("ct_1")],
            &["att_1".into(), "att_2".into()],
        );
        let combined: serde_json::Value = serde_json::from_str(&combined).unwrap_or_default();
        assert_eq!(combined["instruction"], "Review both");
        assert_eq!(combined["comment_refs"][0]["thread_id"], "ct_1");
        assert_eq!(combined["attachment_ids"][1], "att_2");
    }

    #[test]
    fn duplicate_threads_and_empty_required_fields_are_rejected() {
        assert!(validate_comment_refs(&[comment_ref("ct_1"), comment_ref("ct_1")]).is_err());
        let mut invalid = comment_ref("ct_2");
        invalid.comment = "  ".into();
        assert!(validate_comment_refs(&[invalid]).is_err());
    }

    #[test]
    fn comment_only_payload_is_valid_but_empty_payload_is_not() {
        assert!(validate_user_payload("", &[comment_ref("ct_1")], &[]).is_ok());
        assert!(validate_user_payload("", &[], &[]).is_err());
        assert!(validate_user_payload("", &[], &["att_1".into()]).is_ok());
    }
}
