import { Fragment, memo, useState, type ReactNode } from "react";
import { ListTreeIcon } from "lucide-react";
import { HistoricalSubagent } from "@/components/chat/HistoricalSubagent";
import { SmoothStreamdown } from "@/components/chat/SmoothStreamdown";
import { SubagentTrace } from "@/components/chat/SubagentTrace";
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
import type { RunTraceSummary } from "@/contracts/session-protocol";
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
  liveHeader,
}: {
  sessionId: string;
  segment: ActivityRenderSegment;
  questionSlots?: Map<string, ReactNode>;
  showActiveDetails: boolean;
  liveHeader?: ReactNode;
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
          startedAtMs={item.startedAtMs}
          durationMs={item.durationMs}
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
        {liveHeader}
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
  showActiveDetails = true,
  renderLiveHeader,
  liveStreaming = false,
}: {
  sessionId: string;
  segments: RenderSegment[];
  questionSlots?: Map<string, ReactNode>;
  showActiveDetails?: boolean;
  renderLiveHeader?: () => ReactNode;
  liveStreaming?: boolean;
}) {
  const activeIndex = segments.findLastIndex(
    (segment) => segment.kind === "activity" && segment.isActive,
  );
  const lastSegment = segments[segments.length - 1];
  const headerIndex =
    activeIndex === -1 && renderLiveHeader && liveStreaming
      ? lastSegment?.kind === "text" && lastSegment.isStreaming
        ? segments.length - 1
        : segments.length
      : -1;

  return (
    <div className="w-full space-y-1">
      {segments.map((segment, index) => {
        if (segment.kind === "text") {
          return (
            <Fragment key={segment.key}>
              {headerIndex === index && renderLiveHeader?.()}
              <SmoothStreamdown
                text={segment.text}
                isStreaming={segment.isStreaming}
                className={`${CHAT_PROSE} pt-1`}
              />
            </Fragment>
          );
        }
        return (
          <Fragment key={segment.key}>
            {headerIndex === index && renderLiveHeader?.()}
            <ActivitySegment
              sessionId={sessionId}
              segment={segment}
              questionSlots={questionSlots}
              showActiveDetails={showActiveDetails}
              liveHeader={
                index === activeIndex ? renderLiveHeader?.() : undefined
              }
            />
          </Fragment>
        );
      })}
      {headerIndex === segments.length && renderLiveHeader?.()}
    </div>
  );
});

export function HistoricalRunTrace({
  sessionId,
  trace,
  questionSlots,
}: {
  sessionId: string;
  trace: RunTraceSummary;
  questionSlots?: Map<string, ReactNode>;
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
  const outputText = sub.summary?.trim() || streamingText.trim();

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
  showActiveDetails = true,
  renderLiveHeader,
  liveStreaming = false,
}: {
  sessionId: string;
  segments: RenderSegment[];
  questionSlots?: Map<string, ReactNode>;
  showActiveDetails?: boolean;
  renderLiveHeader?: () => ReactNode;
  liveStreaming?: boolean;
}) {
  return (
    <RenderSegments
      sessionId={sessionId}
      segments={segments}
      questionSlots={questionSlots}
      showActiveDetails={showActiveDetails}
      renderLiveHeader={renderLiveHeader}
      liveStreaming={liveStreaming}
    />
  );
}
