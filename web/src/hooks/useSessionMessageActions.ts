import { useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { answerQuestion, postMessage, uploadAttachment } from "@/api/runtime";
import type {
  PendingQuestion,
  QuestionAnswer,
  SessionCommentRef,
  SessionDetail,
} from "@/contracts/session-protocol";
import { randomUuid } from "@/lib/uuid";

export interface UseSessionMessageActionsOptions {
  sessionId: string;
  effectivePendingQuestion: PendingQuestion | null;
  recordResolvedQuestion: (
    question: PendingQuestion,
    status: "answered" | "skipped" | "dismissed",
    answers?: QuestionAnswer[],
  ) => void;
  clearLivePendingQuestion: () => void;
}

export function useSessionMessageActions({
  sessionId,
  effectivePendingQuestion,
  recordResolvedQuestion,
  clearLivePendingQuestion,
}: UseSessionMessageActionsOptions) {
  const queryClient = useQueryClient();
  const [isAnsweringQuestion, setIsAnsweringQuestion] = useState(false);

  const handleSend = async (
    text: string,
    files: File[],
    commentRefs: SessionCommentRef[],
  ) => {
    const attachmentIds: string[] = [];
    for (const file of files) {
      const uploaded = await uploadAttachment(sessionId, file);
      attachmentIds.push(uploaded.attachment_id);
    }
    const response = await postMessage(sessionId, {
      client_msg_id: `msg_${randomUuid()}`,
      content: text,
      comment_refs: commentRefs,
      attachment_ids: attachmentIds,
    });
    // Insert the persisted row under its stable server ID. A temporary
    // optimistic row would change keys when the real row arrives and lose the
    // turn anchor; refetching the whole session here causes the same jump.
    queryClient.setQueryData<SessionDetail>(
      ["session", sessionId],
      (current) => {
        if (!current) {
          return current;
        }
        if (response.queued && response.queue_item) {
          const existingQueue = current.pending_queue ?? [];
          if (
            existingQueue.some(
              (item) => item.queue_id === response.queue_item?.queue_id,
            )
          ) {
            return current;
          }
          return {
            ...current,
            pending_queue: [...existingQueue, response.queue_item],
          };
        }
        if (!response.message) {
          return current;
        }
        const existing = current.messages ?? [];
        if (
          existing.some(
            (message) => message.message_id === response.message?.message_id,
          )
        ) {
          return current;
        }
        return {
          ...current,
          messages: [...existing, response.message],
        };
      },
    );
    if (effectivePendingQuestion) {
      recordResolvedQuestion(effectivePendingQuestion, "dismissed");
    }
    clearLivePendingQuestion();
  };

  const handleAnswerQuestion = async (
    questionId: string,
    answers: QuestionAnswer[],
    skipped: boolean,
  ) => {
    setIsAnsweringQuestion(true);
    try {
      await answerQuestion(sessionId, questionId, { answers, skipped });
      if (
        effectivePendingQuestion &&
        effectivePendingQuestion.question_id === questionId
      ) {
        recordResolvedQuestion(
          effectivePendingQuestion,
          skipped ? "skipped" : "answered",
          answers,
        );
      }
      clearLivePendingQuestion();
      void queryClient.invalidateQueries({ queryKey: ["session", sessionId] });
      void queryClient.invalidateQueries({
        queryKey: ["session-traces", sessionId],
      });
    } finally {
      setIsAnsweringQuestion(false);
    }
  };

  return { isAnsweringQuestion, handleSend, handleAnswerQuestion };
}
