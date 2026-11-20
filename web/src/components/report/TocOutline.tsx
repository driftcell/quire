import { useState } from "react";
import { ListTreeIcon, RotateCcwIcon } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import { Switch } from "@/components/ui/switch";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
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
        <Switch
          size="sm"
          checked={config.enabled}
          onCheckedChange={(checked) => update({ enabled: checked })}
          aria-label="在报告封面下方显示内嵌目录"
        />
      </div>

      <div className="flex items-center justify-between gap-2">
        <span className="text-[11px] text-muted-foreground">层级深度</span>
        <ToggleGroup
          multiple={false}
          value={[String(config.max_depth)]}
          onValueChange={(values) => {
            const selected = values[0];
            if (!selected) return;
            const depth = Number(selected);
            if (depth === 1 || depth === 2 || depth === 3) {
              update({ max_depth: depth });
            }
          }}
          spacing={0}
          className="rounded-md border bg-muted/40 p-0.5"
        >
          {TOC_MAX_DEPTH_OPTIONS.map((depth) => (
            <ToggleGroupItem
              key={depth}
              value={String(depth)}
              size="sm"
              className="h-5 min-w-0 rounded-sm px-1.5 py-0.5 text-[10px] tabular-nums aria-pressed:bg-background aria-pressed:font-medium aria-pressed:text-foreground aria-pressed:shadow-xs"
            >
              {depth}级
            </ToggleGroupItem>
          ))}
        </ToggleGroup>
      </div>

      <div className="flex items-center justify-between gap-2">
        <span className="text-[11px] text-muted-foreground">目录样式</span>
        <ToggleGroup
          multiple={false}
          value={[config.variant]}
          onValueChange={(values) => {
            const selected = values[0];
            if (
              selected === "standard" ||
              selected === "numbered" ||
              selected === "dotted"
            ) {
              update({ variant: selected });
            }
          }}
          spacing={0}
          className="rounded-md border bg-muted/40 p-0.5"
        >
          {TOC_VARIANT_OPTIONS.map((option) => (
            <ToggleGroupItem
              key={option.value}
              value={option.value}
              title={option.description}
              size="sm"
              className="h-5 min-w-0 rounded-sm px-1.5 py-0.5 text-[10px] aria-pressed:bg-background aria-pressed:font-medium aria-pressed:text-foreground aria-pressed:shadow-xs"
            >
              {option.label}
            </ToggleGroupItem>
          ))}
        </ToggleGroup>
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
  const resolvedToc = useTocConfig(docId, toc, isPaged);

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

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger
        render={
          <Button
            type="button"
            size="icon-sm"
            variant={open ? "secondary" : "ghost"}
            aria-label="文档目录大纲"
          />
        }
      >
        <ListTreeIcon className="size-4" />
      </PopoverTrigger>
      <PopoverContent
        side={placement === "top" ? "top" : "bottom"}
        align="end"
        className="w-72 p-3"
      >
        <div className="mb-2 flex items-center justify-between text-xs font-semibold">
          <span>目录大纲与写作状态</span>
          {activeActorsBySection.size > 0 && (
            <span className="inline-flex items-center gap-1 text-[10px] font-normal text-emerald-600">
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
      </PopoverContent>
    </Popover>
  );
}
