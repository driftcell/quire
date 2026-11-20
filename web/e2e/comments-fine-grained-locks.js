/* eslint-disable no-unused-expressions -- Playwright CLI loads this file as a function expression. */
async (page) => {
  const env = globalThis.process?.env ?? {};
  const server = env.E2E_SERVER_URL || 'http://127.0.0.1:8082';
  const runtime = env.E2E_RUNTIME_URL || 'http://127.0.0.1:8081';
  const web = env.E2E_WEB_URL || 'http://localhost:5173';
  const rwToken = env.E2E_QUIRE_RW_TOKEN || 'dev-rw-token-change-me';
  const roToken = env.E2E_QUIRE_RO_TOKEN || 'dev-ro-token-change-me';
  const runtimeToken = env.E2E_RUNTIME_TOKEN || 'dev-runtime-token-change-me';
  const auth = (token) => ({ Authorization: `Bearer ${token}` });

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
    throw new Error('no report with blocks is available for the comments test');
  }

  const documentResponse = await page.request.get(
    `${server}/api/docs/${report.doc_id}`,
    { headers: auth(roToken) },
  );
  const document = await documentResponse.json();
  const block = (document.blocks ?? []).find((entry) => entry.id);
  if (!block?.id) {
    throw new Error('selected report has no block with an ID');
  }

  const sessionResponse = await page.request.post(`${runtime}/api/sessions`, {
    headers: { ...auth(runtimeToken), 'Content-Type': 'application/json' },
    data: {
      title: `E2E comments fine locks ${Date.now()}`,
      doc_id: report.doc_id,
      kb_branch: 'main',
    },
  });
  if (!sessionResponse.ok()) {
    throw new Error(`failed to create session: ${sessionResponse.status()}`);
  }
  const sessionId = (await sessionResponse.json()).session_id;

  const stamp = Date.now();
  const createComment = async (label) => {
    const response = await page.request.post(
      `${server}/api/docs/${report.doc_id}/comments`,
      {
        headers: { ...auth(rwToken), 'Content-Type': 'application/json' },
        data: {
          block_id: block.id,
          body: `E2E fine-grained lock thread ${label} ${stamp}`,
        },
      },
    );
    if (!response.ok()) {
      throw new Error(`failed to create comment ${label}: ${response.status()}`);
    }
    return await response.json();
  };

  const threadA = await createComment('A');
  const threadB = await createComment('B');

  let releaseThreadA;
  const threadAGate = new Promise((resolve) => {
    releaseThreadA = resolve;
  });
  const requests = [];

  try {
    await page.route(
      `**/api/sessions/${sessionId}/comments/*`,
      async (route) => {
        const request = route.request();
        const url = request.url();
        const method = request.method();
        requests.push({
          method,
          url,
          thread: url.includes(threadA.thread_id)
            ? 'A'
            : url.includes(threadB.thread_id)
              ? 'B'
              : 'other',
        });
        if (method === 'PATCH' && url.includes(threadA.thread_id)) {
          await threadAGate;
        }
        await route.continue();
      },
    );

    await page.goto(`${web}/sessions/${sessionId}`);
    await page.getByRole('tab', { name: /批注/ }).click();

    const cardA = page.locator(`#comment-thread-${threadA.thread_id}`);
    const cardB = page.locator(`#comment-thread-${threadB.thread_id}`);
    await cardA.waitFor({ state: 'visible' });
    await cardB.waitFor({ state: 'visible' });

    const archiveA = cardA.getByRole('button', { name: '归档', exact: true });
    const archiveB = cardB.getByRole('button', { name: '归档', exact: true });
    await archiveA.scrollIntoViewIfNeeded();

    await archiveA.click();
    await page.waitForFunction(
      (id) =>
        document
          .querySelector(`#comment-thread-${id}`)
          ?.getAttribute('data-thread-busy') === 'true',
      threadA.thread_id,
    );

    const aBusy = await cardA.getAttribute('data-thread-busy');
    const aDisabled = await archiveA.isDisabled();
    const bEnabledWhileAPending = await archiveB.isEnabled();
    if (aBusy !== 'true' || !aDisabled) {
      throw new Error(
        `thread A did not enter busy state: busy=${aBusy} disabled=${aDisabled}`,
      );
    }
    if (!bEnabledWhileAPending) {
      throw new Error(
        'thread B was incorrectly disabled while thread A was pending',
      );
    }

    await archiveB.click();
    await page.waitForFunction(
      (id) => !document.querySelector(`#comment-thread-${id}`),
      threadB.thread_id,
    );
    const bCompletedWhileAPending = requests.some(
      (request) => request.thread === 'B' && request.method === 'PATCH',
    );

    releaseThreadA();
    await page.waitForFunction(
      (id) => !document.querySelector(`#comment-thread-${id}`),
      threadA.thread_id,
    );
    const aRequests = requests.filter(
      (request) => request.thread === 'A' && request.method === 'PATCH',
    ).length;
    const bRequests = requests.filter(
      (request) => request.thread === 'B' && request.method === 'PATCH',
    ).length;
    if (aRequests !== 1 || bRequests !== 1) {
      throw new Error(
        `unexpected request counts: A=${aRequests} B=${bRequests}`,
      );
    }

    return {
      pass: true,
      scenario: 'comments-fine-grained-locks',
      sessionId,
      docId: report.doc_id,
      threadA: {
        busyWhilePending: aBusy,
        archiveDisabled: aDisabled,
        requests: aRequests,
      },
      threadB: {
        archiveEnabledWhileAPending: bEnabledWhileAPending,
        completedWhileAPending: bCompletedWhileAPending,
        requests: bRequests,
      },
      requests,
    };
  } finally {
    await page.request
      .delete(`${runtime}/api/sessions/${sessionId}`, {
        headers: auth(runtimeToken),
      })
      .catch(() => undefined);
    for (const thread of [threadA, threadB]) {
      await page.request
        .delete(
          `${server}/api/docs/${report.doc_id}/comments/${thread.thread_id}`,
          { headers: auth(rwToken) },
        )
        .catch(() => undefined);
    }
  }
}
