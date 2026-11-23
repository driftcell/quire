import { memo, useCallback, useMemo, useRef } from "react";
import { SidebarEdgeHandle } from "@/components/layout/SidebarEdgeHandle";
import type { FloatingCommentAnchor } from "@/components/report/FloatingCommentPopover";
import { ReportPaneToolbar } from "@/components/report/ReportPaneToolbar";
import { ReportViewer } from "@/components/report/ReportViewer";
import { flashHighlightElement } from "@/components/report/reportDom";
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
  onSelectCommentThread?: (thread: CommentThread) => void;
  onAskAgentWithQuote: (selection: ReportSelectionDescriptor) => void;
  activeSelection?: ReportSelectionDescriptor | null;
  onActiveSelectionChange?: (
    selection: ReportSelectionDescriptor | null,
  ) => void;
}

export const SessionReportPane = memo(function SessionReportPane({
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
  onSelectCommentThread,
  onAskAgentWithQuote,
  activeSelection,
  onActiveSelectionChange,
}: SessionReportPaneProps) {
  const scrollContainerRef = useRef<HTMLDivElement>(null);

  const streamingPreviews = useMemo(
    () =>
      Array.from(ephemeralPreviews.values()).filter(
        (preview) => preview.status === "streaming",
      ),
    [ephemeralPreviews],
  );

  const handleJumpToStreamingPreview = useCallback(() => {
    const latest = streamingPreviews[streamingPreviews.length - 1];
    if (!latest) {
      return;
    }
    const scrollToPreview = () => {
      const container = scrollContainerRef.current ?? window.document;
      const previewEl = container.querySelector<HTMLElement>(
        `[data-preview-id="${latest.preview_id}"]`,
      );
      if (previewEl) {
        previewEl.scrollIntoView({ behavior: "smooth", block: "center" });
        flashHighlightElement(previewEl);
        return;
      }
      if (latest.block.block_id) {
        onJumpToSection(latest.block.block_id);
      }
    };
    if (viewMode === "paged") {
      onViewModeChange("continuous");
      window.setTimeout(scrollToPreview, 60);
    } else {
      scrollToPreview();
    }
  }, [onJumpToSection, onViewModeChange, streamingPreviews, viewMode]);

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
          streamingPreviewsCount={streamingPreviews.length}
          onJumpToStreamingPreview={handleJumpToStreamingPreview}
          themePageChrome={document.theme.page_chrome}
          compactViewToggle
        />
      )}

      {!isMobile && (
        <SidebarEdgeHandle
          side="right"
          open={rightSidebarOpen}
          label={rightSidebarOpen ? "收起右侧对话栏" : "展开右侧对话栏"}
          onToggle={onToggleRightSidebar}
          tooltip
        />
      )}

      <div
        ref={scrollContainerRef}
        className="no-scrollbar flex-1 overflow-x-hidden overflow-y-auto overscroll-contain"
      >
        <ReportViewer
          document={document}
          computed={computed}
          viewMode={viewMode}
          ephemeralPreviews={ephemeralPreviews}
          comments={comments}
          activeThreadId={activeThreadId}
          onOpenCommentForBlock={onOpenCommentForBlock}
          onSelectCommentThread={onSelectCommentThread}
          onAskAgentWithQuote={onAskAgentWithQuote}
          onJumpToSection={onJumpToSection}
          activeSelection={activeSelection}
          onActiveSelectionChange={onActiveSelectionChange}
        />
      </div>
    </div>
  );
});
