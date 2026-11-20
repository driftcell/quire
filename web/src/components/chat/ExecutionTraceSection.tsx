import { Fragment, memo, type ReactNode } from "react";
import { ListTreeIcon, LoaderCircleIcon, UsersIcon } from "lucide-react";
import { Streamdown } from "streamdown";
import { HistoricalSubagent } from "@/components/chat/HistoricalSubagent";
import { ThinkingTrace } from "@/components/chat/ThinkingTrace";
import { ToolCallList } from "@/components/chat/ToolCallList";
import {
  CHAT_PROSE,
  STREAMDOWN_CONTROLS,
} from "@/components/chat/chatMarkdown";
import {
  activityHasQuestion,
  buildHistoricalSegments,
  buildLiveSegments,
  type ActivityRenderSegment,
  type ExecutionActivity,
  type ExecutionSubagent,
  type RenderSegment,
  type TextActivity,
  type ThinkingActivity,
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
import type { LiveAgentTrace, LiveSubagentRun } from "@/lib/liveTrace";

function ExecutionSubagents({
  sessionId,
  subagents,
}: {
  sessionId: string;
  subagents: ExecutionSubagent[];
}) {
  if (subagents.length === 0) {
    return null;
  }
  return (
    <>
      {subagents.map((entry) =>
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
    </>
  );
}

function ExecutionTrace({
  segmentKey,
  sessionId,
  tools,
  subagents,
  defaultOpen,
  questionSlots,
}: {
  segmentKey: string;
  sessionId: string;
  tools: ExecutionActivity["tools"];
  subagents: ExecutionSubagent[];
  defaultOpen: boolean;
  questionSlots?: Map<string, ReactNode>;
}) {
  if (tools.length === 0 && subagents.length === 0) {
    return null;
  }
  const summaryParts: string[] = [];
  if (tools.length > 0) {
    summaryParts.push(`${tools.length} 个工具`);
  }
  if (subagents.length > 0) {
    summaryParts.push(`${subagents.length} 个子代理`);
  }
  return (
    <Trace key={segmentKey}>
      <TraceCollapsible defaultOpen={defaultOpen}>
        <TraceCollapsibleTrigger>
          <TraceLine className="h-6 text-xs leading-5">
            <TraceIcon>
              <ListTreeIcon className="size-3.5" />
            </TraceIcon>
            <TraceTitle>执行详情</TraceTitle>
            <TraceDescription className="text-[11px]">
              {summaryParts.join(" · ")}
            </TraceDescription>
          </TraceLine>
        </TraceCollapsibleTrigger>
        <TraceCollapsibleContent className="mt-1.5 space-y-1.5 border-l pl-4">
          <ToolCallList
            sessionId={sessionId}
            tools={tools}
            questionSlots={questionSlots}
          />
          <ExecutionSubagents sessionId={sessionId} subagents={subagents} />
        </TraceCollapsibleContent>
      </TraceCollapsible>
    </Trace>
  );
}

function ActivitySegment({
  sessionId,
  segment,
  questionSlots,
}: {
  sessionId: string;
  segment: ActivityRenderSegment;
  questionSlots?: Map<string, ReactNode>;
}) {
  const thinkingItems = segment.items.filter(
    (item): item is ThinkingActivity => item.kind === "thinking",
  );
  const executionItems = segment.items.filter(
    (item): item is ExecutionActivity => item.kind === "execution",
  );
  const textItems = segment.items.filter(
    (item): item is TextActivity => item.kind === "text",
  );
  const hasQuestion = activityHasQuestion(segment.items, questionSlots);

  if (executionItems.length === 0 && textItems.length === 0) {
    return (
      <>
        {thinkingItems.map((item) => (
          <ThinkingTrace
            key={item.key}
            content={item.text}
            isStreaming={item.isStreaming}
            isActive={item.isActive}
            defaultOpen={item.defaultOpen}
            durationMs={item.durationMs}
          />
        ))}
      </>
    );
  }

  if (thinkingItems.length === 0 && textItems.length === 0) {
    return (
      <ExecutionTrace
        segmentKey={segment.key}
        sessionId={sessionId}
        tools={executionItems.flatMap((item) => item.tools)}
        subagents={executionItems.flatMap((item) => item.subagents)}
        defaultOpen={
          hasQuestion || executionItems.some((item) => item.defaultOpen)
        }
        questionSlots={questionSlots}
      />
    );
  }

  const tools = executionItems.flatMap((item) => item.tools);
  const subagents = executionItems.flatMap((item) => item.subagents);
  const summaryParts: string[] = [];
  if (tools.length > 0) {
    summaryParts.push(`${tools.length} 个工具`);
  }
  if (subagents.length > 0) {
    summaryParts.push(`${subagents.length} 个子代理`);
  }
  const streaming = segment.items.some(
    (item) =>
      (item.kind === "thinking" || item.kind === "text") && item.isStreaming,
  );
  const title =
    tools.length > 0 || subagents.length > 0
      ? thinkingItems.length > 0
        ? "思考与执行"
        : "执行详情"
      : "思考过程";

  return (
    <Trace key={segment.key}>
      <TraceCollapsible
        defaultOpen={
          hasQuestion ||
          streaming ||
          executionItems.some((item) => item.defaultOpen)
        }
      >
        <TraceCollapsibleTrigger>
          <TraceLine
            status={streaming ? "running" : undefined}
            className="h-6 text-xs leading-5"
          >
            <TraceIcon>
              {streaming ? (
                <LoaderCircleIcon className="size-3.5 animate-spin" />
              ) : (
                <ListTreeIcon className="size-3.5" />
              )}
            </TraceIcon>
            <TraceTitle>{streaming ? "正在处理" : title}</TraceTitle>
            <TraceDescription className="text-[11px]">
              {summaryParts.join(" · ")}
            </TraceDescription>
          </TraceLine>
        </TraceCollapsibleTrigger>
        <TraceCollapsibleContent className="mt-1.5 space-y-2 border-l pl-4">
          {segment.items.map((item) => {
            if (item.kind === "thinking") {
              return (
                <ThinkingTrace
                  key={item.key}
                  content={item.text}
                  isStreaming={item.isStreaming}
                  isActive={item.isActive}
                  defaultOpen={item.defaultOpen}
                  durationMs={item.durationMs}
                />
              );
            }
            if (item.kind === "text") {
              return (
                <div key={item.key} className={`${CHAT_PROSE} text-sm`}>
                  <Streamdown
                    mode={item.isStreaming ? "streaming" : "static"}
                    controls={STREAMDOWN_CONTROLS}
                  >
                    {item.text}
                  </Streamdown>
                </div>
              );
            }
            return (
              <Fragment key={item.key}>
                <ToolCallList
                  sessionId={sessionId}
                  tools={item.tools}
                  questionSlots={questionSlots}
                />
                <ExecutionSubagents
                  sessionId={sessionId}
                  subagents={item.subagents}
                />
              </Fragment>
            );
          })}
        </TraceCollapsibleContent>
      </TraceCollapsible>
    </Trace>
  );
}

export const RenderSegments = memo(function RenderSegments({
  sessionId,
  segments,
  questionSlots,
}: {
  sessionId: string;
  segments: RenderSegment[];
  questionSlots?: Map<string, ReactNode>;
}) {
  return (
    <div className="w-full space-y-1">
      {segments.map((segment) => {
        if (segment.kind === "text") {
          return (
            <div key={segment.key} className={`${CHAT_PROSE} pt-1`}>
              <Streamdown
                mode={segment.isStreaming ? "streaming" : "static"}
                controls={STREAMDOWN_CONTROLS}
              >
                {segment.text}
              </Streamdown>
            </div>
          );
        }
        return (
          <ActivitySegment
            key={segment.key}
            sessionId={sessionId}
            segment={segment}
            questionSlots={questionSlots}
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

function LiveSubagent({
  sessionId,
  sub,
}: {
  sessionId: string;
  sub: LiveSubagentRun;
}) {
  const segments = buildLiveSegments(sub.trace, [], {
    defaultCollapsed: true,
  });
  const title = sub.displayName?.trim() || sub.agentName;
  return (
    <Trace>
      <TraceCollapsible defaultOpen>
        <TraceCollapsibleTrigger>
          <TraceLine
            className="h-6 text-xs leading-5"
            status={
              sub.status === "running"
                ? "running"
                : sub.status === "failed"
                  ? "error"
                  : undefined
            }
          >
            <TraceIcon>
              {sub.status === "running" ? (
                <LoaderCircleIcon className="size-3.5 animate-spin" />
              ) : (
                <UsersIcon className="size-3.5" />
              )}
            </TraceIcon>
            <TraceTitle>{title}</TraceTitle>
            {sub.trace.taskFocus?.title && (
              <TraceDescription className="truncate text-[11px]">
                {sub.trace.taskFocus.title}
              </TraceDescription>
            )}
          </TraceLine>
        </TraceCollapsibleTrigger>
        <TraceCollapsibleContent className="mt-1 space-y-1.5 border-l pl-4">
          <RenderSegments sessionId={sessionId} segments={segments} />
          {sub.summary && (
            <div className="text-xs text-muted-foreground">{sub.summary}</div>
          )}
        </TraceCollapsibleContent>
      </TraceCollapsible>
    </Trace>
  );
}

export function LiveTraceSegments({
  sessionId,
  trace,
  subagents,
  questionSlots,
}: {
  sessionId: string;
  trace: LiveAgentTrace;
  subagents: LiveSubagentRun[];
  questionSlots?: Map<string, ReactNode>;
}) {
  return (
    <RenderSegments
      sessionId={sessionId}
      segments={buildLiveSegments(trace, subagents)}
      questionSlots={questionSlots}
    />
  );
}
