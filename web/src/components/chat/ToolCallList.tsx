import { Fragment, memo, useMemo, useState, type ReactNode } from "react";
import { ChevronDownIcon, ChevronUpIcon } from "lucide-react";
import {
  ToolCallTrace,
  type ToolCallTraceItem,
} from "@/components/chat/ToolCallTrace";
import { isVisibleTraceTool } from "@/components/chat/toolMeta";

const COLLAPSE_THRESHOLD = 5;
const VISIBLE_RECENT_TOOLS = 4;

export const ToolCallList = memo(function ToolCallList({
  sessionId,
  tools: rawTools,
  questionSlots,
}: {
  sessionId: string;
  tools: ToolCallTraceItem[];
  questionSlots?: Map<string, ReactNode>;
}) {
  const [expanded, setExpanded] = useState(false);
  const tools = useMemo(() => rawTools.filter(isVisibleTraceTool), [rawTools]);

  const renderTool = (tool: ToolCallTraceItem) => (
    <Fragment key={tool.toolCallId}>
      <ToolCallTrace sessionId={sessionId} item={tool} />
      {questionSlots?.get(tool.toolCallId)}
    </Fragment>
  );

  if (tools.length === 0) {
    return null;
  }

  if (tools.length <= COLLAPSE_THRESHOLD || expanded) {
    return (
      <div className="space-y-1">
        {tools.length > COLLAPSE_THRESHOLD && (
          <button
            type="button"
            onClick={() => setExpanded(false)}
            className="inline-flex items-center gap-1 rounded px-1.5 py-0.5 text-[11px] text-muted-foreground tabular-nums hover:bg-muted hover:text-foreground"
          >
            <ChevronUpIcon className="size-3" />
            <span>
              收起更早的 {tools.length - VISIBLE_RECENT_TOOLS} 个调用（共{" "}
              {tools.length} 个）
            </span>
          </button>
        )}
        {tools.map(renderTool)}
      </div>
    );
  }

  const splitAt = tools.length - VISIBLE_RECENT_TOOLS;
  const hiddenTools = tools.slice(0, splitAt);
  const recentTools = tools.slice(splitAt);
  const pinnedTools = hiddenTools.filter((tool) =>
    questionSlots?.has(tool.toolCallId),
  );
  const restHidden = hiddenTools.filter(
    (tool) => !questionSlots?.has(tool.toolCallId),
  );
  const runningHidden = restHidden.filter((t) => t.status === "running");

  return (
    <div className="space-y-1">
      {restHidden.length > 0 && (
        <button
          type="button"
          onClick={() => setExpanded(true)}
          className="inline-flex items-center gap-1 rounded px-1.5 py-0.5 text-[11px] text-muted-foreground tabular-nums hover:bg-muted hover:text-foreground"
        >
          <ChevronDownIcon className="size-3" />
          <span>
            显示更早的 {restHidden.length} 个调用（共 {tools.length} 个）
          </span>
        </button>
      )}
      {pinnedTools.map(renderTool)}
      {runningHidden.map(renderTool)}
      {recentTools.map(renderTool)}
    </div>
  );
});
