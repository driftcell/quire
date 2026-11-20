import { useEffect, useMemo } from "react";
import {
  FloatingPortal,
  autoUpdate,
  flip,
  offset,
  shift,
  useDismiss,
  useFloating,
  useInteractions,
  type VirtualElement,
} from "@floating-ui/react";
import { MessageSquarePlusIcon, SparklesIcon } from "lucide-react";
import type { TextQuoteSelector } from "@/contracts/report-protocol";

export interface SelectionQuoteAction {
  blockId: string;
  quote: TextQuoteSelector;
  viewportRect: {
    top: number;
    left: number;
    bottom: number;
    right: number;
    width: number;
    height: number;
  };
}

export interface SelectionQuoteToolbarProps {
  action: SelectionQuoteAction;
  onComment?: (action: SelectionQuoteAction) => void;
  onAskAgent?: (blockId: string, exact: string) => void;
  onDismiss: () => void;
}

export function SelectionQuoteToolbar({
  action,
  onComment,
  onAskAgent,
  onDismiss,
}: SelectionQuoteToolbarProps) {
  const virtualReference = useMemo<VirtualElement>(() => {
    return {
      getBoundingClientRect: () => ({
        x: action.viewportRect.left,
        y: action.viewportRect.top,
        width: action.viewportRect.width,
        height: action.viewportRect.height,
        top: action.viewportRect.top,
        right: action.viewportRect.right,
        bottom: action.viewportRect.bottom,
        left: action.viewportRect.left,
      }),
    };
  }, [action.viewportRect]);

  const {
    refs: floatingRefs,
    floatingStyles,
    context,
  } = useFloating({
    open: true,
    onOpenChange: (open) => {
      if (!open) {
        onDismiss();
      }
    },
    placement: "top",
    whileElementsMounted: autoUpdate,
    middleware: [offset(8), flip({ padding: 8 }), shift({ padding: 8 })],
  });
  useEffect(() => {
    floatingRefs.setPositionReference(virtualReference);
  }, [floatingRefs, virtualReference]);

  const dismiss = useDismiss(context, {
    escapeKey: true,
    outsidePress: true,
  });
  const { getFloatingProps } = useInteractions([dismiss]);

  return (
    <FloatingPortal>
      <div
        ref={(node) => {
          floatingRefs.setFloating(node);
        }}
        style={floatingStyles}
        {...getFloatingProps({
          role: "toolbar",
          "aria-label": "选中文本操作",
          className:
            "z-40 inline-flex items-center gap-1 rounded-full border bg-background/95 p-1 shadow-md backdrop-blur print:hidden",
        })}
      >
        {onComment && (
          <button
            type="button"
            onMouseDown={(event) => event.preventDefault()}
            onClick={() => onComment(action)}
            className="inline-flex items-center gap-1 rounded-full px-2.5 py-0.5 text-xs font-medium text-amber-700 hover:bg-amber-500/10"
          >
            <MessageSquarePlusIcon className="size-3.5" />
            <span>批注</span>
          </button>
        )}
        {onAskAgent && (
          <button
            type="button"
            onMouseDown={(event) => event.preventDefault()}
            onClick={() => onAskAgent(action.blockId, action.quote.exact)}
            className="inline-flex items-center gap-1 rounded-full px-2.5 py-0.5 text-xs font-medium text-primary hover:bg-primary/10"
          >
            <SparklesIcon className="size-3.5" />
            <span>问问 Agent</span>
          </button>
        )}
      </div>
    </FloatingPortal>
  );
}
