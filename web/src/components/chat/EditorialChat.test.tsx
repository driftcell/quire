import { describe, expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import {
  HistoricalRunTrace,
  LiveTraceSegments,
} from "@/components/chat/ExecutionTraceSection";
import { HistoricalMessageItem } from "@/components/chat/HistoricalMessageItem";
import { SessionStreamView } from "@/components/chat/SessionStreamView";
import { UserMessageBubble } from "@/components/chat/UserMessageBubble";
import { PromptComposer } from "@/components/chat/PromptComposer";
import { QuestionCard } from "@/components/chat/QuestionCard";
import type { RenderSegment } from "@/components/chat/traceSegments";
import type {
  PendingQuestion,
  RunTraceSummary,
  SessionMessage,
} from "@/contracts/session-protocol";

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
        onInterrupt={async () => {}}
      />,
    );

    expect(html).toContain(
      'placeholder="任务进行中，输入后按 Enter 排队（Shift + Enter 换行）…"',
    );
    expect(html).toContain('aria-label="输入排队消息"');
    expect(html).toContain('data-slot="session-cost-odometer"');
    expect(html).toContain("19%");
    expect(html).toContain("¥0.1128");
    expect(html).toContain('data-slot="stop-turn"');
    expect(html).not.toContain("正在处理当前任务，新发送的内容将自动排队");
  });

  test("main prose appears only after successful completion and keeps only the last segment", () => {
    const segments: RenderSegment[] = [
      {
        key: "text-1",
        kind: "text",
        text: "中途生成的正文",
        isStreaming: false,
      },
      {
        key: "activity-1",
        kind: "activity",
        items: [],
        isActive: false,
      },
      {
        key: "text-2",
        kind: "text",
        text: "任务完成后的最终正文",
        isStreaming: false,
      },
    ];
    const runningHtml = renderToStaticMarkup(
      <LiveTraceSegments
        sessionId="session-1"
        trace={{ agentId: "main", segments: [], isStreaming: true }}
        segments={segments}
      />,
    );
    expect(runningHtml).not.toContain("中途生成的正文");
    expect(runningHtml).not.toContain("任务完成后的最终正文");

    const completedHtml = renderToStaticMarkup(
      <LiveTraceSegments
        sessionId="session-1"
        trace={{ agentId: "main", segments: [], isStreaming: false }}
        segments={segments}
        showFinalText={true}
      />,
    );
    expect(completedHtml).toContain('data-slot="final-main-output"');
    // The live final response starts with an empty reveal buffer on the client.
    expect(completedHtml).not.toContain("中途生成的正文");
  });

  test("failed and interrupted runs never expose partial main prose", () => {
    const failedTrace: RunTraceSummary = {
      run_id: "run-failed",
      status: "failed",
      segments: [{ kind: "text", text: "失败前的半截正文" }],
    };
    const failedHtml = renderToStaticMarkup(
      <HistoricalRunTrace sessionId="session-1" trace={failedTrace} />,
    );
    expect(failedHtml).not.toContain("失败前的半截正文");

    const interruptedMessage: SessionMessage = {
      message_id: "message-interrupted",
      session_id: "session-1",
      role: "assistant",
      content: "中断前的半截正文",
      run_id: "run-interrupted",
      created_at: "2026-11-24T08:00:00Z",
    };
    const interruptedHtml = renderToStaticMarkup(
      <HistoricalMessageItem
        sessionId="session-1"
        msg={interruptedMessage}
        attachmentMap={new Map()}
        trace={{
          run_id: "run-interrupted",
          status: "interrupted",
          segments: [],
        }}
      />,
    );
    expect(interruptedHtml).not.toContain("中断前的半截正文");

    const completedHtml = renderToStaticMarkup(
      <HistoricalRunTrace
        sessionId="session-1"
        trace={{
          ...failedTrace,
          status: "completed",
          segments: [{ kind: "text", text: "成功完成后的正文" }],
        }}
      />,
    );
    expect(completedHtml).toContain("成功完成后的正文");
  });
  test("awaiting_input hydration renders a single question card and bordered user bubble", () => {
    const bubbleHtml = renderToStaticMarkup(
      <UserMessageBubble body="帮我梳理学科成果" attachmentMap={new Map()} />,
    );
    expect(bubbleHtml).toContain("border-border/80");

    const awaitingTrace: RunTraceSummary = {
      run_id: "run-awaiting",
      status: "completed",
      segments: [
        {
          kind: "execution",
          tools: [
            {
              tool_call_id: "tc_ask",
              tool_name: "ask_user",
              status: "awaiting_input",
              input_preview: "{}",
            },
          ],
          subagent_run_ids: [],
        },
      ],
    };
    const streamHtml = renderToStaticMarkup(
      <SessionStreamView
        sessionId="session-1"
        messages={[
          {
            message_id: "m_user",
            session_id: "session-1",
            role: "user",
            content: "帮我梳理学科成果",
            created_at: "2026-11-24T08:00:00Z",
          },
          {
            message_id: "m_tool",
            session_id: "session-1",
            role: "tool",
            run_id: "run-awaiting",
            content: "",
            created_at: "2026-11-24T08:00:01Z",
          },
        ]}
        traces={[awaitingTrace]}
        liveMain={{
          runId: "run-awaiting",
          agentId: "main",
          isStreaming: false,
          segments: [
            {
              kind: "execution",
              tools: [
                {
                  toolCallId: "tc_ask",
                  toolName: "ask_user",
                  status: "completed",
                  inputPreview: "{}",
                },
              ],
              subagentRunIds: [],
            },
          ],
        }}
        liveSubagents={[]}
        pendingQuestion={{
          question_id: "q_1",
          run_id: "run-awaiting",
          tool_call_id: "tc_ask",
          created_at: "2026-11-24T08:00:01Z",
          questions: [
            {
              id: "peers",
              title: "选取哪组参照高校？",
              options: [{ id: "top5", label: "清北复交南" }],
            },
          ],
        }}
        isAnsweringQuestion={false}
        onAnswerQuestion={() => {}}
      />,
    );
    expect(streamHtml.match(/aria-label="需要你的确认"/g)?.length).toBe(1);
  });
  test("interrupted run trace without assistant message remains visible in stream", () => {
    const interruptedTrace: RunTraceSummary = {
      run_id: "run-interrupted-no-msg",
      status: "interrupted",
      task_focus: {
        title: "核对省属高校立项数据",
        todos: [{ title: "核对省属高校立项数据", status: "in_progress" }],
      },
      segments: [],
    };
    const html = renderToStaticMarkup(
      <SessionStreamView
        sessionId="session-1"
        messages={[
          {
            message_id: "m_user",
            session_id: "session-1",
            role: "user",
            content: "帮我生成分析报告",
            created_at: "2026-11-24T08:00:00Z",
          },
        ]}
        traces={[interruptedTrace]}
        liveMain={null}
        liveSubagents={[]}
        pendingQuestion={null}
        isAnsweringQuestion={false}
        onAnswerQuestion={() => {}}
      />,
    );
    expect(html).toContain("核对省属高校立项数据");
  });
});
