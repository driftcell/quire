import type { ReactNode } from "react";
import { MessageSquareQuoteIcon, SparklesIcon } from "lucide-react";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";

export interface SessionRightPaneProps {
  title: string;
  openCommentsCount: number;
  activeTab: "chat" | "comments";
  onTabChange: (tab: "chat" | "comments") => void;
  chatPane: ReactNode;
  commentsPane: ReactNode;
}

export function SessionRightPane({
  title,
  openCommentsCount,
  activeTab,
  onTabChange,
  chatPane,
  commentsPane,
}: SessionRightPaneProps) {
  return (
    <div className="flex h-full min-w-0 flex-col overflow-hidden bg-background">
      <div className="flex h-11 shrink-0 items-center justify-between gap-2 border-b border-border/90 bg-card/95 px-3.5">
        <div className="flex min-w-0 items-center gap-2">
          <span className="inline-flex size-2 shrink-0 rounded-full bg-[#1D4ED8]" />
          <span className="text-xs font-semibold tracking-tight text-foreground">
            研究协作与审阅
          </span>
          <span
            className="hidden truncate text-[11px] text-muted-foreground @min-[420px]:inline"
            title={title}
          >
            · {title}
          </span>
        </div>
        <Tabs
          value={activeTab}
          onValueChange={(value) => {
            if (value === "chat" || value === "comments") {
              onTabChange(value);
            }
          }}
          className="shrink-0"
        >
          <TabsList className="grid h-7 grid-cols-2 rounded-lg border border-border/80 bg-secondary/75 p-0.5">
            <TabsTrigger
              value="chat"
              className="gap-1 rounded-md px-2.5 text-xs data-[state=active]:bg-card data-[state=active]:shadow-2xs"
            >
              <SparklesIcon
                className="size-3 text-[#1D4ED8]"
                aria-hidden="true"
              />
              <span>对话</span>
            </TabsTrigger>
            <TabsTrigger
              value="comments"
              className="gap-1 rounded-md px-2.5 text-xs tabular-nums data-[state=active]:bg-card data-[state=active]:shadow-2xs"
            >
              <MessageSquareQuoteIcon
                className="size-3 text-muted-foreground"
                aria-hidden="true"
              />
              <span>批注 ({openCommentsCount})</span>
            </TabsTrigger>
          </TabsList>
        </Tabs>
      </div>
      <div className="min-h-0 flex-1">
        {activeTab === "chat" ? chatPane : commentsPane}
      </div>
    </div>
  );
}
