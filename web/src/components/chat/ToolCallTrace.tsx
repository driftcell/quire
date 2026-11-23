import { useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { ChevronRightIcon } from "lucide-react";
import { cn } from "cn";
import { getToolCall } from "@/api/runtime";
import { getToolMeta } from "@/components/chat/toolMeta";

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
        extractJsonField(raw, "section_id") ??
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

  return (
    <div
      data-tool-name={item.toolName}
      data-tool-status={item.status}
      data-slot="tool-trace"
      className="w-full min-w-0"
    >
      <button
        type="button"
        aria-expanded={open}
        onClick={() => setOpen((prev) => !prev)}
        className={cn(
          "group/tool flex min-h-6 w-full max-w-full items-center gap-2 rounded-md px-1.5 py-0.5 text-left text-xs transition-colors hover:bg-muted/60 focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none",
          item.status === "running"
            ? "text-foreground"
            : item.status === "failed"
              ? "text-destructive"
              : "text-muted-foreground hover:text-foreground",
        )}
      >
        <span className="flex size-4 shrink-0 items-center justify-center">
          <Icon
            className={cn(
              "size-3.5",
              item.status === "running"
                ? "text-primary"
                : item.status === "failed"
                  ? "text-destructive"
                  : "text-muted-foreground",
            )}
          />
        </span>
        <span className="shrink-0 font-medium">{meta.label}</span>
        {inlineSummary && (
          <span className="max-w-[230px] truncate rounded bg-muted/70 px-1.5 py-0.2 font-mono text-[10.5px] text-muted-foreground">
            {inlineSummary}
          </span>
        )}
        <span className="ml-auto flex shrink-0 items-center gap-1.5 text-[11px] tabular-nums text-muted-foreground/75">
          {item.status === "running" && (
            <span
              data-slot="tool-running-dot"
              aria-hidden="true"
              className="size-1.5 shrink-0 animate-pulse rounded-full bg-primary/80"
            />
          )}
          {item.status === "skipped" && <span>已跳过</span>}
          {item.status === "failed" && (
            <span className="text-destructive">失败</span>
          )}
          {durationBadge && <span>{durationBadge}</span>}
          <ChevronRightIcon
            className={cn(
              "size-3 opacity-0 transition-all group-hover/tool:opacity-70",
              open && "rotate-90 opacity-70",
            )}
          />
        </span>
      </button>

      <div
        className={cn(
          "grid transition-[grid-template-rows,opacity] duration-150 ease-out",
          open ? "mt-1 grid-rows-[1fr] opacity-100" : "grid-rows-[0fr] opacity-0",
        )}
      >
        <div className="min-h-0 overflow-hidden pl-6">
          <div className="space-y-2 rounded-lg border bg-muted/30 p-2.5 text-xs">
            <div className="font-mono text-[11px] text-muted-foreground">
              {item.toolName}
            </div>
            <div>
              <div className="mb-1 text-[11px] font-medium text-muted-foreground">
                Input
              </div>
              <pre className="no-scrollbar max-h-40 overflow-auto rounded bg-background/80 p-2 font-mono text-[11px] whitespace-pre-wrap break-all">
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
                <pre className="no-scrollbar max-h-48 overflow-auto rounded bg-background/80 p-2 font-mono text-[11px] whitespace-pre-wrap break-all">
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
        </div>
      </div>
    </div>
  );
}
