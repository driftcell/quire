import type { ReactNode } from "react";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";

export interface SessionRightPaneProps {
  openCommentsCount: number;
  activeTab: "chat" | "comments";
  onTabChange: (tab: "chat" | "comments") => void;
  chatPane: ReactNode;
  commentsPane: ReactNode;
}

export function SessionRightPane({
  openCommentsCount,
  activeTab,
  onTabChange,
  chatPane,
  commentsPane,
}: SessionRightPaneProps) {
  return (
    <div className="flex h-full min-w-0 flex-col overflow-hidden bg-background">
      <div className="flex h-11 shrink-0 items-center justify-end border-b border-border/80 px-3.5">
        <Tabs
          value={activeTab}
          onValueChange={(value) => {
            if (value === "chat" || value === "comments") {
              onTabChange(value);
            }
          }}
          className="shrink-0"
        >
          <TabsList className="grid h-7 grid-cols-2 rounded-lg border border-border/70 bg-secondary/60 p-0.5">
            <TabsTrigger
              value="chat"
              className="rounded-md px-3 text-xs data-[state=active]:bg-card data-[state=active]:shadow-2xs"
            >
              对话
            </TabsTrigger>
            <TabsTrigger
              value="comments"
              className="rounded-md px-3 text-xs tabular-nums data-[state=active]:bg-card data-[state=active]:shadow-2xs"
            >
              批注{openCommentsCount > 0 ? ` ${openCommentsCount}` : ""}
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
