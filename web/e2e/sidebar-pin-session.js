/* eslint-disable no-unused-expressions -- Playwright CLI loads this file as a function expression. */
async (page) => {
  const env = globalThis.process?.env ?? {};
  const web = env.E2E_WEB_URL || "http://localhost:5173";
  const runtime = env.E2E_RUNTIME_URL || "http://127.0.0.1:8081";
  const runtimeToken = env.E2E_RUNTIME_TOKEN || "dev-runtime-token-change-me";
  const auth = (token) => ({ Authorization: `Bearer ${token}` });
  const prefsKey = "quire.ui.preferences.v1";
  const title = `E2E 置顶会话 ${Date.now()}`;

  const sessionResponse = await page.request.post(`${runtime}/api/sessions`, {
    headers: {
      ...auth(runtimeToken),
      "Content-Type": "application/json",
    },
    data: {
      title,
      kb_branch: "main",
    },
  });
  if (!sessionResponse.ok()) {
    throw new Error(`failed to create session: ${sessionResponse.status()}`);
  }
  const sessionId = (await sessionResponse.json()).session_id;

  const assert = (condition, message) => {
    if (!condition) throw new Error(message);
  };

  const sessionRow = () =>
    page
      .locator('[data-slot="sidebar-menu-item"]')
      .filter({ hasText: title })
      .first();

  const openSessionMenu = async () => {
    const row = sessionRow();
    await row.waitFor({ state: "visible", timeout: 15000 });
    await row.hover();
    await page.getByRole("button", { name: `会话操作 ${title}` }).click();
  };

  const groupLabels = async () =>
    page.locator('[data-slot="sidebar-group-label"]').allTextContents();

  const assertPinnedFirst = async () => {
    const labels = await groupLabels();
    assert(labels[0] === "置顶", `pinned group must be first, got ${labels}`);
    const pinnedGroup = page.locator('[data-slot="sidebar-group"]').first();
    assert(
      (await pinnedGroup.getByText(title, { exact: true }).count()) === 1,
      "session must be inside the pinned group",
    );
  };

  const sessionPinnedAt = async () => {
    const response = await page.request.get(
      `${runtime}/api/sessions/${sessionId}`,
      { headers: auth(runtimeToken) },
    );
    if (!response.ok()) {
      throw new Error(`failed to read session: ${response.status()}`);
    }
    return (await response.json()).pinned_at;
  };

  try {
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
    await page.goto(`${web}/sessions/${sessionId}`);

    await openSessionMenu();
    const [pinResponse] = await Promise.all([
      page.waitForResponse(
        (response) =>
          response.request().method() === "PATCH" &&
          response.url().endsWith(`/api/sessions/${sessionId}`),
      ),
      page.getByText("置顶会话", { exact: true }).click(),
    ]);
    assert(pinResponse.ok(), `pin request failed: ${pinResponse.status()}`);
    await assertPinnedFirst();
    assert(
      (await sessionPinnedAt()) !== null,
      "pinned_at must be persisted after pinning",
    );

    await page.reload();
    await sessionRow().waitFor({ state: "visible", timeout: 15000 });
    await assertPinnedFirst();
    assert(
      (await sessionPinnedAt()) !== null,
      "pinned_at must survive a page reload",
    );

    await openSessionMenu();
    const [unpinResponse] = await Promise.all([
      page.waitForResponse(
        (response) =>
          response.request().method() === "PATCH" &&
          response.url().endsWith(`/api/sessions/${sessionId}`),
      ),
      page.getByText("取消置顶", { exact: true }).click(),
    ]);
    assert(
      unpinResponse.ok(),
      `unpin request failed: ${unpinResponse.status()}`,
    );
    await page
      .getByText("置顶", { exact: true })
      .waitFor({ state: "detached", timeout: 15000 });
    assert(
      (await groupLabels()).includes("置顶") === false,
      "pinned group must disappear after unpinning the only pinned session",
    );
    assert(
      !(await sessionPinnedAt()),
      "pinned_at must be cleared after unpinning",
    );
    const todayGroup = page
      .locator('[data-slot="sidebar-group"]')
      .filter({ hasText: "今天" })
      .first();
    assert(
      (await todayGroup.getByText(title, { exact: true }).count()) === 1,
      "unpinned session must return to its date group",
    );

    return {
      pass: true,
      scenario: "sidebar-pin-session",
      sessionId,
      results: {
        pinned: true,
        pinnedGroupFirst: true,
        persistedAfterReload: true,
        unpinned: true,
        pinnedGroupRemoved: true,
        returnedToDateGroup: true,
      },
    };
  } finally {
    await page
      .evaluate((key) => {
        window.localStorage.removeItem(key);
      }, prefsKey)
      .catch(() => undefined);
    await page.request
      .delete(`${runtime}/api/sessions/${sessionId}`, {
        headers: auth(runtimeToken),
      })
      .catch(() => undefined);
  }
};
