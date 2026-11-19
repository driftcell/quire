import { memo, useEffect, useMemo, useState } from "react";
import {
  FloatingPortal,
  autoUpdate,
  flip,
  offset,
  shift,
  useDismiss,
  useFloating,
  useInteractions,
  useRole,
  type VirtualElement,
} from "@floating-ui/react";
import { MessageSquarePlusIcon, XIcon } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";
import type { TextQuoteSelector } from "@/contracts/report-protocol";

export interface FloatingCommentAnchor {
  blockId: string;
  quote?: TextQuoteSelector;
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
  onAddComment: (
    blockId: string,
    body: string,
    quote?: TextQuoteSelector,
  ) => Promise<void>;
}

export const FloatingCommentPopover = memo(function FloatingCommentPopover({
  anchor,
  existingCount = 0,
  onClose,
  onAddComment,
}: FloatingCommentPopoverProps) {
  const [newBody, setNewBody] = useState("");
  const [busy, setBusy] = useState(false);

  const virtualReference = useMemo<VirtualElement | null>(() => {
    if (!anchor) {
      return null;
    }
    return {
      getBoundingClientRect: () => ({
        x: anchor.rect.left,
        y: anchor.rect.top,
        width: anchor.rect.width,
        height: anchor.rect.height,
        top: anchor.rect.top,
        right: anchor.rect.right,
        bottom: anchor.rect.bottom,
        left: anchor.rect.left,
      }),
    };
  }, [anchor]);

  const {
    refs: floatingRefs,
    floatingStyles,
    context,
  } = useFloating({
    open: anchor !== null,
    onOpenChange: (open) => {
      if (!open) {
        onClose();
      }
    },
    placement: "right-start",
    whileElementsMounted: autoUpdate,
    middleware: [
      offset(10),
      flip({ padding: 12 }),
      shift({ padding: 12, crossAxis: true }),
    ],
  });
  useEffect(() => {
    floatingRefs.setPositionReference(virtualReference);
  }, [floatingRefs, virtualReference]);
  const dismiss = useDismiss(context, {
    escapeKey: true,
    outsidePress: true,
  });
  const role = useRole(context, { role: "dialog" });
  const { getFloatingProps } = useInteractions([dismiss, role]);

  if (!anchor) {
    return null;
  }

  const submit = () => {
    const trimmed = newBody.trim();
    if (!trimmed || busy) return;
    void (async () => {
      setBusy(true);
      try {
        await onAddComment(anchor.blockId, trimmed, anchor.quote);
        setNewBody("");
        onClose();
      } finally {
        setBusy(false);
      }
    })();
  };

  return (
    <FloatingPortal>
      <div
        ref={(node) => {
          floatingRefs.setFloating(node);
        }}
        style={floatingStyles}
        aria-label="新建块级批注"
        {...getFloatingProps({
          className:
            "bg-popover text-popover-foreground z-50 flex w-[340px] flex-col rounded-xl border p-0 shadow-xl ring-1 ring-black/5 dark:ring-white/10 print:hidden",
        })}
      >
        <div className="flex items-center justify-between border-b px-3 py-2">
          <div className="flex min-w-0 items-center gap-1.5">
            <span className="text-xs font-semibold">新建批注</span>
            <code className="truncate rounded bg-muted px-1.5 py-0.5 font-mono text-[10px] text-muted-foreground">
              #{anchor.blockId}
            </code>
          </div>
          <Button
            type="button"
            size="icon-xs"
            variant="ghost"
            aria-label="关闭批注浮窗"
            onClick={onClose}
          >
            <XIcon className="size-3.5" />
          </Button>
        </div>

        <div className="space-y-2 p-3 text-xs">
          {anchor.quote && (
            <div className="rounded border-l-2 border-amber-500 bg-amber-500/10 px-2 py-1 text-[11px] text-amber-800 dark:text-amber-200">
              选中：“{anchor.quote.exact}”
            </div>
          )}
          {existingCount > 0 && (
            <div className="text-[11px] text-muted-foreground">
              此块已有 {existingCount} 条批注，可在右侧批注面板查看。
            </div>
          )}
          <Textarea
            rows={3}
            value={newBody}
            placeholder="输入批注或修改建议... (⌘/Ctrl + Enter 发送)"
            className="text-xs"
            onChange={(event) => setNewBody(event.target.value)}
            onKeyDown={(event) => {
              if ((event.metaKey || event.ctrlKey) && event.key === "Enter") {
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
              <MessageSquarePlusIcon className="mr-1.5 size-3.5" />
              发送批注
            </Button>
          </div>
        </div>
      </div>
    </FloatingPortal>
  );
});
