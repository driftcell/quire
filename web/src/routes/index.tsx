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

interface EditorialBriefPreset {
  category: string;
  title: string;
  prompt: string;
  deliverable: string;
  icon: typeof CompassIcon;
}

const EDITORIAL_BRIEFS: readonly EditorialBriefPreset[] = [
  {
    category: "学科竞争力诊断",
    title: "核心学科三年建设成效与 ESI 潜势评估",
    prompt:
      "帮我梳理浙江大学近三年的学科建设成果，结合高水平论文、国家级项目与学科梯队给出结构化诊断报告",
    deliverable: "A4 分页报告 · 对标矩阵",
    icon: CompassIcon,
  },
  {
    category: "省域高校对标",
    title: "2025 年省域高校国自然基金立项全景对比",
    prompt:
      "对比浙江省高校2025年国自然基金立项情况，并用图表展示重点高校面上与青年项目结构差异",
    deliverable: "统计图表 · 梯队对比表",
    icon: BarChart3Icon,
  },
  {
    category: "专项规划论证",
    title: "地方高校博士学位授权点建设与冲刺方案",
    prompt:
      "写一份地方高校博士学位点建设调研报告，梳理核心指标缺口、师资引育路径与三年攻坚时间表",
    deliverable: "专项调研报告 · 指标核查",
    icon: BookOpenCheckIcon,
  },
  {
    category: "人才与经费效度",
    title: "对标院校高层次人才结构与科研产出比测算",
    prompt:
      "对比浙江大学与浙江工业大学的高层次人才数量及近五年国家社科与自科基金产出趋势",
    deliverable: "多维趋势图 · 效度评估",
    icon: LayersIcon,
  },
];

const QUICK_PROBES: readonly string[] = [
  "告诉我2025年浙江大学国自然基金立项总数有多少",
  "我想写一份关于浙江海洋大学冲刺双优建设的报告",
  "分析浙江省内高校学科评估的等级分布",
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
      toast.error(errorMessage(err, "创建会话失败"));
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
              aria-label="切换左侧会话栏"
              onClick={toggleSidebar}
            >
              <PanelLeftIcon className="size-4" />
            </Button>
            <span className="font-serif text-sm font-bold tracking-tight text-foreground">
              Quire
            </span>
          </div>
          <span className="text-[11px] text-muted-foreground">
            研究与报告工作台
          </span>
        </header>
      )}

      <div className="relative flex min-h-0 flex-1 overflow-y-auto overscroll-contain">
        {!isMobile && (
          <SidebarEdgeHandle
            side="left"
            open={leftSidebarOpen}
            label={leftSidebarOpen ? "收起左侧会话栏" : "展开左侧会话栏"}
            onToggle={toggleSidebar}
          />
        )}

        <div className="mx-auto flex w-full max-w-3xl flex-col justify-center px-5 py-10 sm:px-8 sm:py-14">
          {/* Editorial Masthead Header */}
          <header className="mb-7 border-b border-border/90 pb-5">
            <div className="mb-3 flex flex-wrap items-center justify-between gap-2 text-xs text-muted-foreground">
              <div className="inline-flex items-center gap-2">
                <BrandMark className="size-5 shrink-0" />
                <span className="font-serif text-sm font-bold tracking-tight text-foreground">
                  Quire
                </span>
                <span className="text-border" aria-hidden="true">
                  /
                </span>
                <span>多智能体研究与结构化报告出版系统</span>
              </div>
              <span className="font-mono text-[11px] text-muted-foreground/80 tabular-nums">
                A4 出版排版 · 溯源引证 · 划词审校
              </span>
            </div>

            <h1 className="font-serif text-2xl leading-snug font-bold tracking-tight text-balance text-foreground sm:text-[30px]">
              从原始数据与多维对标，直接成稿可付印的研究报告
            </h1>
            <p className="mt-2 max-w-[68ch] text-sm leading-relaxed text-muted-foreground">
              输入研究课题、上传数据附件或指定对标维度。Quire
              将自动调度检索、量化制图与写作审校代理，在画布中实时生成带精确引证的出版级文档。
            </p>
          </header>

          {/* Primary Composer Desk */}
          <section aria-label="研究指令输入区" className="mb-7">
            <PromptComposer
              placeholder="描述你的研究课题、对标机构或分析维度（Shift + Enter 换行，支持拖拽或粘贴数据截图）…"
              onSend={startConversation}
            />

            {/* Quick single-line factual probes */}
            <div className="mt-3 flex flex-wrap items-center gap-1.5">
              <span className="mr-1 text-[11px] font-medium text-muted-foreground">
                快速提问：
              </span>
              {QUICK_PROBES.map((probe) => (
                <button
                  key={probe}
                  type="button"
                  onClick={() => {
                    void startConversation(probe, []);
                  }}
                  className="inline-flex items-center rounded-md border border-border/80 bg-secondary/70 px-2.5 py-1 text-left text-xs text-foreground/85 transition-colors hover:border-foreground/30 hover:bg-card hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none"
                >
                  {probe}
                </button>
              ))}
            </div>
          </section>

          {/* Structured Editorial Brief Blueprints */}
          <section aria-label="典型研究课题范式">
            <div className="mb-2.5 flex items-center justify-between">
              <h2 className="text-xs font-semibold text-foreground/90">
                精选研究简报范式
              </h2>
              <span className="text-[11px] text-muted-foreground">
                点击直接启动完整多章节报告编撰
              </span>
            </div>

            <div className="grid grid-cols-1 gap-2.5 sm:grid-cols-2">
              {EDITORIAL_BRIEFS.map((brief) => {
                const Icon = brief.icon;
                return (
                  <button
                    key={brief.title}
                    type="button"
                    aria-label={`${brief.category}：${brief.title}`}
                    onClick={() => {
                      void startConversation(brief.prompt, []);
                    }}
                    className="group flex flex-col justify-between rounded-xl border border-border/90 bg-card p-3.5 text-left transition-colors hover:border-foreground/35 hover:bg-secondary/40 focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none"
                  >
                    <div>
                      <div className="mb-1.5 flex items-center justify-between gap-2">
                        <span className="inline-flex items-center gap-1.5 text-[11px] font-medium text-[#1D4ED8]">
                          <Icon
                            className="size-3.5 shrink-0"
                            aria-hidden="true"
                          />
                          {brief.category}
                        </span>
                        <ArrowUpRightIcon
                          className="size-3.5 shrink-0 text-muted-foreground/50 transition-transform duration-150 group-hover:translate-x-0.5 group-hover:-translate-y-0.5 group-hover:text-foreground"
                          aria-hidden="true"
                        />
                      </div>
                      <div className="font-serif text-[14px] leading-snug font-semibold text-foreground">
                        {brief.title}
                      </div>
                      <p className="mt-1 line-clamp-2 text-xs leading-relaxed text-muted-foreground">
                        {brief.prompt}
                      </p>
                    </div>

                    <div className="mt-3 flex items-center justify-between border-t border-border/60 pt-2 text-[11px] text-muted-foreground">
                      <span>交付形式</span>
                      <span className="font-mono text-[10.5px] text-foreground/80">
                        {brief.deliverable}
                      </span>
                    </div>
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
