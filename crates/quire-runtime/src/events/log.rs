use std::future::Future;

use quire_session_protocol::{
    PendingQuestion, QuestionItem, SessionEvent, SessionEventKind, ToolCallDetail,
};

use crate::{
    error::RuntimeResult,
    store::{RuntimeStore, SessionEventDraft},
};

use super::EventScope;

/// Narrow persistence boundary used by [`super::EventSink`].
///
/// Keeping this separate from the full runtime store makes ordering and failure
/// behaviour testable without a database.
pub trait SessionEventLog: Send + Sync {
    fn append_event(
        &self,
        session_id: &str,
        scope: &EventScope,
        kind: SessionEventKind,
    ) -> impl Future<Output = RuntimeResult<SessionEvent>> + Send;

    fn save_tool_call(
        &self,
        detail: &ToolCallDetail,
    ) -> impl Future<Output = RuntimeResult<()>> + Send;

    fn create_question(
        &self,
        session_id: &str,
        run_id: &str,
        tool_call_id: Option<&str>,
        questions: &[QuestionItem],
    ) -> impl Future<Output = RuntimeResult<PendingQuestion>> + Send;

    /// Append a coalesced batch. Stores override this with one transaction;
    /// the default keeps simple test fakes concise.
    fn append_events(
        &self,
        session_id: &str,
        events: Vec<(EventScope, SessionEventKind)>,
    ) -> impl Future<Output = RuntimeResult<Vec<SessionEvent>>> + Send {
        async move {
            let mut appended = Vec::with_capacity(events.len());
            for (scope, kind) in events {
                appended.push(self.append_event(session_id, &scope, kind).await?);
            }
            Ok(appended)
        }
    }

    /// Persist a tool row and its wire event. The production store performs
    /// both in one transaction.
    fn save_tool_call_with_event(
        &self,
        detail: &ToolCallDetail,
        scope: &EventScope,
        kind: SessionEventKind,
    ) -> impl Future<Output = RuntimeResult<SessionEvent>> + Send {
        async move {
            self.save_tool_call(detail).await?;
            self.append_event(&detail.session_id, scope, kind).await
        }
    }

    /// Create a question and its request event. The production store performs
    /// both in one transaction.
    fn create_question_with_event(
        &self,
        session_id: &str,
        scope: &EventScope,
        tool_call_id: Option<&str>,
        questions: &[QuestionItem],
    ) -> impl Future<Output = RuntimeResult<(PendingQuestion, SessionEvent)>> + Send {
        async move {
            let question = self
                .create_question(session_id, &scope.run_id, tool_call_id, questions)
                .await?;
            let kind = SessionEventKind::QuestionnaireRequested {
                question_id: question.question_id.clone(),
                tool_call_id: question.tool_call_id.clone(),
                questions: question.questions.clone(),
            };
            let event = self.append_event(session_id, scope, kind).await?;
            Ok((question, event))
        }
    }
}

impl SessionEventLog for RuntimeStore {
    async fn append_event(
        &self,
        session_id: &str,
        scope: &EventScope,
        kind: SessionEventKind,
    ) -> RuntimeResult<SessionEvent> {
        self.append_event(
            session_id,
            &scope.run_id,
            &scope.agent_id,
            scope.parent_run_id.as_deref(),
            kind,
        )
        .await
    }

    async fn save_tool_call(&self, detail: &ToolCallDetail) -> RuntimeResult<()> {
        self.save_tool_call(detail).await
    }

    async fn create_question(
        &self,
        session_id: &str,
        run_id: &str,
        tool_call_id: Option<&str>,
        questions: &[QuestionItem],
    ) -> RuntimeResult<PendingQuestion> {
        self.create_question(session_id, run_id, tool_call_id, questions)
            .await
    }

    async fn append_events(
        &self,
        session_id: &str,
        events: Vec<(EventScope, SessionEventKind)>,
    ) -> RuntimeResult<Vec<SessionEvent>> {
        let drafts = events
            .into_iter()
            .map(|(scope, kind)| SessionEventDraft {
                run_id: scope.run_id,
                agent_id: scope.agent_id,
                parent_run_id: scope.parent_run_id,
                kind,
            })
            .collect();
        self.append_event_batch(session_id, drafts).await
    }

    async fn save_tool_call_with_event(
        &self,
        detail: &ToolCallDetail,
        scope: &EventScope,
        kind: SessionEventKind,
    ) -> RuntimeResult<SessionEvent> {
        RuntimeStore::save_tool_call_with_event(
            self,
            detail,
            &scope.agent_id,
            scope.parent_run_id.as_deref(),
            kind,
        )
        .await
    }

    async fn create_question_with_event(
        &self,
        session_id: &str,
        scope: &EventScope,
        tool_call_id: Option<&str>,
        questions: &[QuestionItem],
    ) -> RuntimeResult<(PendingQuestion, SessionEvent)> {
        RuntimeStore::create_question_with_event(
            self,
            session_id,
            &scope.run_id,
            &scope.agent_id,
            scope.parent_run_id.as_deref(),
            tool_call_id,
            questions,
        )
        .await
    }
}
