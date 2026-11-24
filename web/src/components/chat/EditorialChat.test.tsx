import { describe, expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import { PromptComposer } from "@/components/chat/PromptComposer";
import { QuestionCard } from "@/components/chat/QuestionCard";
import type { PendingQuestion } from "@/contracts/session-protocol";

const pending: PendingQuestion = {
  question_id: "question-1",
  run_id: "run-1",
  tool_call_id: "tool-1",
  created_at: "2026-11-24T08:00:00Z",
  questions: [
    {
      id: "scope",
      title: "研究范围与参数",
      description: "选择本次报告采用的对标范围。",
      options: [
        { id: "c9", label: "C9 高校（推荐）" },
        { id: "province", label: "省内同类高校" },
      ],
    },
  ],
};

describe("Borderless editorial chat", () => {
  test("pending decisions use a highlighted document-flow sheet", () => {
    const html = renderToStaticMarkup(
      <QuestionCard
        pending={pending}
        isSubmitting={false}
        onSubmit={() => {}}
      />,
    );

    expect(html).toContain('data-slot="question-card"');
    expect(html).toContain('data-state="pending"');
    expect(html).toContain("border-[#1D4ED8]/45");
    expect(html).toContain("需要你的确认");
    expect(html).toContain("研究范围与参数");
    expect(html).not.toContain("sticky");
  });

  test("resolved decisions collapse to a quiet disclosure with answers retained", () => {
    const html = renderToStaticMarkup(
      <QuestionCard
        pending={pending}
        resolvedState={{
          status: "answered",
          answers: [{ question_id: "scope", selected_option_ids: ["c9"] }],
        }}
        isSubmitting={false}
        onSubmit={() => {}}
      />,
    );

    expect(html).toContain("<details");
    expect(html).toContain('data-state="answered"');
    expect(html).toContain("已确认：研究范围与参数");
    expect(html).toContain("C9 高校（推荐）");
    expect(html).not.toContain("border-[#1D4ED8]/45");
  });

  test("running composer moves queue guidance into the placeholder and keeps live billing", () => {
    const html = renderToStaticMarkup(
      <PromptComposer
        runStatus="running"
        contextUsage={{
          input_tokens: 1200,
          output_tokens: 300,
          cached_tokens: 800,
          context_used: 19,
          context_limit: 100,
          cost_rmb: 0.1128,
        }}
        onSend={async () => {}}
      />,
    );

    expect(html).toContain(
      'placeholder="任务进行中，输入后按 Enter 排队（Shift + Enter 换行）…"',
    );
    expect(html).toContain('aria-label="输入排队消息"');
    expect(html).toContain('data-slot="session-cost-odometer"');
    expect(html).toContain("19%");
    expect(html).toContain("¥0.1128");
    expect(html).not.toContain("正在处理当前任务，新发送的内容将自动排队");
  });
});
