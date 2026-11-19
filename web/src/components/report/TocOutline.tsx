import { useEffect, useRef, useState } from "react";
import { ListTreeIcon, RotateCcwIcon } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import type {
  CommentThread,
  TocConfig,
  TocNode,
} from "@/contracts/report-protocol";
import {
  TOC_MAX_DEPTH_OPTIONS,
  TOC_VARIANT_OPTIONS,
  clearTocConfig,
  saveTocConfigPatch,
  useTocConfig,
  type ResolvedTocConfig,
} from "@/lib/tocConfig";

export interface TocOutlineProps {
  docId: string;
  toc?: TocConfig | null;
  isPaged?: boolean;
  nodes: TocNode[];
  activeActorsBySection: Map<string, string[]>;
  comments: CommentThread[];
  onJumpToSection: (blockId: string) => void;
  placement?: "top" | "bottom";
}

function countCommentsInSubtree(
  node: TocNode,
  commentsByBlock: Map<string, number>,
): number {
  let count = commentsByBlock.get(node.block_id) ?? 0;
  for (const child of node.children ?? []) {
    count += countCommentsInSubtree(child, commentsByBlock);
  }
  return count;
}

function TocBranch({
  nodes,
  depth,
  activeActorsBySection,
  commentsByBlock,
  onJump,
}: {
  nodes: TocNode[];
  depth: number;
  activeActorsBySection: Map<string, string[]>;
  commentsByBlock: Map<string, number>;
  onJump: (blockId: string) => void;
}) {
  return (
    <ul className={depth > 0 ? "mt-1 space-y-1 border-l pl-3" : "space-y-1"}>
      {nodes.map((node) => {
        const actors = activeActorsBySection.get(node.block_id) ?? [];
        const commentCount = countCommentsInSubtree(node, commentsByBlock);
        return (
          <li key={node.block_id}>
            <button
              type="button"
              onClick={() => onJump(node.block_id)}
              className="flex w-full items-center gap-1.5 rounded px-1.5 py-1 text-left text-xs hover:bg-accent"
            >
              {node.number && (
                <span className="shrink-0 font-mono text-[11px] text-muted-foreground">
                  {node.number}
                </span>
              )}
              <span className="min-w-0 flex-1 truncate">{node.text}</span>
              {actors.length > 0 && (
                <span
                  title={`${actors.join(", ")} 撰写中`}
                  className="inline-flex shrink-0 items-center gap-1 rounded-full bg-emerald-500/15 px-1.5 py-0.5 text-[10px] text-emerald-700 dark:text-emerald-300"
                >
                  <span className="size-1.5 animate-pulse rounded-full bg-emerald-500" />
                  {actors[0]}
                </span>
              )}
              {commentCount > 0 && (
                <Badge
                  variant="outline"
                  className="h-4 px-1 text-[10px] tabular-nums"
                >
                  {commentCount}
                </Badge>
              )}
            </button>
            {node.children && node.children.length > 0 && (
              <TocBranch
                nodes={node.children}
                depth={depth + 1}
                activeActorsBySection={activeActorsBySection}
                commentsByBlock={commentsByBlock}
                onJump={onJump}
              />
            )}
          </li>
        );
      })}
    </ul>
  );
}

function TocInlineControls({
  docId,
  config,
  isPaged,
}: {
  docId: string;
  config: ResolvedTocConfig;
  isPaged: boolean;
}) {
  const update = (
    patch: Partial<
      Pick<ResolvedTocConfig, "enabled" | "max_depth" | "variant">
    >,
  ) => {
    saveTocConfigPatch(docId, patch);
  };

  return (
    <div className="mt-2 space-y-2 border-t pt-2">
      <div className="flex items-center justify-between gap-2">
        <span className="text-[11px] font-medium text-muted-foreground">
          内嵌目录
        </span>
        <button
          type="button"
          role="switch"
          aria-checked={config.enabled}
          aria-label="在报告封面下方显示内嵌目录"
          onClick={() => update({ enabled: !config.enabled })}
          className="inline-flex items-center gap-1.5 text-[10px] text-muted-foreground transition-colors hover:text-foreground"
        >
          <span
            aria-hidden="true"
            className={`relative h-3.5 w-6 rounded-full transition-colors ${
              config.enabled ? "bg-emerald-500" : "bg-muted-foreground/30"
            }`}
          >
            <span
              className={`absolute top-0.5 size-2.5 rounded-full bg-white shadow-xs transition-transform ${
                config.enabled ? "translate-x-3" : "translate-x-0.5"
              }`}
            />
          </span>
          <span>{config.enabled ? "已开启" : "已关闭"}</span>
        </button>
      </div>

      <div className="flex items-center justify-between gap-2">
        <span className="text-[11px] text-muted-foreground">层级深度</span>
        <div className="flex items-center rounded-md border bg-muted/40 p-0.5">
          {TOC_MAX_DEPTH_OPTIONS.map((depth) => (
            <button
              key={depth}
              type="button"
              aria-pressed={config.max_depth === depth}
              onClick={() => update({ max_depth: depth })}
              className={`rounded px-1.5 py-0.5 text-[10px] tabular-nums transition-colors ${
                config.max_depth === depth
                  ? "bg-background font-medium text-foreground shadow-xs"
                  : "text-muted-foreground hover:text-foreground"
              }`}
            >
              {depth}级
            </button>
          ))}
        </div>
      </div>

      <div className="flex items-center justify-between gap-2">
        <span className="text-[11px] text-muted-foreground">目录样式</span>
        <div className="flex items-center gap-0.5 rounded-md border bg-muted/40 p-0.5">
          {TOC_VARIANT_OPTIONS.map((option) => (
            <button
              key={option.value}
              type="button"
              title={option.description}
              aria-pressed={config.variant === option.value}
              onClick={() => update({ variant: option.value })}
              className={`rounded px-1.5 py-0.5 text-[10px] transition-colors ${
                config.variant === option.value
                  ? "bg-background font-medium text-foreground shadow-xs"
                  : "text-muted-foreground hover:text-foreground"
              }`}
            >
              {option.label}
            </button>
          ))}
        </div>
      </div>

      <div className="flex items-center justify-between gap-2">
        <span className="text-[10px] text-muted-foreground">
          {isPaged ? "分页视图显示精确页码" : "切换分页视图显示页码"}
        </span>
        <button
          type="button"
          onClick={() => clearTocConfig(docId)}
          className="inline-flex items-center gap-1 text-[10px] text-muted-foreground transition-colors hover:text-foreground"
        >
          <RotateCcwIcon className="size-3" />
          恢复默认
        </button>
      </div>
    </div>
  );
}

export function TocOutline({
  docId,
  toc,
  isPaged = false,
  nodes,
  activeActorsBySection,
  comments,
  onJumpToSection,
  placement = "bottom",
}: TocOutlineProps) {
  const [open, setOpen] = useState(false);
  const containerRef = useRef<HTMLDivElement>(null);
  const resolvedToc = useTocConfig(docId, toc, isPaged);

  useEffect(() => {
    if (!open) return undefined;
    const handlePointerDown = (event: MouseEvent) => {
      const target = event.target;
      if (
        target instanceof Node &&
        containerRef.current &&
        !containerRef.current.contains(target)
      ) {
        setOpen(false);
      }
    };
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        setOpen(false);
      }
    };
    window.addEventListener("mousedown", handlePointerDown);
    window.addEventListener("keydown", handleKeyDown);
    return () => {
      window.removeEventListener("mousedown", handlePointerDown);
      window.removeEventListener("keydown", handleKeyDown);
    };
  }, [open]);

  if (nodes.length === 0) {
    return null;
  }

  const commentsByBlock = new Map<string, number>();
  for (const c of comments) {
    if (c.status !== "archived" && !c.anchor_detached) {
      const target = c.section_id ?? c.block_id;
      commentsByBlock.set(target, (commentsByBlock.get(target) ?? 0) + 1);
    }
  }

  const popoverPosClass =
    placement === "top"
      ? "bottom-full left-1/2 mb-2 -translate-x-1/2"
      : "top-full right-0 mt-1.5";

  return (
    <div ref={containerRef} className="relative">
      <Button
        type="button"
        size="icon-sm"
        variant={open ? "secondary" : "ghost"}
        aria-label="报告目录与协作雷达"
        aria-expanded={open}
        onClick={() => setOpen((prev) => !prev)}
      >
        <ListTreeIcon className="size-4" />
      </Button>
      {open && (
        <div
          className={`bg-popover text-popover-foreground absolute z-50 w-72 rounded-xl border p-3 shadow-lg ${popoverPosClass}`}
        >
          <div className="mb-2 flex items-center justify-between text-xs font-semibold">
            <span>目录大纲与写作状态</span>
            {activeActorsBySection.size > 0 && (
              <span className="inline-flex items-center gap-1 text-[10px] font-normal text-emerald-600 dark:text-emerald-400">
                <span className="size-1.5 animate-pulse rounded-full bg-emerald-500" />
                实时协作中
              </span>
            )}
          </div>
          <div className="max-h-80 overflow-y-auto overscroll-contain">
            <TocBranch
              nodes={nodes}
              depth={0}
              activeActorsBySection={activeActorsBySection}
              commentsByBlock={commentsByBlock}
              onJump={(blockId) => {
                onJumpToSection(blockId);
                setOpen(false);
              }}
            />
          </div>
          <TocInlineControls
            docId={docId}
            config={resolvedToc}
            isPaged={isPaged}
          />
        </div>
      )}
    </div>
  );
}
