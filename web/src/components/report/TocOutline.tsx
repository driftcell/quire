import { useState } from "react";
import { ListTreeIcon } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import type {
  CommentThread,
  TocConfig,
  TocNode,
} from "@/contracts/report-protocol";

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
                  className="inline-flex shrink-0 items-center gap-1 rounded-full bg-emerald-500/15 px-1.5 py-0.5 text-[10px] text-emerald-700"
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

  if (nodes.length === 0) {
    return null;
  }

  const commentsByBlock = new Map<string, number>();
  for (const c of comments) {
    if (c.status === "open" && !c.anchor_detached) {
      const target = c.section_id ?? c.block_id;
      commentsByBlock.set(target, (commentsByBlock.get(target) ?? 0) + 1);
    }
  }

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger
        render={
          <Button
            type="button"
            size="sm"
            variant={open ? "secondary" : "ghost"}
            aria-label="文档目录大纲"
            className="h-7 gap-1.5 rounded-md px-2 text-xs font-medium text-foreground/85 hover:bg-secondary hover:text-foreground"
          />
        }
      >
        <ListTreeIcon className="size-3.5 text-muted-foreground" />
        <span className="hidden @min-[540px]:inline">大纲</span>
      </PopoverTrigger>
      <PopoverContent
        side={placement === "top" ? "top" : "bottom"}
        align="end"
        className="w-72 p-3"
      >
        <div className="mb-2 flex items-center justify-between text-xs font-semibold">
          <span>章节大纲</span>
          {activeActorsBySection.size > 0 && (
            <span className="inline-flex items-center gap-1 text-[10px] font-normal text-emerald-600">
              <span className="size-1.5 animate-pulse rounded-full bg-emerald-500" />
              实时协作中
            </span>
          )}
        </div>
        <div className="no-scrollbar max-h-80 overflow-y-auto overscroll-contain">
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
      </PopoverContent>
    </Popover>
  );
}
