import { Fragment, memo, useState, type ReactNode } from "react";
import { ListTreeIcon } from "lucide-react";
import { HistoricalSubagent } from "@/components/chat/HistoricalSubagent";
import { SmoothStreamdown } from "@/components/chat/SmoothStreamdown";
import { SubagentTrace } from "@/components/chat/SubagentTrace";
import { UserMessageBubble } from "@/components/chat/UserMessageBubble";
import { CHAT_PROSE } from "@/components/chat/chatMarkdown";
import { ThinkingTrace } from "@/components/chat/ThinkingTrace";
import { ToolCallList } from "@/components/chat/ToolCallList";
import {
  activityHasQuestion,
  buildHistoricalSegments,
  extractSubagentLiveView,
  summarizeActivity,
  type ActivityRenderSegment,
  type RenderSegment,
} from "@/components/chat/traceSegments";
import {
  Trace,
  TraceCollapsible,
  TraceCollapsibleContent,
  TraceCollapsibleTrigger,
  TraceDescription,
  TraceIcon,
  TraceLine,
  TraceTitle,
} from "@/components/ui/trace";
import type {
  AttachmentMeta,
  RunTraceSummary,
  SessionMessage,
} from "@/contracts/session-protocol";
import type { LiveSubagentRun } from "@/lib/liveTrace";

function formatActivityDuration(durationMs?: number): string | null {
  if (durationMs === undefined || durationMs <= 0) {
    return null;
  }
  if (durationMs < 1000) {
    return `${durationMs}ms`;
  }
  const totalSeconds = Math.max(1, Math.round(durationMs / 1000));
  if (totalSeconds < 60) {
    return `${totalSeconds}秒`;
  }
  const mins = Math.floor(totalSeconds / 60);
  const secs = totalSeconds % 60;
  return secs > 0 ? `${mins}分${secs}秒` : `${mins}分`;
}

function ActivitySegment({
  sessionId,
  segment,
  questionSlots,
  showActiveDetails,
}: {
  sessionId: string;
  segment: ActivityRenderSegment;
  questionSlots?: Map<string, ReactNode>;
  showActiveDetails: boolean;
}) {
  const hasQuestion = activityHasQuestion(segment.items, questionSlots);
  const summary = summarizeActivity(segment.items);
  const isActive = segment.isActive;
  const defaultOpen = hasQuestion || segment.isLive === true;
  const [userOpen, setUserOpen] = useState<boolean | null>(null);
  const open = userOpen ?? defaultOpen;

  const duration = formatActivityDuration(segment.durationMs);
  const completedParts: string[] = [];
  if (duration) {
    completedParts.push(duration);
  }
  if (summary.subagentCount > 0) {
    completedParts.push(`${summary.subagentCount} 个子代理`);
  }
  const title =
    summary.toolCount > 0
      ? `已完成思考与 ${summary.toolCount} 次操作`
      : summary.subagentCount > 0
        ? `已完成思考与 ${summary.subagentCount} 个子代理`
        : "已完成思考";

  const details = segment.items.map((item) => {
    if (item.kind === "thinking") {
      return (
        <ThinkingTrace
          key={item.key}
          content={item.text}
          isStreaming={item.isStreaming}
          isActive={item.isActive}
          defaultOpen={false}
          startedAtMs={item.startedAtMs}
          durationMs={item.durationMs}
        />
      );
    }
    if (item.kind === "text") {
      return (
        <SmoothStreamdown
          key={item.key}
          text={item.text}
          isStreaming={item.isStreaming}
          className={`${CHAT_PROSE} text-sm`}
        />
      );
    }
    return (
      <Fragment key={item.key}>
        <ToolCallList
          sessionId={sessionId}
          tools={item.tools}
          questionSlots={questionSlots}
        />
        {item.subagents.map((entry) =>
          entry.kind === "historical" ? (
            <HistoricalSubagent
              key={entry.key}
              sessionId={sessionId}
              subagent={entry.subagent}
            />
          ) : (
            <LiveSubagent
              key={entry.key}
              sessionId={sessionId}
              sub={entry.subagent}
            />
          ),
        )}
      </Fragment>
    );
  });

  if (isActive) {
    return (
      <Trace
        data-slot="activity"
        data-active="true"
        data-expanded={showActiveDetails ? "true" : "false"}
      >
        {showActiveDetails && (
          <div
            data-slot="activity-details"
            className="mt-1.5 space-y-2 border-l pl-4"
          >
            {details}
          </div>
        )}
      </Trace>
    );
  }

  return (
    <Trace data-slot="activity" data-active="false">
      <TraceCollapsible open={open} onOpenChange={setUserOpen}>
        <TraceCollapsibleTrigger data-activity-trigger="true">
          <TraceLine className="h-6 text-xs leading-5">
            <TraceIcon>
              <ListTreeIcon className="size-3.5 text-muted-foreground" />
            </TraceIcon>
            <TraceTitle>{title}</TraceTitle>
            {completedParts.length > 0 && (
              <TraceDescription className="text-[11px]">
                {completedParts.join(" · ")}
              </TraceDescription>
            )}
          </TraceLine>
        </TraceCollapsibleTrigger>
        <TraceCollapsibleContent className="mt-1.5 space-y-2 border-l pl-4">
          {details}
        </TraceCollapsibleContent>
      </TraceCollapsible>
    </Trace>
  );
}

export const RenderSegments = memo(function RenderSegments({
  sessionId,
  segments,
  questionSlots,
  onJumpToSection,
  messageById,
  attachmentMap,
  showActiveDetails = true,
}: {
  sessionId: string;
  segments: RenderSegment[];
  questionSlots?: Map<string, ReactNode>;
  onJumpToSection?: (sectionId: string) => void;
  messageById: ReadonlyMap<string, SessionMessage>;
  attachmentMap: Map<string, AttachmentMeta>;
  showActiveDetails?: boolean;
}) {
  return (
    <div className="w-full space-y-1">
      {segments.map((segment) => {
        if (segment.kind === "user") {
          const message = messageById.get(segment.messageId);
          return (
            <div
              key={segment.key}
              data-inline-user-message-id={segment.messageId}
            >
              <UserMessageBubble
                body={message?.content ?? segment.content}
                commentRefs={message?.comment_refs ?? []}
                attachmentIds={message?.attachment_ids ?? []}
                attachmentMap={attachmentMap}
                onJumpToSection={onJumpToSection}
              />
            </div>
          );
        }
        if (segment.kind === "text") {
          return (
            <SmoothStreamdown
              key={segment.key}
              text={segment.text}
              isStreaming={segment.isStreaming}
              className={`${CHAT_PROSE} pt-1`}
            />
          );
        }
        return (
          <ActivitySegment
            key={segment.key}
            sessionId={sessionId}
            segment={segment}
            questionSlots={questionSlots}
            showActiveDetails={showActiveDetails}
          />
        );
      })}
    </div>
  );
});

export function HistoricalRunTrace({
  sessionId,
  trace,
  questionSlots,
  messageById,
  attachmentMap,
  onJumpToSection,
}: {
  sessionId: string;
  trace: RunTraceSummary;
  questionSlots?: Map<string, ReactNode>;
  messageById: ReadonlyMap<string, SessionMessage>;
  attachmentMap: Map<string, AttachmentMeta>;
  onJumpToSection?: (sectionId: string) => void;
}) {
  const segments = buildHistoricalSegments(trace);
  if (segments.length === 0) {
    return null;
  }
  return (
    <RenderSegments
      sessionId={sessionId}
      segments={segments}
      questionSlots={questionSlots}
      messageById={messageById}
      attachmentMap={attachmentMap}
      onJumpToSection={onJumpToSection}
    />
  );
}

function LiveSubagent({
  sessionId,
  sub,
}: {
  sessionId: string;
  sub: LiveSubagentRun;
}) {
  const {
    thinkingText,
    isThinkingStreaming,
    thinkingDurationMs,
    thinkingStartedAtMs,
    tools,
    streamingText,
  } = extractSubagentLiveView(sub.trace);
  const outputText =
    sub.summary?.trim() ||
    (sub.status === "running" ? streamingText.trim() : "");

  return (
    <SubagentTrace
      sessionId={sessionId}
      model={{
        id: sub.subagentRunId,
        title: sub.displayName?.trim() || sub.agentName,
        focusTitle: sub.trace.taskFocus?.title,
        status: sub.status,
        thinking: thinkingText,
        thinkingStreaming: isThinkingStreaming,
        thinkingStartedAtMs,
        thinkingDurationMs,
        tools,
        outputText,
        outputStreaming: !sub.summary && sub.status === "running",
      }}
    />
  );
}

export function LiveTraceSegments({
  sessionId,
  segments,
  questionSlots,
  onJumpToSection,
  messageById,
  attachmentMap,
  showActiveDetails = true,
}: {
  sessionId: string;
  segments: RenderSegment[];
  questionSlots?: Map<string, ReactNode>;
  onJumpToSection?: (sectionId: string) => void;
  messageById: ReadonlyMap<string, SessionMessage>;
  attachmentMap: Map<string, AttachmentMeta>;
  showActiveDetails?: boolean;
}) {
  return (
    <RenderSegments
      sessionId={sessionId}
      segments={segments}
      questionSlots={questionSlots}
      onJumpToSection={onJumpToSection}
      messageById={messageById}
      attachmentMap={attachmentMap}
      showActiveDetails={showActiveDetails}
    />
  );
}
