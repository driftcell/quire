import { useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { createFileRoute, useNavigate } from "@tanstack/react-router";
import { ChevronLeftIcon, ChevronRightIcon, PanelLeftIcon } from "lucide-react";
import { createSession, postMessage, uploadAttachment } from "@/api/runtime";
import { PromptComposer } from "@/components/chat/PromptComposer";
import { BrandSigil } from "@/components/layout/BrandSigil";
import { Button } from "@/components/ui/button";
import { useSidebar } from "@/components/ui/sidebar";
import { useIsMobile } from "@/hooks/use-mobile";
import { randomUuid } from "@/lib/uuid";

export const Route = createFileRoute("/")({
  component: HomePage,
});

const HOME_SUGGESTIONS: readonly string[] = [
  "告诉我2025年浙江大学国自然基金立项总数有多少",
  "我想写一份关于浙江海洋大学冲刺双优建设的报告",
  "对比浙江省高校2025年国自然基金立项情况",
  "帮我梳理浙江大学近三年的学科建设成果",
  "分析浙江省内高校学科评估的等级分布",
  "写一份地方高校博士学位点建设调研报告",
  "对比浙江大学与浙江工业大学的高层次人才数量",
  "用图表展示浙江高校近五年国家社科基金趋势",
];

function deriveSessionTitle(text: string): string {
  const cleaned = text.trim().replace(/\s+/g, " ");
  if (!cleaned) return "新报告会话";
  return cleaned.length > 28 ? `${cleaned.slice(0, 28)}…` : cleaned;
}

function HomePage() {
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const isMobile = useIsMobile();
  const { open: leftSidebarOpen, toggleSidebar } = useSidebar();
  const [error, setError] = useState<string | null>(null);

  const startConversation = async (text: string, files: File[]) => {
    setError(null);
    try {
      const session = await createSession({
        title: deriveSessionTitle(text),
      });
      const attachmentIds: string[] = [];
      for (const file of files) {
        const uploaded = await uploadAttachment(session.session_id, file);
        attachmentIds.push(uploaded.attachment_id);
      }
      await postMessage(session.session_id, {
        client_msg_id: `msg_${randomUuid()}`,
        content: text || `请分析已上传的 ${files.length} 个附件并生成报告。`,
        attachment_ids: attachmentIds,
      });
      void queryClient.invalidateQueries({ queryKey: ["sessions"] });
      void navigate({
        to: "/sessions/$sessionId",
        params: { sessionId: session.session_id },
      });
    } catch (err) {
      setError(err instanceof Error ? err.message : "创建会话失败");
    }
  };

  return (
    <div className="flex h-full flex-col bg-background">
      {isMobile && (
        <header className="flex h-12 shrink-0 items-center gap-2 px-3">
          <Button
            type="button"
            size="icon-sm"
            variant="ghost"
            aria-label="切换左侧会话栏"
            onClick={toggleSidebar}
          >
            <PanelLeftIcon className="size-4" />
          </Button>
          <span className="text-sm font-medium text-muted-foreground">
            青塔 · 智能报告助手
          </span>
        </header>
      )}

      <div className="relative flex min-h-0 flex-1 items-center justify-center">
        {!isMobile && (
          <button
            type="button"
            title={leftSidebarOpen ? "收起左侧会话栏" : "展开左侧会话栏"}
            aria-label={leftSidebarOpen ? "收起左侧会话栏" : "展开左侧会话栏"}
            onClick={toggleSidebar}
            className="focus-visible:ring-ring absolute top-1/2 left-0 z-30 flex h-12 w-4 -translate-y-1/2 items-center justify-center rounded-r-md border border-l-0 bg-background/90 text-muted-foreground opacity-60 shadow-xs backdrop-blur transition-opacity hover:w-5 hover:text-foreground hover:opacity-100 focus-visible:opacity-100 focus-visible:ring-2 focus-visible:outline-none"
          >
            {leftSidebarOpen ? (
              <ChevronLeftIcon className="size-3.5" />
            ) : (
              <ChevronRightIcon className="size-3.5" />
            )}
          </button>
        )}

        <div className="flex w-full flex-col items-center px-4 pb-12 sm:px-6">
          <div className="brand-lockup mb-10">
            <BrandSigil />
            <h1 className="sr-only">青塔 · 智能报告助手</h1>
            <p className="brand-tagline">青塔·智能报告助手</p>
          </div>

          {error && (
            <div className="mb-3 w-full max-w-2xl rounded-lg border border-destructive/40 bg-destructive/10 px-3 py-2 text-xs text-destructive">
              {error}
            </div>
          )}

          <div className="w-full max-w-2xl">
            <div className="suggestion-fade-x no-scrollbar mb-3 overflow-hidden motion-reduce:overflow-x-auto">
              <div className="suggestion-marquee flex w-max hover:[animation-play-state:paused] active:[animation-play-state:paused] has-[:focus-visible]:[animation:none] motion-reduce:animate-none">
                {[0, 1].map((copy) => (
                  <div
                    key={copy}
                    aria-hidden={copy === 1 || undefined}
                    className={`flex shrink-0 gap-2 pr-2 ${
                      copy === 1 ? "motion-reduce:hidden" : ""
                    }`}
                  >
                    {HOME_SUGGESTIONS.map((suggestion) => (
                      <Button
                        key={suggestion}
                        type="button"
                        variant="outline"
                        size="sm"
                        className="shrink-0"
                        tabIndex={copy === 1 ? -1 : undefined}
                        onClick={() => {
                          void startConversation(suggestion, []);
                        }}
                      >
                        {suggestion}
                      </Button>
                    ))}
                  </div>
                ))}
              </div>
            </div>
            <PromptComposer onSend={startConversation} />
          </div>
        </div>
      </div>
    </div>
  );
}
