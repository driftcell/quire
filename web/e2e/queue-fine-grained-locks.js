/* eslint-disable no-unused-expressions -- Playwright CLI loads this file as a function expression. */
async (page) => {
  const env = globalThis.process?.env ?? {};
  const runtime = env.E2E_RUNTIME_URL || "http://127.0.0.1:8081";
  const web = env.E2E_WEB_URL || "http://localhost:5173";
  const runtimeToken = env.E2E_RUNTIME_TOKEN || "dev-runtime-token-change-me";
  const auth = { Authorization: `Bearer ${runtimeToken}` };

  const sessionResponse = await page.request.post(`${runtime}/api/sessions`, {
    headers: { ...auth, "Content-Type": "application/json" },
    data: { title: `E2E queue fine locks ${Date.now()}`, kb_branch: "main" },
  });
  if (!sessionResponse.ok()) {
    throw new Error(`failed to create session: ${sessionResponse.status()}`);
  }
  const sessionId = (await sessionResponse.json()).session_id;

  const queueA = "queue-e2e-1";
  const queueB = "queue-e2e-2";
  const now = new Date().toISOString();
  const pendingQueue = [
    {
      queue_id: queueA,
      session_id: sessionId,
      content: "E2E queue A pending lock check",
      source: "user",
      created_at: now,
    },
    {
      queue_id: queueB,
      session_id: sessionId,
      content: "E2E queue B pending lock check",
      source: "user",
      created_at: now,
    },
  ];

  let releaseQueueA;
  const queueAGate = new Promise((resolve) => {
    releaseQueueA = resolve;
  });
  const requests = [];

  try {
    await page.route(`**/api/sessions/${sessionId}`, async (route) => {
      if (route.request().method() !== "GET") {
        await route.continue();
        return;
      }
      const response = await route.fetch();
      const body = await response.json();
      body.pending_queue = pendingQueue;
      await route.fulfill({ response, json: body });
    });

    await page.route(`**/api/sessions/${sessionId}/queue/*`, async (route) => {
      const request = route.request();
      const url = request.url();
      const method = request.method();
      const queue = url.includes(queueA)
        ? "A"
        : url.includes(queueB)
          ? "B"
          : "other";
      requests.push({ method, url, queue });
      if (
        method === "DELETE" &&
        (url.includes(queueA) || url.includes(queueB))
      ) {
        if (url.includes(queueA)) {
          await queueAGate;
        }
        await route.fulfill({ status: 204 });
        return;
      }
      await route.continue();
    });

    await page.goto(`${web}/sessions/${sessionId}`);

    const bubbleA = page.locator(`[data-queue-id="${queueA}"]`);
    const bubbleB = page.locator(`[data-queue-id="${queueB}"]`);
    await bubbleA.waitFor({ state: "visible" });
    await bubbleB.waitFor({ state: "visible" });
    await bubbleA.scrollIntoViewIfNeeded();

    const deleteA = bubbleA.getByRole("button", { name: "删除排队消息" });
    const deleteB = bubbleB.getByRole("button", { name: "删除排队消息" });

    await deleteA.click();
    await page.waitForFunction(
      (id) =>
        document
          .querySelector(`[data-queue-id="${id}"]`)
          ?.getAttribute("data-queue-busy") === "true",
      queueA,
    );

    const aBusy = await bubbleA.getAttribute("data-queue-busy");
    const aDisabled = await deleteA.isDisabled();
    const bEnabledWhileAPending = await deleteB.isEnabled();
    if (aBusy !== "true" || !aDisabled) {
      throw new Error(
        `queue A did not enter busy state: busy=${aBusy} disabled=${aDisabled}`,
      );
    }
    if (!bEnabledWhileAPending) {
      throw new Error(
        "queue B was incorrectly disabled while queue A was pending",
      );
    }

    await deleteB.click();
    await page.waitForFunction(
      (id) => !document.querySelector(`[data-queue-id="${id}"]`),
      queueB,
    );
    const bCompletedWhileAPending = requests.some(
      (request) => request.queue === "B" && request.method === "DELETE",
    );

    releaseQueueA();
    await page.waitForFunction(
      (id) => !document.querySelector(`[data-queue-id="${id}"]`),
      queueA,
    );
    const aRequests = requests.filter(
      (request) => request.queue === "A" && request.method === "DELETE",
    ).length;
    const bRequests = requests.filter(
      (request) => request.queue === "B" && request.method === "DELETE",
    ).length;
    if (aRequests !== 1 || bRequests !== 1) {
      throw new Error(
        `unexpected request counts: A=${aRequests} B=${bRequests}`,
      );
    }

    return {
      pass: true,
      scenario: "queue-fine-grained-locks",
      sessionId,
      queueA: {
        busyWhilePending: aBusy,
        deleteDisabled: aDisabled,
        requests: aRequests,
      },
      queueB: {
        deleteEnabledWhileAPending: bEnabledWhileAPending,
        completedWhileAPending: bCompletedWhileAPending,
        requests: bRequests,
      },
      requests,
    };
  } finally {
    await page.request
      .delete(`${runtime}/api/sessions/${sessionId}`, { headers: auth })
      .catch(() => undefined);
  }
};
