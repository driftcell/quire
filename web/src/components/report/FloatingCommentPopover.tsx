import { memo, useEffect, useMemo, useRef, useState } from "react";
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
import { XIcon } from "lucide-react";
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
  const inputRef = useRef<HTMLTextAreaElement>(null);

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
  useEffect(() => {
    if (anchor) {
      inputRef.current?.focus();
    }
  }, [anchor]);
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
        aria-label="添加批注"
        {...getFloatingProps({
          className:
            "bg-popover text-popover-foreground z-50 flex w-[300px] flex-col rounded-lg border p-0 shadow-lg print:hidden",
        })}
      >
        <div className="flex items-center justify-between border-b px-3.5 py-2.5">
          <span className="text-xs font-medium">添加批注</span>
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

        <div className="space-y-3 p-3.5 text-xs">
          {anchor.quote && (
            <p className="line-clamp-2 border-l-2 border-border pl-2 text-[11px] text-muted-foreground">
              {anchor.quote.exact}
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
              if (event.key === "Escape") {
                event.preventDefault();
                event.currentTarget.blur();
                onClose();
                return;
              }
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
      </div>
    </FloatingPortal>
  );
});
