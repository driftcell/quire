import {
  ChevronLeftIcon,
  ChevronRightIcon,
  type LucideIcon,
} from "lucide-react";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { cn } from "cn";

export interface SidebarEdgeHandleProps {
  side: "left" | "right";
  open: boolean;
  label: string;
  onToggle: () => void;
  tooltip?: boolean;
  className?: string;
}

export function SidebarEdgeHandle({
  side,
  open,
  label,
  onToggle,
  tooltip = false,
  className,
}: SidebarEdgeHandleProps) {
  const Icon: LucideIcon =
    side === "left"
      ? open
        ? ChevronLeftIcon
        : ChevronRightIcon
      : open
        ? ChevronRightIcon
        : ChevronLeftIcon;

  const button = (
    <button
      type="button"
      title={tooltip ? undefined : label}
      aria-label={label}
      aria-expanded={open}
      data-slot="sidebar-edge-handle"
      data-side={side}
      data-state={open ? "open" : "closed"}
      onClick={onToggle}
      className={cn(
        "focus-visible:ring-ring absolute top-1/2 z-30 flex h-12 w-2.5 -translate-y-1/2 items-center justify-center border bg-background/90 text-muted-foreground opacity-60 shadow-xs backdrop-blur transition-opacity hover:w-3.5 hover:text-foreground hover:opacity-100 focus-visible:opacity-100 focus-visible:ring-2 focus-visible:outline-none",
        side === "left"
          ? "left-0 rounded-r-md border-l-0"
          : "right-0 rounded-l-md border-r-0",
        className,
      )}
    >
      <Icon className="size-2.5" />
    </button>
  );

  if (!tooltip) {
    return button;
  }

  return (
    <Tooltip>
      <TooltipTrigger render={button} />
      <TooltipContent side={side === "left" ? "right" : "left"}>
        {label}
      </TooltipContent>
    </Tooltip>
  );
}
