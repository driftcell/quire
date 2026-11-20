/* eslint-disable no-unused-expressions -- Playwright CLI loads this file as a function expression. */
async (page) => {
  const env = globalThis.process?.env ?? {};
  const web = env.E2E_WEB_URL || "http://localhost:5173";
  const prefsKey = "quire.ui.preferences.v1";
  const now = new Date().toISOString();
  const sessionA = "e2e-scroll-long-a";
  const sessionB = "e2e-scroll-long-b";
  const historyDepth = 12;

  const paragraph = (seed, label) =>
    `第 ${seed} 段（${label}）：本节记录高校对标研报的统计口径、样本范围与阶段结论，` +
    `包含 2021 年至 2025 年的立项数量、经费规模与成果转化对比，` +
    `用于撑开长历史会话的滚动高度并验证首帧定位。`;

  const titleFor = (sessionId) =>
    sessionId === sessionA ? "E2E 长历史会话 A" : "E2E 长历史会话 B";
  const lastMarker = (sessionId) =>
    sessionId === sessionA ? "最后一条历史消息 A" : "最后一条历史消息 B";

  const buildHistory = (sessionId) => {
    const label = sessionId === sessionA ? "A" : "B";
    const messages = [];
    for (let index = 1; index <= historyDepth; index += 1) {
      messages.push({
        message_id: `${sessionId}-user-${index}`,
        session_id: sessionId,
        role: "user",
        content: `问题 ${index}：请分析第 ${index} 组对标数据。${paragraph(index, label)}`,
        created_at: now,
      });
      messages.push({
        message_id: `${sessionId}-assistant-${index}`,
        session_id: sessionId,
        role: "assistant",
        content: [
          paragraph(index, label),
          paragraph(index + 100, label),
          `**阶段结论 ${index}**：${paragraph(index + 200, label)}`,
        ].join("\n\n"),
        created_at: now,
      });
    }
    messages.push({
      message_id: `${sessionId}-user-last`,
      session_id: sessionId,
      role: "user",
      content: lastMarker(sessionId),
      created_at: now,
    });
    return messages;
  };

  const histories = {
    [sessionA]: buildHistory(sessionA),
    [sessionB]: buildHistory(sessionB),
  };
  const sentMessages = {
    [sessionA]: [],
    [sessionB]: [],
  };

  const summarize = (sessionId, updatedAt) => ({
    session_id: sessionId,
    title: titleFor(sessionId),
    status: "active",
    created_at: now,
    updated_at: updatedAt,
    pinned_at: null,
  });

  const detail = (sessionId) => ({
    session_id: sessionId,
    title: titleFor(sessionId),
    status: "active",
    active_run_status: "idle",
    created_at: now,
    updated_at: now,
    last_event_seq: 0,
    doc_id: null,
    linked_doc_id: null,
    messages: [...histories[sessionId], ...sentMessages[sessionId]],
    attachments: [],
    pending_queue: [],
    resolved_questions: [],
    compacted_at: null,
    compacting: false,
  });

  const event = (seq, payload) => ({
    seq,
    run_id: "run-scroll",
    agent_id: "main",
    parent_run_id: null,
    timestamp: new Date().toISOString(),
    ...payload,
  });

  const sseBody = (events) =>
    events.map((entry) => `data: ${JSON.stringify(entry)}\n\n`).join("");

  const streamMarker = (step) => `流式输出第 ${step} 步`;
  const streamText = (step) =>
    [1, 2, 3, 4, 5, 6]
      .map(
        (index) =>
          `${streamMarker(step)} · ${index}：${paragraph(
            step * 10 + index,
            "流式",
          )}`,
      )
      .join("\n\n");

  const state = {
    streaming: false,
    batches: [
      [event(101, { event: "turn_started" })],
      [event(102, { event: "text_delta", delta: streamText(1) })],
      [event(103, { event: "text_delta", delta: streamText(2) })],
      [event(104, { event: "text_delta", delta: streamText(3) })],
      [event(105, { event: "turn_finished" })],
    ],
  };

  const handleSessionApi = async (route) => {
    const request = route.request();
    const url = new URL(request.url());
    const path = url.pathname;

    if (path === "/api/sessions" && request.method() === "GET") {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify([
          summarize(sessionB, now),
          summarize(sessionA, now),
        ]),
      });
      return;
    }

    const match = path.match(
      /^\/api\/sessions\/([^/]+)(?:\/(events|traces|messages))?$/,
    );
    if (!match) {
      await route.continue();
      return;
    }
    const sessionId = decodeURIComponent(match[1]);
    const suffix = match[2];
    if (!histories[sessionId]) {
      await route.continue();
      return;
    }

    if (suffix === "events") {
      const events = state.streaming ? state.batches.shift() : undefined;
      await route.fulfill({
        status: 200,
        headers: {
          "Content-Type": "text/event-stream",
          "Cache-Control": "no-cache",
          Connection: "keep-alive",
        },
        body: events ? sseBody(events) : ": keepalive\n\n",
      });
      return;
    }

    if (suffix === "traces") {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: "[]",
      });
      return;
    }

    if (suffix === "messages" && request.method() === "POST") {
      const payload = request.postDataJSON();
      const message = {
        message_id: `${sessionId}-sent-${sentMessages[sessionId].length + 1}`,
        session_id: sessionId,
        role: "user",
        content: payload.content,
        created_at: new Date().toISOString(),
      };
      sentMessages[sessionId].push(message);
      state.streaming = true;
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({ inserted: true, queued: false, message }),
      });
      return;
    }

    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify(detail(sessionId)),
    });
  };

  await page.route("**/api/sessions**", handleSessionApi);

  const assert = (condition, message) => {
    if (!condition) {
      throw new Error(message);
    }
  };

  const chatScroller = page.locator('[data-slot="message-scroller"]').first();
  const viewport = chatScroller
    .locator('[data-slot="message-scroller-viewport"]')
    .first();

  const scrollMetrics = () =>
    viewport.evaluate((element) => ({
      scrollTop: element.scrollTop,
      clientHeight: element.clientHeight,
      scrollHeight: element.scrollHeight,
      opacity: getComputedStyle(element).opacity,
    }));

  const assertOverflowing = (metrics, label) => {
    assert(
      metrics.scrollHeight > metrics.clientHeight + 200,
      `${label}: history must overflow the viewport (scrollHeight=${metrics.scrollHeight}, clientHeight=${metrics.clientHeight})`,
    );
  };

  const distanceToBottom = (metrics) =>
    metrics.scrollHeight - metrics.scrollTop - metrics.clientHeight;

  const assertAtBottom = async (label) => {
    const metrics = await scrollMetrics();
    assertOverflowing(metrics, label);
    const distance = distanceToBottom(metrics);
    assert(
      distance <= 8,
      `${label}: expected the viewport at the bottom, distance=${distance}`,
    );
    assert(
      metrics.opacity === "1",
      `${label}: viewport stayed hidden, opacity=${metrics.opacity}`,
    );
    return { ...metrics, distance };
  };

  const waitForMarker = (text) =>
    page.waitForFunction(
      (value) => document.body.innerText.includes(value),
      text,
      { timeout: 15000 },
    );

  const results = {};

  await page.goto(`${web}/`);
  await page.evaluate((key) => {
    window.localStorage.setItem(
      key,
      JSON.stringify({
        leftSidebarOpen: true,
        rightSidebarOpen: true,
        reportViewMode: "continuous",
        rightPanelPct: 34,
      }),
    );
  }, prefsKey);

  await page.goto(`${web}/sessions/${sessionA}`);
  await page.getByText(lastMarker(sessionA), { exact: true }).waitFor({
    state: "visible",
    timeout: 15000,
  });
  await viewport.waitFor({ state: "visible", timeout: 15000 });
  await page.evaluate(() => document.fonts.ready);
  await page.waitForTimeout(300);

  const initial = await assertAtBottom("initial open");
  const initialButton = await chatScroller
    .locator('[data-slot="message-scroller-button"]')
    .getAttribute("data-active");
  assert(
    initialButton === "false",
    `initial open: jump-to-latest button must be inactive, got ${initialButton}`,
  );

  const antiFlash = await page.evaluate(() => {
    const element = document.querySelector(
      '[data-slot="message-scroller-viewport"]',
    );
    const hasClass = Boolean(
      element && element.className.includes("data-[pending-scroll]:opacity-0"),
    );
    const probe = document.createElement("div");
    probe.className = "data-[pending-scroll]:opacity-0";
    probe.setAttribute("data-pending-scroll", "");
    probe.style.position = "fixed";
    probe.style.inset = "0";
    document.body.appendChild(probe);
    const hiddenOpacity = getComputedStyle(probe).opacity;
    probe.remove();
    return { hasClass, hiddenOpacity };
  });
  assert(
    antiFlash.hasClass,
    "viewport must carry data-[pending-scroll]:opacity-0",
  );
  assert(
    antiFlash.hiddenOpacity === "0",
    `data-pending-scroll must collapse to opacity 0, got ${antiFlash.hiddenOpacity}`,
  );

  results.initialOpen = {
    distanceToBottom: initial.distance,
    scrollHeight: initial.scrollHeight,
    clientHeight: initial.clientHeight,
    jumpButtonActive: initialButton,
    pendingScrollClass: antiFlash.hasClass,
    pendingScrollOpacity: antiFlash.hiddenOpacity,
  };

  // Rolling the current session to the top, then switching through the
  // sidebar, must reset scroll state and land the next session at its bottom.
  await viewport.evaluate((element) => {
    element.scrollTop = 0;
  });
  const scrolledUp = await scrollMetrics();
  assert(
    distanceToBottom(scrolledUp) > 200,
    "test setup: scrolling to top must move the viewport away from the bottom",
  );

  await page
    .locator('[data-slot="sidebar-menu-item"]')
    .filter({ hasText: titleFor(sessionB) })
    .first()
    .click();
  await page.getByText(lastMarker(sessionB), { exact: true }).waitFor({
    state: "visible",
    timeout: 15000,
  });
  await page.evaluate(() => document.fonts.ready);
  await page.waitForTimeout(300);
  const switched = await assertAtBottom("sidebar switch to B");

  await page
    .locator('[data-slot="sidebar-menu-item"]')
    .filter({ hasText: titleFor(sessionA) })
    .first()
    .click();
  await page.getByText(lastMarker(sessionA), { exact: true }).waitFor({
    state: "visible",
    timeout: 15000,
  });
  await page.evaluate(() => document.fonts.ready);
  await page.waitForTimeout(300);
  const switchedBack = await assertAtBottom("sidebar switch back to A");

  results.sidebarSwitch = {
    toBDistance: switched.distance,
    backToADistance: switchedBack.distance,
  };

  // Sending a message and receiving streamed deltas must keep the viewport
  // pinned to the bottom while autoScroll is armed.
  const sentText = "请基于长历史继续补充结论。";
  const composer = page.getByPlaceholder(
    "Shift + Enter 换行 · 支持拖拽或粘贴截图",
  );
  await composer.fill(sentText);
  await page.getByRole("button", { name: "发送消息" }).click();
  await waitForMarker(sentText);
  const afterSend = await assertAtBottom("after send");

  const streamChecks = [];
  for (const step of [1, 2, 3]) {
    await waitForMarker(streamMarker(step));
    await page.waitForTimeout(150);
    const metrics = await assertAtBottom(`stream step ${step}`);
    streamChecks.push(metrics.distance);
  }

  results.sendAndStream = {
    sent: true,
    afterSendDistance: afterSend.distance,
    streamDistances: streamChecks,
  };

  return {
    pass: true,
    scenario: "session-initial-scroll",
    web,
    results,
  };
};
