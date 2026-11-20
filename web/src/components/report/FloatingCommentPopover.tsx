import { memo, useMemo, useRef, useState } from "react";
import { XIcon } from "lucide-react";
import { toast } from "sonner";
import { toPopoverAnchor } from "@/components/report/reportDom";
import { Button } from "@/components/ui/button";
import { Popover, PopoverContent, PopoverTitle } from "@/components/ui/popover";
import { Textarea } from "@/components/ui/textarea";
import type { TextQuoteSelector } from "@/contracts/report-protocol";
import { errorMessage } from "@/lib/errors";

export interface FloatingCommentAnchor {
  blockId: string;
  endBlockId?: string;
  blockIds?: string[];
  quote?: TextQuoteSelector;
  endQuote?: TextQuoteSelector;
  anchorSnapshot?: string;
  /** Viewport coordinates of the trigger element or selection rect. */
  rect: {
    top: number;
    left: number;
    bottom: number;
    right: number;
    width: number;
    height: number;
  };
}

export interface FloatingCommentPopoverProps {
  anchor: FloatingCommentAnchor | null;
  existingCount?: number;
  onClose: () => void;
  onAddComment: (anchor: FloatingCommentAnchor, body: string) => Promise<void>;
}

export const FloatingCommentPopover = memo(function FloatingCommentPopover({
  anchor,
  existingCount = 0,
  onClose,
  onAddComment,
}: FloatingCommentPopoverProps) {
  const [newBody, setNewBody] = useState("");
  const [busy, setBusy] = useState(false);
  const inputRef = useRef<HTMLTextAreaElement>(null);

  const popoverAnchor = useMemo(
    () => (anchor ? toPopoverAnchor(anchor.rect) : null),
    [anchor],
  );

  if (!anchor) {
    return null;
  }

  const submit = () => {
    const trimmed = newBody.trim();
    if (!trimmed || busy) return;
    void (async () => {
      setBusy(true);
      try {
        await onAddComment(anchor, trimmed);
        setNewBody("");
        onClose();
      } catch (error) {
        toast.error(errorMessage(error, "添加批注失败，请稍后重试"));
      } finally {
        setBusy(false);
      }
    })();
  };

  return (
    <Popover
      open={anchor !== null}
      onOpenChange={(open) => {
        if (!open) {
          onClose();
        }
      }}
    >
      <PopoverContent
        anchor={popoverAnchor}
        side="right"
        align="start"
        sideOffset={10}
        initialFocus={inputRef}
        aria-label="添加批注"
        className="w-[300px] gap-0 rounded-lg border bg-popover p-0 text-popover-foreground shadow-lg print:hidden"
      >
        <div className="flex items-center justify-between border-b px-3.5 py-2.5">
          <PopoverTitle className="text-xs font-medium">添加批注</PopoverTitle>
          <Button
            type="button"
            size="icon-xs"
            variant="ghost"
            aria-label="关闭批注浮窗"
            onClick={onClose}
          >
            <XIcon />
          </Button>
        </div>

        <div className="flex flex-col gap-3 p-3.5 text-xs">
          {anchor.quote && (
            <p className="line-clamp-2 border-l-2 border-border pl-2 text-[11px] text-muted-foreground">
              {anchor.anchorSnapshot ?? anchor.quote.exact}
            </p>
          )}
          {existingCount > 0 && (
            <p className="text-[11px] text-muted-foreground">
              此块已有 {existingCount} 条批注
            </p>
          )}
          <Textarea
            ref={inputRef}
            rows={3}
            value={newBody}
            placeholder="添加批注..."
            className="text-xs"
            onChange={(event) => setNewBody(event.target.value)}
            onKeyDown={(event) => {
              if (
                event.key === "Enter" &&
                !event.shiftKey &&
                !event.nativeEvent.isComposing
              ) {
                event.preventDefault();
                submit();
              }
            }}
          />
          <div className="flex justify-end">
            <Button
              type="button"
              size="xs"
              disabled={busy || !newBody.trim()}
              onClick={submit}
            >
              添加
            </Button>
          </div>
        </div>
      </PopoverContent>
    </Popover>
  );
});
