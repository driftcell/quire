/* eslint-disable no-unused-expressions -- Playwright CLI loads this file as a function expression. */
async (page) => {
  const env = globalThis.process?.env ?? {};
  const web = env.E2E_WEB_URL || "http://localhost:5173";
  const runtime = env.E2E_RUNTIME_URL || "http://127.0.0.1:8081";
  const sessionId = "e2e-pending-inline";
  const runId = "run-pending-inline";
  const now = new Date().toISOString();
  const consoleErrors = [];
  page.on("console", (message) => {
    if (message.type() === "error") consoleErrors.push(message.text());
  });
  page.on("pageerror", (error) => consoleErrors.push(error.message));

  const initialText = "先更新标题，再继续回答。";
  const injectedText = "排队指令：请补充风险提示。";
  const finalText = "已完成插队消息后的最终回复。";
  const tool = (id, status = "completed") => ({
    tool_call_id: id,
    tool_name: "set_session_title",
    status,
    input_preview: "{}",
    output_preview: status === "completed" ? "{}" : undefined,
    duration_ms: status === "completed" ? 40 : undefined,
    truncated: false,
  });
  const detail = (historical) => ({
    session_id: sessionId,
    title: "E2E pending inline timeline",
    status: "active",
    active_run_status: historical ? "idle" : "running",
    created_at: now,
    updated_at: now,
    last_event_seq: 9,
    doc_id: null,
    linked_doc_id: null,
    messages: [
      {
        message_id: "msg-initial",
        session_id: sessionId,
        role: "user",
        content: initialText,
        created_at: now,
      },
      ...(historical
        ? [
            {
              message_id: "msg-injected",
              session_id: sessionId,
              role: "user",
              content: injectedText,
              run_id: runId,
              created_at: now,
            },
            {
              message_id: "msg-assistant",
              session_id: sessionId,
              role: "assistant",
              content: "",
              run_id: runId,
              created_at: now,
            },
          ]
        : []),
    ],
    attachments: [],
    pending_queue: [],
    resolved_questions: [],
  });
  const finalTraces = [
    {
      run_id: runId,
      status: "completed",
      duration_ms: 900,
      segments: [
        { kind: "thinking", text: "首轮思考", duration_ms: 100 },
        {
          kind: "execution",
          tools: [tool("tool-1")],
          subagents: [],
        },
        {
          kind: "user_message",
          message_id: "msg-injected",
          content: injectedText,
          created_at: now,
        },
        { kind: "thinking", text: "第二轮思考", duration_ms: 100 },
        {
          kind: "execution",
          tools: [tool("tool-2")],
          subagents: [],
        },
        { kind: "text", text: finalText },
      ],
    },
  ];
  const event = (seq, payload) => ({
    seq,
    run_id: runId,
    agent_id: "main",
    parent_run_id: null,
    timestamp: now,
    ...payload,
  });
  const liveEvents = [
    event(1, { event: "turn_started" }),
    event(2, { event: "thinking_delta", delta: "首轮思考" }),
    event(3, {
      event: "tool_call_started",
      tool_call_id: "tool-1",
      tool_name: "set_session_title",
      input_preview: "{}",
      truncated: false,
    }),
    event(4, {
      event: "tool_call_finished",
      tool_call_id: "tool-1",
      status: "completed",
      output_preview: "{}",
      duration_ms: 40,
      truncated: false,
    }),
    event(5, {
      event: "user_message_injected",
      message_id: "msg-injected",
      content: injectedText,
      created_at: now,
    }),
    event(6, { event: "thinking_delta", delta: "第二轮思考" }),
    event(7, {
      event: "tool_call_started",
      tool_call_id: "tool-2",
      tool_name: "set_session_title",
      input_preview: "{}",
      truncated: false,
    }),
    event(8, {
      event: "tool_call_finished",
      tool_call_id: "tool-2",
      status: "completed",
      output_preview: "{}",
      duration_ms: 40,
      truncated: false,
    }),
    event(9, { event: "text_delta", delta: finalText }),
  ];
  const sseBody = (events) =>
    events.map((entry) => `data: ${JSON.stringify(entry)}\n\n`).join("");

  let historical = false;
  let eventRequests = 0;
  const handleSessionApi = async (route) => {
    const request = route.request();
    const path = new URL(request.url()).pathname;
    if (path === "/api/sessions" && request.method() === "GET") {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: "[]",
      });
      return;
    }
    if (path === `/api/sessions/${sessionId}/events`) {
      eventRequests += 1;
      const body =
        !historical && eventRequests === 1
          ? sseBody(liveEvents)
          : ": keepalive\n\n";
      await route.fulfill({
        status: 200,
        headers: {
          "Content-Type": "text/event-stream",
          "Cache-Control": "no-cache",
          Connection: "keep-alive",
        },
        body,
      });
      return;
    }
    if (path === `/api/sessions/${sessionId}/traces`) {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify(historical ? finalTraces : []),
      });
      return;
    }
    if (path === `/api/sessions/${sessionId}` && request.method() === "GET") {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify(detail(historical)),
      });
      return;
    }
    await route.continue();
  };
  await page.route("**/api/sessions", handleSessionApi);
  await page.route("**/api/sessions/**", handleSessionApi);
  await page.route(`${runtime}/api/sessions`, handleSessionApi);
  await page.route(`${runtime}/api/sessions/**`, handleSessionApi);

  const assert = (condition, message) => {
    if (!condition) throw new Error(message);
  };
  const positions = async () => {
    await page.locator('[data-inline-user-message-id="msg-injected"]').waitFor({
      state: "visible",
      timeout: 15000,
    });
    await page.getByText(finalText, { exact: true }).waitFor({
      state: "visible",
      timeout: 15000,
    });
    return page.evaluate(
      ({ initial, final }) => {
        const activities = Array.from(
          document.querySelectorAll('[data-slot="activity"]'),
        );
        const textElement = (text) =>
          Array.from(document.querySelectorAll("div")).find(
            (element) => element.textContent === text,
          );
        const entries = [
          ["initial", textElement(initial)],
          ["activity-1", activities[0]],
          [
            "injected",
            document.querySelector(
              '[data-inline-user-message-id="msg-injected"]',
            ),
          ],
          ["activity-2", activities[1]],
          ["final", textElement(final)],
        ];
        return entries.map(([key, element]) => ({
          key,
          top: element?.getBoundingClientRect().top ?? -10000,
          found: Boolean(element),
        }));
      },
      { initial: initialText, final: finalText },
    );
  };
  const assertTimeline = async (label) => {
    const entries = await positions();
    assert(
      entries.every((entry) => entry.found),
      `${label}: missing timeline entries ${JSON.stringify(entries)}`,
    );
    for (let index = 1; index < entries.length; index += 1) {
      assert(
        entries[index].top >= entries[index - 1].top,
        `${label}: wrong DOM order ${JSON.stringify(entries)}`,
      );
    }
    const injectedCount = await page
      .getByText(injectedText, { exact: true })
      .count();
    assert(
      injectedCount === 1,
      `${label}: injected message rendered ${injectedCount} times`,
    );
    const activityTitles = await page
      .locator("[data-activity-trigger]")
      .allInnerTexts();
    assert(
      activityTitles.length === 2 &&
        activityTitles.every((title) =>
          title.includes("已完成思考与 1 次操作"),
        ),
      `${label}: activity groups were not settled around the user message: ${activityTitles.join(" | ")}`,
    );
    return entries;
  };

  await page.goto(`${web}/sessions/${sessionId}`);
  const live = await assertTimeline("live");
  historical = true;
  await page.reload();
  const refreshed = await assertTimeline("refresh");
  assert(
    JSON.stringify(live.map((entry) => entry.key)) ===
      JSON.stringify(refreshed.map((entry) => entry.key)),
    "live and historical segment kinds diverged",
  );
  assert(
    consoleErrors.length === 0,
    `browser console/page errors: ${consoleErrors.join(" | ")}`,
  );

  return {
    pass: true,
    scenario: "pending-user-message-timeline",
    web,
    runtime,
    live,
    refreshed,
  };
};
