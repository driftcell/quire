import * as React from "react";
import { BookOpenIcon, ExternalLinkIcon } from "lucide-react";
import { cn } from "cn";
import {
  Trace,
  TraceCollapsible,
  TraceCollapsibleContent,
  TraceCollapsibleTrigger,
  TraceDescription,
  TraceIcon,
  TraceLine,
  TraceTitle,
} from "@/components/ui/trace";

export type SourcesProps = React.ComponentProps<typeof TraceCollapsible>;

function Sources({
  className,
  defaultOpen = false,
  ...props
}: SourcesProps) {
  return (
    <Trace data-slot="sources" className={cn("mt-2", className)}>
      <TraceCollapsible defaultOpen={defaultOpen} {...props} />
    </Trace>
  );
}

export type SourcesTriggerProps = React.ComponentProps<
  typeof TraceCollapsibleTrigger
> & {
  count: number;
};

function SourcesTrigger({
  className,
  count,
  children,
  ...props
}: SourcesTriggerProps) {
  return (
    <TraceCollapsibleTrigger
      data-slot="sources-trigger"
      className={cn(
        "inline-flex h-7 items-center rounded-full border border-border/65 bg-foreground/[0.03] px-2.5 text-xs font-medium text-muted-foreground transition-colors hover:bg-foreground/[0.06] hover:text-foreground",
        className,
      )}
      {...props}
    >
      <TraceLine className="text-xs">
        <TraceIcon className="size-3.5 text-muted-foreground">
          <BookOpenIcon className="size-3" />
        </TraceIcon>
        <TraceTitle className="font-medium">
          {children ?? `${count} 个参考来源`}
        </TraceTitle>
      </TraceLine>
    </TraceCollapsibleTrigger>
  );
}

export type SourcesContentProps = React.ComponentProps<
  typeof TraceCollapsibleContent
>;

function SourcesContent({
  className,
  children,
  ...props
}: SourcesContentProps) {
  return (
    <TraceCollapsibleContent
      data-slot="sources-content"
      className={cn("mt-2 space-y-1.5 border-l border-border/60 pl-3", className)}
      {...props}
    >
      <Trace className="gap-1.5">{children}</Trace>
    </TraceCollapsibleContent>
  );
}

export type SourceProps = React.ComponentProps<"a"> & {
  index?: number;
  title?: string;
  publisher?: string;
  excerpt?: string;
};

function Source({
  href,
  index,
  title,
  publisher,
  excerpt,
  className,
  children,
  ...props
}: SourceProps) {
  const displayTitle = title || href || "未命名来源";
  const content = (
    <div
      data-slot="source-item"
      className={cn(
        "group/source flex w-full min-w-0 flex-col gap-1 rounded-xl border border-border/60 bg-background px-3 py-2 text-xs transition-colors hover:border-foreground/25 hover:bg-foreground/[0.02]",
        className,
      )}
    >
      <TraceLine className="w-full text-xs">
        {index !== undefined && (
          <span className="inline-flex h-4 min-w-4 shrink-0 items-center justify-center rounded-full bg-foreground/[0.06] px-1 font-mono text-[10px] font-medium text-foreground/80 tabular-nums">
            {index}
          </span>
        )}
        <TraceTitle className="font-medium text-foreground">
          {children ?? displayTitle}
        </TraceTitle>
        {publisher && (
          <TraceDescription className="text-[11px] text-muted-foreground">
            · {publisher}
          </TraceDescription>
        )}
        {href && (
          <TraceIcon className="ml-auto size-3 text-muted-foreground/60 group-hover/source:text-foreground">
            <ExternalLinkIcon className="size-3" />
          </TraceIcon>
        )}
      </TraceLine>
      {excerpt && (
        <p className="line-clamp-2 pl-5.5 text-[11.5px] leading-relaxed text-muted-foreground">
          {excerpt}
        </p>
      )}
    </div>
  );

  if (href) {
    return (
      <a
        href={href}
        target="_blank"
        rel="noreferrer noopener"
        className="block w-full min-w-0 no-underline outline-none focus-visible:ring-2 focus-visible:ring-ring"
        {...props}
      >
        {content}
        <span className="sr-only">(在新标签页中打开)</span>
      </a>
    );
  }

  return <div className="w-full min-w-0">{content}</div>;
}

export { Sources, SourcesTrigger, SourcesContent, Source };
