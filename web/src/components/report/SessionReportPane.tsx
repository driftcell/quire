import { ChevronLeftIcon, ChevronRightIcon } from "lucide-react";
import type { FloatingCommentAnchor } from "@/components/report/FloatingCommentPopover";
import { ReportPaneToolbar } from "@/components/report/ReportPaneToolbar";
import { ReportViewer } from "@/components/report/ReportViewer";
import type { ReportSelectionDescriptor } from "@/components/report/reportSelection";
import type {
  CommentThread,
  ComputedDocumentMeta,
  EphemeralPreviewEvent,
} from "@/contracts/report-protocol";
import type { ReportDocumentView } from "@/lib/document";
import type { ReportViewMode } from "@/lib/preferences";

export interface SessionReportPaneProps {
  docId: string;
  document: ReportDocumentView;
  computed: ComputedDocumentMeta;
  currentRev: number;
  viewMode: ReportViewMode;
  ephemeralPreviews: Map<string, EphemeralPreviewEvent>;
  comments: CommentThread[];
  activeThreadId?: string | null;
  activeActorsBySection: Map<string, string[]>;
  openCommentsCount: number;
  isMobile: boolean;
  rightSidebarOpen: boolean;
  onToggleRightSidebar: () => void;
  onViewModeChange: (mode: ReportViewMode) => void;
  onJumpToSection: (blockId: string) => void;
  onShowComments: () => void;
  onOpenCommentForBlock: (anchor: FloatingCommentAnchor) => void;
  onAskAgentWithQuote: (selection: ReportSelectionDescriptor) => void;
}

export function SessionReportPane({
  docId,
  document,
  computed,
  currentRev,
  viewMode,
  ephemeralPreviews,
  comments,
  activeThreadId,
  activeActorsBySection,
  openCommentsCount,
  isMobile,
  rightSidebarOpen,
  onToggleRightSidebar,
  onViewModeChange,
  onJumpToSection,
  onShowComments,
  onOpenCommentForBlock,
  onAskAgentWithQuote,
}: SessionReportPaneProps) {
  return (
    <div
      className={
        isMobile
          ? "@container relative flex h-full min-w-0 flex-col overflow-hidden bg-background"
          : "@container relative flex h-full min-w-0 flex-col overflow-hidden rounded-xl border bg-background shadow-2xs"
      }
    >
      {!isMobile && (
        <ReportPaneToolbar
          className="h-11"
          docId={docId}
          title={document.cover.title || docId}
          revision={currentRev}
          toc={document.toc}
          tocNodes={computed.toc_tree}
          comments={comments}
          activeActorsBySection={activeActorsBySection}
          viewMode={viewMode}
          onViewModeChange={onViewModeChange}
          onJumpToSection={onJumpToSection}
          onShowComments={onShowComments}
          openCommentsCount={openCommentsCount}
          themePageChrome={document.theme.page_chrome}
          compactViewToggle
        />
      )}

      {!isMobile && (
        <button
          type="button"
          title={rightSidebarOpen ? "收起右侧对话栏" : "展开右侧对话栏"}
          aria-label={rightSidebarOpen ? "收起右侧对话栏" : "展开右侧对话栏"}
          onClick={onToggleRightSidebar}
          className="focus-visible:ring-ring absolute top-1/2 right-0 z-30 flex h-12 w-4 -translate-y-1/2 items-center justify-center rounded-l-md border border-r-0 bg-background/90 text-muted-foreground opacity-60 shadow-xs backdrop-blur transition-opacity hover:w-5 hover:text-foreground hover:opacity-100 focus-visible:opacity-100 focus-visible:ring-2 focus-visible:outline-none"
        >
          {rightSidebarOpen ? (
            <ChevronRightIcon className="size-3.5" />
          ) : (
            <ChevronLeftIcon className="size-3.5" />
          )}
        </button>
      )}

      <div className="no-scrollbar flex-1 overflow-x-hidden overflow-y-auto overscroll-contain">
        <ReportViewer
          document={document}
          computed={computed}
          viewMode={viewMode}
          ephemeralPreviews={ephemeralPreviews}
          comments={comments}
          activeThreadId={activeThreadId}
          onOpenCommentForBlock={onOpenCommentForBlock}
          onAskAgentWithQuote={onAskAgentWithQuote}
          onJumpToSection={onJumpToSection}
        />
      </div>
    </div>
  );
}
