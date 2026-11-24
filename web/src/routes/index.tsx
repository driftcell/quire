import { useQueryClient } from "@tanstack/react-query";
import { createFileRoute, useNavigate } from "@tanstack/react-router";
import {
  ArrowUpRightIcon,
  BarChart3Icon,
  BookOpenCheckIcon,
  CompassIcon,
  LayersIcon,
  PanelLeftIcon,
} from "lucide-react";
import { toast } from "sonner";
import { createSession, postMessage, uploadAttachment } from "@/api/runtime";
import type { SessionCommentRef } from "@/contracts/session-protocol";
import { PromptComposer } from "@/components/chat/PromptComposer";
import { BrandMark } from "@/components/layout/BrandMark";
import { SidebarEdgeHandle } from "@/components/layout/SidebarEdgeHandle";
import { Button } from "@/components/ui/button";
import { useSidebar } from "@/components/ui/sidebar";
import { useIsMobile } from "@/hooks/use-mobile";
import { errorMessage } from "@/lib/errors";
import { randomUuid } from "@/lib/uuid";

export const Route = createFileRoute("/")({
  component: HomePage,
});

interface TopicStarter {
  category: string;
  title: string;
  prompt: string;
  icon: typeof CompassIcon;
}

const TOPIC_STARTERS: readonly TopicStarter[] = [
  {
    category: "学科建设",
    title: "近三年核心学科建设成效梳理",
    prompt:
      "帮我梳理浙江大学近三年的学科建设成果，结合高水平论文、国家级项目与学科平台给出分析报告",
    icon: CompassIcon,
  },
  {
    category: "科研项目",
    title: "省内高校 2025 年国自然立项对比",
    prompt:
      "对比浙江省高校 2025 年国自然基金立项情况，并用图表展示重点高校面上与青年项目结构差异",
    icon: BarChart3Icon,
  },
  {
    category: "学位点与双优",
    title: "浙江海洋大学冲刺双优建设对标分析",
    prompt:
      "写一份关于浙江海洋大学冲刺双优建设的对标分析报告，梳理核心指标表现与下一步建设重点",
    icon: BookOpenCheckIcon,
  },
  {
    category: "师资与人才",
    title: "对标高校高层次人才与基金产出对比",
    prompt:
      "对比浙江大学与浙江工业大学的高层次人才数量及近五年国家社科与自科基金立项趋势",
    icon: LayersIcon,
  },
];

const QUICK_QUERIES: readonly string[] = [
  "2025 年浙江大学国自然基金立项总数有多少",
  "分析浙江省内高校学科评估的档位分布",
  "用图表展示浙江高校近五年国家社科基金立项趋势",
  "梳理地方高校博士学位授权点申报核心指标",
];

function deriveSessionTitle(text: string): string {
  const cleaned = text.trim().replace(/\s+/g, " ");
  if (!cleaned) return "新对话";
  return cleaned.length > 28 ? `${cleaned.slice(0, 28)}…` : cleaned;
}

function HomePage() {
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const isMobile = useIsMobile();
  const { open: leftSidebarOpen, toggleSidebar } = useSidebar();

  const startConversation = async (
    text: string,
    files: File[],
    commentRefs: SessionCommentRef[] = [],
  ) => {
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
        content: text,
        comment_refs: commentRefs,
        attachment_ids: attachmentIds,
      });
      void queryClient.invalidateQueries({ queryKey: ["sessions"] });
      void navigate({
        to: "/sessions/$sessionId",
        params: { sessionId: session.session_id },
      });
    } catch (err) {
      toast.error(errorMessage(err, "创建对话失败"));
    }
  };

  return (
    <div className="flex h-full flex-col bg-background">
      {isMobile && (
        <header className="flex h-12 shrink-0 items-center justify-between border-b border-border/80 bg-background/95 px-3">
          <div className="flex items-center gap-2">
            <Button
              type="button"
              size="icon-sm"
              variant="ghost"
              aria-label="切换左侧对话栏"
              onClick={toggleSidebar}
            >
              <PanelLeftIcon className="size-4" />
            </Button>
            <span className="font-serif text-sm font-bold tracking-tight text-foreground">
              Quire
            </span>
          </div>
        </header>
      )}

      <div className="relative flex min-h-0 flex-1 overflow-y-auto overscroll-contain">
        {!isMobile && (
          <SidebarEdgeHandle
            side="left"
            open={leftSidebarOpen}
            label={leftSidebarOpen ? "收起左侧对话栏" : "展开左侧对话栏"}
            onToggle={toggleSidebar}
          />
        )}

        <div className="mx-auto my-auto flex w-full max-w-2xl flex-col px-5 py-8 sm:px-8 sm:py-12">
          {/* Minimalist Centered Greeting */}
          <header className="mb-6 flex flex-col items-center text-center">
            <div className="mb-3 inline-flex items-center gap-2">
              <BrandMark className="size-6 shrink-0" />
              <span className="font-serif text-base font-bold tracking-tight text-foreground">
                Quire
              </span>
            </div>
            <h1 className="font-serif text-2xl leading-snug font-semibold tracking-tight text-balance text-foreground sm:text-[26px]">
              有什么高教数据或报告需求？
            </h1>
          </header>

          {/* Primary Composer */}
          <section aria-label="输入研究或查询需求" className="mb-6">
            <PromptComposer
              placeholder="输入高校名称、学科领域或报告主题（支持上传表格与附件，Shift + Enter 换行）…"
              onSend={startConversation}
            />

            {/* Quick Queries */}
            <div className="mt-3 flex flex-wrap items-center justify-center gap-1.5">
              {QUICK_QUERIES.map((query) => (
                <button
                  key={query}
                  type="button"
                  onClick={() => {
                    void startConversation(query, []);
                  }}
                  className="inline-flex items-center rounded-full border border-border/80 bg-card/80 px-3 py-1 text-left text-xs text-muted-foreground transition-colors hover:border-foreground/25 hover:bg-card hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none"
                >
                  {query}
                </button>
              ))}
            </div>
          </section>

          {/* Clean 2x2 Topic Starters without Feature Bragging */}
          <section aria-label="常用报告课题">
            <div className="grid grid-cols-1 gap-2.5 sm:grid-cols-2">
              {TOPIC_STARTERS.map((starter) => {
                const Icon = starter.icon;
                return (
                  <button
                    key={starter.title}
                    type="button"
                    aria-label={`${starter.category}：${starter.title}`}
                    onClick={() => {
                      void startConversation(starter.prompt, []);
                    }}
                    className="group flex flex-col justify-between rounded-xl border border-border/85 bg-card/90 p-3.5 text-left transition-colors hover:border-foreground/30 hover:bg-card focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none"
                  >
                    <div className="mb-1 flex items-center justify-between gap-2">
                      <span className="inline-flex items-center gap-1.5 text-[11px] font-medium text-muted-foreground">
                        <Icon
                          className="size-3.5 shrink-0 text-[#1D4ED8]"
                          aria-hidden="true"
                        />
                        {starter.category}
                      </span>
                      <ArrowUpRightIcon
                        className="size-3.5 shrink-0 text-muted-foreground/40 transition-transform duration-150 group-hover:translate-x-0.5 group-hover:-translate-y-0.5 group-hover:text-foreground"
                        aria-hidden="true"
                      />
                    </div>
                    <div className="text-[13.5px] leading-snug font-medium text-foreground">
                      {starter.title}
                    </div>
                    <p className="mt-1 line-clamp-2 text-xs leading-relaxed text-muted-foreground">
                      {starter.prompt}
                    </p>
                  </button>
                );
              })}
            </div>
          </section>
        </div>
      </div>
    </div>
  );
}
