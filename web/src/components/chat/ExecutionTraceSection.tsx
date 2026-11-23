import { Fragment, memo, useState, type ReactNode } from "react";
import { SparklesIcon } from "lucide-react";
import { HistoricalSubagent } from "@/components/chat/HistoricalSubagent";
import { SmoothStreamdown } from "@/components/chat/SmoothStreamdown";
import { SubagentTrace } from "@/components/chat/SubagentTrace";
import { ThinkingTrace } from "@/components/chat/ThinkingTrace";
import { ToolCallList } from "@/components/chat/ToolCallList";
import { CHAT_PROSE } from "@/components/chat/chatMarkdown";
import {
  activityHasQuestion,
  buildHistoricalSegments,
  extractSubagentLiveView,
  summarizeActivity,
  type ActivityItem,
  type ActivityRenderSegment,
  type ExecutionSubagent,
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

function ActivityItems({
  sessionId,
  items,
  questionSlots,
}: {
  sessionId: string;
  items: ActivityItem[];
  questionSlots?: Map<string, ReactNode>;
}) {
  const renderSubagent = (entry: ExecutionSubagent) =>
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
    );

  return (
    <div className="space-y-1.5">
      {items.map((item) => {
        if (item.kind === "thinking") {
          return (
            <ThinkingTrace
              key={item.key}
              content={item.text}
              isStreaming={item.isStreaming}
              isActive={item.isActive}
              autoExpand={item.isActive}
              maxSmoothBacklog={160}
              startedAtMs={item.startedAtMs}
              durationMs={item.durationMs}
            />
          );
        }
        return (
          <div key={item.key} className="space-y-1.5">
            <ToolCallList
              sessionId={sessionId}
              tools={item.tools}
              questionSlots={questionSlots}
            />
            {item.subagents.map(renderSubagent)}
          </div>
        );
      })}
    </div>
  );
}

function ActivitySegment({
  sessionId,
  segment,
  questionSlots,
  showActiveDetails = true,
  liveHeader,
}: {
  sessionId: string;
  segment: ActivityRenderSegment;
  questionSlots?: Map<string, ReactNode>;
  showActiveDetails?: boolean;
  liveHeader?: ReactNode;
}) {
  const hasQuestion = activityHasQuestion(segment.items, questionSlots);
  const [userOpen, setUserOpen] = useState<boolean | undefined>(undefined);
  const summary = summarizeActivity(segment.items);

  if (segment.isActive) {
    return (
      <div
        data-slot="execution-activity"
        data-state="active"
        className="w-full space-y-1.5"
      >
        {liveHeader}
        {(showActiveDetails || hasQuestion) && (
          <ActivityItems
            sessionId={sessionId}
            items={segment.items}
            questionSlots={questionSlots}
          />
        )}
      </div>
    );
  }

  const open = hasQuestion ? true : (userOpen ?? false);
  const summaryParts: string[] = [];
  if (summary.toolCount > 0) {
    summaryParts.push(`${summary.toolCount} 次工具调用`);
  }
  if (summary.subagentCount > 0) {
    summaryParts.push(`${summary.subagentCount} 个子代理`);
  }
  const summaryText = summaryParts.join(" · ");

  return (
    <Trace
      data-slot="execution-activity"
      data-state="settled"
      className="w-full"
    >
      <TraceCollapsible open={open} onOpenChange={setUserOpen}>
        <TraceCollapsibleTrigger data-activity-trigger="true">
          <TraceLine className="h-6 text-xs leading-5">
            <TraceIcon>
              <SparklesIcon className="size-3.5 text-muted-foreground" />
            </TraceIcon>
            <TraceTitle>思考与执行过程</TraceTitle>
            {summaryText && <TraceDescription>{summaryText}</TraceDescription>}
          </TraceLine>
        </TraceCollapsibleTrigger>
        <TraceCollapsibleContent>
          <ActivityItems
            sessionId={sessionId}
            items={segment.items}
            questionSlots={questionSlots}
          />
        </TraceCollapsibleContent>
      </TraceCollapsible>
    </Trace>
  );
}

const RenderSegments = memo(function RenderSegments({
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

const LiveSubagent = memo(function LiveSubagent({
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
    isTextStreaming,
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
        outputStreaming: !sub.summary && isTextStreaming,
      }}
    />
  );
});

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
