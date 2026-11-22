import { Link } from "@tanstack/react-router";
import type { ReactNode } from "react";
import {
  BookOpenIcon,
  MessageSquareQuoteIcon,
  PrinterIcon,
  ScrollTextIcon,
} from "lucide-react";
import { PageChromeDialog } from "@/components/report/PageChromeDialog";
import { TocOutline } from "@/components/report/TocOutline";
import { Badge } from "@/components/ui/badge";
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
  return (
    <div
      className={`flex shrink-0 items-center justify-between gap-2 border-b bg-background/95 px-3 ${className}`}
    >
      <div className="flex min-w-0 flex-1 items-center gap-2 overflow-hidden">
        {leading}
        <span className="truncate text-sm font-medium">{title}</span>
        <Badge
          variant="outline"
          className="hidden shrink-0 text-[11px] tabular-nums @min-[420px]:inline-flex"
        >
          版本 {revision}
        </Badge>
        {streamingPreviewsCount > 0 && (
          <Tooltip>
            <TooltipTrigger
              render={
                <button
                  type="button"
                  aria-label={`实时撰写中 ${streamingPreviewsCount} 块`}
                  onClick={onJumpToStreamingPreview}
                  className="inline-flex shrink-0 items-center gap-1.5 rounded-full border border-emerald-500/30 bg-emerald-500/10 px-2 py-0.5 text-[11px] font-medium text-emerald-700 transition-colors hover:bg-emerald-500/20 dark:text-emerald-300"
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

      <div className="flex shrink-0 items-center gap-1">
        {viewMode === "paged" && (
          <PageChromeDialog docId={docId} themePageChrome={themePageChrome} />
        )}

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

        <Tooltip>
          <TooltipTrigger
            render={
              <Button
                type="button"
                size="icon-sm"
                variant="ghost"
                aria-label="批注列表"
                className="relative"
                onClick={onShowComments}
              />
            }
          >
            <MessageSquareQuoteIcon className="size-4" />
            {openCommentsCount > 0 && (
              <span className="absolute -top-0.5 -right-0.5 flex size-4 items-center justify-center rounded-full bg-amber-500 text-[10px] font-bold text-white tabular-nums">
                {openCommentsCount}
              </span>
            )}
          </TooltipTrigger>
          <TooltipContent>批注列表</TooltipContent>
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
                  size: "icon-sm",
                })}
              />
            }
          >
            <PrinterIcon className="size-4" />
          </TooltipTrigger>
          <TooltipContent>导出 PDF / 打印视图</TooltipContent>
        </Tooltip>

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
          className="rounded-lg border bg-muted/40 p-0.5"
        >
          <ToggleGroupItem
            value="continuous"
            size="sm"
            className="h-6 min-w-0 rounded-[min(var(--radius-md),10px)] px-2 text-xs aria-pressed:bg-background aria-pressed:font-medium aria-pressed:text-foreground aria-pressed:shadow-xs"
          >
            <ScrollTextIcon
              className={`size-3.5 ${compactViewToggle ? "@min-[560px]:mr-1" : "mr-1"}`}
            />
            <span
              className={compactViewToggle ? "hidden @min-[560px]:inline" : ""}
            >
              连续
            </span>
          </ToggleGroupItem>
          <ToggleGroupItem
            value="paged"
            size="sm"
            className="h-6 min-w-0 rounded-[min(var(--radius-md),10px)] px-2 text-xs aria-pressed:bg-background aria-pressed:font-medium aria-pressed:text-foreground aria-pressed:shadow-xs"
          >
            <BookOpenIcon
              className={`size-3.5 ${compactViewToggle ? "@min-[560px]:mr-1" : "mr-1"}`}
            />
            <span
              className={compactViewToggle ? "hidden @min-[560px]:inline" : ""}
            >
              分页
            </span>
          </ToggleGroupItem>
        </ToggleGroup>
      </div>
    </div>
  );
}
