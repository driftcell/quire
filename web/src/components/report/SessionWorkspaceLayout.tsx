import type { ReactNode, RefObject } from "react";
import { ChevronLeftIcon, ChevronRightIcon, PanelLeftIcon } from "lucide-react";
import {
  ResizableHandle,
  ResizablePanel,
  ResizablePanelGroup,
} from "@/components/ui/resizable";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import type { PanelImperativeHandle } from "react-resizable-panels";
import type { ActiveRunStatus } from "@/contracts/session-protocol";

export type SessionMobileTab = "report" | "chat" | "comments";

export interface SessionWorkspaceLayoutProps {
  isMobile: boolean;
  hasReportData: boolean;
  sessionTitle: string;
  runStatus: ActiveRunStatus;
  leftSidebarOpen: boolean;
  toggleSidebar: () => void;
  rightSidebarOpen: boolean;
  rightPanelPct: number;
  rightPanelAnimating: boolean;
  rightPanelRef: RefObject<PanelImperativeHandle | null>;
  onChatPanelResize: (pct: number) => void;
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
  runStatus,
  leftSidebarOpen,
  toggleSidebar,
  rightSidebarOpen,
  rightPanelPct,
  rightPanelAnimating,
  rightPanelRef,
  onChatPanelResize,
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
            {runStatus === "running" && (
              <Badge variant="secondary" className="text-[11px]">
                生成中
              </Badge>
            )}
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
            className="flex h-full flex-col"
          >
            <div className="border-b bg-background px-3 py-1.5">
              <TabsList className="grid w-full grid-cols-3">
                <TabsTrigger value="report">报告</TabsTrigger>
                <TabsTrigger value="chat">对话</TabsTrigger>
                <TabsTrigger value="comments">批注</TabsTrigger>
              </TabsList>
            </div>
            <TabsContent
              value="report"
              className="min-h-0 flex-1 overflow-hidden"
            >
              {reportPane}
            </TabsContent>
            <TabsContent
              value="chat"
              className="min-h-0 flex-1 overflow-hidden"
            >
              {chatPane}
            </TabsContent>
            <TabsContent
              value="comments"
              className="min-h-0 flex-1 overflow-hidden"
            >
              {commentsPane}
            </TabsContent>
          </Tabs>
        ) : (
          <ResizablePanelGroup
            orientation="horizontal"
            className={`h-full min-w-0 ${
              rightPanelAnimating
                ? "[&_[data-panel]]:transition-[flex-grow] [&_[data-panel]]:duration-300 [&_[data-panel]]:ease-out"
                : ""
            }`}
            onLayoutChanged={(layout, meta) => {
              if (!meta.isUserInteraction) {
                return;
              }
              const pct = layout["chat-panel"];
              if (pct !== undefined) {
                onChatPanelResize(pct);
              }
            }}
          >
            <ResizablePanel
              id="report-panel"
              defaultSize={`${rightSidebarOpen ? 100 - rightPanelPct : 100}%`}
              minSize="35%"
              className="min-w-0"
            >
              {reportPane}
            </ResizablePanel>
            <ResizableHandle
              disabled={!rightSidebarOpen}
              className={`bg-transparent after:hidden ${
                rightPanelAnimating
                  ? "transition-[width] duration-300 ease-out"
                  : ""
              } ${rightSidebarOpen ? "w-1.5" : "w-0"}`}
            />
            <ResizablePanel
              id="chat-panel"
              panelRef={rightPanelRef}
              collapsible
              collapsedSize="0%"
              defaultSize={rightSidebarOpen ? `${rightPanelPct}%` : "0%"}
              minSize="22%"
              maxSize="55%"
              className="min-w-0 overflow-hidden"
            >
              <div
                className="h-full min-w-0"
                inert={!rightSidebarOpen}
                aria-hidden={!rightSidebarOpen}
              >
                {rightPane}
              </div>
            </ResizablePanel>
          </ResizablePanelGroup>
        )}
      </div>
    </div>
  );
}
