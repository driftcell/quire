import { memo, type ReactNode } from "react";
import type {
  CitationMapEntry,
  TextQuoteSelector,
} from "@/contracts/report-protocol";

export interface InlineMarkdownProps {
  text: string;
  citationMap?: Record<string, CitationMapEntry>;
  onCitationClick?: (sourceId: string) => void;
  highlightQuotes?: TextQuoteSelector[];
  onQuoteClick?: (quote: TextQuoteSelector, rect: DOMRect) => void;
}

const TOKEN_RE =
  /(\[\^([A-Za-z0-9_-]+)\]|`([^`]+)`|\*\*([^*]+)\*\*|\*([^*]+)\*|\[([^\]]+)\]\(([^)]+)\))/g;

function renderHighlightedSegment(
  segment: string,
  quotes: TextQuoteSelector[] | undefined,
  onQuoteClick: ((quote: TextQuoteSelector, rect: DOMRect) => void) | undefined,
  keyPrefix: string,
): ReactNode {
  if (!quotes || quotes.length === 0 || segment.length === 0) {
    return segment;
  }

  let bestIdx = -1;
  let bestNeedle = "";
  let bestQuote: TextQuoteSelector | null = null;

  for (const q of quotes) {
    const needle = q.exact.trim();
    if (!needle) continue;
    const idx = segment.indexOf(needle);
    if (idx !== -1 && (bestIdx === -1 || idx < bestIdx)) {
      bestIdx = idx;
      bestNeedle = needle;
      bestQuote = q;
    }
  }

  if (bestIdx === -1 || !bestQuote) {
    return segment;
  }

  const before = segment.slice(0, bestIdx);
  const match = segment.slice(bestIdx, bestIdx + bestNeedle.length);
  const after = segment.slice(bestIdx + bestNeedle.length);
  const matchedQuote = bestQuote;

  return (
    <>
      {before}
      {onQuoteClick ? (
        <button
          key={`${keyPrefix}-q`}
          type="button"
          title="点击查看或回复此划词批注"
          onClick={(event) => {
            event.stopPropagation();
            onQuoteClick(
              matchedQuote,
              event.currentTarget.getBoundingClientRect(),
            );
          }}
          className="inline cursor-pointer rounded-xs border-b-2 border-amber-500/80 bg-amber-500/10 px-0.5 text-left font-inherit transition-colors hover:bg-amber-500/20 focus-visible:ring-2 focus-visible:ring-amber-500 focus-visible:outline-none dark:border-amber-400/80 dark:bg-amber-400/15 dark:hover:bg-amber-400/25"
        >
          {match}
        </button>
      ) : (
        <mark
          key={`${keyPrefix}-mark`}
          className="rounded-xs border-b-2 border-amber-500/80 bg-amber-500/10 px-0.5 text-inherit dark:border-amber-400/80 dark:bg-amber-400/15"
        >
          {match}
        </mark>
      )}
      {renderHighlightedSegment(after, quotes, onQuoteClick, `${keyPrefix}-r`)}
    </>
  );
}

export const InlineMarkdown = memo(function InlineMarkdown({
  text,
  citationMap,
  onCitationClick,
  highlightQuotes,
  onQuoteClick,
}: InlineMarkdownProps) {
  const nodes: ReactNode[] = [];
  let lastIndex = 0;
  TOKEN_RE.lastIndex = 0;
  let match: RegExpExecArray | null = TOKEN_RE.exec(text);
  let key = 0;

  while (match !== null) {
    if (match.index > lastIndex) {
      const plain = text.slice(lastIndex, match.index);
      nodes.push(
        <span key={`txt-${key++}`}>
          {renderHighlightedSegment(
            plain,
            highlightQuotes,
            onQuoteClick,
            `seg-${key}`,
          )}
        </span>,
      );
    }

    const citeId = match[2];
    const codeText = match[3];
    const boldText = match[4];
    const italicText = match[5];
    const linkLabel = match[6];
    const linkHref = match[7];

    if (citeId !== undefined) {
      const entry = citationMap?.[citeId];
      const label = entry?.label ?? `[${citeId}]`;
      nodes.push(
        <sup key={`cite-${key++}`} className="ml-0.5">
          <a
            href={`#bib-${citeId}`}
            title={entry?.title ?? citeId}
            onClick={(event) => {
              if (onCitationClick) {
                event.preventDefault();
                onCitationClick(citeId);
              }
            }}
            className="rounded px-0.5 text-[11px] font-medium text-blue-600 no-underline hover:bg-blue-50 hover:underline dark:text-blue-400"
          >
            {label}
          </a>
        </sup>,
      );
    } else if (codeText !== undefined) {
      nodes.push(
        <code
          key={`code-${key++}`}
          className="rounded bg-black/5 px-1 py-0.5 font-mono text-[0.88em] dark:bg-white/10"
        >
          {codeText}
        </code>,
      );
    } else if (boldText !== undefined) {
      nodes.push(
        <strong key={`b-${key++}`}>
          {renderHighlightedSegment(
            boldText,
            highlightQuotes,
            onQuoteClick,
            `b-${key}`,
          )}
        </strong>,
      );
    } else if (italicText !== undefined) {
      nodes.push(
        <em key={`i-${key++}`}>
          {renderHighlightedSegment(
            italicText,
            highlightQuotes,
            onQuoteClick,
            `i-${key}`,
          )}
        </em>,
      );
    } else if (linkLabel !== undefined && linkHref !== undefined) {
      const safeHref =
        linkHref.startsWith("http://") ||
        linkHref.startsWith("https://") ||
        linkHref.startsWith("#")
          ? linkHref
          : "#";
      nodes.push(
        safeHref.startsWith("http") ? (
          <a
            key={`a-${key++}`}
            href={safeHref}
            target="_blank"
            rel="noreferrer"
            className="underline decoration-current/40 underline-offset-2 hover:decoration-current"
          >
            {linkLabel}
          </a>
        ) : (
          <a
            key={`a-${key++}`}
            href={safeHref}
            className="underline decoration-current/40 underline-offset-2 hover:decoration-current"
          >
            {linkLabel}
          </a>
        ),
      );
    }

    lastIndex = match.index + match[0].length;
    match = TOKEN_RE.exec(text);
  }

  if (lastIndex < text.length) {
    const tail = text.slice(lastIndex);
    nodes.push(
      <span key={`txt-${key++}`}>
        {renderHighlightedSegment(
          tail,
          highlightQuotes,
          onQuoteClick,
          `tail-${key}`,
        )}
      </span>,
    );
  }

  return <>{nodes}</>;
});
