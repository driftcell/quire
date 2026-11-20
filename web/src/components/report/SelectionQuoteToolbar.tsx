import { useMemo } from "react";
import { MessageSquarePlusIcon, SparklesIcon } from "lucide-react";
import { toPopoverAnchor } from "@/components/report/reportDom";
import { Button } from "@/components/ui/button";
import { Popover, PopoverContent } from "@/components/ui/popover";
import type { SelectionQuoteAction } from "@/components/report/reportSelection";

export type { SelectionQuoteAction } from "@/components/report/reportSelection";

export interface SelectionQuoteToolbarProps {
  action: SelectionQuoteAction;
  onComment?: (action: SelectionQuoteAction) => void;
  onAskAgent?: (action: SelectionQuoteAction) => void;
  onDismiss: () => void;
}

export function SelectionQuoteToolbar({
  action,
  onComment,
  onAskAgent,
  onDismiss,
}: SelectionQuoteToolbarProps) {
  const anchor = useMemo(
    () => toPopoverAnchor(action.viewportRect),
    [action.viewportRect],
  );

  return (
    <Popover
      open
      onOpenChange={(open) => {
        if (!open) {
          onDismiss();
        }
      }}
    >
      <PopoverContent
        anchor={anchor}
        side="top"
        align="center"
        sideOffset={8}
        initialFocus={false}
        finalFocus={false}
        role="toolbar"
        aria-label="选中文本操作"
        className="inline-flex w-auto flex-row items-center gap-1 rounded-full border bg-background/95 p-1 shadow-md backdrop-blur print:hidden"
      >
        {onComment && (
          <Button
            type="button"
            size="xs"
            variant="ghost"
            onMouseDown={(event) => event.preventDefault()}
            onClick={() => onComment(action)}
            className="rounded-full px-2.5 text-amber-700 hover:bg-amber-500/10 hover:text-amber-800"
          >
            <MessageSquarePlusIcon data-icon="inline-start" />
            <span>批注</span>
          </Button>
        )}
        {onAskAgent && (
          <Button
            type="button"
            size="xs"
            variant="ghost"
            onMouseDown={(event) => event.preventDefault()}
            onClick={() => onAskAgent(action)}
            className="rounded-full px-2.5 text-primary hover:bg-primary/10"
          >
            <SparklesIcon data-icon="inline-start" />
            <span>问问 Agent</span>
          </Button>
        )}
      </PopoverContent>
    </Popover>
  );
}
