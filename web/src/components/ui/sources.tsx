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
    <Trace data-slot="sources" className={cn("mt-2.5", className)}>
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
        "rounded-lg border border-border/60 bg-muted/30 px-2.5 py-1 text-xs font-medium text-muted-foreground hover:bg-muted/60 hover:text-foreground",
        className,
      )}
      {...props}
    >
      <TraceLine className="text-xs">
        <TraceIcon className="size-3.5 text-primary/80">
          <BookOpenIcon className="size-3.5" />
        </TraceIcon>
        <TraceTitle className="font-medium">
          {children ?? `参考了 ${count} 个来源`}
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
      className={cn("mt-2 space-y-1.5 border-l-border/70 pl-3", className)}
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
        "group/source flex w-full min-w-0 flex-col gap-0.5 rounded-lg border border-border/50 bg-background/80 px-2.5 py-1.5 text-xs transition-colors hover:border-border hover:bg-muted/30",
        className,
      )}
    >
      <TraceLine className="w-full text-xs">
        {index !== undefined && (
          <span className="inline-flex size-4 shrink-0 items-center justify-center rounded bg-primary/10 font-mono text-[10px] font-semibold text-primary tabular-nums">
            {index}
          </span>
        )}
        <TraceTitle className="font-medium text-foreground">
          {children ?? displayTitle}
        </TraceTitle>
        {publisher && (
          <TraceDescription className="text-[11px]">
            · {publisher}
          </TraceDescription>
        )}
        {href && (
          <TraceIcon className="ml-auto size-3 text-muted-foreground/70 group-hover/source:text-foreground">
            <ExternalLinkIcon className="size-3" />
          </TraceIcon>
        )}
      </TraceLine>
      {excerpt && (
        <p className="line-clamp-2 pl-6 text-[11px] leading-4 text-muted-foreground/85">
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
