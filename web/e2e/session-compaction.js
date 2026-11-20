/* eslint-disable no-unused-expressions -- Playwright CLI loads this file as a function expression. */
async (page) => {
  const env = globalThis.process?.env ?? {};
  const web = env.E2E_WEB_URL || "http://localhost:5173";
  const runtime = env.E2E_RUNTIME_URL || "http://127.0.0.1:8081";
  const sessionId = "e2e-session-compaction";
  const now = new Date().toISOString();
  const summary =
    "## Objective & Goal\n完成高校对标研报。\n\n## Confirmed Scope & Calibers\n统计口径：2021 年立项数。";

  const baseMessages = [
    {
      message_id: "msg_old_user",
      session_id: sessionId,
      role: "user",
      content: "旧问题：请总结五校科研竞争力。",
      created_at: now,
    },
    {
      message_id: "msg_old_assistant",
      session_id: sessionId,
      role: "assistant",
      content: "旧回答：已完成第一轮对标分析。",
      run_id: "run_old",
      created_at: now,
    },
  ];

  const detail = (activeStatus, compacted, compacting = false) => ({
    session_id: sessionId,
    title: "E2E session compaction",
    status: "active",
    active_run_status: activeStatus,
    created_at: now,
    updated_at: now,
    last_event_seq: 2,
    doc_id: "report-compaction",
    linked_doc_id: "report-compaction",
    compacting,
    messages: compacted
      ? [
          ...baseMessages,
          {
            message_id: "msg_compaction",
            session_id: sessionId,
            role: "compaction",
            content: summary,
            created_at: new Date().toISOString(),
          },
        ]
      : baseMessages,
    attachments: [],
    pending_queue: [],
    resolved_questions: [],
    compacted_at: compacted ? new Date().toISOString() : null,
    context_usage: compacted
      ? {
          input_tokens: 12_000,
          output_tokens: 0,
          cached_tokens: 0,
          context_used: 20_000,
          context_limit: 200_000,
        }
      : {
          input_tokens: 150_000,
          output_tokens: 20_000,
          cached_tokens: 40_000,
          context_used: 170_000,
          context_limit: 200_000,
        },
  });

  const state = {
    activeStatus: "running",
    compacted: false,
    compacting: false,
    compactRequests: 0,
  };

  let releaseCompaction = () => undefined;
  const compactionGate = new Promise((resolve) => {
    releaseCompaction = resolve;
  });

  const handleSessionApi = async (route) => {
    const request = route.request();
    const path = new URL(request.url()).pathname;
    if (path === `/api/sessions/${sessionId}/compact`) {
      if (request.method() !== "POST") {
        await route.continue();
        return;
      }
      state.compactRequests += 1;
      state.compacted = true;
      state.compacting = false;
      state.activeStatus = "idle";
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify(detail("idle", true)),
      });
      return;
    }
    if (path === `/api/sessions/${sessionId}/traces`) {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: "[]",
      });
      return;
    }
    if (path === `/api/sessions/${sessionId}/events`) {
      if (state.compacting) {
        await compactionGate;
        state.compacting = false;
        state.compacted = true;
        await route.fulfill({
          status: 200,
          headers: {
            "Content-Type": "text/event-stream",
            "Cache-Control": "no-cache",
          },
          body: `data: ${JSON.stringify({
            seq: 5,
            run_id: "compaction",
            agent_id: "main",
            parent_run_id: null,
            timestamp: new Date().toISOString(),
            event: "compaction_finished",
            success: true,
          })}\n\n`,
        });
        return;
      }
      await route.fulfill({
        status: 200,
        headers: {
          "Content-Type": "text/event-stream",
          "Cache-Control": "no-cache",
        },
        body: ": keepalive\n\n",
      });
      return;
    }
    if (path === `/api/sessions/${sessionId}` && request.method() === "GET") {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify(
          state.compacted
            ? detail("idle", true)
            : detail(state.activeStatus, false, state.compacting),
        ),
      });
      return;
    }
    await route.continue();
  };

  await page.route("**/api/sessions/**", handleSessionApi);
  await page.route(`${runtime}/api/sessions/**`, handleSessionApi);

  const assert = (condition, message) => {
    if (!condition) throw new Error(message);
  };

  await page.goto(`${web}/sessions/${sessionId}`);
  const runningContext = page.getByRole("button", { name: "85%" });
  await runningContext.waitFor({ state: "visible", timeout: 15000 });
  await runningContext.hover();

  const compactButton = page.locator('[data-slot="compact-context-button"]');
  await compactButton.waitFor({ state: "visible", timeout: 15000 });
  assert(await compactButton.isDisabled(), "compact button must be disabled while running");
  await compactButton.locator("..").hover();
  await page
    .getByText("Agent 运行中暂不可压缩", { exact: true })
    .waitFor({ state: "visible", timeout: 5000 });

  state.activeStatus = "idle";
  await page.reload();
  const idleContext = page.getByRole("button", { name: "85%" });
  await idleContext.waitFor({ state: "visible", timeout: 15000 });
  await idleContext.hover();
  await compactButton.waitFor({ state: "visible", timeout: 15000 });
  assert(await compactButton.isEnabled(), "compact button must be enabled while idle");

  await compactButton.click();
  await page
    .getByRole("button", { name: "10%" })
    .waitFor({ state: "visible", timeout: 15000 });
  assert(state.compactRequests === 1, `unexpected compact request count ${state.compactRequests}`);

  const divider = page.locator('[data-slot="compaction-divider"]');
  await divider.waitFor({ state: "visible", timeout: 15000 });
  await divider.locator("summary").click();
  await page
    .getByText("## Objective & Goal", { exact: false })
    .waitFor({ state: "visible", timeout: 5000 });

  await page
    .getByText("旧问题：请总结五校科研竞争力。", { exact: true })
    .waitFor({ state: "visible", timeout: 5000 });
  await page
    .getByText("旧回答：已完成第一轮对标分析。", { exact: true })
    .waitFor({ state: "visible", timeout: 5000 });

  state.compacted = false;
  state.compacting = true;
  state.activeStatus = "idle";
  await page.reload();
  const recoveringContext = page.getByRole("button", { name: "85%" });
  await recoveringContext.waitFor({ state: "visible", timeout: 15000 });
  await recoveringContext.hover();
  await compactButton.waitFor({ state: "visible", timeout: 15000 });
  assert(
    await compactButton.isDisabled(),
    "server compacting state must disable the button after refresh",
  );
  releaseCompaction();

  await page
    .getByRole("button", { name: "10%" })
    .waitFor({ state: "visible", timeout: 15000 });
  await page
    .locator('[data-slot="compaction-divider"]')
    .waitFor({ state: "visible", timeout: 15000 });

  return {
    pass: true,
    scenario: "session-compaction",
    web,
    runtime,
    results: {
      runningDisabled: true,
      idleEnabled: true,
      contextPercentBefore: 85,
      contextPercentAfter: 10,
      compactRequests: state.compactRequests,
      dividerVisible: true,
      historyPreserved: true,
      refreshWhileCompactingDisabled: true,
      sseFinishedRecovered: true,
    },
  };
}
