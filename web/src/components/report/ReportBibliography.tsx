import { BookOpenIcon, ExternalLinkIcon } from "lucide-react";
import type { BibliographyEntry } from "@/contracts/report-protocol";

function referenceDomain(url: string | null | undefined): string | null {
  if (!url) {
    return null;
  }
  try {
    return new URL(url).hostname.replace(/^www\./, "");
  } catch {
    return null;
  }
}

export interface ReportBibliographyProps {
  entries: BibliographyEntry[];
  title: string;
  measurementOnly?: boolean;
}

export function ReportBibliography({
  entries,
  title,
  measurementOnly = false,
}: ReportBibliographyProps) {
  if (entries.length === 0) {
    return null;
  }
  return (
    <section className="mt-10 border-t border-[var(--rpt-border)] pt-6">
      <h2 className="mb-4 flex items-center gap-2 text-base font-semibold text-[var(--rpt-ink)]">
        <BookOpenIcon className="size-4 text-[var(--rpt-accent)]" />
        <span>{title}</span>
      </h2>
      <ol className="space-y-3.5 text-xs leading-relaxed">
        {entries.map((entry) => {
          const source = entry.source;
          const attribution =
            source.publisher?.trim() || source.authors?.join(", ") || null;
          const domain = referenceDomain(source.url);
          const badge = entry.label.trim() || `[${entry.number}]`;
          const hasMetadata = Boolean(attribution || source.date || source.url);
          return (
            <li
              key={entry.source_id}
              id={measurementOnly ? undefined : `bib-${entry.source_id}`}
              className="flex scroll-mt-24 gap-3"
            >
              <span className="mt-px shrink-0 font-mono text-[11px] text-muted-foreground tabular-nums">
                {badge}
              </span>
              <div className="min-w-0 flex-1 space-y-0.5">
                {source.url ? (
                  <a
                    href={source.url}
                    target="_blank"
                    rel="noreferrer noopener"
                    className="font-medium text-[var(--rpt-ink)] underline-offset-2 hover:text-[var(--rpt-accent)] hover:underline"
                  >
                    {source.title}
                  </a>
                ) : (
                  <div className="font-medium text-foreground">
                    {source.title}
                  </div>
                )}
                {hasMetadata && (
                  <div className="flex flex-wrap items-center gap-x-1.5 gap-y-0.5 text-[11px] text-muted-foreground">
                    {attribution && <span>{attribution}</span>}
                    {attribution && (source.date || source.url) && (
                      <span aria-hidden="true">·</span>
                    )}
                    {source.date && <span>{source.date}</span>}
                    {source.date && source.url && (
                      <span aria-hidden="true">·</span>
                    )}
                    {source.url && (
                      <a
                        href={source.url}
                        target="_blank"
                        rel="noreferrer noopener"
                        className="inline-flex items-center gap-0.5 text-slate-600 underline decoration-slate-400 hover:text-[var(--rpt-accent)]"
                      >
                        {domain ?? "链接"}
                        <ExternalLinkIcon className="size-2.5" />
                      </a>
                    )}
                  </div>
                )}
                {entry.excerpt && (
                  <div className="mt-1 rounded border-l-2 border-slate-300 bg-slate-50 px-2 py-1 text-[11px] leading-relaxed text-slate-600">
                    “{entry.excerpt}”
                  </div>
                )}
              </div>
            </li>
          );
        })}
      </ol>
    </section>
  );
}
