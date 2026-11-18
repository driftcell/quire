import { useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { createFileRoute, useNavigate } from "@tanstack/react-router";
import {
  BarChart3Icon,
  GitCompareArrowsIcon,
  PanelLeftIcon,
  ShieldAlertIcon,
  SparklesIcon,
} from "lucide-react";
import { createSession, postMessage, uploadAttachment } from "@/api/runtime";
import { PromptComposer } from "@/components/chat/PromptComposer";
import { Button } from "@/components/ui/button";
import { useSidebar } from "@/components/ui/sidebar";

export const Route = createFileRoute("/")({
  component: HomePage,
});

const TEMPLATE_CARDS = [
  {
    title: "经营分析报告",
    subtitle: "多级收入拆解表 + ECharts 趋势图",
    prompt:
      "生成一份 2026 年 Q3 SaaS 业务经营分析报告，含多级收入表与 ECharts 6.0 图表",
    icon: BarChart3Icon,
  },
  {
    title: "供应链风险评估",
    subtitle: "桑基流向图 + 风险雷达与应对矩阵",
    prompt: "撰写一份新能源汽车供应链风险评估报告，包含桑基图与雷达图对比",
    icon: ShieldAlertIcon,
  },
  {
    title: "高校学科对标研判",
    subtitle: "核心指标对比表 + 差距分析结论",
    prompt: "撰写一份重点学科建设对标分析报告，包含核心指标对比表与可视化图表",
    icon: GitCompareArrowsIcon,
  },
] as const;

function deriveSessionTitle(text: string): string {
  const cleaned = text.trim().replace(/\s+/g, " ");
  if (!cleaned) return "新报告会话";
  return cleaned.length > 28 ? `${cleaned.slice(0, 28)}…` : cleaned;
}

function HomePage() {
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const { toggleSidebar } = useSidebar();
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
        client_msg_id: `msg_${crypto.randomUUID()}`,
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
      <header className="flex h-12 shrink-0 items-center gap-2 border-b px-3">
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
          新会话
        </span>
      </header>

      <div className="flex flex-1 flex-col items-center justify-center overflow-y-auto overscroll-contain px-4 pb-12">
        <div className="w-full max-w-2xl space-y-6">
          <div className="space-y-2 text-center">
            <div className="mx-auto flex size-11 items-center justify-center rounded-2xl bg-primary/10 text-primary ring-1 ring-primary/15">
              <SparklesIcon className="size-5" />
            </div>
            <h1 className="text-2xl font-semibold tracking-tight">
              今天想一起写一份什么报告？
            </h1>
            <p className="text-sm text-muted-foreground">
              发送需求或拖入资料立即开始对话，报告生成时自动展开三栏画布。
            </p>
          </div>

          {error && (
            <div className="rounded-lg border border-destructive/40 bg-destructive/10 px-3 py-2 text-xs text-destructive">
              {error}
            </div>
          )}

          <PromptComposer onSend={startConversation} />

          <div className="grid gap-2.5 sm:grid-cols-3">
            {TEMPLATE_CARDS.map((card) => {
              const Icon = card.icon;
              return (
                <button
                  key={card.title}
                  type="button"
                  onClick={() => {
                    void startConversation(card.prompt, []);
                  }}
                  className="group flex flex-col items-start gap-1.5 rounded-xl border bg-card p-3 text-left transition-colors hover:border-primary/30 hover:bg-accent/60 focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none"
                >
                  <div className="flex items-center gap-1.5 text-xs font-medium text-foreground">
                    <Icon className="size-3.5 text-primary" />
                    <span>{card.title}</span>
                  </div>
                  <p className="m-0 line-clamp-2 text-[11px] leading-relaxed text-muted-foreground">
                    {card.subtitle}
                  </p>
                </button>
              );
            })}
          </div>
        </div>
      </div>
    </div>
  );
}
