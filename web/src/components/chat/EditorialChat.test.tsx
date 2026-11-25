import { describe, expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import {
  HistoricalRunTrace,
  LiveTraceSegments,
} from "@/components/chat/ExecutionTraceSection";
import { HistoricalMessageItem } from "@/components/chat/HistoricalMessageItem";
import { SessionStreamView } from "@/components/chat/SessionStreamView";
import { UserMessageBubble } from "@/components/chat/UserMessageBubble";
import { ChatRichContext } from "@/components/chat/chatRichContext";
import { PromptComposer } from "@/components/chat/PromptComposer";
import { QuestionCard } from "@/components/chat/QuestionCard";
import { RunActivityHeader } from "@/components/chat/RunActivityHeader";
import { SmoothStreamdown } from "@/components/chat/SmoothStreamdown";
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
            message_id: "m_question",
            session_id: "session-1",
            role: "question",
            run_id: "run-awaiting",
            question_id: "q_1",
            content: "选取哪组参照高校？",
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
    expect(streamHtml).not.toContain('data-message-id="orphan-run-awaiting"');
  });

  test("question card and pre-question trace stay anchored before resumed assistant output", () => {
    const resolvedQ = {
      question: {
        question_id: "q_prev",
        run_id: "run_1",
        tool_call_id: "tc_ask",
        created_at: "2026-11-24T08:00:01Z",
        questions: [
          {
            id: "peers",
            title: "选取哪组参照高校？",
            options: [{ id: "top5", label: "清北复交南" }],
          },
        ],
      },
      status: "answered" as const,
      answers: [{ question_id: "peers", selected_option_ids: ["top5"] }],
    };

    const run1Trace: RunTraceSummary = {
      run_id: "run_1",
      status: "completed",
      task_focus: {
        title: "查询双优建设数据",
        todos: [{ title: "核实双优指标", status: "completed" }],
      },
      segments: [
        {
          kind: "execution",
          tools: [
            {
              tool_call_id: "tc_data",
              tool_name: "shuangyou_intel_list",
              status: "completed",
              input_preview: "{}",
            },
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

    const run2Trace: RunTraceSummary = {
      run_id: "run_2",
      status: "completed",
      segments: [{ kind: "text", text: "最终对比分析结论。" }],
    };

    const html = renderToStaticMarkup(
      <SessionStreamView
        sessionId="session-1"
        messages={[
          {
            message_id: "m_user",
            session_id: "session-1",
            role: "user",
            content: "分析双优建设成效",
            created_at: "2026-11-24T08:00:00Z",
          },
          {
            message_id: "m_question",
            session_id: "session-1",
            role: "question",
            run_id: "run_1",
            question_id: "q_prev",
            content: "选取哪组参照高校？",
            created_at: "2026-11-24T08:00:01Z",
          },
          {
            message_id: "m_final",
            session_id: "session-1",
            role: "assistant",
            run_id: "run_2",
            content: "最终对比分析结论。",
            created_at: "2026-11-24T08:01:00Z",
          },
        ]}
        traces={[run1Trace, run2Trace]}
        liveMain={null}
        liveSubagents={[]}
        pendingQuestion={null}
        resolvedQuestions={[resolvedQ]}
        isAnsweringQuestion={false}
        onAnswerQuestion={() => {}}
      />,
    );

    const userIdx = html.indexOf("分析双优建设成效");
    const traceIdx = html.indexOf("查询双优建设数据");
    const cardIdx = html.indexOf("已确认：选取哪组参照高校？");
    const finalIdx = html.indexOf("最终对比分析结论。");
    expect(userIdx).toBeGreaterThan(-1);
    expect(traceIdx).toBeGreaterThan(userIdx);
    expect(cardIdx).toBeGreaterThan(traceIdx);
    expect(finalIdx).toBeGreaterThan(cardIdx);
    expect(html).not.toContain('data-message-id="orphan-run_1"');
    expect(html).not.toContain("发起结构化确认");
  });

  test("question card renders below its run assistant prose when run 1 wrote prose before ask_user", () => {
    const resolvedQ = {
      question: {
        question_id: "q_prose",
        run_id: "run_1",
        tool_call_id: "tc_ask",
        created_at: "2026-11-24T08:00:02Z",
        questions: [
          {
            id: "peers",
            title: "确认对比范围",
            options: [{ id: "c9", label: "C9 高校" }],
          },
        ],
      },
      status: "answered" as const,
      answers: [{ question_id: "peers", selected_option_ids: ["c9"] }],
    };

    const html = renderToStaticMarkup(
      <SessionStreamView
        sessionId="session-1"
        messages={[
          {
            message_id: "m_user",
            session_id: "session-1",
            role: "user",
            content: "帮我先排查后确认",
            created_at: "2026-11-24T08:00:00Z",
          },
          {
            message_id: "m_pre_prose",
            session_id: "session-1",
            role: "assistant",
            run_id: "run_1",
            content: "我先确认一个参数：",
            created_at: "2026-11-24T08:00:01Z",
          },
          {
            message_id: "m_question",
            session_id: "session-1",
            role: "question",
            run_id: "run_1",
            question_id: "q_prose",
            content: "确认对比范围",
            created_at: "2026-11-24T08:00:02Z",
          },
          {
            message_id: "m_final",
            session_id: "session-1",
            role: "assistant",
            run_id: "run_2",
            content: "已按 C9 口径完成分析。",
            created_at: "2026-11-24T08:01:00Z",
          },
        ]}
        traces={[
          { run_id: "run_1", status: "completed", segments: [{ kind: "text", text: "我先确认一个参数：" }] },
          { run_id: "run_2", status: "completed", segments: [{ kind: "text", text: "已按 C9 口径完成分析。" }] },
        ]}
        liveMain={null}
        liveSubagents={[]}
        pendingQuestion={null}
        resolvedQuestions={[resolvedQ]}
        isAnsweringQuestion={false}
        onAnswerQuestion={() => {}}
      />,
    );

    const proseIdx = html.indexOf("我先确认一个参数：");
    const cardIdx = html.indexOf("已确认：确认对比范围");
    const finalIdx = html.indexOf("已按 C9 口径完成分析。");
    expect(proseIdx).toBeGreaterThan(-1);
    expect(cardIdx).toBeGreaterThan(proseIdx);
    expect(finalIdx).toBeGreaterThan(cardIdx);
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

  test("earlier orphan traces stay in chronological order before later completed messages", () => {
    const html = renderToStaticMarkup(
      <SessionStreamView
        sessionId="session-1"
        messages={[
          {
            message_id: "m_user_1",
            session_id: "session-1",
            role: "user",
            content: "第一轮提问",
            created_at: "2026-11-25T08:00:00Z",
          },
          {
            message_id: "m_user_2",
            session_id: "session-1",
            role: "user",
            content: "第二轮提问",
            created_at: "2026-11-25T08:02:00Z",
          },
          {
            message_id: "m_assistant_2",
            session_id: "session-1",
            role: "assistant",
            run_id: "run_2",
            content: "第二轮最终结论",
            created_at: "2026-11-25T08:03:00Z",
          },
        ]}
        traces={[
          {
            run_id: "run_1_failed",
            status: "failed",
            started_at: "2026-11-25T08:00:05Z",
            duration_ms: 21000,
            task_focus: { title: "查询双优建设数据" },
            segments: [],
          },
          {
            run_id: "run_2",
            status: "completed",
            started_at: "2026-11-25T08:02:05Z",
            segments: [{ kind: "text", text: "第二轮最终结论" }],
          },
        ]}
        liveMain={null}
        liveSubagents={[]}
        pendingQuestion={null}
        isAnsweringQuestion={false}
        onAnswerQuestion={() => {}}
      />,
    );

    const u1Idx = html.indexOf("第一轮提问");
    const failedIdx = html.indexOf("查询双优建设数据");
    const u2Idx = html.indexOf("第二轮提问");
    const a2Idx = html.indexOf("第二轮最终结论");
    expect(u1Idx).toBeGreaterThan(-1);
    expect(failedIdx).toBeGreaterThan(u1Idx);
    expect(u2Idx).toBeGreaterThan(failedIdx);
    expect(a2Idx).toBeGreaterThan(u2Idx);
  });

  test("pure ask_user turn after turn_finished renders no RunActivityHeader card", () => {
    const html = renderToStaticMarkup(
      <SessionStreamView
        sessionId="session-1"
        messages={[
          {
            message_id: "m_user",
            session_id: "session-1",
            role: "user",
            content: "帮我生成研报",
            created_at: "2026-11-25T08:00:00Z",
          },
        ]}
        traces={[]}
        liveMain={{
          runId: "run_ask",
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
          question_id: "q_pure",
          run_id: "run_ask",
          tool_call_id: "tc_ask",
          created_at: "2026-11-25T08:00:01Z",
          questions: [
            {
              id: "scope",
              title: "确认研报范围",
              options: [{ id: "all", label: "全部高校" }],
            },
          ],
        }}
        isAnsweringQuestion={false}
        onAnswerQuestion={() => {}}
      />,
    );

    expect(html).toContain("确认研报范围");
    expect(html).not.toContain('data-live-header="true"');
  });

  test("SmoothStreamdown renders full tables without max-height, short citation tags, and omits empty source_id footer entries", () => {
    const sources = new Map([
      [
        "zstu_web",
        {
          id: "zstu_web",
          index: 1,
          title: "浙江理工大学“十四五”学科建设规划",
          publisher: "浙江理工大学官网",
          url: "https://www.zstu.edu.cn/plan",
        },
      ],
      [
        "ihe_rep",
        {
          id: "ihe_rep",
          index: 2,
          title: "省属高校学科竞争力评估报告",
          publisher: "高等教育研究所",
          excerpt: "省属重点高校在纺织科学与工程等学科保持领先。",
        },
      ],
    ]);

    const markdown = [
      "核心结论已由官方渠道验证<citation id=\"zstu_web\"/>并参考研究所数据<citation id=\"ihe_rep\"/>，忽略未登记的占位引用<citation id=\"source_id\"/>与幻觉引用<citation id=\"dangling_unknown\"/>。",
      "",
      "| 高校 | 指标 |",
      "| --- | --- |",
      "| 浙江理工大学 | 18 |",
    ].join("\n");

    const html = renderToStaticMarkup(
      <ChatRichContext.Provider value={{ sources, charts: new Map() }}>
        <SmoothStreamdown text={markdown} isStreaming={false} />
      </ChatRichContext.Provider>,
    );

    // 1. No vertical max-height on streamdown table container
    expect(html).toContain('data-streamdown="table"');
    expect(html).not.toContain("max-height:300px");

    // 2. Inline citations render short publisher/source labels instead of bare numbers
    expect(html).toContain(">浙江理工大学官网</span>");
    expect(html).toContain(">高等教育研究所</span>");

    // 3. Sources footer includes only real resolved sources (2 sources, no "source_id" or dangling entry)
    expect(html).toContain("2 个参考来源");
    expect(html).not.toContain(">source_id<");
    expect(html).not.toContain("dangling_unknown");
  });

  test("RunActivityHeader disables pointer and toggle button when settled run has no expandable stages", () => {
    const html = renderToStaticMarkup(
      <RunActivityHeader
        model={{
          title: "已完成问答回复",
          status: "completed",
          subagents: [],
          milestones: [],
          todos: [],
        }}
        isStreaming={false}
        detailsOpen={false}
        onToggleDetails={() => {}}
      />,
    );

    expect(html).toContain("disabled");
    expect(html).toContain("cursor-default");
    expect(html).not.toContain("aria-expanded");
  });
});
