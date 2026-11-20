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

        <Button
          type="button"
          size="icon-sm"
          variant="ghost"
          aria-label="批注列表"
          className="relative"
          onClick={onShowComments}
        >
          <MessageSquareQuoteIcon className="size-4" />
          {openCommentsCount > 0 && (
            <span className="absolute -top-0.5 -right-0.5 flex size-4 items-center justify-center rounded-full bg-amber-500 text-[10px] font-bold text-white tabular-nums">
              {openCommentsCount}
            </span>
          )}
        </Button>

        <Link
          to="/docs/$docId/print"
          params={{ docId }}
          target="_blank"
          rel="noreferrer"
          aria-label="导出 PDF / 打印视图"
          title="导出 PDF / 打印视图"
          className={buttonVariants({ variant: "ghost", size: "icon-sm" })}
        >
          <PrinterIcon className="size-4" />
        </Link>

        <div className="flex items-center rounded-lg border bg-muted/40 p-0.5">
          <Button
            type="button"
            size="xs"
            variant={viewMode === "continuous" ? "default" : "ghost"}
            onClick={() => onViewModeChange("continuous")}
          >
            <ScrollTextIcon
              className={`size-3.5 ${compactViewToggle ? "@min-[560px]:mr-1" : "mr-1"}`}
            />
            <span
              className={compactViewToggle ? "hidden @min-[560px]:inline" : ""}
            >
              连续
            </span>
          </Button>
          <Button
            type="button"
            size="xs"
            variant={viewMode === "paged" ? "default" : "ghost"}
            onClick={() => onViewModeChange("paged")}
          >
            <BookOpenIcon
              className={`size-3.5 ${compactViewToggle ? "@min-[560px]:mr-1" : "mr-1"}`}
            />
            <span
              className={compactViewToggle ? "hidden @min-[560px]:inline" : ""}
            >
              分页
            </span>
          </Button>
        </div>
      </div>
    </div>
  );
}
