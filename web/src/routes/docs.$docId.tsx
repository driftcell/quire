import { startTransition, useCallback, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { createFileRoute } from "@tanstack/react-router";
import { PanelLeftIcon } from "lucide-react";
import { getDocument, listComments } from "@/api/quire";
import { CommentsPanel } from "@/components/report/CommentsDrawer";
import { ReportPaneToolbar } from "@/components/report/ReportPaneToolbar";
import { ReportViewer } from "@/components/report/ReportViewer";
import { flashHighlightElement } from "@/components/report/reportDom";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { useSidebar } from "@/components/ui/sidebar";
import {
  loadPreferences,
  savePreferences,
  type ReportViewMode,
} from "@/lib/preferences";

export const Route = createFileRoute("/docs/$docId")({
  component: DocumentPage,
});

const EMPTY_ACTORS = new Map<string, string[]>();

function DocumentPage() {
  const { docId } = Route.useParams();
  const { toggleSidebar } = useSidebar();
  const [prefs, setPrefs] = useState(() => loadPreferences());
  const [commentsOpen, setCommentsOpen] = useState(false);
  const [activeCommentThreadId, setActiveCommentThreadId] = useState<
    string | null
  >(null);

  const docQuery = useQuery({
    queryKey: ["doc", docId],
    queryFn: () => getDocument(docId),
  });

  const commentsQuery = useQuery({
    queryKey: ["comments", docId],
    queryFn: () => listComments(docId, "all"),
  });

  const setViewMode = (mode: ReportViewMode) => {
    startTransition(() => {
      setPrefs((prev) => {
        const next = { ...prev, reportViewMode: mode };
        savePreferences(next);
        return next;
      });
    });
  };

  const jumpToSection = useCallback((sectionId: string) => {
    const el = window.document.getElementById(`block-${sectionId}`);
    if (!el) return;
    el.scrollIntoView({ behavior: "smooth", block: "start" });
    flashHighlightElement(el);
  }, []);

  const openCommentsCount =
    commentsQuery.data?.filter((c) => c.status !== "archived").length ?? 0;

  return (
    <div className="flex h-full flex-col overflow-hidden">
      {docQuery.data ? (
        <ReportPaneToolbar
          className="h-12"
          docId={docId}
          title={docQuery.data.document.cover.title || docId}
          revision={docQuery.data.current_rev}
          toc={docQuery.data.document.toc}
          tocNodes={docQuery.data.computed.toc_tree}
          comments={commentsQuery.data ?? []}
          activeActorsBySection={EMPTY_ACTORS}
          viewMode={prefs.reportViewMode}
          onViewModeChange={setViewMode}
          onJumpToSection={jumpToSection}
          onShowComments={() => {
            setActiveCommentThreadId(null);
            setCommentsOpen(true);
          }}
          openCommentsCount={openCommentsCount}
          themePageChrome={docQuery.data.document.theme.page_chrome}
          leading={
            <Button
              type="button"
              size="icon-sm"
              variant="ghost"
              aria-label="切换左侧栏"
              onClick={toggleSidebar}
            >
              <PanelLeftIcon className="size-4" />
            </Button>
          }
        />
      ) : (
        <header className="flex h-12 shrink-0 items-center gap-2 border-b px-3">
          <Button
            type="button"
            size="icon-sm"
            variant="ghost"
            aria-label="切换左侧栏"
            onClick={toggleSidebar}
          >
            <PanelLeftIcon className="size-4" />
          </Button>
          <span className="truncate text-sm font-medium">{docId}</span>
        </header>
      )}

      <div className="min-h-0 flex-1 overflow-y-auto overscroll-contain bg-muted/20">
        {docQuery.isLoading && (
          <div className="mx-auto max-w-[760px] space-y-4 px-5 py-10">
            <Skeleton className="h-9 w-2/3" />
            <Skeleton className="h-4 w-1/3" />
            <Skeleton className="mt-6 h-24 w-full" />
            <Skeleton className="h-40 w-full" />
          </div>
        )}
        {docQuery.error && (
          <div className="m-6 rounded-lg border border-destructive/40 bg-destructive/10 p-4 text-sm text-destructive">
            加载报告失败：{docQuery.error.message}
          </div>
        )}
        {docQuery.data && (
          <ReportViewer
            document={docQuery.data.document}
            computed={docQuery.data.computed}
            viewMode={prefs.reportViewMode}
            comments={commentsQuery.data}
            onOpenCommentThread={(threadId, blockId) => {
              setActiveCommentThreadId(threadId);
              setCommentsOpen(true);
              jumpToSection(blockId);
            }}
            onJumpToSection={jumpToSection}
          />
        )}
      </div>

      {commentsOpen && (
        <div className="fixed top-14 right-2 bottom-2 z-30 w-[380px] max-w-[calc(100vw-1rem)] overflow-hidden rounded-xl border bg-background shadow-xl">
          <CommentsPanel
            docId={docId}
            threads={commentsQuery.data ?? []}
            activeThreadId={activeCommentThreadId}
            readOnly
            onClose={() => setCommentsOpen(false)}
            onSelectThread={(thread) => {
              setActiveCommentThreadId(thread.thread_id);
              jumpToSection(thread.block_id);
            }}
            onSelectBlock={(blockId) => {
              jumpToSection(blockId);
            }}
          />
        </div>
      )}
    </div>
  );
}
