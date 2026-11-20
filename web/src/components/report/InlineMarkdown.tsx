import { memo, type ReactNode } from "react";
import {
  CitationHoverCard,
  type CitationRef,
} from "@/components/report/CitationHoverCard";
import type {
  BibliographyEntry,
  CitationMapEntry,
} from "@/contracts/report-protocol";

export interface InlineMarkdownProps {
  text: string;
  citationMap?: Record<string, CitationMapEntry>;
  bibliographyById?: Map<string, BibliographyEntry>;
  printMode?: boolean;
  onCitationClick?: (sourceId: string) => void;
}

const TOKEN_RE =
  /(((?:\[\^[A-Za-z0-9_-]+\][ \t]*)+)|`([^`]+)`|\*\*([^*]+)\*\*|\*([^*]+)\*|\[([^\]]+)\]\(([^)]+)\))/g;

const CITATION_ID_RE = /\[\^([A-Za-z0-9_-]+)\]/g;

function stripOuterBrackets(label: string): string {
  const trimmed = label.trim();
  if (trimmed.startsWith("[") && trimmed.endsWith("]")) {
    return trimmed.slice(1, -1);
  }
  return trimmed;
}

function parseCitationIds(group: string): string[] {
  const ids: string[] = [];
  const seen = new Set<string>();
  for (const match of group.matchAll(CITATION_ID_RE)) {
    const citeId = match[1];
    if (citeId && !seen.has(citeId)) {
      seen.add(citeId);
      ids.push(citeId);
    }
  }
  return ids;
}

function buildCitationRefs(
  citeIds: string[],
  citationMap: Record<string, CitationMapEntry> | undefined,
  bibliographyById: Map<string, BibliographyEntry> | undefined,
): CitationRef[] {
  const refs: CitationRef[] = citeIds.map((citeId) => {
    const entry = citationMap?.[citeId];
    const bib = bibliographyById?.get(citeId);
    const number =
      entry && typeof entry.number === "number" ? entry.number : null;
    const label = stripOuterBrackets(entry?.label ?? citeId);
    return {
      citeId,
      sourceId: entry?.source_id ?? citeId,
      number,
      label,
      badge: number !== null ? `[${number}]` : `[${label}]`,
      title: bib?.source.title ?? entry?.title ?? citeId,
      authors: bib?.source.authors?.join(", "),
      date: bib?.source.date,
      excerpt: bib?.excerpt,
      url: bib?.source.url,
    };
  });

  if (refs.every((ref) => ref.number !== null)) {
    refs.sort((a, b) => (a.number ?? 0) - (b.number ?? 0));
  }
  return refs;
}

function mergedCitationLabel(refs: CitationRef[]): string {
  if (refs.every((ref) => ref.number !== null)) {
    return `[${refs.map((ref) => ref.number).join(",")}]`;
  }
  return `[${refs.map((ref) => ref.label).join(",")}]`;
}

function CitationGroup({
  refs,
  printMode,
  onCitationClick,
}: {
  refs: CitationRef[];
  printMode: boolean;
  onCitationClick?: (sourceId: string) => void;
}) {
  const first = refs[0];
  if (!first) {
    return null;
  }
  const label = mergedCitationLabel(refs);

  const locate = (sourceId: string) => {
    if (onCitationClick) {
      onCitationClick(sourceId);
      return;
    }
    const target = window.document.getElementById(`bib-${sourceId}`);
    target?.scrollIntoView({ behavior: "smooth", block: "center" });
  };

  if (printMode) {
    return (
      <sup className="ml-0.5">
        <span className="inline-block rounded px-1 py-0.5 text-[11px] font-medium text-muted-foreground tabular-nums">
          {label}
        </span>
      </sup>
    );
  }

  return <CitationHoverCard refs={refs} label={label} onLocate={locate} />;
}

export const InlineMarkdown = memo(function InlineMarkdown({
  text,
  citationMap,
  bibliographyById,
  printMode = false,
  onCitationClick,
}: InlineMarkdownProps) {
  const nodes: ReactNode[] = [];
  let lastIndex = 0;
  const tokenRe = new RegExp(TOKEN_RE);
  let match: RegExpExecArray | null = tokenRe.exec(text);
  let key = 0;

  while (match !== null) {
    if (match.index > lastIndex) {
      const plain = text.slice(lastIndex, match.index);
      nodes.push(<span key={`txt-${key++}`}>{plain}</span>);
    }

    const citationGroup = match[2];
    const codeText = match[3];
    const boldText = match[4];
    const italicText = match[5];
    const linkLabel = match[6];
    const linkHref = match[7];

    if (citationGroup !== undefined) {
      const citeIds = parseCitationIds(citationGroup);
      const refs = buildCitationRefs(citeIds, citationMap, bibliographyById);
      nodes.push(
        <CitationGroup
          key={`cite-${key++}`}
          refs={refs}
          printMode={printMode}
          onCitationClick={onCitationClick}
        />,
      );
    } else if (codeText !== undefined) {
      nodes.push(
        <code
          key={`code-${key++}`}
          className="rounded bg-black/5 px-1 py-0.5 font-mono text-[0.88em]"
        >
          {codeText}
        </code>,
      );
    } else if (boldText !== undefined) {
      nodes.push(<strong key={`b-${key++}`}>{boldText}</strong>);
    } else if (italicText !== undefined) {
      nodes.push(<em key={`i-${key++}`}>{italicText}</em>);
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
    match = tokenRe.exec(text);
  }

  if (lastIndex < text.length) {
    const tail = text.slice(lastIndex);
    nodes.push(<span key={`txt-${key++}`}>{tail}</span>);
  }

  return <>{nodes}</>;
});
