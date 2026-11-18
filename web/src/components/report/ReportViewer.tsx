import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { MessageSquareIcon } from "lucide-react";
import { CustomHtmlView } from "@/components/report/CustomHtmlView";
import { EchartsCanvas } from "@/components/report/EchartsCanvas";
import { InlineMarkdown } from "@/components/report/InlineMarkdown";
import { TableBlockView } from "@/components/report/TableBlockView";
import {
  A4_CONTENT_WIDTH_PX,
  A4_PAGE_HEIGHT_PX,
  A4_PAGE_WIDTH_PX,
  A4_PADDING_X_PX,
  A4_PADDING_Y_PX,
  paginateBlocks,
  type PageSlice,
} from "@/components/report/pagination";
import type {
  CommentThread,
  ComputedDocumentMeta,
  EphemeralPreviewEvent,
} from "@/contracts/report-protocol";
import type { ReportBlock, ReportDocumentView } from "@/lib/document";
import type { ReportViewMode } from "@/lib/preferences";

export interface ReportViewerProps {
  document: ReportDocumentView;
  computed: ComputedDocumentMeta;
  viewMode: ReportViewMode;
  ephemeralPreviews?: Map<string, EphemeralPreviewEvent>;
  comments?: CommentThread[];
  printMode?: boolean;
  onOpenCommentForBlock?: (blockId: string) => void;
}

function renderHeadingTag(
  level: number,
  blockId: string,
  text: string,
  citationMap: ComputedDocumentMeta["citation_map"],
  onCitationClick: (sourceId: string) => void,
) {
  const content = (
    <InlineMarkdown
      text={text}
      citationMap={citationMap}
      onCitationClick={onCitationClick}
    />
  );
  switch (level) {
    case 1:
      return (
        <h1
          id={`block-${blockId}`}
          className="mt-8 mb-4 text-2xl font-bold tracking-tight"
        >
          {content}
        </h1>
      );
    case 2:
      return (
        <h2
          id={`block-${blockId}`}
          className="mt-7 mb-3 border-b border-black/10 pb-1.5 text-xl font-semibold dark:border-white/10"
        >
          {content}
        </h2>
      );
    case 3:
      return (
        <h3 id={`block-${blockId}`} className="mt-5 mb-2 text-lg font-semibold">
          {content}
        </h3>
      );
    case 4:
      return (
        <h4
          id={`block-${blockId}`}
          className="mt-4 mb-1.5 text-base font-semibold"
        >
          {content}
        </h4>
      );
    default:
      return (
        <h5 id={`block-${blockId}`} className="mt-3 mb-1 text-sm font-semibold">
          {content}
        </h5>
      );
  }
}

function EphemeralPreviewCard({ preview }: { preview: EphemeralPreviewEvent }) {
  const { block } = preview;
  return (
    <div className="my-3 rounded-xl border border-dashed border-primary/40 bg-primary/[0.03] px-4 py-3 text-sm">
      <div className="mb-1.5 flex items-center justify-between text-[11px] text-primary">
        <span className="font-medium">
          {preview.actor} 正在实时生成 ({block.block_type})...
        </span>
        <span className="inline-block size-2 animate-pulse rounded-full bg-primary" />
      </div>
      {block.title && (
        <div className="mb-1 font-medium text-foreground">{block.title}</div>
      )}
      {block.text ? (
        <p className="m-0 leading-relaxed whitespace-pre-wrap text-foreground/90">
          {block.text}
          <span className="ml-0.5 inline-block h-4 w-1.5 animate-pulse bg-primary align-middle" />
        </p>
      ) : (
        <div className="h-24 animate-pulse rounded-lg bg-muted/60" />
      )}
    </div>
  );
}

interface MixedRenderItem {
  key: string;
  slice?: PageSlice;
  preview?: EphemeralPreviewEvent;
}

function buildMixedItems(
  slices: PageSlice[],
  previews: EphemeralPreviewEvent[],
  allBlocks: ReportBlock[],
): MixedRenderItem[] {
  if (previews.length === 0) {
    return slices.map((slice, idx) => ({
      key: `${slice.block.id}-${slice.rowStart ?? 0}-${idx}`,
      slice,
    }));
  }

  const sectionLastBlock = new Map<string, string>();
  let activeSection: string | null = null;
  for (const b of allBlocks) {
    if (b.type === "heading") {
      activeSection = b.id;
    }
    if (activeSection) {
      sectionLastBlock.set(activeSection, b.id);
    }
  }

  const afterMap = new Map<string, EphemeralPreviewEvent[]>();
  const beforeMap = new Map<string, EphemeralPreviewEvent[]>();
  const replaceMap = new Map<string, EphemeralPreviewEvent>();
  const tailPreviews: EphemeralPreviewEvent[] = [];

  for (const p of previews) {
    const anchor = p.anchor;
    if (anchor.type === "replace_block") {
      replaceMap.set(anchor.block_id, p);
    } else if (anchor.type === "before_block") {
      const list = beforeMap.get(anchor.block_id) ?? [];
      list.push(p);
      beforeMap.set(anchor.block_id, list);
    } else if (anchor.type === "after_block") {
      const list = afterMap.get(anchor.block_id) ?? [];
      list.push(p);
      afterMap.set(anchor.block_id, list);
    } else if (anchor.type === "append_to_section") {
      const targetId =
        sectionLastBlock.get(anchor.section_id) ?? anchor.section_id;
      const list = afterMap.get(targetId) ?? [];
      list.push(p);
      afterMap.set(targetId, list);
    } else {
      tailPreviews.push(p);
    }
  }

  const out: MixedRenderItem[] = [];
  for (let idx = 0; idx < slices.length; idx += 1) {
    const slice = slices[idx];
    if (!slice) continue;
    const bid = slice.block.id;

    for (const p of beforeMap.get(bid) ?? []) {
      out.push({ key: `pre-${p.preview_id}`, preview: p });
    }
    const replacement = replaceMap.get(bid);
    if (replacement) {
      out.push({ key: `rep-${replacement.preview_id}`, preview: replacement });
    } else {
      out.push({
        key: `${bid}-${slice.rowStart ?? 0}-${idx}`,
        slice,
      });
    }
    for (const p of afterMap.get(bid) ?? []) {
      out.push({ key: `post-${p.preview_id}`, preview: p });
    }
  }
  for (const p of tailPreviews) {
    out.push({ key: `tail-${p.preview_id}`, preview: p });
  }
  return out;
}

function blockTitle(block: ReportBlock): string | undefined {
  const title = block.title;
  return typeof title === "string" ? title : undefined;
}

export function ReportViewer({
  document,
  computed,
  viewMode,
  ephemeralPreviews,
  comments = [],
  printMode = false,
  onOpenCommentForBlock,
}: ReportViewerProps) {
  const measureRef = useRef<HTMLDivElement>(null);
  const [heights, setHeights] = useState<Record<string, number>>({});

  const openCountByBlock = useMemo(() => {
    const map = new Map<string, number>();
    for (const thread of comments) {
      if (thread.status !== "archived" && !thread.anchor_detached) {
        map.set(thread.block_id, (map.get(thread.block_id) ?? 0) + 1);
      }
    }
    return map;
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

  const layoutSignature = useMemo(
    () =>
      `${document.revision}:${document.blocks
        .map((block) => block.id)
        .join(",")}`,
    [document.blocks, document.revision],
  );

  useLayoutEffect(() => {
    if (viewMode !== "paged" && !printMode) {
      return undefined;
    }
    const container = measureRef.current;
    if (!container || layoutSignature.length === 0) {
      return undefined;
    }
    const nextHeights: Record<string, number> = {};
    const items = container.querySelectorAll<HTMLElement>("[data-measure-id]");
    for (const el of items) {
      const id = el.dataset.measureId;
      if (id) {
        nextHeights[id] = el.getBoundingClientRect().height;
      }
    }
    setHeights(nextHeights);
    return undefined;
  }, [layoutSignature, viewMode, printMode]);

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
  for (const [k, v] of Object.entries(document.theme.tokens ?? {})) {
    themeVars[k] = v;
  }

  const scrollToSource = (sourceId: string) => {
    const el = window.document.getElementById(`bib-${sourceId}`);
    el?.scrollIntoView({ behavior: "smooth", block: "center" });
  };

  const renderBlock = (slice: PageSlice, isPaged: boolean) => {
    const { block } = slice;
    const framing = computed.block_framing[block.id];
    const commentCount = openCountByBlock.get(block.id) ?? 0;

    let bodyNode: React.ReactNode = null;
    if (block.type === "heading") {
      bodyNode = renderHeadingTag(
        block.level ?? 2,
        block.id,
        block.text,
        computed.citation_map,
        scrollToSource,
      );
    } else if (block.type === "paragraph") {
      bodyNode = (
        <p
          id={`block-${block.id}`}
          className="my-3 text-[15px] leading-7 text-current/90"
        >
          <InlineMarkdown
            text={block.text}
            citationMap={computed.citation_map}
            onCitationClick={scrollToSource}
          />
        </p>
      );
    } else if (block.type === "quote") {
      bodyNode = (
        <blockquote
          id={`block-${block.id}`}
          className="my-4 border-l-4 border-current/25 pl-4 text-[15px] leading-7 italic opacity-90"
        >
          <InlineMarkdown
            text={block.text}
            citationMap={computed.citation_map}
            onCitationClick={scrollToSource}
          />
        </blockquote>
      );
    } else if (block.type === "table") {
      const titleText = block.title?.trim();
      bodyNode = (
        <figure id={`block-${block.id}`} className="my-5 space-y-2">
          {(framing || titleText) && (
            <figcaption className="text-sm font-semibold">
              {framing ? framing.label : "表"}
              {titleText ? ` — ${titleText}` : ""}
              {slice.continuation ? "（续）" : ""}
            </figcaption>
          )}
          <TableBlockView
            table={block}
            layout={computed.table_layouts[block.id]}
            rowStart={slice.rowStart}
            rowEnd={slice.rowEnd}
            includeSummary={slice.includeSummary}
          />
          {framing?.attribution && slice.includeSummary !== false && (
            <div className="text-xs text-muted-foreground">
              {framing.attribution}
            </div>
          )}
        </figure>
      );
    } else if (block.type === "echarts") {
      const titleText = blockTitle(block);
      bodyNode = (
        <figure id={`block-${block.id}`} className="my-5 space-y-2">
          {(framing || titleText) && (
            <figcaption className="text-sm font-semibold">
              {framing ? framing.label : "图"}
              {titleText ? ` — ${titleText}` : ""}
            </figcaption>
          )}
          <EchartsCanvas
            blockId={block.id}
            chart={block}
            chartTables={computed.chart_tables?.[block.id]}
            requirements={computed.chart_requirements?.[block.id]}
            renderer={isPaged || printMode ? "svg" : "canvas"}
            printSafe={isPaged || printMode}
          />
          {framing?.attribution && (
            <div className="text-xs text-muted-foreground">
              {framing.attribution}
            </div>
          )}
        </figure>
      );
    } else if (block.type === "custom_html") {
      const titleText = blockTitle(block);
      bodyNode = (
        <figure id={`block-${block.id}`} className="my-5 space-y-2">
          {(framing || titleText) && (
            <figcaption className="text-sm font-semibold">
              {framing ? framing.label : ""}
              {framing && titleText ? " — " : ""}
              {titleText ?? ""}
            </figcaption>
          )}
          <CustomHtmlView block={block} />
          {framing?.attribution && (
            <div className="text-xs text-muted-foreground">
              {framing.attribution}
            </div>
          )}
        </figure>
      );
    }

    return (
      <div
        data-block-id={block.id}
        className="group/block relative rounded-lg transition-colors hover:bg-black/[0.015] dark:hover:bg-white/[0.02]"
      >
        {!printMode && onOpenCommentForBlock && (
          <button
            type="button"
            title="查看或添加批注"
            onClick={() => onOpenCommentForBlock(block.id)}
            className={`absolute top-1 -right-1 z-10 inline-flex items-center gap-1 rounded-full border bg-background px-2 py-0.5 text-[11px] shadow-xs transition-opacity print:hidden ${
              commentCount > 0
                ? "border-amber-500/50 text-amber-700 opacity-100 dark:text-amber-300"
                : "text-muted-foreground opacity-0 group-hover/block:opacity-100"
            }`}
          >
            <MessageSquareIcon className="size-3" />
            {commentCount > 0 && <span>{commentCount}</span>}
          </button>
        )}
        {bodyNode}
      </div>
    );
  };

  const renderCover = () => {
    const cover = document.cover;
    if (!cover.enabled || !cover.title) {
      return null;
    }
    const kicker = cover.meta?.kicker;
    const authors = cover.authors ?? [];
    return (
      <header className="mb-8 border-b border-black/15 pb-6 dark:border-white/15">
        {kicker && (
          <div className="mb-2 text-xs font-semibold tracking-wider text-primary uppercase">
            {kicker}
          </div>
        )}
        <h1 className="text-3xl font-bold tracking-tight">{cover.title}</h1>
        {cover.subtitle && (
          <p className="mt-2 text-base text-muted-foreground">
            {cover.subtitle}
          </p>
        )}
        <div className="mt-4 flex flex-wrap gap-4 text-xs text-muted-foreground">
          {authors.length > 0 && <span>作者：{authors.join("、")}</span>}
          {cover.date && <span>日期：{cover.date}</span>}
        </div>
      </header>
    );
  };

  const renderBibliography = () => {
    const references = document.references;
    if (!references.enabled || computed.bibliography.length === 0) {
      return null;
    }
    return (
      <section className="mt-10 border-t border-black/15 pt-6 dark:border-white/15">
        <h2 className="mb-3 text-lg font-semibold">
          {references.title || "参考资料"}
        </h2>
        <ol className="space-y-2.5 pl-5 text-xs leading-relaxed">
          {computed.bibliography.map((entry) => {
            const source = entry.source;
            const authors = source.authors ?? [];
            return (
              <li
                key={entry.source_id}
                id={`bib-${entry.source_id}`}
                className="space-y-0.5"
              >
                <div>
                  <span className="font-medium">{source.title}</span>
                  {authors.length > 0 && (
                    <span className="text-muted-foreground">
                      {" "}
                      — {authors.join(", ")}
                    </span>
                  )}
                  {source.date && (
                    <span className="text-muted-foreground">
                      {" "}
                      ({source.date})
                    </span>
                  )}
                  {source.url && (
                    <a
                      href={source.url}
                      target="_blank"
                      rel="noreferrer noopener"
                      className="ml-2 text-blue-600 underline dark:text-blue-400"
                    >
                      链接
                    </a>
                  )}
                </div>
                {entry.excerpt && (
                  <div className="rounded bg-black/[0.03] px-2 py-1 text-[11px] text-muted-foreground dark:bg-white/[0.04]">
                    “{entry.excerpt}”
                  </div>
                )}
              </li>
            );
          })}
        </ol>
      </section>
    );
  };

  const isPaged = viewMode === "paged" || printMode;

  const pages = useMemo(() => {
    if (!isPaged) return [];
    return paginateBlocks(document.blocks, heights);
  }, [isPaged, document.blocks, heights]);

  const continuousItems = useMemo(() => {
    if (isPaged) return [];
    const baseSlices: PageSlice[] = document.blocks.map((block) => ({ block }));
    return buildMixedItems(baseSlices, previewList, document.blocks);
  }, [isPaged, document.blocks, previewList]);

  return (
    <div style={themeVars} className="report-root w-full">
      {isPaged && (
        <div
          ref={measureRef}
          aria-hidden="true"
          style={{ width: `${A4_CONTENT_WIDTH_PX}px` }}
          className="pointer-events-none fixed top-0 -left-[99999px] opacity-0"
        >
          {document.blocks.map((block) => (
            <div key={block.id} data-measure-id={block.id}>
              {renderBlock({ block }, true)}
            </div>
          ))}
        </div>
      )}

      {!isPaged ? (
        <article className="mx-auto w-full max-w-[760px] px-4 py-8 sm:px-8">
          {renderCover()}
          {continuousItems.map((item) => (
            <div key={item.key}>
              {item.preview ? (
                <EhemeralBlock preview={item.preview} />
              ) : item.slice ? (
                renderBlock(item.slice, false)
              ) : null}
            </div>
          ))}
          {renderBibliography()}
        </article>
      ) : (
        <div className="flex flex-col items-center gap-6 py-6 print:gap-0 print:py-0">
          {pages.map((page, pageIdx) => (
            <section
              key={page.pageNumber}
              style={{
                width: `${A4_PAGE_WIDTH_PX}px`,
                minHeight: `${A4_PAGE_HEIGHT_PX}px`,
                padding: `${A4_PADDING_Y_PX}px ${A4_PADDING_X_PX}px`,
              }}
              className="a4-page relative flex flex-col justify-between bg-white text-neutral-900 shadow-md ring-1 ring-black/10 print:shadow-none print:ring-0"
            >
              <div className="flex-1">
                {pageIdx === 0 && renderCover()}
                {page.items.map((slice, idx) => (
                  <div key={`${slice.block.id}-${slice.rowStart ?? 0}-${idx}`}>
                    {renderBlock(slice, true)}
                  </div>
                ))}
                {pageIdx === pages.length - 1 && renderBibliography()}
              </div>
              <footer className="mt-4 flex items-center justify-between border-t border-black/10 pt-2 text-[11px] text-neutral-500">
                <span>{document.cover.title || document.doc_id}</span>
                <span className="tabular-nums">
                  第 {page.pageNumber} 页 / 共 {pages.length} 页
                </span>
              </footer>
            </section>
          ))}
        </div>
      )}
    </div>
  );
}

function EhemeralBlock({ preview }: { preview: EphemeralPreviewEvent }) {
  return <EphemeralPreviewCard preview={preview} />;
}
