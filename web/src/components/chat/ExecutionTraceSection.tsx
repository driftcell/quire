import { memo, useState, type ReactNode } from "react";
import { ChevronRightIcon, SparklesIcon } from "lucide-react";
import { cn } from "cn";
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
    <div className="space-y-2 border-l border-border/70 pl-3">
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
}: {
  sessionId: string;
  segment: ActivityRenderSegment;
  questionSlots?: Map<string, ReactNode>;
  showActiveDetails?: boolean;
}) {
  const hasQuestion = activityHasQuestion(segment.items, questionSlots);
  const [userOpen, setUserOpen] = useState<boolean | undefined>(undefined);
  const summary = summarizeActivity(segment.items);

  if (segment.isActive) {
    const visible = showActiveDetails || hasQuestion;
    return (
      <div
        data-slot="execution-activity"
        data-state="active"
        className={cn(
          "grid w-full transition-[grid-template-rows,opacity] duration-200 ease-out",
          visible ? "grid-rows-[1fr] opacity-100" : "grid-rows-[0fr] opacity-0",
        )}
      >
        <div className="min-h-0 overflow-hidden">
          <ActivityItems
            sessionId={sessionId}
            items={segment.items}
            questionSlots={questionSlots}
          />
        </div>
      </div>
    );
  }

  const open = hasQuestion ? true : (userOpen ?? false);
  const summaryParts: string[] = [];
  if (summary.toolCount > 0) {
    summaryParts.push(`${summary.toolCount} 次工具`);
  }
  if (summary.subagentCount > 0) {
    summaryParts.push(`${summary.subagentCount} 个子代理`);
  }
  const summaryText = summaryParts.join(" · ");

  return (
    <div
      data-slot="execution-activity"
      data-state="settled"
      className="w-full min-w-0"
    >
      <button
        type="button"
        data-activity-trigger="true"
        aria-expanded={open}
        onClick={() => setUserOpen((prev) => !(prev ?? open))}
        className="inline-flex h-6 max-w-full items-center gap-1.5 rounded-md px-1 text-left text-xs text-muted-foreground transition-colors hover:bg-muted/50 hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none"
      >
        <SparklesIcon className="size-3.5 shrink-0 text-muted-foreground/80" />
        <span className="font-medium">思考与执行过程</span>
        {summaryText && (
          <span className="text-[11px] tabular-nums text-muted-foreground/75">
            · {summaryText}
          </span>
        )}
        <ChevronRightIcon
          className={cn(
            "size-3.5 shrink-0 text-muted-foreground/70 transition-transform duration-150",
            open && "rotate-90",
          )}
        />
      </button>

      <div
        className={cn(
          "grid transition-[grid-template-rows,opacity] duration-200 ease-out",
          open ? "mt-1.5 grid-rows-[1fr] opacity-100" : "grid-rows-[0fr] opacity-0",
        )}
      >
        <div className="min-h-0 overflow-hidden">
          <ActivityItems
            sessionId={sessionId}
            items={segment.items}
            questionSlots={questionSlots}
          />
        </div>
      </div>
    </div>
  );
}

const RenderSegments = memo(function RenderSegments({
  sessionId,
  segments,
  questionSlots,
  showActiveDetails = true,
}: {
  sessionId: string;
  segments: RenderSegment[];
  questionSlots?: Map<string, ReactNode>;
  showActiveDetails?: boolean;
}) {
  return (
    <div className="w-full space-y-2.5">
      {segments.map((segment) => {
        if (segment.kind === "text") {
          return (
            <SmoothStreamdown
              key={segment.key}
              text={segment.text}
              isStreaming={segment.isStreaming}
              className={CHAT_PROSE}
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
}: {
  sessionId: string;
  segments: RenderSegment[];
  questionSlots?: Map<string, ReactNode>;
  showActiveDetails?: boolean;
}) {
  return (
    <RenderSegments
      sessionId={sessionId}
      segments={segments}
      questionSlots={questionSlots}
      showActiveDetails={showActiveDetails}
    />
  );
}
