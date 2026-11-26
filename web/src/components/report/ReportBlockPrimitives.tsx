import type { CSSProperties, ReactNode } from "react";
import { cn } from "cn";

export interface ReportTextBlockViewProps {
  type: "heading" | "paragraph" | "quote";
  children: ReactNode;
  domId?: string;
  level?: number;
  callout?: boolean;
}

/** Shared committed/streaming text rendering so theme CSS applies identically. */
export function ReportTextBlockView({
  type,
  children,
  domId,
  level = 2,
  callout = false,
}: ReportTextBlockViewProps) {
  if (type === "paragraph") {
    return callout ? (
      <div
        id={domId}
        data-block-variant="callout"
        style={{
          fontSize: "var(--rpt-body-size, 14.5px)",
          lineHeight: "var(--rpt-line-height, 1.82)",
          borderRadius: "var(--rpt-radius, 6px)",
        }}
        className="rpt-callout my-4 rounded-lg border border-[var(--rpt-border)] border-l-[3px] border-l-[var(--rpt-accent)] bg-[var(--rpt-surface)] px-4 py-3 tracking-[0.01em] text-[var(--rpt-ink)]"
      >
        {children}
      </div>
    ) : (
      <p
        id={domId}
        style={{
          fontSize: "var(--rpt-body-size, 14.5px)",
          lineHeight: "var(--rpt-line-height, 1.82)",
          marginTop: "var(--rpt-para-gap, 0.75rem)",
          marginBottom: "var(--rpt-para-gap, 0.75rem)",
          textIndent: "var(--rpt-para-indent, 0em)",
        }}
        className="rpt-paragraph my-3 tracking-[0.01em] text-[var(--rpt-ink)] [text-align:var(--rpt-para-align,left)]"
      >
        {children}
      </p>
    );
  }

  if (type === "quote") {
    return (
      <blockquote
        id={domId}
        style={{ borderRadius: "var(--rpt-radius, 6px)" }}
        className="rpt-quote my-4 rounded-r-lg border-l-[3px] border-[var(--rpt-accent)] bg-[var(--rpt-quote-bg,var(--rpt-surface))] px-4 py-3 text-[14px] leading-[1.75] text-[var(--rpt-ink)]/90"
      >
        {children}
      </blockquote>
    );
  }

  switch (level) {
    case 1:
      return (
        <h1
          id={domId}
          data-rpt-h="1"
          style={{
            fontSize: "var(--rpt-h1-size, 22px)",
            fontWeight:
              "var(--rpt-heading-weight, 700)" as CSSProperties["fontWeight"],
            letterSpacing: "var(--rpt-heading-tracking, -0.01em)",
          }}
          className="rpt-h1 mt-8 mb-4 border-b-2 border-[var(--rpt-accent)] pb-2 text-xl font-bold tracking-tight text-balance text-[var(--rpt-ink)]"
        >
          {children}
        </h1>
      );
    case 2:
      return (
        <h2
          id={domId}
          data-rpt-h="2"
          style={{
            fontWeight:
              "var(--rpt-heading-weight, 600)" as CSSProperties["fontWeight"],
            letterSpacing: "var(--rpt-heading-tracking, 0em)",
          }}
          className="rpt-h2 mt-7 mb-3 border-b border-[var(--rpt-border)] border-l-[3px] border-l-[var(--rpt-accent)] pb-1.5 pl-2.5 text-[17px] font-semibold text-balance text-[var(--rpt-ink)]"
        >
          {children}
        </h2>
      );
    case 3:
      return (
        <h3
          id={domId}
          data-rpt-h="3"
          className="rpt-h3 mt-5 mb-2 text-[15px] font-semibold text-balance text-[var(--rpt-ink)]"
        >
          {children}
        </h3>
      );
    case 4:
      return (
        <h4
          id={domId}
          data-rpt-h="4"
          className="rpt-h4 mt-4 mb-1.5 text-[14px] font-semibold text-balance text-[var(--rpt-ink)]"
        >
          {children}
        </h4>
      );
    default:
      return (
        <h5
          id={domId}
          data-rpt-h="5"
          className="rpt-h5 mt-3 mb-1 text-[13px] font-semibold text-balance text-[var(--rpt-ink)]"
        >
          {children}
        </h5>
      );
  }
}

export function ReportFigureCaption({
  children,
  separated = false,
}: {
  children: ReactNode;
  separated?: boolean;
}) {
  return (
    <figcaption
      className={cn(
        "text-center text-[13.5px] font-semibold tracking-wide whitespace-pre-wrap text-[var(--rpt-ink)]",
        separated && "pt-1",
      )}
    >
      {children}
    </figcaption>
  );
}
