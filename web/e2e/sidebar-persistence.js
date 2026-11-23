/* eslint-disable no-unused-expressions -- Playwright CLI loads this file as a function expression. */
async (page) => {
  const env = globalThis.process?.env ?? {};
  const web = env.E2E_WEB_URL || "http://localhost:5173";
  const runtime = env.E2E_RUNTIME_URL || "http://127.0.0.1:8081";
  const server = env.E2E_SERVER_URL || "http://127.0.0.1:8082";
  const roToken = env.E2E_QUIRE_RO_TOKEN || "dev-ro-token-change-me";
  const runtimeToken = env.E2E_RUNTIME_TOKEN || "dev-runtime-token-change-me";
  const auth = (token) => ({ Authorization: `Bearer ${token}` });
  const prefsKey = "quire.ui.preferences.v1";

  const reportListResponse = await page.request.get(`${server}/api/docs`, {
    headers: auth(roToken),
  });
  if (!reportListResponse.ok()) {
    throw new Error(`failed to list reports: ${reportListResponse.status()}`);
  }
  const reports = (await reportListResponse.json()).reports ?? [];
  const report = [...reports]
    .filter((entry) => entry.block_count > 0)
    .sort((a, b) => a.block_count - b.block_count)[0];
  if (!report) {
    throw new Error("no report with blocks is available for the sidebar test");
  }

  const sessionResponse = await page.request.post(`${runtime}/api/sessions`, {
    headers: {
      ...auth(runtimeToken),
      "Content-Type": "application/json",
    },
    data: {
      title: `E2E sidebar persistence ${Date.now()}`,
      doc_id: report.doc_id,
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

  try {
    await page.goto(`${web}/`);
    await page.evaluate((key) => {
      window.localStorage.removeItem(key);
    }, prefsKey);
    await page.goto(`${web}/sessions/${sessionId}`);

    const wrapper = page.locator('[data-slot="sidebar-wrapper"]');
    const sidebar = page.locator('[data-slot="sidebar"]');
    await wrapper.waitFor({ state: "visible", timeout: 15000 });
    await sidebar.waitFor({ state: "visible", timeout: 15000 });
    assert(
      (await sidebar.getAttribute("data-state")) === "expanded",
      "sidebar should start expanded on a clean profile",
    );

    await page.getByRole("button", { name: "收起左侧会话栏" }).click();
    await page.waitForFunction(
      () =>
        document
          .querySelector('[data-slot="sidebar"]')
          ?.getAttribute("data-state") === "collapsed",
    );

    const viewToggle = page.getByRole("button", { name: "分页" });
    await viewToggle.waitFor({ state: "visible", timeout: 20000 });
    await viewToggle.click();
    await page.waitForFunction((key) => {
      const raw = window.localStorage.getItem(key);
      if (!raw) return false;
      const parsed = JSON.parse(raw);
      return (
        parsed.leftSidebarOpen === false && parsed.reportViewMode === "paged"
      );
    }, prefsKey);

    await page.reload();
    await sidebar.waitFor({ state: "attached", timeout: 15000 });
    await page.waitForFunction(
      () =>
        document
          .querySelector('[data-slot="sidebar"]')
          ?.getAttribute("data-state") === "collapsed",
    );

    const firstPaint = await page.evaluate(() => {
      const wrapperElement = document.querySelector(
        '[data-slot="sidebar-wrapper"]',
      );
      const gap = document.querySelector('[data-slot="sidebar-gap"]');
      const container = document.querySelector(
        '[data-slot="sidebar-container"]',
      );
      if (!wrapperElement || !gap || !container) return null;
      return {
        animating: wrapperElement.getAttribute("data-sidebar-animating"),
        gapWidth: window.getComputedStyle(gap).width,
        transitionDuration: window.getComputedStyle(gap).transitionDuration,
        containerDuration:
          window.getComputedStyle(container).transitionDuration,
      };
    });
    assert(firstPaint !== null, "sidebar metrics must be available");
    assert(
      firstPaint.animating === "false",
      "refresh first paint must not enable sidebar animation",
    );
    assert(
      firstPaint.gapWidth === "0px",
      `collapsed sidebar gap must be 0px, got ${firstPaint.gapWidth}`,
    );
    assert(
      firstPaint.transitionDuration === "0s",
      `refresh first paint must have no width transition, got ${firstPaint.transitionDuration}`,
    );
    assert(
      firstPaint.containerDuration === "0s",
      `refresh first paint must not transition the sidebar container, got ${firstPaint.containerDuration}`,
    );

    await page.getByRole("button", { name: "展开左侧会话栏" }).click();
    await page.waitForFunction(
      () => {
        const gap = document.querySelector('[data-slot="sidebar-gap"]');
        return gap ? window.getComputedStyle(gap).width !== "0px" : false;
      },
      null,
      { timeout: 3000 },
    );
    const during = await page.evaluate(() => {
      const gap = document.querySelector('[data-slot="sidebar-gap"]');
      const container = document.querySelector(
        '[data-slot="sidebar-container"]',
      );
      return {
        duration: gap ? window.getComputedStyle(gap).transitionDuration : "",
        containerDuration: container
          ? window.getComputedStyle(container).transitionDuration
          : "",
        gapWidth: gap ? window.getComputedStyle(gap).width : "",
      };
    });
    assert(
      during.duration === "0s",
      `manual toggle must not animate sidebar width, got ${during.duration}`,
    );
    assert(
      during.containerDuration === "0s",
      `manual toggle must not animate the sidebar container, got ${during.containerDuration}`,
    );
    assert(during.gapWidth !== "0px", "manual expand must show the sidebar");

    await page.waitForFunction(
      () =>
        document
          .querySelector('[data-slot="sidebar-wrapper"]')
          ?.getAttribute("data-sidebar-animating") === "false",
      null,
      { timeout: 3000 },
    );
    assert(
      (await sidebar.getAttribute("data-state")) === "expanded",
      "sidebar should be expanded after the manual toggle",
    );

    return {
      pass: true,
      scenario: "sidebar-persistence",
      sessionId,
      docId: report.doc_id,
      results: {
        collapsedBeforeReload: true,
        viewModePaged: true,
        collapsedAfterReload: true,
        refreshAnimating: firstPaint.animating,
        refreshTransitionDuration: firstPaint.transitionDuration,
        refreshGapWidth: firstPaint.gapWidth,
        manualAnimationSeen: false,
        manualTransitionDuration: during.duration,
        manualContainerDuration: during.containerDuration,
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
