import { useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Loader2Icon } from "lucide-react";
import { getToolMeta } from "@/components/chat/toolMeta";
import { getToolCall } from "@/api/runtime";
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

export interface ToolCallTraceItem {
  toolCallId: string;
  toolName: string;
  status: "running" | "completed" | "failed" | "skipped";
  inputPreview: string;
  outputPreview?: string;
  truncated?: boolean;
  durationMs?: number;
}

export interface ToolCallTraceProps {
  sessionId: string;
  item: ToolCallTraceItem;
}

function extractJsonField(raw: string, field: string): string | null {
  const escaped = field.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const pattern = new RegExp(`"${escaped}"\\s*:\\s*"([^"\\\\]{1,64})"`);
  const match = pattern.exec(raw);
  return match?.[1]?.trim() || null;
}

function extractToolInlineSummary(item: ToolCallTraceItem): string | null {
  const raw = item.inputPreview || "";
  if (!raw) return null;

  switch (item.toolName) {
    case "apply_operations": {
      const summary = extractJsonField(raw, "summary");
      if (summary) return summary;
      const sectionId =
        extractJsonField(raw, "section_id") ??
        extractJsonField(raw, "target_id") ??
        extractJsonField(raw, "block_id");
      return sectionId ? `#${sectionId}` : null;
    }
    case "commit_chart": {
      const blockId = extractJsonField(raw, "block_id");
      const summary = extractJsonField(raw, "summary");
      return blockId ? `#${blockId}` : summary;
    }
    case "preview_echarts": {
      return (
        extractJsonField(raw, "title") ??
        extractJsonField(raw, "based_on_preview_id")
      );
    }
    case "create_report":
    case "set_session_title":
    case "describe_task": {
      return extractJsonField(raw, "title");
    }
    case "get_document_digest":
    case "read_as_markdown":
    case "read_blocks": {
      return (
        extractJsonField(raw, "section_heading") ??
        extractJsonField(raw, "query") ??
        extractJsonField(raw, "doc_id")
      );
    }
    case "load_skill":
    case "read_skill_resource": {
      const skill =
        extractJsonField(raw, "name") ?? extractJsonField(raw, "skill");
      const path = extractJsonField(raw, "path");
      return skill && path ? `${skill}/${path}` : (skill ?? path);
    }
    case "sandbox_execute": {
      return extractJsonField(raw, "language");
    }
    case "sandbox_read_file":
    case "sandbox_write_file": {
      return extractJsonField(raw, "path");
    }
    case "sandbox_mount_attachment": {
      return extractJsonField(raw, "attachment_id");
    }
    default: {
      return (
        extractJsonField(raw, "title") ??
        extractJsonField(raw, "query") ??
        extractJsonField(raw, "name") ??
        extractJsonField(raw, "school_name") ??
        extractJsonField(raw, "university")
      );
    }
  }
}

function formatDurationBadge(durationMs?: number): string | null {
  if (durationMs === undefined || durationMs < 0) return null;
  if (durationMs < 1000) return `${durationMs}ms`;
  return `${(durationMs / 1000).toFixed(1)}s`;
}

export function ToolCallTrace({ sessionId, item }: ToolCallTraceProps) {
  const [open, setOpen] = useState(false);
  const meta = getToolMeta(item.toolName);
  const Icon = meta.icon;

  const detailQuery = useQuery({
    queryKey: ["tool-call", sessionId, item.toolCallId],
    queryFn: () => getToolCall(sessionId, item.toolCallId),
    enabled: open && item.status !== "running",
    staleTime: 5 * 60_000,
  });

  const inlineSummary = useMemo(() => extractToolInlineSummary(item), [item]);
  const durationBadge = formatDurationBadge(item.durationMs);

  const lineStatus =
    item.status === "running"
      ? "running"
      : item.status === "failed"
        ? "error"
        : undefined;

  return (
    <Trace
      data-tool-name={item.toolName}
      data-tool-status={item.status}
      data-slot="tool-trace"
    >
      <TraceCollapsible open={open} onOpenChange={setOpen}>
        <TraceCollapsibleTrigger showChevron={false}>
          <TraceLine status={lineStatus} className="h-6 text-xs leading-5">
            <TraceIcon>
              {item.status === "running" ? (
                <Loader2Icon className="size-3.5 animate-spin" />
              ) : (
                <Icon
                  className={
                    item.status === "failed"
                      ? "size-3.5 text-destructive"
                      : "size-3.5 text-muted-foreground"
                  }
                />
              )}
            </TraceIcon>
            <TraceTitle className="text-xs">{meta.label}</TraceTitle>
            {inlineSummary && (
              <TraceDescription className="max-w-[210px] truncate text-[11px] text-muted-foreground/85">
                {inlineSummary}
              </TraceDescription>
            )}
            {item.status === "skipped" && (
              <TraceDescription className="text-[11px]">
                已跳过
              </TraceDescription>
            )}
            {item.status === "failed" && (
              <TraceDescription className="text-[11px] text-destructive">
                失败
              </TraceDescription>
            )}
            {durationBadge && (
              <TraceDescription className="text-[11px] tabular-nums">
                {durationBadge}
              </TraceDescription>
            )}
          </TraceLine>
        </TraceCollapsibleTrigger>
        <TraceCollapsibleContent className="mt-1.5 pl-5">
          <div className="space-y-2 rounded-lg border bg-muted/30 p-2.5 text-xs">
            <div className="font-mono text-[11px] text-muted-foreground">
              {item.toolName}
            </div>
            <div>
              <div className="mb-1 text-[11px] font-medium text-muted-foreground">
                Input
              </div>
              <pre className="no-scrollbar max-h-40 overflow-auto rounded bg-background/70 p-2 font-mono text-[11px] whitespace-pre-wrap break-all">
                {detailQuery.data
                  ? JSON.stringify(detailQuery.data.input, null, 2)
                  : item.inputPreview}
              </pre>
            </div>
            {(detailQuery.data || item.outputPreview !== undefined) && (
              <div>
                <div className="mb-1 text-[11px] font-medium text-muted-foreground">
                  Output
                </div>
                <pre className="no-scrollbar max-h-48 overflow-auto rounded bg-background/70 p-2 font-mono text-[11px] whitespace-pre-wrap break-all">
                  {detailQuery.data
                    ? JSON.stringify(detailQuery.data.output, null, 2)
                    : item.outputPreview}
                </pre>
              </div>
            )}
            {detailQuery.isLoading && (
              <div className="text-[11px] text-muted-foreground">
                正在加载完整参数与返回值...
              </div>
            )}
          </div>
        </TraceCollapsibleContent>
      </TraceCollapsible>
    </Trace>
  );
}
