import { useCallback, useEffect, useMemo, useRef } from "react";
import { CommentHoverCard } from "@/components/report/CommentHoverCard";
import { EphemeralPreviewCard } from "@/components/report/EphemeralPreviewCard";
import type { FloatingCommentAnchor } from "@/components/report/FloatingCommentPopover";
import {
  buildMixedItems,
  type MixedRenderItem,
} from "@/components/report/ephemeralPreviewMix";
import { ReportBibliography } from "@/components/report/ReportBibliography";
import { ReportBlockItem } from "@/components/report/ReportBlockItem";
import { ReportCover } from "@/components/report/ReportCover";
import { ReportPage } from "@/components/report/ReportPage";
import { ReportTocSection } from "@/components/report/ReportTocSection";
import { SelectionQuoteToolbar } from "@/components/report/SelectionQuoteToolbar";
import {
  commentThreadBlockIds,
  type ReportSelectionDescriptor,
} from "@/components/report/reportSelection";
import { flashHighlightElement } from "@/components/report/reportDom";
import {
  A4_CONTENT_HEIGHT_PX,
  A4_CONTENT_WIDTH_PX,
  A4_FOOTER_HEIGHT_PX,
  A4_HEADER_HEIGHT_PX,
  buildBlockPageMap,
  paginateBlocks,
  type BibliographyRange,
  type PageSlice,
  type TocRange,
} from "@/components/report/pagination";
import { usePageScale } from "@/components/report/usePageScale";
import { useReportHighlights } from "@/components/report/useReportHighlights";
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
  activeThreadId?: string | null;
  printMode?: boolean;
  previewMode?: boolean;
  pageChrome?: PageChromeConfig;
  pageFilter?: number;
  onPagesReady?: (totalPages: number) => void;
  onOpenCommentForBlock?: (anchor: FloatingCommentAnchor) => void;
  onSelectCommentThread?: (thread: CommentThread) => void;
  onAskAgentWithQuote?: (selection: ReportSelectionDescriptor) => void;
  onJumpToSection?: (blockId: string) => void;
  activeSelection?: ReportSelectionDescriptor | null;
  onActiveSelectionChange?: (
    selection: ReportSelectionDescriptor | null,
  ) => void;
}

export function ReportViewer({
  document,
  computed,
  viewMode,
  ephemeralPreviews,
  comments = [],
  activeThreadId,
  printMode = false,
  previewMode = false,
  pageChrome,
  pageFilter,
  onPagesReady,
  onOpenCommentForBlock,
  onSelectCommentThread,
  onAskAgentWithQuote,
  onJumpToSection,
  activeSelection = null,
  onActiveSelectionChange,
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
  const isCustomFullPageCover = Boolean(
    document.cover.enabled && document.cover.custom_html,
  );
  const rootRef = useRef<HTMLDivElement>(null);
  const pageScale = usePageScale(rootRef, isPaged, suppressChrome);
  const pendingSelection = activeSelection;

  const { quoteAction, setQuoteAction } = useTextSelectionQuote({
    enabled: !suppressChrome,
    containerRef: rootRef,
    onOpenCommentForBlock,
    onAskAgentWithQuote,
  });

  const {
    supported: highlightSupported,
    hoveredComment,
    keepHoverOpen,
    scheduleHoverClose,
    dismissHover,
  } = useReportHighlights({
    containerRef: rootRef,
    comments,
    activeThreadId,
    pendingSelection,
    enableHoverPreview: !suppressChrome,
  });

  const nonTextBlockIds = useMemo(
    () =>
      new Set(
        document.blocks
          .filter((block) =>
            ["table", "echarts", "custom_html"].includes(block.type),
          )
          .map((block) => block.id),
      ),
    [document.blocks],
  );

  const { openCountByBlock, commentOutlineIds } = useMemo(() => {
    const counts = new Map<string, number>();
    const outlines = new Set<string>();
    const addOutline = (blockId: string, includeText: boolean) => {
      if (includeText || nonTextBlockIds.has(blockId)) {
        outlines.add(blockId);
      }
    };
    for (const thread of comments) {
      // Only open threads stay visible on the report canvas; resolved
      // (addressed/archived) threads are hidden unless actively selected.
      if (thread.status !== "open" || thread.anchor_detached) {
        continue;
      }
      const blockIds = commentThreadBlockIds(thread);
      for (const blockId of blockIds) {
        counts.set(blockId, (counts.get(blockId) ?? 0) + 1);
        addOutline(blockId, !highlightSupported);
      }
    }
    const active = comments.find(
      (thread) => thread.thread_id === activeThreadId,
    );
    if (active && !active.anchor_detached) {
      const blockIds = commentThreadBlockIds(active);
      for (const blockId of blockIds) {
        addOutline(blockId, !highlightSupported);
      }
    }
    for (const blockId of pendingSelection?.blockIds ?? []) {
      addOutline(blockId, !highlightSupported);
    }
    return { openCountByBlock: counts, commentOutlineIds: outlines };
  }, [
    activeThreadId,
    comments,
    highlightSupported,
    nonTextBlockIds,
    pendingSelection,
  ]);

  useEffect(() => {
    if (!activeThreadId) {
      return;
    }
    const thread = comments.find(
      (candidate) => candidate.thread_id === activeThreadId,
    );
    if (!thread) {
      return;
    }
    const blockIds = commentThreadBlockIds(thread);
    for (const blockId of blockIds) {
      const element = rootRef.current?.querySelector<HTMLElement>(
        `[data-block-id="${blockId}"]`,
      );
      if (element) {
        flashHighlightElement(element);
      }
    }
  }, [activeThreadId, comments]);

  const previewList = useMemo(
    () =>
      ephemeralPreviews
        ? Array.from(ephemeralPreviews.values()).filter(
            (p) => p.status === "streaming",
          )
        : [],
    [ephemeralPreviews],
  );
  // NOTE: Ephemeral streaming previews are intentionally restricted to continuous mode.
  // Paged mode relies on pixel-exact A4 box packing; mixing live streaming preview cards
  // into A4 pages triggers expensive re-layout/jank and overflows fixed page heights.
  // Do not re-enable ephemeral previews in paged mode.
  const visiblePreviews = useMemo(
    () => (suppressChrome || isPaged ? [] : previewList),
    [isPaged, previewList, suppressChrome],
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
      tocPageBreakBefore: document.toc.page_break_before,
      tocPageBreakAfter: document.toc.page_break_after,
      coverFullPage: isCustomFullPageCover,
      headingPageBreak: effectivePageChrome.heading_page_break,
    });
  }, [
    isPaged,
    document.blocks,
    document.toc.page_break_before,
    document.toc.page_break_after,
    measurements,
    effectivePageHeight,
    referencesBreak,
    isCustomFullPageCover,
    effectivePageChrome.heading_page_break,
  ]);

  const blockPageMap = useMemo(() => buildBlockPageMap(pages), [pages]);

  const pagedItems = useMemo(() => {
    if (!isPaged) return [];
    return pages.map((page) =>
      page.items.map((slice, idx) => ({
        kind: "slice" as const,
        key: `${slice.block.id}-${slice.rowStart ?? 0}-${idx}`,
        slice,
      })),
    );
  }, [isPaged, pages]);

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

  const continuousSlices = useMemo<PageSlice[]>(
    () => (isPaged ? [] : document.blocks.map((block) => ({ block }))),
    [isPaged, document.blocks],
  );
  const continuousItems = useMemo(() => {
    if (isPaged) return [];
    return buildMixedItems(continuousSlices, visiblePreviews, document.blocks);
  }, [isPaged, continuousSlices, visiblePreviews, document.blocks]);

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

  const renderToc = (
    pageMap?: ReadonlyMap<string, number>,
    range?: TocRange,
  ) => (
    <ReportTocSection
      config={resolvedToc}
      nodes={computed.toc_tree}
      isPaged={isPaged}
      pageMap={pageMap}
      range={range}
      printMode={suppressChrome}
      onJumpToSection={suppressChrome ? undefined : handleTocJump}
    />
  );

  const renderBibliography = (
    measurementOnly = false,
    range?: BibliographyRange,
  ) =>
    showReferences ? (
      <ReportBibliography
        entries={computed.bibliography}
        title={referencesTitle}
        measurementOnly={measurementOnly}
        startIndex={range?.start}
        endIndex={range?.end}
        continued={range?.start !== undefined && range.start > 0}
      />
    ) : null;

  return (
    <div
      ref={rootRef}
      style={themeVars}
      data-theme-preset={document.theme.preset ?? "default"}
      className="report-root relative w-full"
    >
      {!suppressChrome && !quoteAction && (
        <CommentHoverCard
          hovered={hoveredComment}
          onMouseEnter={keepHoverOpen}
          onMouseLeave={scheduleHoverClose}
          onDismiss={dismissHover}
          onOpenPopoverForBlock={onOpenCommentForBlock}
          onSelectThread={onSelectCommentThread}
        />
      )}

      {quoteAction &&
        (onOpenCommentForBlock || onAskAgentWithQuote) &&
        !suppressChrome && (
          <SelectionQuoteToolbar
            action={quoteAction}
            onComment={
              onOpenCommentForBlock
                ? (action) => {
                    const selection: ReportSelectionDescriptor = {
                      blockId: action.blockId,
                      endBlockId: action.endBlockId,
                      blockIds: action.blockIds,
                      quote: action.quote,
                      endQuote: action.endQuote,
                      anchorSnapshot: action.anchorSnapshot,
                    };
                    onActiveSelectionChange?.(selection);
                    onOpenCommentForBlock({
                      ...selection,
                      rect: action.viewportRect,
                    });
                    setQuoteAction(null);
                    window.getSelection()?.removeAllRanges();
                  }
                : undefined
            }
            onAskAgent={
              onAskAgentWithQuote
                ? (action) => {
                    const selection: ReportSelectionDescriptor = {
                      blockId: action.blockId,
                      endBlockId: action.endBlockId,
                      blockIds: action.blockIds,
                      quote: action.quote,
                      endQuote: action.endQuote,
                      anchorSnapshot: action.anchorSnapshot,
                    };
                    onActiveSelectionChange?.(selection);
                    onAskAgentWithQuote(action);
                    setQuoteAction(null);
                    window.getSelection()?.removeAllRanges();
                  }
                : undefined
            }
            onDismiss={() => {
              setQuoteAction(null);
              onActiveSelectionChange?.(null);
            }}
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
            <ReportCover cover={document.cover} isPaged />
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
                citationMap={computed.citation_map}
                framing={computed.block_framing[block.id]}
                tableLayout={computed.table_layouts[block.id]}
                chartTables={computed.chart_tables?.[block.id]}
                chartRequirements={computed.chart_requirements?.[block.id]}
                bibliographyById={bibliographyById}
                commentCount={0}
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
          {isCustomFullPageCover ? (
            <div
              data-slot="continuous-custom-cover"
              className="-mx-5 -mt-8 mb-10 overflow-hidden sm:-mx-10"
            >
              <div className="px-5 pt-8 sm:px-10">
                <ReportCover cover={document.cover} isPaged={false} />
              </div>
            </div>
          ) : (
            <ReportCover cover={document.cover} isPaged={false} />
          )}
          {renderToc(blockPageMap)}
          {continuousItems.map((item: MixedRenderItem) => (
            <div key={item.key}>
              {item.kind === "preview" ? (
                <EphemeralPreviewCard
                  preview={item.preview}
                  citationMap={computed.citation_map}
                  bibliographyById={bibliographyById}
                  onCitationClick={scrollToSource}
                />
              ) : (
                <ReportBlockItem
                  slice={item.slice}
                  isPaged={false}
                  printMode={suppressChrome}
                  showChartTools={!suppressChrome}
                  citationMap={computed.citation_map}
                  framing={computed.block_framing[item.slice.block.id]}
                  tableLayout={computed.table_layouts[item.slice.block.id]}
                  chartTables={computed.chart_tables?.[item.slice.block.id]}
                  chartRequirements={
                    computed.chart_requirements?.[item.slice.block.id]
                  }
                  bibliographyById={bibliographyById}
                  commentCount={openCountByBlock.get(item.slice.block.id) ?? 0}
                  commentOutline={commentOutlineIds.has(item.slice.block.id)}
                  onCitationClick={scrollToSource}
                  onOpenCommentForBlock={onOpenCommentForBlock}
                />
              )}
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
              items={pagedItems[pageIdx] ?? []}
              cover={
                page.includeCover ? (
                  <ReportCover cover={document.cover} isPaged />
                ) : null
              }
              toc={
                page.includeToc ? renderToc(blockPageMap, page.tocRange) : null
              }
              bibliography={
                page.bibliographyRange
                  ? renderBibliography(false, page.bibliographyRange)
                  : null
              }
              computed={computed}
              bibliographyById={bibliographyById}
              openCountByBlock={openCountByBlock}
              commentOutlineIds={commentOutlineIds}
              showChartTools={!suppressChrome}
              printMode={suppressChrome}
              coverFullPage={isCustomFullPageCover}
              onCitationClick={scrollToSource}
              onOpenCommentForBlock={onOpenCommentForBlock}
            />
          ))}
        </div>
      )}
    </div>
  );
}
