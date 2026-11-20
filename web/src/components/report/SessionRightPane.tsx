import type { ReactNode } from "react";
import { Badge } from "@/components/ui/badge";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import type { ActiveRunStatus } from "@/contracts/session-protocol";

export interface SessionRightPaneProps {
  title: string;
  runStatus: ActiveRunStatus;
  openCommentsCount: number;
  activeTab: "chat" | "comments";
  onTabChange: (tab: "chat" | "comments") => void;
  chatPane: ReactNode;
  commentsPane: ReactNode;
}

export function SessionRightPane({
  title,
  runStatus,
  openCommentsCount,
  activeTab,
  onTabChange,
  chatPane,
  commentsPane,
}: SessionRightPaneProps) {
  return (
    <div className="flex h-full min-w-0 flex-col overflow-hidden rounded-xl border bg-background shadow-2xs">
      <div className="flex h-11 shrink-0 items-center justify-between gap-2 border-b px-3">
        <div className="flex min-w-0 items-center gap-2">
          <span className="truncate text-sm font-medium">{title}</span>
          {runStatus === "running" && (
            <Badge variant="secondary" className="shrink-0 text-[11px]">
              生成中
            </Badge>
          )}
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
          <TabsList className="grid grid-cols-2">
            <TabsTrigger value="chat" className="text-xs">
              对话
            </TabsTrigger>
            <TabsTrigger value="comments" className="text-xs">
              批注 ({openCommentsCount})
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
