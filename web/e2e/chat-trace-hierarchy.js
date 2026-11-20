/* eslint-disable no-unused-expressions -- Playwright CLI loads this file as a function expression. */
async (page) => {
  const env = globalThis.process?.env ?? {};
  const web = env.E2E_WEB_URL || "http://localhost:5173";
  const runtime = env.E2E_RUNTIME_URL || "http://127.0.0.1:8081";
  const consoleErrors = [];
  page.on("console", (message) => {
    if (message.type() === "error") consoleErrors.push(message.text());
  });
  page.on("pageerror", (error) => consoleErrors.push(error.message));

  const now = new Date().toISOString();
  const runningId = "e2e-trace-running";
  const finishedId = "e2e-trace-finished";
  const statusId = "e2e-trace-status";
  const smoothId = "e2e-trace-smooth";

  const detail = (sessionId, overrides = {}) => ({
    session_id: sessionId,
    title: `E2E trace hierarchy ${sessionId}`,
    status: "active",
    active_run_status: "running",
    created_at: now,
    updated_at: now,
    last_event_seq: 0,
    doc_id: null,
    linked_doc_id: null,
    messages: [],
    attachments: [],
    pending_queue: [],
    resolved_questions: [],
    ...overrides,
  });

  const event = (seq, runId, agentId, payload, parentRunId = null) => ({
    seq,
    run_id: runId,
    agent_id: agentId,
    parent_run_id: parentRunId,
    timestamp: now,
    ...payload,
  });

  const sseBody = (events) =>
    events.map((entry) => `data: ${JSON.stringify(entry)}\n\n`).join("");

  const tool = (id, name, status, duration = 80, output = "{}") => ({
    tool_call_id: id,
    tool_name: name,
    status,
    input_preview: "{}",
    output_preview: output,
    duration_ms: duration,
    truncated: false,
  });

  const subagent = (
    id,
    name,
    displayName,
    focus,
    thinking,
    tools,
    summary,
  ) => ({
    subagent_run_id: id,
    agent_name: name,
    display_name: displayName,
    task: summary,
    status: "completed",
    thinking,
    task_focus: { title: focus },
    tools,
    summary,
  });

  const runningEvents = [
    event(1, "run-live", "main", { event: "turn_started" }),
    event(2, "run-live", "main", {
      event: "task_focus",
      title: "梳理报告结构",
    }),
    event(3, "run-live", "main", {
      event: "thinking_delta",
      delta: "先读取报告内容，再把章节任务分派给子代理。",
    }),
    event(4, "run-live", "main", {
      event: "tool_call_started",
      tool_call_id: "main-read",
      tool_name: "read_blocks",
      input_preview: '{"limit":20}',
      truncated: false,
    }),
    event(5, "run-live", "main", {
      event: "tool_call_finished",
      tool_call_id: "main-read",
      status: "completed",
      output_preview: '{"blocks":3}',
      duration_ms: 120,
      truncated: false,
    }),
    event(6, "run-live", "main", {
      event: "tool_call_started",
      tool_call_id: "main-spawn",
      tool_name: "spawn_subagents",
      input_preview: '{"tasks":["sub-1","sub-2"]}',
      truncated: false,
    }),
    event(7, "run-live", "main", {
      event: "tool_call_started",
      tool_call_id: "main-describe",
      tool_name: "describe_task",
      input_preview: '{"task":"chart"}',
      truncated: false,
    }),
    event(8, "run-live", "main", {
      event: "tool_call_started",
      tool_call_id: "main-chart",
      tool_name: "preview_echarts",
      input_preview: '{"chart":"bar"}',
      truncated: false,
    }),
    event(9, "run-live", "main", {
      event: "tool_call_finished",
      tool_call_id: "main-chart",
      status: "completed",
      output_preview: '{"preview_id":"pv-1"}',
      duration_ms: 800,
      truncated: false,
    }),
    event(20, "run-live", "main", {
      event: "tool_call_started",
      tool_call_id: "main-list-subagents",
      tool_name: "list_subagents",
      input_preview: "{}",
      truncated: false,
    }),
    event(21, "run-live", "main", {
      event: "tool_call_finished",
      tool_call_id: "main-list-subagents",
      status: "completed",
      output_preview: '{"subagents":["writer","analyst"]}',
      duration_ms: 40,
      truncated: false,
    }),
    event(10, "run-live", "main", {
      event: "subagent_started",
      subagent_run_id: "sub-1",
      agent_name: "writer",
      display_name: "章节撰写",
      task: "撰写第 1 章现状",
      section_id: null,
    }),
    event(11, "run-live", "main", {
      event: "subagent_started",
      subagent_run_id: "sub-2",
      agent_name: "analyst",
      display_name: "量化分析师",
      task: "绘制对标图表",
      section_id: null,
    }),
    event(
      12,
      "sub-1",
      "writer",
      {
        event: "task_focus",
        title: "撰写第 1 章现状",
      },
      "run-live",
    ),
    event(
      13,
      "sub-1",
      "writer",
      {
        event: "thinking_delta",
        delta: "先拆解章节目标。",
      },
      "run-live",
    ),
    event(
      14,
      "sub-1",
      "writer",
      {
        event: "tool_call_started",
        tool_call_id: "sub1-read",
        tool_name: "read_as_markdown",
        input_preview: "{}",
        truncated: false,
      },
      "run-live",
    ),
    event(
      15,
      "sub-1",
      "writer",
      {
        event: "tool_call_finished",
        tool_call_id: "sub1-read",
        status: "completed",
        output_preview: '{"markdown":"..."}',
        duration_ms: 90,
        truncated: false,
      },
      "run-live",
    ),
    event(
      16,
      "sub-2",
      "analyst",
      {
        event: "task_focus",
        title: "绘制对标图表",
      },
      "run-live",
    ),
    event(
      17,
      "sub-2",
      "analyst",
      {
        event: "thinking_delta",
        delta: "准备近五年立项对比数据。",
      },
      "run-live",
    ),
    event(
      18,
      "sub-2",
      "analyst",
      {
        event: "tool_call_started",
        tool_call_id: "sub2-chart",
        tool_name: "preview_echarts",
        input_preview: "{}",
        truncated: false,
      },
      "run-live",
    ),
    event(
      19,
      "sub-2",
      "analyst",
      {
        event: "tool_call_finished",
        tool_call_id: "sub2-chart",
        status: "completed",
        output_preview: '{"preview_id":"pv-2"}',
        duration_ms: 760,
        truncated: false,
      },
      "run-live",
    ),
  ];

  const statusEvents = [
    event(1, "run-status", "main", { event: "turn_started" }),
    event(2, "run-status", "main", {
      event: "thinking_delta",
      delta: "检查报告工具结果。",
    }),
    event(3, "run-status", "main", {
      event: "tool_call_started",
      tool_call_id: "status-read",
      tool_name: "read_blocks",
      input_preview: "{}",
      truncated: false,
    }),
    event(4, "run-status", "main", {
      event: "tool_call_finished",
      tool_call_id: "status-read",
      status: "completed",
      output_preview: '{"ok":true}',
      duration_ms: 60,
      truncated: false,
    }),
    event(5, "run-status", "main", {
      event: "tool_call_started",
      tool_call_id: "status-lint",
      tool_name: "lint_document",
      input_preview: "{}",
      truncated: false,
    }),
    event(6, "run-status", "main", {
      event: "tool_call_finished",
      tool_call_id: "status-lint",
      status: "failed",
      output_preview: '{"errors":["missing reference"]}',
      duration_ms: 140,
      truncated: false,
    }),
  ];

  const smoothTextChunks = [
    "流式输出",
    "平滑验证：",
    "即使网络",
    "以 50–150ms",
    "的抖动分段",
    "到达，正文也",
    "应保持稳定",
    "柔和的阅读流速。",
    "最终全文必须完整。",
  ];
  const smoothText = smoothTextChunks.join("");
  const smoothThinking = Array.from(
    { length: 24 },
    (_, index) =>
      `思考步骤 ${index + 1}：检查上下文、工具结果与中英文混排的节奏。`,
  ).join("\n");
  const smoothEvents = [
    event(1, "run-smooth", "main", { event: "turn_started" }),
    ...Array.from({ length: 12 }, (_, index) =>
      event(index + 2, "run-smooth", "main", {
        event: "thinking_delta",
        delta: `${smoothThinking.slice(
          Math.floor((smoothThinking.length / 12) * index),
          Math.floor((smoothThinking.length / 12) * (index + 1)),
        )}\n`,
      }),
    ),
    ...smoothTextChunks.map((delta, index) =>
      event(20 + index, "run-smooth", "main", {
        event: "text_delta",
        delta,
      }),
    ),
  ];

  let releaseFinish;
  const finishGate = new Promise((resolve) => {
    releaseFinish = resolve;
  });

  const finalTrace = {
    run_id: "run-final",
    status: "completed",
    duration_ms: 2400,
    segments: [
      {
        kind: "thinking",
        text: "梳理章节结构并分派两个并发子代理。",
      },
      {
        kind: "execution",
        tools: [
          tool("main-read", "read_blocks", "completed", 120),
          tool("main-spawn", "spawn_subagents", "completed", 20),
          tool("main-describe", "describe_task", "completed", 30),
        ],
        subagents: [
          subagent(
            "sub-1",
            "writer",
            "章节撰写",
            "撰写第 1 章现状",
            "拆解章节目标并读取正文。",
            [tool("sub1-read", "read_as_markdown", "completed", 90)],
            "子代理一已完成：第 1 章现状",
          ),
          subagent(
            "sub-2",
            "analyst",
            "量化分析师",
            "绘制对标图表",
            "准备近五年立项对比数据。",
            [tool("sub2-chart", "preview_echarts", "completed", 760)],
            "子代理二已完成：对标图表",
          ),
        ],
      },
      {
        kind: "text",
        text: "这是主代理最终正文，已完成第 1 章现状与对标图表。",
      },
    ],
  };

  const scenarios = {
    [runningId]: {
      detail: detail(runningId),
      traces: [],
      batches: [runningEvents],
      sseCount: 0,
    },
    [finishedId]: {
      detail: detail(finishedId),
      finalDetail: detail(finishedId, {
        active_run_status: "idle",
        last_event_seq: 20,
        messages: [
          {
            message_id: "finished-user",
            session_id: finishedId,
            role: "user",
            content: "请撰写第 1 章现状，并绘制对标图表。",
            created_at: now,
          },
          {
            message_id: "finished-assistant",
            session_id: finishedId,
            role: "assistant",
            content: "",
            run_id: "run-final",
            created_at: now,
          },
        ],
      }),
      traces: [],
      finalTraces: [finalTrace],
      batches: [
        runningEvents.slice(0, 13),
        [
          event(
            20,
            "sub-1",
            "writer",
            {
              event: "subagent_finished",
              subagent_run_id: "sub-1",
              status: "completed",
              summary: "子代理一已完成：第 1 章现状",
            },
            "run-live",
          ),
          event(
            21,
            "sub-2",
            "analyst",
            {
              event: "subagent_finished",
              subagent_run_id: "sub-2",
              status: "completed",
              summary: "子代理二已完成：对标图表",
            },
            "run-live",
          ),
          event(22, "run-live", "main", {
            event: "text_delta",
            delta: "这是主代理最终正文，已完成第 1 章现状与对标图表。",
          }),
          event(23, "run-live", "main", { event: "turn_finished" }),
        ],
      ],
      sseCount: 0,
      finished: false,
    },
    [statusId]: {
      detail: detail(statusId),
      traces: [],
      batches: [statusEvents],
      sseCount: 0,
    },
    [smoothId]: {
      detail: detail(smoothId),
      traces: [],
      batches: [smoothEvents],
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
      let events = scenario.batches[index] ?? [];
      if (scenario.finished === false && index >= 1) {
        await finishGate;
        scenario.finished = true;
        events = scenario.finalTraces ? scenario.batches[1] : [];
      }
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
        body: JSON.stringify(
          scenario.finished ? (scenario.finalTraces ?? []) : scenario.traces,
        ),
      });
      return;
    }

    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify(
        scenario.finished ? scenario.finalDetail : scenario.detail,
      ),
    });
  };

  await page.route("**/api/sessions/**", handleSessionApi);
  await page.route(`${runtime}/api/sessions/**`, handleSessionApi);

  const assert = (condition, message) => {
    if (!condition) {
      throw new Error(message);
    }
  };

  const waitForExpanded = (triggerSelector, expected, message) =>
    page
      .waitForFunction(
        ({ selector, value }) =>
          document.querySelector(selector)?.getAttribute("aria-expanded") ===
          value,
        { selector: triggerSelector, value: expected },
        { timeout: 15000 },
      )
      .catch(() => {
        throw new Error(message);
      });

  const results = {};

  await page.goto(`${web}/sessions/${runningId}`);
  await page.locator("[data-activity-trigger]").first().waitFor({
    state: "visible",
    timeout: 15000,
  });
  await waitForExpanded(
    "[data-activity-trigger]",
    "true",
    "running activity did not expand",
  );

  const liveHeaderCount = await page
    .locator('[data-activity-trigger], [data-live-focus-fallback="true"]')
    .count();
  assert(
    liveHeaderCount === 1,
    `expected exactly one live process header, got ${liveHeaderCount}`,
  );
  const fallbackHeaderCount = await page
    .locator('[data-live-focus-fallback="true"]')
    .count();
  assert(
    fallbackHeaderCount === 0,
    "fallback task-focus row duplicated the active activity header",
  );
  const activeStopCount = await page.locator('[data-slot="stop-turn"]').count();
  assert(
    activeStopCount === 1,
    `expected exactly one active stop button, got ${activeStopCount}`,
  );
  const stopNestedInTrigger = await page
    .locator('[data-slot="stop-turn"]')
    .first()
    .evaluate((element) => element.closest("[data-activity-trigger]") !== null);
  assert(
    !stopNestedInTrigger,
    "stop button must be a sibling of the collapse trigger, not nested inside it",
  );
  const activeHeaderText = (
    await page.locator("[data-activity-trigger]").first().innerText()
  ).replace(/\s+/g, " ");
  assert(
    activeHeaderText.includes("梳理报告结构"),
    `active header missing task focus title: ${activeHeaderText}`,
  );

  const runningActivity = page.locator('[data-slot="activity"]').first();
  const runningText = (await runningActivity.innerText()).replace(/\s+/g, " ");
  assert(
    runningText.includes("2 个子代理运行中"),
    `running title missing concurrent count: ${runningText}`,
  );
  assert(
    runningText.includes("读取报告内容") &&
      runningText.includes("预渲染图表") &&
      runningText.includes("查看可用子代理"),
    "semantic tool labels missing from the running trace",
  );
  assert(
    !runningText.includes("spawn_subagents") &&
      !runningText.includes("describe_task"),
    "orchestration tools leaked into the running trace",
  );

  const mainRead = page.locator('[data-tool-name="read_blocks"]').first();
  assert(
    (await mainRead.count()) === 1,
    "expected exactly one visible read_blocks tool row",
  );
  const mainReadIcons = await mainRead
    .locator('[data-slot="trace-collapsible-trigger"] svg')
    .count();
  assert(
    mainReadIcons === 1,
    `read_blocks row rendered ${mainReadIcons} icons instead of 1`,
  );

  const subagentRows = page.locator('[data-slot="subagent"]');
  assert(
    (await subagentRows.count()) === 2,
    `expected 2 subagent rows, got ${await subagentRows.count()}`,
  );
  const subagentText = (await subagentRows.allInnerTexts()).join(" ");
  assert(
    subagentText.includes("撰写第 1 章现状") &&
      subagentText.includes("绘制对标图表"),
    "subagent focus titles missing",
  );
  assert(
    (await page
      .locator('[data-slot="subagent"] [data-subagent-trigger] .animate-spin')
      .count()) === 2,
    "expected both subagents to show running spinners",
  );

  results.streaming = {
    title: (
      await page.locator("[data-activity-trigger]").first().innerText()
    ).replace(/\s+/g, " "),
    toolLabels: ["读取报告内容", "预渲染图表", "查看可用子代理"],
    hiddenTools: ["spawn_subagents", "describe_task"],
    subagents: ["撰写第 1 章现状", "绘制对标图表"],
    toolIcons: mainReadIcons,
    liveHeaderCount,
    fallbackHeaderCount,
    activeStopCount,
    stopNestedInTrigger,
  };

  const runningTrigger = page.locator("[data-activity-trigger]").first();
  await runningTrigger.click();
  await waitForExpanded(
    "[data-activity-trigger]",
    "false",
    "running activity did not collapse on click",
  );
  await runningTrigger.click();
  await waitForExpanded(
    "[data-activity-trigger]",
    "true",
    "running activity did not expand again on click",
  );

  await page.goto(`${web}/sessions/${finishedId}`);
  await page.locator("[data-activity-trigger]").first().waitFor({
    state: "visible",
    timeout: 15000,
  });
  await waitForExpanded(
    "[data-activity-trigger]",
    "true",
    "finished scenario did not start in the running expanded state",
  );
  releaseFinish();

  await page.waitForFunction(
    () =>
      document.body.innerText.includes(
        "这是主代理最终正文，已完成第 1 章现状与对标图表。",
      ),
    undefined,
    { timeout: 15000 },
  );
  await waitForExpanded(
    "[data-activity-trigger]",
    "false",
    "completed activity did not auto-collapse",
  );

  const finishedActivity = page.locator('[data-slot="activity"]').first();
  const finishedTrigger = page.locator("[data-activity-trigger]").first();
  const finishedTitle = (await finishedTrigger.innerText()).replace(
    /\s+/g,
    " ",
  );
  assert(
    finishedTitle.includes("已完成思考与 1 次操作"),
    `unexpected completed title: ${finishedTitle}`,
  );
  assert(
    finishedTitle.includes("2 个子代理"),
    `completed summary missing subagent count: ${finishedTitle}`,
  );

  const activityBox = await finishedActivity.boundingBox();
  const bodyBox = await page
    .getByText("这是主代理最终正文，已完成第 1 章现状与对标图表。", {
      exact: false,
    })
    .last()
    .boundingBox();
  assert(activityBox && bodyBox, "could not measure activity/body positions");
  assert(
    bodyBox.y >= activityBox.y + activityBox.height,
    "final body was not rendered below the process block",
  );

  await finishedTrigger.click();
  await waitForExpanded(
    "[data-activity-trigger]",
    "true",
    "completed activity did not expand on click",
  );

  const subOne = page.locator('[data-subagent-id="sub-1"]');
  await subOne.locator("[data-subagent-trigger]").click();
  await waitForExpanded(
    '[data-subagent-id="sub-1"] [data-subagent-trigger]',
    "true",
    "subagent row did not expand",
  );

  const subOneText = (await subOne.innerText()).replace(/\s+/g, " ");
  assert(
    subOneText.includes("读取报告正文"),
    "subagent tool list did not render flat under the subagent row",
  );
  assert(
    !(await page.getByText("执行详情", { exact: true }).count()),
    "legacy execution-detail wrapper is still rendered",
  );
  const subagentOutputs = await subOne
    .locator('[data-slot="subagent-output"]')
    .count();
  assert(
    subagentOutputs === 1,
    `subagent output rendered ${subagentOutputs} times instead of 1`,
  );
  const summaryMatches = await page
    .getByText("子代理一已完成：第 1 章现状", { exact: true })
    .count();
  assert(
    summaryMatches === 1,
    `subagent summary rendered ${summaryMatches} times instead of 1`,
  );

  results.completed = {
    title: finishedTitle,
    autoCollapsed: true,
    finalBodyBelow: true,
    nestedExecutionDetail: 0,
    subagentOutputCount: subagentOutputs,
    summaryCount: summaryMatches,
  };

  await page.goto(`${web}/sessions/${statusId}`);
  const statusTrigger = page.locator("[data-activity-trigger]").first();
  await statusTrigger.waitFor({ state: "visible", timeout: 15000 });
  if ((await statusTrigger.getAttribute("aria-expanded")) !== "true") {
    await statusTrigger.click();
    await waitForExpanded(
      "[data-activity-trigger]",
      "true",
      "settled status activity did not expand on click",
    );
  }
  await page.locator('[data-tool-name="read_blocks"]').first().waitFor({
    state: "visible",
    timeout: 15000,
  });
  await page.locator('[data-tool-name="lint_document"]').first().waitFor({
    state: "visible",
    timeout: 15000,
  });

  const completedLine = page.locator(
    '[data-tool-name="read_blocks"] [data-slot="trace-line"]',
  );
  const failedLine = page.locator(
    '[data-tool-name="lint_document"] [data-slot="trace-line"]',
  );
  const completedStatus = await completedLine.getAttribute("data-status");
  const failedStatus = await failedLine.getAttribute("data-status");
  assert(completedStatus === null, `completed tool status=${completedStatus}`);
  assert(failedStatus === "error", `failed tool status=${failedStatus}`);
  assert(
    (await page
      .locator('[data-tool-name="read_blocks"] .text-emerald-600')
      .count()) === 0,
    "completed tool still renders a green check accent",
  );
  const completedIcons = await page
    .locator(
      '[data-tool-name="read_blocks"] [data-slot="trace-collapsible-trigger"] svg',
    )
    .count();
  const failedIcons = await page
    .locator(
      '[data-tool-name="lint_document"] [data-slot="trace-collapsible-trigger"] svg',
    )
    .count();
  assert(
    completedIcons === 1 && failedIcons === 1,
    `tool icon counts completed=${completedIcons} failed=${failedIcons}`,
  );
  assert(
    (await page.getByText("校验报告格式", { exact: true }).count()) === 1,
    "failed tool did not show its Chinese semantic label",
  );

  results.status = {
    completedStatus,
    failedStatus,
    completedIcons,
    failedIcons,
    greenCheckCount: 0,
  };

  await page.goto(`${web}/sessions/${smoothId}`);
  await page.locator("[data-activity-trigger]").first().waitFor({
    state: "visible",
    timeout: 15000,
  });
  await page.waitForFunction(
    (text) => document.body.innerText.includes(text),
    smoothText,
    { timeout: 15000 },
  );
  const smoothFallbackCount = await page
    .locator('[data-live-focus-fallback="true"]')
    .count();
  const smoothActiveCount = await page
    .locator('[data-slot="activity"][data-active="true"]')
    .count();
  assert(
    smoothFallbackCount === 1 && smoothActiveCount === 0,
    `streaming body fallback header mismatch: fallback=${smoothFallbackCount} active=${smoothActiveCount}`,
  );
  const thinkingTrigger = page
    .locator(
      '[data-slot="thinking-trace"] [data-slot="trace-collapsible-trigger"]',
    )
    .first();
  await thinkingTrigger.waitFor({ state: "visible", timeout: 8000 });
  await thinkingTrigger.click();
  await page.waitForFunction(
    () => {
      const viewport = document.querySelector(
        '[data-slot="thinking-trace"] [data-slot="message-scroller-viewport"]',
      );
      return viewport ? viewport.scrollHeight > viewport.clientHeight : false;
    },
    undefined,
    { timeout: 15000 },
  );
  await page.waitForTimeout(180);
  const smoothMetrics = await page.evaluate(() => {
    const viewport = document.querySelector(
      '[data-slot="thinking-trace"] [data-slot="message-scroller-viewport"]',
    );
    return {
      animatedSpans: document.querySelectorAll("[data-sd-animate]").length,
      atBottom:
        viewport !== null &&
        viewport.scrollTop + viewport.clientHeight >= viewport.scrollHeight - 8,
    };
  });
  assert(
    smoothMetrics.animatedSpans > 0,
    "Streamdown char animation spans were not rendered",
  );
  assert(
    smoothMetrics.atBottom,
    "MessageScroller did not stick to the bottom during thinking deltas",
  );
  assert(
    consoleErrors.length === 0,
    `browser console/page errors: ${consoleErrors.join(" | ")}`,
  );

  results.smooth = {
    textComplete: true,
    animatedSpans: smoothMetrics.animatedSpans,
    autoScrolled: smoothMetrics.atBottom,
    consoleErrors: 0,
    fallbackHeaderCount: smoothFallbackCount,
    activeActivityCount: smoothActiveCount,
  };

  return {
    pass: true,
    scenario: "chat-trace-hierarchy",
    web,
    runtime,
    results,
  };
};
