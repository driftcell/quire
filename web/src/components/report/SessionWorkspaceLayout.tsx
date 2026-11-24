import type { ReactNode } from "react";
import { PanelLeftIcon } from "lucide-react";
import { SidebarEdgeHandle } from "@/components/layout/SidebarEdgeHandle";
import { Button } from "@/components/ui/button";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";

export type SessionMobileTab = "report" | "chat" | "comments";

export interface SessionWorkspaceLayoutProps {
  isMobile: boolean;
  hasReportData: boolean;
  sessionTitle: string;
  leftSidebarOpen: boolean;
  toggleSidebar: () => void;
  rightSidebarOpen: boolean;
  rightPanelPct: number;
  reportPane: ReactNode;
  chatPane: ReactNode;
  rightPane: ReactNode;
  commentsPane: ReactNode;
  mobileTab: SessionMobileTab;
  onMobileTabChange: (tab: SessionMobileTab) => void;
}

export function SessionWorkspaceLayout({
  isMobile,
  hasReportData,
  sessionTitle,
  leftSidebarOpen,
  toggleSidebar,
  rightSidebarOpen,
  rightPanelPct,
  reportPane,
  chatPane,
  rightPane,
  commentsPane,
  mobileTab,
  onMobileTabChange,
}: SessionWorkspaceLayoutProps) {
  return (
    <div className="flex h-full min-w-0 flex-col overflow-hidden bg-background">
      {(!hasReportData || isMobile) && (
        <header className="flex h-11 shrink-0 items-center justify-between gap-2 border-b border-border/90 bg-card/95 px-3.5">
          <div className="flex min-w-0 items-center gap-2">
            <Button
              type="button"
              size="icon-sm"
              variant="ghost"
              aria-label="切换左侧会话栏"
              onClick={toggleSidebar}
            >
              <PanelLeftIcon className="size-4" />
            </Button>
            <span className="truncate font-serif text-sm font-semibold text-foreground">
              {sessionTitle}
            </span>
          </div>
        </header>
      )}

      <div className="relative min-h-0 min-w-0 flex-1 overflow-hidden">
        {!isMobile && (
          <SidebarEdgeHandle
            side="left"
            open={leftSidebarOpen}
            label={leftSidebarOpen ? "收起左侧会话栏" : "展开左侧会话栏"}
            onToggle={toggleSidebar}
            tooltip
          />
        )}
        {!hasReportData ? (
          chatPane
        ) : isMobile ? (
          <Tabs
            value={mobileTab}
            onValueChange={(value) => {
              if (
                value === "report" ||
                value === "chat" ||
                value === "comments"
              ) {
                onMobileTabChange(value);
              }
            }}
            className="flex h-full min-h-0 flex-col"
          >
            <div className="shrink-0 border-b border-border/80 bg-card px-3 py-1.5">
              <TabsList className="grid w-full grid-cols-3">
                <TabsTrigger value="report">报告</TabsTrigger>
                <TabsTrigger value="chat">对话</TabsTrigger>
                <TabsTrigger value="comments">批注</TabsTrigger>
              </TabsList>
            </div>
            <TabsContent value="report" className="min-h-0 flex-1">
              {reportPane}
            </TabsContent>
            <TabsContent value="chat" className="min-h-0 flex-1">
              {chatPane}
            </TabsContent>
            <TabsContent value="comments" className="min-h-0 flex-1">
              {commentsPane}
            </TabsContent>
          </Tabs>
        ) : (
          <div className="flex h-full min-h-0 min-w-0 divide-x divide-border/90">
            <div className="min-h-0 min-w-0 flex-1">{reportPane}</div>
            <div
              data-slot="session-right-panel"
              data-state={rightSidebarOpen ? "expanded" : "collapsed"}
              aria-hidden={!rightSidebarOpen}
              className={`h-full min-h-0 min-w-0 shrink-0 overflow-hidden ${
                rightSidebarOpen
                  ? ""
                  : "pointer-events-none invisible border-l-0"
              }`}
              style={{ width: rightSidebarOpen ? `${rightPanelPct}%` : "0%" }}
            >
              {rightPane}
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
