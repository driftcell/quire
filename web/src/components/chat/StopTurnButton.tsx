import { SquareIcon } from "lucide-react";
import { cn } from "cn";

export function StopTurnButton({
  onInterrupt,
  className,
}: {
  onInterrupt: () => void | Promise<void>;
  className?: string;
}) {
  return (
    <button
      type="button"
      data-slot="stop-turn"
      onClick={() => {
        void onInterrupt();
      }}
      className={cn(
        "inline-flex h-6 shrink-0 items-center gap-1 rounded-md px-1.5 text-xs font-medium text-destructive/85 transition-colors hover:bg-destructive/10 hover:text-destructive focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none",
        className,
      )}
    >
      <SquareIcon aria-hidden className="size-2.5 fill-current" />
      <span>停止</span>
    </button>
  );
}
