/* eslint-disable no-unused-expressions -- Playwright CLI loads this file as a function expression. */
async (page) => {
  const web = "http://localhost:5173";
  const sessionId = "sess_initial_report_layout";
  const docId = "initial-report-layout";
  let releaseSession;
  let releaseDocument;
  const sessionGate = new Promise((resolve) => {
    releaseSession = resolve;
  });
  const documentGate = new Promise((resolve) => {
    releaseDocument = resolve;
  });
  const json = (body) => ({
    status: 200,
    contentType: "application/json",
    body: JSON.stringify(body),
  });
  const sessionDetail = {
    session_id: sessionId,
    title: "Initial report layout",
    doc_id: docId,
    linked_doc_id: docId,
    active_run_status: "idle",
    last_event_seq: 0,
    messages: [],
    attachments: [],
    pending_queue: [],
    resolved_questions: [],
    active_subagent_traces: [],
  };
  const documentPayload = {
    doc_id: docId,
    session_id: sessionId,
    revision: 1,
    current_rev: 1,
    cover: { enabled: true, title: "Loaded report" },
    toc: { enabled: false },
    theme: { preset: "default", tokens: {}, custom_css: "" },
    references: { enabled: false },
    blocks: [{ id: "p1", type: "paragraph", text: "Loaded report body" }],
    sources: {},
    computed: {
      bibliography: [],
      block_framing: {},
      citation_map: {},
      table_layouts: {},
      toc_tree: [],
    },
  };

  await page.addInitScript(() => {
    window.localStorage.setItem(
      "quire.ui.preferences.v1",
      JSON.stringify({
        leftSidebarOpen: true,
        rightSidebarOpen: true,
        reportViewMode: "continuous",
        rightPanelPct: 34,
      }),
    );
  });
  await page.route("**/api/sessions**", async (route) => {
    const path = new URL(route.request().url()).pathname;
    if (path === `/api/sessions/${sessionId}`) {
      await sessionGate;
      await route.fulfill(json(sessionDetail));
      return;
    }
    if (path === `/api/sessions/${sessionId}/traces`) {
      await route.fulfill(json([]));
      return;
    }
    if (path === `/api/sessions/${sessionId}/events`) {
      await route.fulfill({
        status: 200,
        contentType: "text/event-stream",
        body: ": connected\n\n",
      });
      return;
    }
    await route.fulfill(json([]));
  });
  await page.route("**/api/docs/**", async (route) => {
    const path = new URL(route.request().url()).pathname;
    if (path === `/api/docs/${docId}`) {
      await documentGate;
      await route.fulfill(json(documentPayload));
      return;
    }
    if (path === `/api/docs/${docId}/comments`) {
      await route.fulfill(json([]));
      return;
    }
    if (path === `/api/docs/${docId}/events`) {
      await route.fulfill({
        status: 200,
        contentType: "text/event-stream",
        body: ": connected\n\n",
      });
      return;
    }
    await route.continue();
  });

  await page.goto(`${web}/sessions/${sessionId}`);
  await page.locator("[data-slot=session-report-loading]").waitFor({
    state: "visible",
    timeout: 15000,
  });
  const before = await page.evaluate(() => {
    const right = document.querySelector("[data-slot=session-right-panel]");
    return {
      rightState: right?.getAttribute("data-state"),
      rightWidth: right?.style.width,
      loading: document.querySelectorAll("[data-slot=session-report-loading]")
        .length,
      blocks: document.querySelectorAll("[data-block-id]").length,
    };
  });
  if (
    before.rightState !== "expanded" ||
    before.rightWidth !== "34%" ||
    before.loading !== 1 ||
    before.blocks !== 0
  ) {
    throw new Error(
      `chat-only frame appeared before the document: ${JSON.stringify(before)}`,
    );
  }

  releaseSession();
  releaseDocument();
  await page.waitForFunction(
    () => document.querySelectorAll("[data-block-id]").length === 1,
    null,
    { timeout: 15000 },
  );
  const after = await page.evaluate(() => ({
    blocks: document.querySelectorAll("[data-block-id]").length,
    loading: document.querySelectorAll("[data-slot=session-report-loading]")
      .length,
    text: document.querySelector("[data-block-id=p1]")?.textContent,
  }));
  if (after.blocks !== 1 || after.loading !== 0) {
    throw new Error(
      `report did not replace its loading skeleton: ${JSON.stringify(after)}`,
    );
  }

  return { before, after };
};
