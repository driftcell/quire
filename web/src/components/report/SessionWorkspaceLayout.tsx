import type { ReactNode } from "react";
import { ChevronLeftIcon, ChevronRightIcon, PanelLeftIcon } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";

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
    <div className="flex h-full min-w-0 flex-col overflow-hidden">
      {(!hasReportData || isMobile) && (
        <header className="flex h-12 shrink-0 items-center justify-between gap-2 border-b bg-background px-3">
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
            <span className="truncate text-sm font-medium">{sessionTitle}</span>
          </div>
        </header>
      )}

      <div
        className={
          hasReportData && !isMobile
            ? "relative min-h-0 min-w-0 flex-1 overflow-hidden p-2"
            : "relative min-h-0 min-w-0 flex-1 overflow-hidden"
        }
      >
        {!isMobile && (
          <Tooltip>
            <TooltipTrigger
              render={
                <button
                  type="button"
                  aria-label={
                    leftSidebarOpen ? "收起左侧会话栏" : "展开左侧会话栏"
                  }
                  onClick={toggleSidebar}
                  className="focus-visible:ring-ring absolute top-1/2 left-0 z-30 flex h-12 w-4 -translate-y-1/2 items-center justify-center rounded-r-md border border-l-0 bg-background/90 text-muted-foreground opacity-60 shadow-xs backdrop-blur transition-opacity hover:w-5 hover:text-foreground hover:opacity-100 focus-visible:opacity-100 focus-visible:ring-2 focus-visible:outline-none"
                />
              }
            >
              {leftSidebarOpen ? (
                <ChevronLeftIcon className="size-3.5" />
              ) : (
                <ChevronRightIcon className="size-3.5" />
              )}
            </TooltipTrigger>
            <TooltipContent>
              {leftSidebarOpen ? "收起左侧会话栏" : "展开左侧会话栏"}
            </TooltipContent>
          </Tooltip>
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
            <div className="shrink-0 border-b bg-background px-3 py-1.5">
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
          <div className="flex h-full min-h-0 min-w-0">
            <div className="min-h-0 min-w-0 flex-1">{reportPane}</div>
            <div
              data-slot="session-right-panel"
              data-state={rightSidebarOpen ? "expanded" : "collapsed"}
              aria-hidden={!rightSidebarOpen}
              className={`h-full min-h-0 min-w-0 shrink-0 overflow-hidden ${
                rightSidebarOpen
                  ? "border-l border-border/60"
                  : "pointer-events-none invisible"
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
