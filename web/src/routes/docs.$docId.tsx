import { useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { createFileRoute, Link } from "@tanstack/react-router";
import {
  BookOpenIcon,
  MessageSquareQuoteIcon,
  PanelLeftIcon,
  PrinterIcon,
  ScrollTextIcon,
} from "lucide-react";
import {
  createComment,
  getDocument,
  listComments,
  replyComment,
  updateCommentStatus,
} from "@/api/quire";
import { CommentsDrawer } from "@/components/report/CommentsDrawer";
import { ReportViewer } from "@/components/report/ReportViewer";
import { Button } from "@/components/ui/button";
import { useSidebar } from "@/components/ui/sidebar";
import type { CommentStatus } from "@/contracts/report-protocol";
import {
  loadPreferences,
  savePreferences,
  type ReportViewMode,
} from "@/lib/preferences";

export const Route = createFileRoute("/docs/$docId")({
  component: DocumentPage,
});

function DocumentPage() {
  const { docId } = Route.useParams();
  const queryClient = useQueryClient();
  const { toggleSidebar } = useSidebar();
  const [prefs, setPrefs] = useState(() => loadPreferences());
  const [commentsOpen, setCommentsOpen] = useState(false);
  const [selectedBlock, setSelectedBlock] = useState<string | null>(null);

  const docQuery = useQuery({
    queryKey: ["doc", docId],
    queryFn: () => getDocument(docId),
  });

  const commentsQuery = useQuery({
    queryKey: ["comments", docId],
    queryFn: () => listComments(docId, "all"),
  });

  const setViewMode = (mode: ReportViewMode) => {
    setPrefs((prev) => {
      const next = { ...prev, reportViewMode: mode };
      savePreferences(next);
      return next;
    });
  };

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
          <Button
            type="button"
            size="icon-sm"
            variant="ghost"
            aria-label="批注"
            onClick={() => {
              setSelectedBlock(null);
              setCommentsOpen(true);
            }}
          >
            <MessageSquareQuoteIcon className="size-4" />
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
        </div>
      </header>

      <div className="min-h-0 flex-1 overflow-y-auto bg-muted/20">
        {docQuery.isLoading && (
          <div className="p-8 text-center text-sm text-muted-foreground">
            正在加载报告...
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
            onOpenCommentForBlock={(blockId) => {
              setSelectedBlock(blockId);
              setCommentsOpen(true);
            }}
          />
        )}
      </div>

      <CommentsDrawer
        open={commentsOpen}
        onOpenChange={setCommentsOpen}
        threads={commentsQuery.data ?? []}
        selectedBlockId={selectedBlock}
        onAddComment={async (blockId, body) => {
          await createComment(docId, {
            block_id: blockId,
            author: "reviewer",
            body,
          });
          void queryClient.invalidateQueries({ queryKey: ["comments", docId] });
        }}
        onReply={async (threadId, body) => {
          await replyComment(docId, threadId, { author: "reviewer", body });
          void queryClient.invalidateQueries({ queryKey: ["comments", docId] });
        }}
        onUpdateStatus={async (threadId, status: CommentStatus) => {
          await updateCommentStatus(docId, threadId, status);
          void queryClient.invalidateQueries({ queryKey: ["comments", docId] });
        }}
      />
    </div>
  );
}
