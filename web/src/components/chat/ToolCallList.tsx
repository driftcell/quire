import { memo, useMemo, useState } from "react";
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
}: {
  sessionId: string;
  tools: ToolCallTraceItem[];
}) {
  const [expanded, setExpanded] = useState(false);
  const tools = useMemo(() => rawTools.filter(isVisibleTraceTool), [rawTools]);

  const renderTool = (tool: ToolCallTraceItem) => (
    <ToolCallTrace key={tool.toolCallId} sessionId={sessionId} item={tool} />
  );

  if (tools.length === 0) {
    return null;
  }

  if (tools.length <= COLLAPSE_THRESHOLD || expanded) {
    return (
      <div className="space-y-0.5">
        {tools.length > COLLAPSE_THRESHOLD && (
          <button
            type="button"
            onClick={() => setExpanded(false)}
            className="mb-1 inline-flex items-center gap-1.5 rounded-full border border-border/60 bg-muted/40 px-2.5 py-0.5 text-[11px] text-muted-foreground tabular-nums transition-colors hover:bg-muted hover:text-foreground"
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
  const runningHidden = hiddenTools.filter((t) => t.status === "running");

  return (
    <div className="space-y-0.5">
      {hiddenTools.length > 0 && (
        <button
          type="button"
          onClick={() => setExpanded(true)}
          className="mb-1 inline-flex items-center gap-1.5 rounded-full border border-border/60 bg-muted/40 px-2.5 py-0.5 text-[11px] text-muted-foreground tabular-nums transition-colors hover:bg-muted hover:text-foreground"
        >
          <ChevronDownIcon className="size-3" />
          <span>
            展开更早的 {hiddenTools.length} 个步骤（共 {tools.length} 步）
          </span>
        </button>
      )}
      {runningHidden.map(renderTool)}
      {recentTools.map(renderTool)}
    </div>
  );
});
