import { memo, useEffect, useMemo, useRef, useState } from "react";
import {
  ArrowDownIcon,
  CompassIcon,
  FileTextIcon,
  UsersIcon,
} from "lucide-react";
import { Streamdown } from "streamdown";
import { QuestionCard } from "@/components/chat/QuestionCard";
import { ThinkingTrace } from "@/components/chat/ThinkingTrace";
import {
  ToolCallTrace,
  type ToolCallTraceItem,
} from "@/components/chat/ToolCallTrace";
import {
  Attachment,
  AttachmentContent,
  AttachmentDescription,
  AttachmentGroup,
  AttachmentMedia,
  AttachmentTitle,
} from "@/components/ui/attachment";
import { Badge } from "@/components/ui/badge";
import {
  Message,
  MessageContent,
  MessageGroup,
  MessageHeader,
} from "@/components/ui/message";
import {
  Trace,
  TraceCollapsible,
  TraceCollapsibleContent,
  TraceCollapsibleTrigger,
  TraceIcon,
  TraceLine,
  TraceTitle,
} from "@/components/ui/trace";
import type {
  AttachmentMeta,
  PendingQuestion,
  QuestionAnswer,
  SessionMessage,
} from "@/contracts/session-protocol";

export interface LiveAgentTrace {
  agentId: string;
  thinking: string;
  text: string;
  taskFocus?: { title: string; detail?: string; sectionId?: string };
  tools: ToolCallTraceItem[];
  isStreaming: boolean;
}

export interface LiveSubagentRun {
  subagentRunId: string;
  agentName: string;
  task: string;
  sectionId?: string;
  status: "running" | "completed" | "failed";
  summary?: string;
  trace: LiveAgentTrace;
}

export interface SessionStreamViewProps {
  sessionId: string;
  messages: SessionMessage[];
  attachments?: AttachmentMeta[];
  liveMain: LiveAgentTrace | null;
  liveSubagents: LiveSubagentRun[];
  pendingQuestion: PendingQuestion | null;
  resolvedQuestion?: {
    question: PendingQuestion;
    status: "answered" | "skipped" | "dismissed";
    answers?: QuestionAnswer[];
  } | null;
  isAnsweringQuestion: boolean;
  onAnswerQuestion: (
    questionId: string,
    answers: QuestionAnswer[],
    skipped: boolean,
  ) => void;
  onJumpToSection?: (sectionId: string) => void;
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

const HistoricalMessageItem = memo(function HistoricalMessageItem({
  msg,
  attachmentMap,
}: {
  msg: SessionMessage;
  attachmentMap: Map<string, AttachmentMeta>;
}) {
  const msgAttachmentIds = msg.attachment_ids ?? [];
  return (
    <MessageGroup>
      <Message align={msg.role === "user" ? "end" : "start"}>
        <MessageHeader>
          {msg.role === "user" ? "你" : "Quire Agent"}
        </MessageHeader>
        {msg.role === "user" && msgAttachmentIds.length > 0 && (
          <AttachmentGroup className="mb-1.5 justify-end">
            {msgAttachmentIds.map((attId) => {
              const meta = attachmentMap.get(attId);
              return (
                <Attachment key={attId} size="xs">
                  <AttachmentMedia variant="icon">
                    <FileTextIcon className="size-3.5" />
                  </AttachmentMedia>
                  <AttachmentContent>
                    <AttachmentTitle>{meta?.filename ?? attId}</AttachmentTitle>
                    {meta && (
                      <AttachmentDescription>
                        {formatBytes(meta.size_bytes)}
                      </AttachmentDescription>
                    )}
                  </AttachmentContent>
                </Attachment>
              );
            })}
          </AttachmentGroup>
        )}
        <MessageContent className="prose prose-sm dark:prose-invert max-w-none">
          {msg.role === "user" ? (
            <p className="m-0 whitespace-pre-wrap">{msg.content}</p>
          ) : (
            <Streamdown>{msg.content}</Streamdown>
          )}
        </MessageContent>
      </Message>
    </MessageGroup>
  );
});

export function SessionStreamView({
  sessionId,
  messages,
  attachments = [],
  liveMain,
  liveSubagents,
  pendingQuestion,
  resolvedQuestion,
  isAnsweringQuestion,
  onAnswerQuestion,
  onJumpToSection,
}: SessionStreamViewProps) {
  const bottomRef = useRef<HTMLDivElement>(null);
  const [atBottom, setAtBottom] = useState(true);

  const attachmentMap = useMemo(() => {
    const map = new Map<string, AttachmentMeta>();
    for (const item of attachments) {
      map.set(item.attachment_id, item);
    }
    return map;
  }, [attachments]);

  const visibleMessages = useMemo(
    () =>
      messages.filter((msg) => msg.role === "user" || msg.role === "assistant"),
    [messages],
  );

  useEffect(() => {
    const el = bottomRef.current;
    if (!el) return undefined;
    const observer = new IntersectionObserver(
      (entries) => {
        const first = entries[0];
        if (first) {
          setAtBottom(first.isIntersecting);
        }
      },
      { threshold: 0.1 },
    );
    observer.observe(el);
    return () => observer.disconnect();
  }, []);

  const scrollSignal = [
    visibleMessages.length,
    liveMain?.text.length ?? 0,
    liveMain?.thinking.length ?? 0,
    liveSubagents.length,
    pendingQuestion?.question_id ?? "",
  ].join(":");

  useEffect(() => {
    if (scrollSignal.length === 0 || !atBottom) {
      return undefined;
    }
    bottomRef.current?.scrollIntoView({ behavior: "smooth", block: "end" });
    return undefined;
  }, [scrollSignal, atBottom]);

  return (
    <div className="relative flex flex-col gap-4 py-4">
      {visibleMessages.map((msg) => (
        <HistoricalMessageItem
          key={msg.message_id}
          msg={msg}
          attachmentMap={attachmentMap}
        />
      ))}

      {liveMain && (
        <MessageGroup>
          <Message align="start">
            <MessageHeader>Quire Agent</MessageHeader>
            <MessageContent className="w-full space-y-2">
              {liveMain.taskFocus && (
                <button
                  type="button"
                  onClick={() => {
                    if (liveMain.taskFocus?.sectionId) {
                      onJumpToSection?.(liveMain.taskFocus.sectionId);
                    }
                  }}
                  className="flex w-fit items-center gap-1.5 rounded-full border bg-muted/50 px-2.5 py-1 text-xs text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
                >
                  <CompassIcon className="size-3.5 text-primary" />
                  <span className="font-medium">
                    {liveMain.taskFocus.title}
                  </span>
                  {liveMain.taskFocus.detail && (
                    <span className="opacity-75">
                      — {liveMain.taskFocus.detail}
                    </span>
                  )}
                </button>
              )}

              <ThinkingTrace
                content={liveMain.thinking}
                isStreaming={liveMain.isStreaming && liveMain.text.length === 0}
              />

              {liveMain.tools.map((tool) => (
                <ToolCallTrace
                  key={tool.toolCallId}
                  sessionId={sessionId}
                  item={tool}
                />
              ))}

              {liveSubagents.length > 0 && (
                <div className="space-y-1.5 pt-1">
                  {liveSubagents.map((sub) => (
                    <Trace key={sub.subagentRunId}>
                      <TraceCollapsible defaultOpen={sub.status === "running"}>
                        <TraceCollapsibleTrigger>
                          <TraceLine
                            status={
                              sub.status === "running"
                                ? "running"
                                : sub.status === "failed"
                                  ? "error"
                                  : undefined
                            }
                          >
                            <TraceIcon>
                              <UsersIcon className="size-3.5" />
                            </TraceIcon>
                            <TraceTitle>{sub.agentName}</TraceTitle>
                            <Badge variant="outline" className="text-[10px]">
                              {sub.task}
                            </Badge>
                          </TraceLine>
                        </TraceCollapsibleTrigger>
                        <TraceCollapsibleContent className="mt-1 space-y-1.5 border-l pl-4">
                          {sub.trace.taskFocus && (
                            <div className="text-xs text-muted-foreground">
                              {sub.trace.taskFocus.title}
                            </div>
                          )}
                          <ThinkingTrace
                            content={sub.trace.thinking}
                            isStreaming={sub.status === "running"}
                          />
                          {sub.trace.tools.map((tool) => (
                            <ToolCallTrace
                              key={tool.toolCallId}
                              sessionId={sessionId}
                              item={tool}
                            />
                          ))}
                          {sub.summary && (
                            <div className="text-xs text-muted-foreground">
                              {sub.summary}
                            </div>
                          )}
                        </TraceCollapsibleContent>
                      </TraceCollapsible>
                    </Trace>
                  ))}
                </div>
              )}

              {liveMain.text && (
                <div className="prose prose-sm dark:prose-invert max-w-none pt-1">
                  <Streamdown>{liveMain.text}</Streamdown>
                </div>
              )}
            </MessageContent>
          </Message>
        </MessageGroup>
      )}

      {resolvedQuestion && !pendingQuestion && (
        <QuestionCard
          pending={resolvedQuestion.question}
          resolvedState={{
            status: resolvedQuestion.status,
            answers: resolvedQuestion.answers,
          }}
          isSubmitting={false}
          onSubmit={() => {}}
        />
      )}

      {pendingQuestion && (
        <QuestionCard
          pending={pendingQuestion}
          isSubmitting={isAnsweringQuestion}
          onSubmit={(answers, skipped) =>
            onAnswerQuestion(pendingQuestion.question_id, answers, skipped)
          }
        />
      )}

      {!atBottom && (
        <div className="pointer-events-none sticky bottom-3 z-20 flex justify-center">
          <button
            type="button"
            onClick={() => {
              bottomRef.current?.scrollIntoView({
                behavior: "smooth",
                block: "end",
              });
            }}
            className="pointer-events-auto inline-flex items-center gap-1 rounded-full border bg-background/95 px-3 py-1 text-xs font-medium shadow-md backdrop-blur hover:bg-accent"
          >
            <ArrowDownIcon className="size-3.5" />
            <span>跳到最新</span>
          </button>
        </div>
      )}

      <div ref={bottomRef} className="h-px" />
    </div>
  );
}
