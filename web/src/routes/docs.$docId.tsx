import { startTransition, useCallback, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { createFileRoute, Link } from "@tanstack/react-router";
import {
  BookOpenIcon,
  MessageSquareQuoteIcon,
  PanelLeftIcon,
  PrinterIcon,
  ScrollTextIcon,
} from "lucide-react";
import { getDocument, listComments } from "@/api/quire";
import { CommentsPanel } from "@/components/report/CommentsDrawer";
import { ReportViewer } from "@/components/report/ReportViewer";
import { PageChromeDialog } from "@/components/report/PageChromeDialog";
import { TocOutline } from "@/components/report/TocOutline";
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
    el.classList.remove("quire-flash-highlight");
    void el.offsetWidth;
    el.classList.add("quire-flash-highlight");
    window.setTimeout(() => {
      el.classList.remove("quire-flash-highlight");
    }, 1600);
  }, []);

  const openCommentsCount =
    commentsQuery.data?.filter((c) => c.status !== "archived").length ?? 0;

  return (
    <div className="flex h-full flex-col overflow-hidden">
      <header className="flex h-12 shrink-0 items-center justify-between border-b px-3">
        <div className="flex items-center gap-2">
          <Button
            type="button"
            size="icon-sm"
            variant="ghost"
            aria-label="切换左侧栏"
            onClick={toggleSidebar}
          >
            <PanelLeftIcon className="size-4" />
          </Button>
          <span className="truncate text-sm font-medium">
            {docQuery.data?.document.cover.title || docId}
          </span>
        </div>

        <div className="flex items-center gap-1.5">
          {prefs.reportViewMode === "paged" && docQuery.data && (
            <PageChromeDialog
              docId={docId}
              themePageChrome={docQuery.data.document.theme.page_chrome}
            />
          )}
          {docQuery.data && (
            <TocOutline
              docId={docId}
              toc={docQuery.data.document.toc}
              isPaged={prefs.reportViewMode === "paged"}
              nodes={docQuery.data.computed.toc_tree}
              activeActorsBySection={EMPTY_ACTORS}
              comments={commentsQuery.data ?? []}
              onJumpToSection={jumpToSection}
            />
          )}
          <Button
            type="button"
            size="icon-sm"
            variant="ghost"
            aria-label="批注列表"
            className="relative"
            onClick={() => {
              setActiveCommentThreadId(null);
              setCommentsOpen(true);
            }}
          >
            <MessageSquareQuoteIcon className="size-4" />
            {openCommentsCount > 0 && (
              <span className="absolute -top-0.5 -right-0.5 flex size-4 items-center justify-center rounded-full bg-amber-500 text-[10px] font-bold text-white tabular-nums">
                {openCommentsCount}
              </span>
            )}
          </Button>
          <Button
            size="icon-sm"
            variant="ghost"
            aria-label="打印视图"
            nativeButton={false}
            render={
              <Link
                to="/docs/$docId/print"
                params={{ docId }}
                target="_blank"
              />
            }
          >
            <PrinterIcon className="size-4" />
          </Button>
          <div className="flex items-center rounded-lg border bg-muted/40 p-0.5">
            <Button
              type="button"
              size="xs"
              variant={
                prefs.reportViewMode === "continuous" ? "default" : "ghost"
              }
              onClick={() => setViewMode("continuous")}
            >
              <ScrollTextIcon className="mr-1 size-3.5" />
              连续
            </Button>
            <Button
              type="button"
              size="xs"
              variant={prefs.reportViewMode === "paged" ? "default" : "ghost"}
              onClick={() => setViewMode("paged")}
            >
              <BookOpenIcon className="mr-1 size-3.5" />
              分页
            </Button>
          </div>
        </div>
      </header>

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
