import { useCallback, useMemo } from "react";
import type { CoverConfig } from "@/contracts/report-protocol";

const BLOCKED_COVER_TAGS = "script, iframe, object, embed";
const URL_ATTRIBUTES = new Set([
  "href",
  "src",
  "xlink:href",
  "action",
  "formaction",
]);

/**
 * Strip executable markup from agent-authored cover HTML before it reaches
 * `innerHTML`. Script-capable elements, inline event handlers and
 * `javascript:` URLs are removed; ordinary layout markup is preserved.
 */
export function sanitizeCoverHtml(raw: string): string {
  const doc = new DOMParser().parseFromString(raw, "text/html");
  for (const element of Array.from(doc.querySelectorAll(BLOCKED_COVER_TAGS))) {
    element.remove();
  }
  for (const element of Array.from(doc.body.querySelectorAll("*"))) {
    for (const attribute of Array.from(element.attributes)) {
      const name = attribute.name.toLowerCase();
      if (name.startsWith("on")) {
        element.removeAttribute(attribute.name);
        continue;
      }
      if (!URL_ATTRIBUTES.has(name)) {
        continue;
      }
      const value = Array.from(attribute.value)
        .filter((char) => {
          const code = char.charCodeAt(0);
          return code > 0x20 && code !== 0x7f;
        })
        .join("")
        .toLowerCase();
      if (value.startsWith("javascript:") || value.startsWith("vbscript:")) {
        element.removeAttribute(attribute.name);
      }
    }
  }
  return doc.body.innerHTML;
}

function CoverCustomHtml({ html }: { html: string }) {
  const sanitized = useMemo(() => sanitizeCoverHtml(html), [html]);
  const setHost = useCallback(
    (node: HTMLDivElement | null) => {
      if (node) {
        node.innerHTML = sanitized;
      }
    },
    [sanitized],
  );

  return <div ref={setHost} className="report-custom-cover-html" />;
}

export function ReportCover({
  cover,
  isPaged = false,
}: {
  cover: CoverConfig;
  isPaged?: boolean;
}) {
  if (!cover.enabled) {
    return null;
  }
  const authors = cover.authors ?? [];
  if (cover.custom_html) {
    let html = cover.custom_html;
    const replacements: Record<string, string> = {
      title: cover.title ?? "",
      subtitle: cover.subtitle ?? "",
      date: cover.date ?? "",
      signature: cover.meta?.signature?.trim() || authors.join("、"),
    };
    for (const [key, value] of Object.entries(replacements)) {
      html = html.replaceAll(`{{${key}}}`, value);
    }
    return (
      <div
        data-report-cover="custom"
        className={
          isPaged
            ? "report-cover-custom w-full"
            : "report-cover-custom mb-10 border-b border-[var(--rpt-border)] pb-8"
        }
      >
        <CoverCustomHtml html={html} />
      </div>
    );
  }
  if (!cover.title) {
    return null;
  }

  const variant = cover.variant ?? "classic";
  const kicker = cover.meta?.kicker?.trim();
  const unit =
    cover.meta?.unit?.trim() ||
    cover.meta?.organization?.trim() ||
    cover.meta?.institution?.trim() ||
    null;
  const metaEntries = [
    authors.length > 0 ? `作者：${authors.join("、")}` : null,
    unit ? `单位：${unit}` : null,
    cover.date ? `日期：${cover.date}` : null,
  ].filter((entry): entry is string => entry !== null);
  const metaClass =
    variant === "modern"
      ? "rounded-full border border-[var(--rpt-border)] bg-[var(--rpt-surface)] px-2.5 py-0.5"
      : "";
  const renderMeta = (className: string) =>
    metaEntries.length > 0 ? (
      <div
        className={`flex flex-wrap gap-x-5 gap-y-1 text-xs text-[var(--rpt-muted)] ${className}`}
      >
        {metaEntries.map((entry) => (
          <span key={entry} className={metaClass}>
            {entry}
          </span>
        ))}
      </div>
    ) : null;

  if (variant === "modern") {
    return (
      <header className="mb-10 flex gap-4 border-b border-[var(--rpt-border)] pb-6">
        <div className="w-1 shrink-0 rounded-full bg-[var(--rpt-accent)]" />
        <div className="min-w-0 flex-1">
          {kicker && (
            <div className="mb-2 text-[11px] font-semibold tracking-[0.2em] text-[var(--rpt-accent)] uppercase">
              {kicker}
            </div>
          )}
          <h1 className="text-balance text-[26px] leading-snug font-bold tracking-tight text-[var(--rpt-ink)] sm:text-[28px]">
            {cover.title}
          </h1>
          {cover.subtitle && (
            <p className="mt-2 text-sm text-[var(--rpt-muted)]">
              {cover.subtitle}
            </p>
          )}
          {renderMeta("mt-4")}
        </div>
      </header>
    );
  }

  if (variant === "minimal") {
    return (
      <header className="mb-10 border-b border-[var(--rpt-border)] pb-5">
        {kicker && (
          <div className="mb-2 text-[11px] font-medium tracking-[0.24em] text-[var(--rpt-muted)] uppercase">
            {kicker}
          </div>
        )}
        <h1 className="text-balance text-2xl leading-snug font-semibold tracking-tight text-[var(--rpt-ink)]">
          {cover.title}
        </h1>
        {cover.subtitle && (
          <p className="mt-2 text-sm text-[var(--rpt-muted)]">
            {cover.subtitle}
          </p>
        )}
        {renderMeta("mt-3")}
      </header>
    );
  }

  if (variant === "split") {
    return (
      <header className="mb-10 grid gap-5 border-b-2 border-[var(--rpt-accent)] pb-6 sm:grid-cols-[1.6fr_1fr] sm:gap-8">
        <div className="min-w-0">
          {kicker && (
            <div className="mb-2 text-[11px] font-semibold tracking-[0.22em] text-[var(--rpt-accent)] uppercase">
              {kicker}
            </div>
          )}
          <h1 className="text-balance text-[26px] leading-snug font-bold tracking-tight text-[var(--rpt-ink)] sm:text-[28px]">
            {cover.title}
          </h1>
          {cover.subtitle && (
            <p className="mt-2 text-sm text-[var(--rpt-muted)]">
              {cover.subtitle}
            </p>
          )}
        </div>
        <div className="flex flex-col justify-end gap-y-1 border-[var(--rpt-border)] text-xs text-[var(--rpt-muted)] sm:border-l sm:pl-6">
          {metaEntries.map((entry) => (
            <span key={entry}>{entry}</span>
          ))}
        </div>
      </header>
    );
  }

  return (
    <header className="mb-10 text-center">
      {kicker && (
        <div className="mb-2 text-[11px] font-semibold tracking-[0.28em] text-[var(--rpt-accent)] uppercase">
          {kicker}
        </div>
      )}
      <h1 className="mx-auto max-w-[92%] text-balance text-[26px] leading-snug font-bold tracking-tight text-[var(--rpt-ink)] sm:text-[28px]">
        {cover.title}
      </h1>
      {cover.subtitle && (
        <p className="mt-3 text-sm text-[var(--rpt-muted)]">
          —— {cover.subtitle} ——
        </p>
      )}
      {renderMeta("mt-5 justify-center")}
    </header>
  );
}
