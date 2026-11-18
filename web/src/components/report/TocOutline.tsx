import { useEffect, useRef, useState } from "react";
import { ListTreeIcon } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import type { CommentThread, TocNode } from "@/contracts/report-protocol";

export interface TocOutlineProps {
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

export function TocOutline({
  nodes,
  activeActorsBySection,
  comments,
  onJumpToSection,
  placement = "bottom",
}: TocOutlineProps) {
  const [open, setOpen] = useState(false);
  const containerRef = useRef<HTMLDivElement>(null);

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
        </div>
      )}
    </div>
  );
}
