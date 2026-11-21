import { useMemo } from "react";
import type { TocNode } from "@/contracts/report-protocol";
import {
  filterTocNodes,
  tocHeadingHasNumber,
  type ResolvedTocConfig,
} from "@/lib/tocConfig";

export interface ReportTocSectionProps {
  config: ResolvedTocConfig;
  nodes: TocNode[];
  isPaged: boolean;
  pageMap?: ReadonlyMap<string, number>;
  printMode?: boolean;
  onJumpToSection?: (blockId: string) => void;
}

function levelClassName(level: number): string {
  if (level <= 1) {
    return "text-[13.5px] font-semibold text-[var(--rpt-ink)]";
  }
  if (level === 2) {
    return "pl-4 text-[13px] font-normal text-[var(--rpt-ink)]";
  }
  return "pl-8 text-[12.5px] font-normal text-[var(--rpt-muted)]";
}

function TocEntry({
  node,
  config,
  isPaged,
  pageMap,
  printMode,
  onJumpToSection,
}: {
  node: TocNode;
  config: ResolvedTocConfig;
  isPaged: boolean;
  pageMap?: ReadonlyMap<string, number>;
  printMode: boolean;
  onJumpToSection?: (blockId: string) => void;
}) {
  const showNumber =
    (config.variant === "numbered" || config.variant === "dotted") &&
    node.number.trim().length > 0 &&
    !tocHeadingHasNumber(node);
  const showLeaders = config.variant === "dotted";
  const pageNumber = isPaged ? pageMap?.get(node.block_id) : undefined;
  const interactive = !printMode && Boolean(onJumpToSection);

  const content = (
    <>
      {showNumber && (
        <span className="shrink-0 font-mono text-[11px] text-[var(--rpt-muted)] tabular-nums">
          {node.number}
        </span>
      )}
      <span className="min-w-0">{node.text}</span>
      {showLeaders && (
        <span
          aria-hidden="true"
          className="mx-2 min-w-4 flex-1 translate-y-[-3px] border-b border-dotted border-[var(--rpt-border)]"
        />
      )}
      {isPaged && (
        <span
          className={`${showLeaders ? "ml-2" : "ml-auto pl-3"} min-w-8 shrink-0 text-right font-mono text-xs text-[var(--rpt-muted)] tabular-nums`}
        >
          {pageNumber ?? ""}
        </span>
      )}
    </>
  );

  const className = `flex w-full items-baseline gap-1.5 rounded-sm px-1 py-0.5 text-left transition-colors ${levelClassName(node.level)} ${
    interactive
      ? "cursor-pointer hover:bg-black/[0.035] focus-visible:ring-1 focus-visible:ring-[var(--rpt-accent)] focus-visible:outline-none"
      : ""
  }`;

  return (
    <li className={node.level <= 1 ? "mt-2 first:mt-0" : undefined}>
      {interactive ? (
        <button
          type="button"
          data-toc-block-id={node.block_id}
          data-toc-level={node.level}
          data-toc-page={pageNumber ?? ""}
          onClick={() => onJumpToSection?.(node.block_id)}
          className={className}
        >
          {content}
        </button>
      ) : (
        <div
          data-toc-block-id={node.block_id}
          data-toc-level={node.level}
          data-toc-page={pageNumber ?? ""}
          className={className}
        >
          {content}
        </div>
      )}
      {node.children && node.children.length > 0 && (
        <TocEntryList
          nodes={node.children}
          config={config}
          isPaged={isPaged}
          pageMap={pageMap}
          printMode={printMode}
          onJumpToSection={onJumpToSection}
        />
      )}
    </li>
  );
}

function TocEntryList({
  nodes,
  config,
  isPaged,
  pageMap,
  printMode,
  onJumpToSection,
}: {
  nodes: TocNode[];
  config: ResolvedTocConfig;
  isPaged: boolean;
  pageMap?: ReadonlyMap<string, number>;
  printMode: boolean;
  onJumpToSection?: (blockId: string) => void;
}) {
  return (
    <ul className="space-y-0.5">
      {nodes.map((node) => (
        <TocEntry
          key={node.block_id}
          node={node}
          config={config}
          isPaged={isPaged}
          pageMap={pageMap}
          printMode={printMode}
          onJumpToSection={onJumpToSection}
        />
      ))}
    </ul>
  );
}

export function ReportTocSection({
  config,
  nodes,
  isPaged,
  pageMap,
  printMode = false,
  onJumpToSection,
}: ReportTocSectionProps) {
  const visibleNodes = useMemo(
    () => filterTocNodes(nodes, config.max_depth),
    [nodes, config.max_depth],
  );

  if (!config.enabled || visibleNodes.length === 0) {
    return null;
  }

  return (
    <section className="my-8" data-report-toc="true" aria-label={config.title}>
      <h2 className="mb-3 flex items-center gap-2 border-b border-[var(--rpt-border)] pb-2 text-[15px] font-semibold tracking-wide text-[var(--rpt-ink)]">
        <span
          aria-hidden="true"
          className="h-3.5 w-1 shrink-0 rounded-full bg-[var(--rpt-accent)]"
        />
        <span>{config.title}</span>
      </h2>
      <TocEntryList
        nodes={visibleNodes}
        config={config}
        isPaged={isPaged}
        pageMap={pageMap}
        printMode={printMode}
        onJumpToSection={onJumpToSection}
      />
    </section>
  );
}
