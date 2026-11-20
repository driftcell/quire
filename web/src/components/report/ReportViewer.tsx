import { useCallback, useEffect, useMemo, useRef } from "react";
import { EphemeralPreviewCard } from "@/components/report/EphemeralPreviewCard";
import type { FloatingCommentAnchor } from "@/components/report/FloatingCommentPopover";
import type { QuoteHighlight } from "@/components/report/InlineMarkdown";
import {
  buildMixedItems,
  mixPreviewsIntoPages,
  type MixedRenderItem,
} from "@/components/report/ephemeralPreviewMix";
import { ReportBibliography } from "@/components/report/ReportBibliography";
import { ReportBlockItem } from "@/components/report/ReportBlockItem";
import { ReportCover } from "@/components/report/ReportCover";
import { ReportPage } from "@/components/report/ReportPage";
import { ReportTocSection } from "@/components/report/ReportTocSection";
import { SelectionQuoteToolbar } from "@/components/report/SelectionQuoteToolbar";
import { flashHighlightElement } from "@/components/report/reportDom";
import {
  A4_CONTENT_HEIGHT_PX,
  A4_CONTENT_WIDTH_PX,
  A4_FOOTER_HEIGHT_PX,
  A4_HEADER_HEIGHT_PX,
  buildBlockPageMap,
  paginateBlocks,
  type PageSlice,
} from "@/components/report/pagination";
import { usePageScale } from "@/components/report/usePageScale";
import { useReportMeasurements } from "@/components/report/useReportMeasurements";
import { useTextSelectionQuote } from "@/components/report/useTextSelectionQuote";
import type {
  BibliographyEntry,
  CommentThread,
  ComputedDocumentMeta,
  EphemeralPreviewEvent,
  PageChromeConfig,
} from "@/contracts/report-protocol";
import type { ReportDocumentView } from "@/lib/document";
import {
  hasPageFooter,
  hasPageHeader,
  normalizePageChromeConfig,
  usePageChromeConfig,
} from "@/lib/pageChrome";
import {
  normalizeReferencesTitle,
  referencesEnabled,
  referencesPageBreakBefore,
  useTocConfig,
} from "@/lib/tocConfig";
import type { ReportViewMode } from "@/lib/preferences";

export interface ReportViewerProps {
  document: ReportDocumentView;
  computed: ComputedDocumentMeta;
  viewMode: ReportViewMode;
  ephemeralPreviews?: Map<string, EphemeralPreviewEvent>;
  comments?: CommentThread[];
  printMode?: boolean;
  previewMode?: boolean;
  pageChrome?: PageChromeConfig;
  pageFilter?: number;
  onPagesReady?: (totalPages: number) => void;
  onOpenCommentForBlock?: (anchor: FloatingCommentAnchor) => void;
  onOpenCommentThread?: (threadId: string, blockId: string) => void;
  onAskAgentWithQuote?: (blockId: string, exact: string) => void;
  onJumpToSection?: (blockId: string) => void;
}

export function ReportViewer({
  document,
  computed,
  viewMode,
  ephemeralPreviews,
  comments = [],
  printMode = false,
  previewMode = false,
  pageChrome,
  pageFilter,
  onPagesReady,
  onOpenCommentForBlock,
  onOpenCommentThread,
  onAskAgentWithQuote,
  onJumpToSection,
}: ReportViewerProps) {
  const suppressChrome = printMode || previewMode;
  const isPaged = viewMode === "paged" || printMode;
  const resolvedPageChrome = usePageChromeConfig(
    document.doc_id,
    document.theme.page_chrome,
  );
  const effectivePageChrome = pageChrome
    ? normalizePageChromeConfig(pageChrome)
    : resolvedPageChrome;
  const effectivePageHeight =
    A4_CONTENT_HEIGHT_PX -
    (hasPageHeader(effectivePageChrome) ? A4_HEADER_HEIGHT_PX : 0) -
    (hasPageFooter(effectivePageChrome) ? A4_FOOTER_HEIGHT_PX : 0);
  const showFooter = hasPageFooter(effectivePageChrome);
  const resolvedToc = useTocConfig(document.doc_id, document.toc, isPaged);
  const referencesTitle = normalizeReferencesTitle(document.references);
  const referencesBreak = referencesPageBreakBefore(document.references);
  const showReferences =
    referencesEnabled(document.references) && computed.bibliography.length > 0;
  const rootRef = useRef<HTMLDivElement>(null);
  const pageScale = usePageScale(rootRef, isPaged, suppressChrome);

  const { quoteAction, setQuoteAction } = useTextSelectionQuote({
    enabled: !suppressChrome,
    onOpenCommentForBlock,
    onAskAgentWithQuote,
  });

  const { openCountByBlock, quotesByBlock } = useMemo(() => {
    const counts = new Map<string, number>();
    const quotes = new Map<string, QuoteHighlight[]>();
    for (const thread of comments) {
      if (thread.status !== "archived" && !thread.anchor_detached) {
        counts.set(thread.block_id, (counts.get(thread.block_id) ?? 0) + 1);
        if (thread.quote) {
          const list = quotes.get(thread.block_id) ?? [];
          list.push({ threadId: thread.thread_id, quote: thread.quote });
          quotes.set(thread.block_id, list);
        }
      }
    }
    return { openCountByBlock: counts, quotesByBlock: quotes };
  }, [comments]);

  const previewList = useMemo(
    () =>
      ephemeralPreviews
        ? Array.from(ephemeralPreviews.values()).filter(
            (p) => p.status === "streaming",
          )
        : [],
    [ephemeralPreviews],
  );
  const visiblePreviews = useMemo(
    () => (suppressChrome ? [] : previewList),
    [previewList, suppressChrome],
  );

  const layoutSignature = useMemo(
    () =>
      `${document.revision}:${document.blocks
        .map((block) => block.id)
        .join(
          ",",
        )}:${computed.bibliography.length}:${document.cover.title ?? ""}:${
        resolvedToc.enabled ? 1 : 0
      }:${resolvedToc.max_depth}:${resolvedToc.variant}:${resolvedToc.title}:${
        computed.toc_tree.length
      }:${referencesTitle}`,
    [
      document.blocks,
      document.revision,
      computed.bibliography.length,
      computed.toc_tree.length,
      document.cover.title,
      resolvedToc.enabled,
      resolvedToc.max_depth,
      resolvedToc.variant,
      resolvedToc.title,
      referencesTitle,
    ],
  );

  const { measureRef, measurements } = useReportMeasurements({
    isPaged,
    layoutSignature,
  });

  const customCss = document.theme.custom_css;

  useEffect(() => {
    if (customCss === undefined || customCss.trim().length === 0) {
      return undefined;
    }
    const style = window.document.createElement("style");
    style.setAttribute("data-quire-theme", document.doc_id);
    style.textContent = customCss;
    window.document.head.appendChild(style);
    return () => {
      style.remove();
    };
  }, [document.doc_id, customCss]);

  const themeVars: Record<string, string> = {};
  const themeTokens = document.theme.tokens ?? {};
  for (const [k, v] of Object.entries(themeTokens)) {
    themeVars[k] = v;
  }
  if (
    themeTokens["--rpt-accent"] === undefined &&
    themeTokens["--rpt-primary"] !== undefined
  ) {
    themeVars["--rpt-accent"] = themeTokens["--rpt-primary"];
  }

  const bibliographyById = useMemo(() => {
    const map = new Map<string, BibliographyEntry>();
    for (const entry of computed.bibliography) {
      map.set(entry.source_id, entry);
    }
    return map;
  }, [computed.bibliography]);

  const scrollToSource = useCallback((sourceId: string) => {
    const el = window.document.getElementById(`bib-${sourceId}`);
    if (!el) {
      return;
    }
    el.scrollIntoView({ behavior: "smooth", block: "center" });
    flashHighlightElement(el);
  }, []);

  const pages = useMemo(() => {
    if (!isPaged) return [];
    return paginateBlocks(document.blocks, measurements, effectivePageHeight, {
      referencesPageBreakBefore: referencesBreak,
    });
  }, [
    isPaged,
    document.blocks,
    measurements,
    effectivePageHeight,
    referencesBreak,
  ]);

  const blockPageMap = useMemo(() => buildBlockPageMap(pages), [pages]);

  const pagedMixedItems = useMemo(() => {
    if (!isPaged) return [];
    return mixPreviewsIntoPages(pages, visiblePreviews, document.blocks);
  }, [isPaged, pages, visiblePreviews, document.blocks]);

  const visiblePages = useMemo(() => {
    const entries = pages.map((page, pageIdx) => ({ page, pageIdx }));
    if (pageFilter === undefined) {
      return entries;
    }
    const targetIndex = pageFilter - 1;
    return entries.filter(({ pageIdx }) => pageIdx === targetIndex);
  }, [pages, pageFilter]);

  useEffect(() => {
    if (!isPaged) {
      return undefined;
    }
    onPagesReady?.(pages.length);
    return undefined;
  }, [isPaged, onPagesReady, pages.length]);

  const continuousItems = useMemo(() => {
    if (isPaged) return [];
    const baseSlices: PageSlice[] = document.blocks.map((block) => ({ block }));
    return buildMixedItems(baseSlices, visiblePreviews, document.blocks);
  }, [isPaged, document.blocks, visiblePreviews]);

  const handleTocJump = useCallback(
    (blockId: string) => {
      if (onJumpToSection) {
        onJumpToSection(blockId);
        return;
      }
      const el = window.document.getElementById(`block-${blockId}`);
      el?.scrollIntoView({ behavior: "smooth", block: "start" });
    },
    [onJumpToSection],
  );

  const renderToc = (pageMap?: ReadonlyMap<string, number>) => (
    <ReportTocSection
      config={resolvedToc}
      nodes={computed.toc_tree}
      isPaged={isPaged}
      pageMap={pageMap}
      printMode={suppressChrome}
      onJumpToSection={suppressChrome ? undefined : handleTocJump}
    />
  );

  const renderBibliography = (measurementOnly = false) =>
    showReferences ? (
      <ReportBibliography
        entries={computed.bibliography}
        title={referencesTitle}
        measurementOnly={measurementOnly}
      />
    ) : null;

  return (
    <div
      ref={rootRef}
      style={themeVars}
      data-theme-preset={document.theme.preset ?? "default"}
      className="report-root relative w-full"
    >
      {quoteAction &&
        (onOpenCommentForBlock || onAskAgentWithQuote) &&
        !suppressChrome && (
          <SelectionQuoteToolbar
            action={quoteAction}
            onComment={
              onOpenCommentForBlock
                ? (action) => {
                    onOpenCommentForBlock({
                      blockId: action.blockId,
                      quote: action.quote,
                      rect: action.viewportRect,
                    });
                    setQuoteAction(null);
                    window.getSelection()?.removeAllRanges();
                  }
                : undefined
            }
            onAskAgent={
              onAskAgentWithQuote
                ? (blockId, exact) => {
                    onAskAgentWithQuote(blockId, exact);
                    setQuoteAction(null);
                    window.getSelection()?.removeAllRanges();
                  }
                : undefined
            }
            onDismiss={() => setQuoteAction(null)}
          />
        )}

      {isPaged && (
        <div
          ref={measureRef}
          data-measure-root="true"
          aria-hidden="true"
          style={{ width: `${A4_CONTENT_WIDTH_PX}px` }}
          className="report-root pointer-events-none absolute top-0 -left-[99999px] opacity-0 print:hidden"
        >
          <div data-measure-cover="true" className="flow-root">
            <ReportCover cover={document.cover} />
          </div>
          <div data-measure-toc="true" className="flow-root">
            {renderToc(blockPageMap)}
          </div>
          {document.blocks.map((block) => (
            <div
              key={block.id}
              data-measure-id={block.id}
              className="flow-root"
            >
              <ReportBlockItem
                slice={{ block }}
                isPaged
                printMode
                showChartTools
                computed={computed}
                bibliographyById={bibliographyById}
                commentCount={0}
                blockQuotes={quotesByBlock.get(block.id)}
                measurementOnly
                onCitationClick={scrollToSource}
              />
            </div>
          ))}
          <div data-measure-bib="true" className="flow-root">
            {renderBibliography(true)}
          </div>
        </div>
      )}

      {!isPaged ? (
        <article className="mx-auto w-full max-w-[760px] bg-white px-5 py-8 text-[var(--rpt-ink)] shadow-sm ring-1 ring-black/5 sm:px-10">
          <ReportCover cover={document.cover} />
          {renderToc(blockPageMap)}
          {continuousItems.map((item: MixedRenderItem) => (
            <div key={item.key}>
              {item.preview ? (
                <EphemeralPreviewCard preview={item.preview} />
              ) : item.slice ? (
                <ReportBlockItem
                  slice={item.slice}
                  isPaged={false}
                  printMode={suppressChrome}
                  showChartTools={!suppressChrome}
                  computed={computed}
                  bibliographyById={bibliographyById}
                  commentCount={openCountByBlock.get(item.slice.block.id) ?? 0}
                  blockQuotes={quotesByBlock.get(item.slice.block.id)}
                  onCitationClick={scrollToSource}
                  onOpenCommentForBlock={onOpenCommentForBlock}
                  onOpenCommentThread={onOpenCommentThread}
                />
              ) : null}
            </div>
          ))}
          {renderBibliography()}
        </article>
      ) : (
        <div className="flex flex-col items-center gap-6 py-6 print:block print:w-full print:gap-0 print:p-0 print:m-0">
          {visiblePages.map(({ page, pageIdx }) => (
            <ReportPage
              key={page.pageNumber}
              page={page}
              pageIdx={pageIdx}
              totalPages={pages.length}
              document={document}
              pageChrome={effectivePageChrome}
              showHeader={hasPageHeader(effectivePageChrome)}
              showFooter={showFooter}
              pageScale={pageScale}
              plain={suppressChrome}
              items={pagedMixedItems[pageIdx] ?? []}
              cover={
                page.includeCover ? (
                  <ReportCover cover={document.cover} />
                ) : null
              }
              toc={page.includeToc ? renderToc(blockPageMap) : null}
              bibliography={
                page.includeBibliography ? renderBibliography() : null
              }
              computed={computed}
              bibliographyById={bibliographyById}
              openCountByBlock={openCountByBlock}
              quotesByBlock={quotesByBlock}
              showChartTools={!suppressChrome}
              printMode={suppressChrome}
              onCitationClick={scrollToSource}
              onOpenCommentForBlock={onOpenCommentForBlock}
              onOpenCommentThread={onOpenCommentThread}
            />
          ))}
        </div>
      )}
    </div>
  );
}
