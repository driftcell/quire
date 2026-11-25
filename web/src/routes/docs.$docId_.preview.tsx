import { useEffect, useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { createFileRoute } from "@tanstack/react-router";
import { getDocument } from "@/api/quire";
import { ReportViewer } from "@/components/report/ReportViewer";
import type { ReportBlock, ReportDocumentView } from "@/lib/document";
import type { ReportViewMode } from "@/lib/preferences";

interface PreviewSearch {
  mode?: ReportViewMode;
  sectionId?: string;
  blockIds?: string;
  page?: number;
  rev?: number;
}

export const Route = createFileRoute("/docs/$docId_/preview")({
  validateSearch: (search: Record<string, unknown>): PreviewSearch => {
    const mode = search.mode === "continuous" ? "continuous" : "paged";
    const sectionId =
      typeof search.sectionId === "string" && search.sectionId.trim()
        ? search.sectionId.trim()
        : undefined;
    const blockIds =
      typeof search.blockIds === "string" && search.blockIds.trim()
        ? search.blockIds.trim()
        : undefined;
    const rawPage = Number(search.page);
    const page =
      Number.isFinite(rawPage) && rawPage >= 1
        ? Math.floor(rawPage)
        : undefined;
    const rawRev = Number(search.rev);
    if (
      search.rev !== undefined &&
      (!Number.isSafeInteger(rawRev) || rawRev < 1)
    ) {
      throw new Error("预览版本号必须是正整数。");
    }
    const rev = search.rev === undefined ? undefined : rawRev;
    return { mode, sectionId, blockIds, page, rev };
  },
  component: DocumentPreviewPage,
});

function readHashRoToken(): string | undefined {
  if (typeof window === "undefined") return undefined;
  const hash = window.location.hash.replace(/^#/, "");
  if (!hash) return undefined;
  const params = new URLSearchParams(hash);
  const token = params.get("ro_token")?.trim();
  return token && token.length > 0 ? token : undefined;
}

function sliceBlocksBySection(
  blocks: ReportBlock[],
  sectionId: string,
): ReportBlock[] {
  const startIdx = blocks.findIndex((b) => b.id === sectionId);
  if (startIdx === -1) return [];
  const startBlock = blocks[startIdx];
  if (!startBlock) return [];
  const startLevel =
    startBlock.type === "heading" ? (startBlock.level ?? 2) : 6;
  const out: ReportBlock[] = [startBlock];
  for (let i = startIdx + 1; i < blocks.length; i += 1) {
    const b = blocks[i];
    if (!b) continue;
    if (b.type === "heading" && (b.level ?? 2) <= startLevel) {
      break;
    }
    out.push(b);
  }
  return out;
}

function DocumentPreviewPage() {
  const { docId } = Route.useParams();
  const search = Route.useSearch();
  const mode: ReportViewMode = search.mode ?? "paged";
  const roToken = useMemo(() => readHashRoToken(), []);
  const [totalPages, setTotalPages] = useState(1);
  const [readyDocKey, setReadyDocKey] = useState<string | null>(null);

  const docQuery = useQuery({
    queryKey: ["doc-preview", docId, search.rev ?? "latest"],
    queryFn: () => getDocument(docId, roToken, search.rev),
    retry: false,
  });

  const previewDocKey = `${docId}:${docQuery.data?.current_rev ?? 0}`;
  const previewReady = readyDocKey === previewDocKey && docQuery.isSuccess;
  const isPartial = Boolean(search.blockIds || search.sectionId);

  const filteredDocument: ReportDocumentView | null = useMemo(() => {
    if (!docQuery.data) return null;
    const doc = docQuery.data.document;
    let blocks = doc.blocks;
    if (search.blockIds) {
      const wanted = new Set(
        search.blockIds
          .split(",")
          .map((s) => s.trim())
          .filter(Boolean),
      );
      blocks = blocks.filter((b) => wanted.has(b.id));
    } else if (search.sectionId) {
      blocks = sliceBlocksBySection(blocks, search.sectionId);
    }
    return {
      ...doc,
      cover: isPartial ? { ...doc.cover, enabled: false } : doc.cover,
      toc: isPartial ? { ...doc.toc, enabled: false } : doc.toc,
      references: isPartial
        ? { ...doc.references, enabled: false }
        : doc.references,
      blocks,
    };
  }, [docQuery.data, isPartial, search.blockIds, search.sectionId]);

  const filteredComputed = useMemo(() => {
    if (!docQuery.data) return null;
    if (!isPartial) return docQuery.data.computed;
    return {
      ...docQuery.data.computed,
      toc_tree: [],
    };
  }, [docQuery.data, isPartial]);

  useEffect(() => {
    if (!docQuery.isSuccess || !filteredDocument || !docQuery.data) {
      return undefined;
    }
    let cancelled = false;
    const docData = docQuery.data;

    const markReady = async () => {
      if ("fonts" in window.document) {
        await window.document.fonts.ready;
      }
      await new Promise<void>((resolve) => {
        window.requestAnimationFrame(() => {
          window.requestAnimationFrame(() => resolve());
        });
      });
      if (cancelled) return;
      Reflect.set(window, "__QUIRE_PREVIEW_META__", {
        docId,
        revision: docData.current_rev,
        mode,
        totalPages,
        themeTokens: docData.document.theme.tokens ?? {},
        customCss: docData.document.theme.custom_css ?? "",
      });
      setReadyDocKey(previewDocKey);
    };

    void markReady();
    return () => {
      cancelled = true;
    };
  }, [
    docQuery.isSuccess,
    docQuery.data,
    filteredDocument,
    docId,
    mode,
    previewDocKey,
    totalPages,
  ]);

  return (
    <div
      data-preview-root="true"
      data-preview-ready={previewReady ? "true" : "false"}
      data-doc-id={docId}
      data-revision={docQuery.data?.current_rev ?? 0}
      data-total-pages={totalPages}
      className="min-h-svh bg-neutral-100"
    >
      {docQuery.isLoading && (
        <div className="p-12 text-center text-sm text-muted-foreground">
          正在渲染预览页面…
        </div>
      )}
      {docQuery.error && (
        <div
          data-preview-error="true"
          className="m-6 rounded-lg border border-destructive/40 bg-destructive/10 p-4 text-sm text-destructive"
        >
          预览加载失败：{docQuery.error.message}
        </div>
      )}
      {filteredDocument && filteredComputed && (
        <ReportViewer
          document={filteredDocument}
          computed={filteredComputed}
          viewMode={mode}
          previewMode
          pageFilter={mode === "paged" ? search.page : undefined}
          onPagesReady={setTotalPages}
        />
      )}
    </div>
  );
}
