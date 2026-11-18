import type { ReactNode } from "react";
import type { CitationMapEntry } from "@/contracts/report-protocol";

export interface InlineMarkdownProps {
  text: string;
  citationMap?: Record<string, CitationMapEntry>;
  onCitationClick?: (sourceId: string) => void;
}

const TOKEN_RE =
  /(\[\^([A-Za-z0-9_-]+)\]|`([^`]+)`|\*\*([^*]+)\*\*|\*([^*]+)\*|\[([^\]]+)\]\(([^)]+)\))/g;

export function InlineMarkdown({
  text,
  citationMap,
  onCitationClick,
}: InlineMarkdownProps) {
  const nodes: ReactNode[] = [];
  let lastIndex = 0;
  let match: RegExpExecArray | null = TOKEN_RE.exec(text);
  let key = 0;

  while (match !== null) {
    if (match.index > lastIndex) {
      nodes.push(text.slice(lastIndex, match.index));
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
    match = TOKEN_RE.exec(text);
  }

  if (lastIndex < text.length) {
    nodes.push(text.slice(lastIndex));
  }

  return <>{nodes}</>;
}
