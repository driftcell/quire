import { Link } from "@tanstack/react-router";
import { useEffect, useRef, type ReactNode } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { getDeepCheckStatus, triggerDeepCheck, type DeepCheckStatus } from "@/api/runtime";
import {
  BookOpenIcon,
  FileTextIcon,
  Loader2Icon,
  MessageSquareQuoteIcon,
  ShieldCheckIcon,
  PrinterIcon,
  ScrollTextIcon,
} from "lucide-react";
import { PageChromeDialog } from "@/components/report/PageChromeDialog";
import { TocOutline } from "@/components/report/TocOutline";
import { Button, buttonVariants } from "@/components/ui/button";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import type {
  CommentThread,
  PageChromeConfig,
  TocConfig,
  TocNode,
} from "@/contracts/report-protocol";
import type { ReportViewMode } from "@/lib/preferences";

export interface ReportPaneToolbarProps {
  docId: string;
  title: string;
  revision: number;
  toc: TocConfig;
  tocNodes: TocNode[];
  comments: CommentThread[];
  activeActorsBySection: Map<string, string[]>;
  viewMode: ReportViewMode;
  onViewModeChange: (mode: ReportViewMode) => void;
  onJumpToSection: (blockId: string) => void;
  onShowComments: () => void;
  openCommentsCount: number;
  streamingPreviewsCount?: number;
  onJumpToStreamingPreview?: () => void;
  themePageChrome?: PageChromeConfig | null;
  compactViewToggle?: boolean;
  className?: string;
  leading?: ReactNode;
}

export function ReportPaneToolbar({
  docId,
  title,
  revision,
  toc,
  tocNodes,
  comments,
  activeActorsBySection,
  viewMode,
  onViewModeChange,
  onJumpToSection,
  onShowComments,
  openCommentsCount,
  streamingPreviewsCount = 0,
  onJumpToStreamingPreview,
  themePageChrome,
  compactViewToggle = false,
  className = "",
  leading,
}: ReportPaneToolbarProps) {
  const queryClient = useQueryClient();
  const deepCheckQuery = useQuery({
    queryKey: ["deep-check", docId],
    queryFn: () => getDeepCheckStatus(docId),
    enabled: Boolean(docId),
    refetchInterval: (query) =>
      query.state.data?.status === "running" ? 2000 : false,
  });

  const deepCheckMutation = useMutation({
    mutationFn: () => triggerDeepCheck(docId),
    onSuccess: (next) => {
      queryClient.setQueryData<DeepCheckStatus>(["deep-check", docId], next);
      toast.info("已启动深度检查，结果将自动写入批注列表");
    },
    onError: (err: unknown) => {
      const msg = err instanceof Error ? err.message : "启动深度检查失败";
      toast.error(msg);
    },
  });

  const deepCheck = deepCheckQuery.data;
  const isDeepCheckRunning =
    deepCheck?.status === "running" || deepCheckMutation.isPending;
  const isDeepCheckEnabled = deepCheck?.enabled ?? false;
  const prevStatusRef = useRef<DeepCheckStatus["status"] | undefined>(undefined);

  useEffect(() => {
    const prev = prevStatusRef.current;
    const curr = deepCheck?.status;
    prevStatusRef.current = curr;
    if (prev === "running" && curr === "completed") {
      void queryClient.invalidateQueries({ queryKey: ["comments", docId] });
      const created = deepCheck?.created_threads ?? 0;
      toast.success(
        created > 0
          ? `深度检查完成，新增 ${created} 条批注`
          : "深度检查完成，未发现新的问题",
      );
    } else if (prev === "running" && curr === "failed") {
      toast.error(deepCheck?.error_message ?? "深度检查执行失败");
    }
  }, [deepCheck, docId, queryClient]);

  return (
    <div
      className={`flex shrink-0 items-center justify-between gap-3 border-b border-border/90 bg-card/95 px-3.5 backdrop-blur-xs ${className}`}
    >
      {/* Left Cluster: Document Folio Title & Version Ledger */}
      <div className="flex min-w-0 flex-1 items-center gap-2 overflow-hidden">
        {leading}
        <FileTextIcon
          className="hidden size-4 shrink-0 text-[#1D4ED8] @min-[480px]:inline-block"
          aria-hidden="true"
        />
        <span
          className="truncate font-serif text-[14px] font-semibold tracking-tight text-foreground"
          title={title}
        >
          {title}
        </span>

        <span
          className="hidden shrink-0 items-center gap-1 rounded-md border border-border/90 bg-secondary/80 px-1.5 py-0.5 font-mono text-[11px] font-medium text-muted-foreground tabular-nums @min-[420px]:inline-flex"
          title={`当前修订版本 ${revision}`}
        >
          版本 {revision}
        </span>

        {streamingPreviewsCount > 0 && (
          <Tooltip>
            <TooltipTrigger
              render={
                <button
                  type="button"
                  aria-label={`实时撰写中 ${streamingPreviewsCount} 块`}
                  onClick={onJumpToStreamingPreview}
                  className="inline-flex shrink-0 items-center gap-1.5 rounded-full border border-emerald-500/35 bg-emerald-500/10 px-2.5 py-0.5 text-[11px] font-medium text-emerald-700 transition-colors hover:bg-emerald-500/20 dark:text-emerald-300"
                />
              }
            >
              <span className="size-1.5 animate-pulse rounded-full bg-emerald-500" />
              <span className="tabular-nums">
                实时撰写中 · {streamingPreviewsCount}
              </span>
            </TooltipTrigger>
            <TooltipContent>
              {viewMode === "paged"
                ? "点击切换至连续视图并定位到正在流式撰写的章节块"
                : "点击定位到正在流式撰写的章节块"}
            </TooltipContent>
          </Tooltip>
        )}
      </div>

      {/* Right Cluster: Structured Editorial Command Bar */}
      <div className="flex shrink-0 items-center gap-1.5">
        {/* Group 1: Document Structure & Page Setup */}
        <div className="flex items-center gap-0.5">
          <TocOutline
            placement="bottom"
            docId={docId}
            toc={toc}
            isPaged={viewMode === "paged"}
            nodes={tocNodes}
            activeActorsBySection={activeActorsBySection}
            comments={comments}
            onJumpToSection={onJumpToSection}
          />

          {viewMode === "paged" && (
            <PageChromeDialog docId={docId} themePageChrome={themePageChrome} />
          )}
        </div>

        <span className="h-4 w-px bg-border/80" aria-hidden="true" />

        {/* Group 2: Review Annotations & Print/PDF Export */}
        <div className="flex items-center gap-1">
          {isDeepCheckEnabled ? (
            <Tooltip>
              <TooltipTrigger
                render={
                  <Button
                    type="button"
                    size="sm"
                    variant="ghost"
                    aria-label="深度检查"
                    disabled={isDeepCheckRunning}
                    className="h-7 gap-1.5 rounded-md px-2 text-xs font-medium text-foreground/85 hover:bg-secondary hover:text-foreground disabled:opacity-50"
                    onClick={() => deepCheckMutation.mutate()}
                  />
                }
              >
                {isDeepCheckRunning ? (
                  <Loader2Icon className="size-3.5 animate-spin text-[#1D4ED8]" />
                ) : (
                  <ShieldCheckIcon className="size-3.5 text-muted-foreground" />
                )}
                <span className="hidden @min-[540px]:inline">
                  {isDeepCheckRunning ? "检查中..." : "深度检查"}
                </span>
              </TooltipTrigger>
              <TooltipContent>
                {isDeepCheckRunning
                  ? "正在检查全文，完成后自动生成批注"
                  : "语义与数据一致性深度检查"}
              </TooltipContent>
            </Tooltip>
          ) : null}

          <Tooltip>
            <TooltipTrigger
              render={
                <Button
                  type="button"
                  size="sm"
                  variant="ghost"
                  aria-label="批注列表"
                  className="relative h-7 gap-1.5 rounded-md px-2 text-xs font-medium text-foreground/85 hover:bg-secondary hover:text-foreground"
                  onClick={onShowComments}
                />
              }
            >
              <MessageSquareQuoteIcon className="size-3.5 text-muted-foreground" />
              <span className="hidden @min-[540px]:inline">批注</span>
              {openCommentsCount > 0 && (
                <span className="inline-flex h-4 min-w-4 items-center justify-center rounded-full bg-amber-500/15 px-1 font-mono text-[10.5px] font-semibold text-amber-700 tabular-nums">
                  {openCommentsCount}
                </span>
              )}
            </TooltipTrigger>
            <TooltipContent>审阅批注列表</TooltipContent>
          </Tooltip>

          <Tooltip>
            <TooltipTrigger
              render={
                <Link
                  to="/docs/$docId/print"
                  params={{ docId }}
                  target="_blank"
                  rel="noreferrer"
                  aria-label="导出 PDF / 打印视图"
                  className={buttonVariants({
                    variant: "ghost",
                    size: "sm",
                    className:
                      "h-7 gap-1.5 rounded-md px-2 text-xs font-medium text-foreground/85 hover:bg-secondary hover:text-foreground",
                  })}
                />
              }
            >
              <PrinterIcon className="size-3.5 text-muted-foreground" />
              <span className="hidden @min-[580px]:inline">导出 PDF</span>
            </TooltipTrigger>
            <TooltipContent>导出 A4 PDF 或打开打印预览</TooltipContent>
          </Tooltip>
        </div>

        <span className="h-4 w-px bg-border/80" aria-hidden="true" />

        {/* Group 3: Canvas Layout Mode Switcher */}
        <ToggleGroup
          multiple={false}
          value={[viewMode]}
          onValueChange={(values) => {
            const next = values[0];
            if (next === "continuous" || next === "paged") {
              onViewModeChange(next);
            }
          }}
          spacing={0}
          className="rounded-lg border border-border/90 bg-secondary/75 p-0.5"
        >
          <ToggleGroupItem
            value="continuous"
            size="sm"
            className="h-6 min-w-0 rounded-md px-2 text-xs text-muted-foreground transition-colors aria-pressed:bg-card aria-pressed:font-medium aria-pressed:text-foreground aria-pressed:shadow-2xs"
          >
            <ScrollTextIcon
              className={`size-3.5 ${compactViewToggle ? "@min-[540px]:mr-1" : "mr-1"}`}
            />
            <span
              className={compactViewToggle ? "hidden @min-[540px]:inline" : ""}
            >
              连续
            </span>
          </ToggleGroupItem>
          <ToggleGroupItem
            value="paged"
            size="sm"
            className="h-6 min-w-0 rounded-md px-2 text-xs text-muted-foreground transition-colors aria-pressed:bg-card aria-pressed:font-medium aria-pressed:text-foreground aria-pressed:shadow-2xs"
          >
            <BookOpenIcon
              className={`size-3.5 ${compactViewToggle ? "@min-[540px]:mr-1" : "mr-1"}`}
            />
            <span
              className={compactViewToggle ? "hidden @min-[540px]:inline" : ""}
            >
              分页
            </span>
          </ToggleGroupItem>
        </ToggleGroup>
      </div>
    </div>
  );
}
