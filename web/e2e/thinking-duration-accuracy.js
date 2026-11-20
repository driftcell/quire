/* eslint-disable no-unused-expressions -- Playwright CLI loads this file as a function expression. */
async (page) => {
  const env = globalThis.process?.env ?? {};
  const web = env.E2E_WEB_URL || "http://localhost:5173";
  const runtime = env.E2E_RUNTIME_URL || "http://127.0.0.1:8081";

  const now = Date.now();
  const liveId = "e2e-thinking-live";
  const historyId = "e2e-thinking-history";

  const detail = (sessionId, overrides = {}) => ({
    session_id: sessionId,
    title: `E2E thinking duration ${sessionId}`,
    status: "active",
    active_run_status: "running",
    created_at: new Date(now).toISOString(),
    updated_at: new Date(now).toISOString(),
    last_event_seq: 0,
    doc_id: null,
    linked_doc_id: null,
    messages: [],
    attachments: [],
    pending_queue: [],
    resolved_questions: [],
    ...overrides,
  });

  const event = (
    seq,
    runId,
    agentId,
    timestampMs,
    payload,
    parentRunId = null,
  ) => ({
    seq,
    run_id: runId,
    agent_id: agentId,
    parent_run_id: parentRunId,
    timestamp: new Date(timestampMs).toISOString(),
    ...payload,
  });

  const sseBody = (events) =>
    events.map((entry) => `data: ${JSON.stringify(entry)}\n\n`).join("");

  const liveEvents = [
    event(1, "run-live", "main", now - 12_000, { event: "turn_started" }),
    event(2, "run-live", "main", now - 12_000, {
      event: "thinking_delta",
      delta: "正在核对五校科研口径与统计范围。",
    }),
  ];

  const historyTrace = {
    run_id: "run-history",
    status: "completed",
    duration_ms: 60_000,
    segments: [
      {
        kind: "thinking",
        text: "先用 4 秒梳理统计口径，再执行工具。",
        duration_ms: 4_000,
      },
      {
        kind: "execution",
        tools: [
          {
            tool_call_id: "history-tool",
            tool_name: "read_blocks",
            status: "completed",
            input_preview: "{}",
            output_preview: "{}",
            duration_ms: 56_000,
            truncated: false,
          },
        ],
        subagents: [
          {
            subagent_run_id: "sub-1",
            agent_name: "analyst",
            display_name: "量化分析师",
            task: "核对数据",
            status: "completed",
            thinking: "子代理只累计真正思考的 3 秒。",
            thinking_duration_ms: 3_000,
            task_focus: { title: "核对统计口径" },
            tools: [],
            summary: "子代理已完成核对。",
          },
        ],
      },
      {
        kind: "text",
        text: "历史正文：整轮耗时 60 秒，但思考只占 4 秒。",
      },
    ],
  };

  const scenarios = {
    [liveId]: {
      detail: detail(liveId),
      traces: [],
      batches: [liveEvents],
      sseCount: 0,
    },
    [historyId]: {
      detail: detail(historyId, {
        active_run_status: "idle",
        messages: [
          {
            message_id: "history-assistant",
            session_id: historyId,
            role: "assistant",
            content: "",
            run_id: "run-history",
            created_at: new Date(now).toISOString(),
          },
        ],
      }),
      traces: [historyTrace],
      batches: [[]],
      sseCount: 0,
    },
  };

  const handleSessionApi = async (route) => {
    const request = route.request();
    if (request.method() !== "GET") {
      await route.continue();
      return;
    }
    const path = new URL(request.url()).pathname;
    const match = path.match(
      /^\/api\/sessions\/([^/]+)(?:\/(events|traces))?$/,
    );
    if (!match) {
      await route.continue();
      return;
    }
    const sessionId = decodeURIComponent(match[1]);
    const suffix = match[2];
    const scenario = scenarios[sessionId];
    if (!scenario) {
      await route.continue();
      return;
    }

    if (suffix === "events") {
      const index = scenario.sseCount;
      scenario.sseCount += 1;
      const events = scenario.batches[index] ?? [];
      await route.fulfill({
        status: 200,
        headers: {
          "Content-Type": "text/event-stream",
          "Cache-Control": "no-cache",
          Connection: "keep-alive",
        },
        body: events.length > 0 ? sseBody(events) : ": keepalive\n\n",
      });
      return;
    }

    if (suffix === "traces") {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify(scenario.traces),
      });
      return;
    }

    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify(scenario.detail),
    });
  };

  await page.route("**/api/sessions/**", handleSessionApi);
  await page.route(`${runtime}/api/sessions/**`, handleSessionApi);

  const assert = (condition, message) => {
    if (!condition) {
      throw new Error(message);
    }
  };

  const secondsFromText = (text) => {
    const match = text.match(/思考用时\s*(\d+)秒|正在思考\s*(\d+)秒/);
    if (!match) return null;
    return Number(match[1] ?? match[2]);
  };

  await page.goto(`${web}/sessions/${liveId}`);
  await page.locator('[data-live-header="true"]').first().waitFor({
    state: "visible",
    timeout: 15000,
  });
  await page.waitForFunction(
    () => {
      const trace = document.querySelector('[data-slot="thinking-trace"]');
      return Boolean(trace && /\d+秒/.test(trace.textContent ?? ""));
    },
    undefined,
    { timeout: 15000 },
  );

  const liveThinkingText = (
    await page.locator('[data-slot="thinking-trace"]').first().innerText()
  ).replace(/\s+/g, " ");
  const liveSeconds = secondsFromText(liveThinkingText);
  assert(
    liveSeconds !== null && liveSeconds >= 11,
    `live thinking timer was not restored from the server timestamp: ${liveThinkingText}`,
  );

  await page.goto(`${web}/sessions/${historyId}`);
  const activityTrigger = page.locator("[data-activity-trigger]").first();
  await activityTrigger.waitFor({ state: "visible", timeout: 15000 });
  assert(
    (await activityTrigger.getAttribute("aria-expanded")) === "false",
    "historical activity should start collapsed",
  );
  await activityTrigger.click();
  await page.waitForFunction(
    () =>
      document
        .querySelector("[data-activity-trigger]")
        ?.getAttribute("aria-expanded") === "true",
    undefined,
    { timeout: 15000 },
  );

  const mainThinking = page.locator('[data-slot="thinking-trace"]').first();
  await mainThinking.waitFor({ state: "visible", timeout: 15000 });
  const mainThinkingText = (await mainThinking.innerText()).replace(
    /\s+/g,
    " ",
  );
  const mainSeconds = secondsFromText(mainThinkingText);
  assert(
    mainSeconds === 4,
    `main thinking should use segment duration 4s, got: ${mainThinkingText}`,
  );
  assert(
    !mainThinkingText.includes("60秒"),
    `main thinking incorrectly used total run duration: ${mainThinkingText}`,
  );

  await page
    .locator('[data-subagent-id="sub-1"] [data-subagent-trigger]')
    .click();
  await page.waitForFunction(
    () =>
      document
        .querySelector('[data-subagent-id="sub-1"] [data-subagent-trigger]')
        ?.getAttribute("aria-expanded") === "true",
    undefined,
    { timeout: 15000 },
  );

  const subThinking = page.locator(
    '[data-subagent-id="sub-1"] [data-slot="thinking-trace"]',
  );
  await subThinking.waitFor({ state: "visible", timeout: 15000 });
  const subThinkingText = (await subThinking.innerText()).replace(/\s+/g, " ");
  const subSeconds = secondsFromText(subThinkingText);
  assert(
    subSeconds === 3,
    `subagent thinking should use thinking_duration_ms 3s, got: ${subThinkingText}`,
  );

  return {
    pass: true,
    scenario: "thinking-duration-accuracy",
    web,
    runtime,
    results: {
      live: {
        seconds: liveSeconds,
        text: liveThinkingText,
      },
      history: {
        mainSeconds,
        mainText: mainThinkingText,
        subagentSeconds: subSeconds,
        subagentText: subThinkingText,
      },
    },
  };
};
