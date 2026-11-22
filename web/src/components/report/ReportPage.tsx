import type { ReactNode } from "react";
import { ReportBlockItem } from "@/components/report/ReportBlockItem";
import { ScaledPage } from "@/components/report/ScaledPage";
import {
  A4_PAGE_HEIGHT_PX,
  A4_PAGE_WIDTH_PX,
  A4_PADDING_X_PX,
  A4_PADDING_Y_PX,
  type A4Page,
} from "@/components/report/pagination";
import type { MixedRenderItem } from "@/components/report/ephemeralPreviewMix";
import type { FloatingCommentAnchor } from "@/components/report/FloatingCommentPopover";
import { formatPageTemplate, type ResolvedPageChrome } from "@/lib/pageChrome";
import type {
  BibliographyEntry,
  ComputedDocumentMeta,
} from "@/contracts/report-protocol";
import type { ReportDocumentView } from "@/lib/document";

export interface ReportPageProps {
  page: A4Page;
  pageIdx: number;
  totalPages: number;
  document: ReportDocumentView;
  pageChrome: ResolvedPageChrome;
  showHeader: boolean;
  showFooter: boolean;
  pageScale: number;
  plain: boolean;
  items: Array<Extract<MixedRenderItem, { kind: "slice" }>>;
  cover: ReactNode;
  toc: ReactNode;
  bibliography: ReactNode;
  computed: ComputedDocumentMeta;
  bibliographyById: Map<string, BibliographyEntry>;
  openCountByBlock: Map<string, number>;
  commentOutlineIds: ReadonlySet<string>;
  showChartTools: boolean;
  printMode: boolean;
  /** Custom-HTML covers occupy the whole first page with no header/footer. */
  coverFullPage?: boolean;
  onCitationClick: (sourceId: string) => void;
  onOpenCommentForBlock?: (anchor: FloatingCommentAnchor) => void;
}

export function ReportPage({
  page,
  pageIdx,
  totalPages,
  document,
  pageChrome,
  showHeader,
  showFooter,
  pageScale,
  plain,
  items,
  cover,
  toc,
  bibliography,
  computed,
  bibliographyById,
  openCountByBlock,
  commentOutlineIds,
  showChartTools,
  printMode,
  coverFullPage = false,
  onCitationClick,
  onOpenCommentForBlock,
}: ReportPageProps) {
  const pageContext = {
    page: page.pageNumber,
    total: totalPages,
    title: document.cover.title || document.doc_id,
    subtitle: document.cover.subtitle ?? "",
    date: document.cover.date ?? "",
    kicker: document.cover.meta?.kicker ?? "",
    authors: (document.cover.authors ?? []).join("、"),
  };
  const fullPageCover = Boolean(coverFullPage && page.includeCover);

  return (
    <ScaledPage scale={plain ? 1 : pageScale} plain={plain}>
      <section
        data-page-number={page.pageNumber}
        data-include-cover={page.includeCover ? "true" : undefined}
        data-include-toc={page.includeToc ? "true" : undefined}
        data-include-bibliography={page.bibliographyRange ? "true" : undefined}
        data-bibliography-start={page.bibliographyRange?.start}
        data-bibliography-end={page.bibliographyRange?.end}
        data-full-page-cover={fullPageCover ? "true" : undefined}
        style={{
          width: `${A4_PAGE_WIDTH_PX}px`,
          height: `${A4_PAGE_HEIGHT_PX}px`,
          padding: `${A4_PADDING_Y_PX}px ${A4_PADDING_X_PX}px`,
        }}
        className="a4-page relative flex flex-col overflow-hidden bg-white text-[var(--rpt-ink)] shadow-md ring-1 ring-black/10 print:shadow-none print:ring-0"
      >
        <div className="flex min-h-0 flex-1 flex-col overflow-visible print:overflow-hidden">
          {showHeader &&
            !fullPageCover &&
            (pageIdx > 0 || !pageChrome.hide_header_on_first_page) && (
              <div
                className="mb-3 grid shrink-0 grid-cols-3 items-center gap-2 border-b border-[var(--rpt-border)] pb-1 text-[10px] tracking-[0.06em] text-[var(--rpt-muted)]"
                data-page-header="true"
              >
                <div className="truncate text-left">
                  {formatPageTemplate(pageChrome.header.left, pageContext)}
                </div>
                <div className="truncate text-center">
                  {formatPageTemplate(pageChrome.header.center, pageContext)}
                </div>
                <div className="truncate text-right">
                  {formatPageTemplate(pageChrome.header.right, pageContext)}
                </div>
              </div>
            )}
          <div
            className={
              fullPageCover
                ? "flex h-full min-h-0 flex-1 flex-col justify-center overflow-visible print:overflow-hidden"
                : "min-h-0 flex-1 overflow-visible print:overflow-hidden"
            }
          >
            {cover}
            {toc}
            {items.map((item) => (
              <div key={item.key}>
                <ReportBlockItem
                  slice={item.slice}
                  isPaged
                  printMode={printMode}
                  showChartTools={showChartTools}
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
                  onCitationClick={onCitationClick}
                  onOpenCommentForBlock={onOpenCommentForBlock}
                />
              </div>
            ))}
            {bibliography}
          </div>
        </div>
        {showFooter &&
          !fullPageCover &&
          (pageIdx > 0 || !pageChrome.hide_footer_on_first_page) && (
            <footer
              className="mt-4 grid shrink-0 grid-cols-3 items-center gap-2 border-t border-[var(--rpt-border)] pt-2 text-[11px] text-[var(--rpt-muted)]"
              data-page-footer="true"
            >
              <span className="truncate text-left">
                {formatPageTemplate(pageChrome.footer.left, pageContext)}
              </span>
              <span className="truncate text-center tabular-nums">
                {formatPageTemplate(pageChrome.footer.center, pageContext)}
              </span>
              <span className="truncate text-right tabular-nums">
                {formatPageTemplate(pageChrome.footer.right, pageContext)}
              </span>
            </footer>
          )}
      </section>
    </ScaledPage>
  );
}
