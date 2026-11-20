/* eslint-disable no-unused-expressions -- Playwright CLI loads this file as a function expression. */
async (page) => {
  const env = globalThis.process?.env ?? {};
  const web = env.E2E_WEB_URL || "http://localhost:5173";
  const server = env.E2E_SERVER_URL || "http://127.0.0.1:8082";
  const runtime = env.E2E_RUNTIME_URL || "http://127.0.0.1:8081";
  const rwToken = env.E2E_QUIRE_RW_TOKEN || "dev-rw-token-change-me";
  const roToken = env.E2E_QUIRE_RO_TOKEN || "dev-ro-token-change-me";
  const runtimeToken = env.E2E_RUNTIME_TOKEN || "dev-runtime-token-change-me";
  const docId = `e2e-cross-block-annotation-${Date.now()}`;
  const xSessionId = `e2e_cross_${Date.now()}`;
  const protocolVersion = "2025-11-25";
  const imageData =
    "data:image/svg+xml;base64,PHN2ZyB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciIHdpZHRoPSIzMjAiIGhlaWdodD0iMTIwIj48cmVjdCB3aWR0aD0iMzIwIiBoZWlnaHQ9IjEyMCIgZmlsbD0iI2YxZjVmOSIvPjx0ZXh0IHg9IjE2MCIgeT0iNjAiIHRleHQtYW5jaG9yPSJtaWRkbGUiIGZvbnQtc2l6ZT0iMTYiIGZpbGw9IiMzMzQiPkltYWdlPC90ZXh0Pjwvc3ZnPg==";

  const assert = (condition, message) => {
    if (!condition) throw new Error(message);
  };

  let sessionId = "";
  const consoleErrors = [];
  page.on("console", (message) => {
    if (message.type() === "error") consoleErrors.push(message.text());
  });
  page.on("pageerror", (error) => consoleErrors.push(error.message));

  const auth = (token) => ({ Authorization: `Bearer ${token}` });
  const mcpHeaders = {
    Authorization: `Bearer ${rwToken}`,
    Accept: "application/json, text/event-stream",
    "Content-Type": "application/json",
    "X-Session-Id": xSessionId,
    "X-Actor": "e2e-cross-block",
    "MCP-Protocol-Version": protocolVersion,
  };

  const parseRpc = async (response) => {
    const text = await response.text();
    if (!text.trim()) return {};
    if (
      (response.headers()["content-type"] || "").includes("text/event-stream")
    ) {
      let found = null;
      for (const line of text.split("\n")) {
        const data = line.startsWith("data:") ? line.slice(5).trim() : "";
        if (!data) continue;
        try {
          const parsed = JSON.parse(data);
          if (parsed.result !== undefined || parsed.error !== undefined) {
            found = parsed;
          }
        } catch {
          // Ignore SSE comments and keep-alive frames.
        }
      }
      return found ?? {};
    }
    return JSON.parse(text);
  };

  const initialize = await page.request.post(`${server}/mcp`, {
    headers: mcpHeaders,
    data: {
      jsonrpc: "2.0",
      id: 1,
      method: "initialize",
      params: {
        protocolVersion,
        capabilities: {},
        clientInfo: { name: "e2e-cross-block", version: "1.0" },
      },
    },
  });
  assert(initialize.ok(), `MCP initialize failed: ${initialize.status()}`);
  const mcpSessionId = initialize.headers()["mcp-session-id"];
  assert(Boolean(mcpSessionId), "MCP initialize did not return a session id");
  await page.request.post(`${server}/mcp`, {
    headers: { ...mcpHeaders, "Mcp-Session-Id": mcpSessionId },
    data: { jsonrpc: "2.0", method: "notifications/initialized" },
  });

  let rpcId = 2;
  const callTool = async (name, args) => {
    const response = await page.request.post(`${server}/mcp`, {
      headers: { ...mcpHeaders, "Mcp-Session-Id": mcpSessionId },
      data: {
        jsonrpc: "2.0",
        id: rpcId++,
        method: "tools/call",
        params: { name, arguments: args },
      },
    });
    assert(response.ok(), `MCP ${name} failed: ${response.status()}`);
    const body = await parseRpc(response);
    assert(!body.error, `MCP ${name} error: ${JSON.stringify(body.error)}`);
    const result = body.result;
    assert(result !== undefined, `MCP ${name} returned no result`);
    assert(
      result.isError !== true,
      `MCP ${name} returned isError: ${JSON.stringify(result)}`,
    );
    if (result.structuredContent !== undefined) {
      return result.structuredContent;
    }
    const text = result.content?.[0]?.text;
    assert(typeof text === "string", `MCP ${name} returned no content`);
    try {
      return JSON.parse(text);
    } catch {
      return text;
    }
  };

  const waitForThread = async (threadCountBefore) => {
    const deadline = Date.now() + 15_000;
    while (Date.now() < deadline) {
      const response = await page.request.get(
        `${server}/api/docs/${docId}/comments?status=all`,
        { headers: auth(roToken) },
      );
      if (response.ok()) {
        const body = await response.json();
        const comments = body.comments ?? [];
        if (comments.length > threadCountBefore) return comments;
      }
      await page.waitForTimeout(250);
    }
    throw new Error("timed out waiting for the comment thread");
  };

  const selectRange = async (startBlockId, endBlockId) => {
    await page.evaluate(
      ({ startBlockId: startId, endBlockId: endId }) => {
        const blocks = Array.from(
          document.querySelectorAll("[data-block-id]"),
        ).filter((element) => !element.closest("[data-measure-root]"));
        const start = blocks.find(
          (element) => element.dataset.blockId === startId,
        );
        const end = blocks.find((element) => element.dataset.blockId === endId);
        if (!start || !end) throw new Error("selection blocks missing");
        const textNode = (root, last) => {
          const walker = document.createTreeWalker(root, 4);
          let found = null;
          let current = walker.nextNode();
          while (current) {
            if ((current.textContent ?? "").trim()) found = current;
            if (!last && found) return found;
            current = walker.nextNode();
          }
          return found;
        };
        const startNode = textNode(start, false);
        const endNode = textNode(end, true);
        if (!startNode || !endNode) throw new Error("selection text missing");
        const range = document.createRange();
        range.setStart(startNode, 0);
        range.setEnd(endNode, endNode.textContent?.length ?? 0);
        const selection = window.getSelection();
        selection?.removeAllRanges();
        selection?.addRange(range);
        document.dispatchEvent(new MouseEvent("mouseup", { bubbles: true }));
      },
      { startBlockId, endBlockId },
    );
  };

  const clickToolbar = async (label) => {
    const button = page.getByRole("button", { name: label, exact: true });
    await button.waitFor({ state: "visible", timeout: 8000 });
    await button.click();
  };

  try {
    await callTool("create_report", {
      doc_id: docId,
      title: "跨块批注 E2E",
      enable_cover: true,
      enable_toc: true,
    });
    const revision = (await callTool("get_document_digest", { doc_id: docId }))
      .digest.current_rev;
    const receipt = await callTool("apply_operations", {
      doc_id: docId,
      expected_revision: revision,
      summary: "cross block annotation fixtures",
      ops: [
        {
          type: "append",
          blocks: [
            { id: "h_start", type: "heading", level: 1, text: "跨块批注" },
            {
              id: "p_start",
              type: "paragraph",
              text: "开篇段落：营收增长与成本优化是本年度的核心主题。",
            },
            {
              id: "p_adjacent",
              type: "paragraph",
              text: "相邻段落：这一段用于验证纯文本跨块选区。",
            },
            {
              id: "t_mid",
              type: "table",
              title: "核心指标",
              columns: [
                { key: "metric", label: "指标" },
                { key: "region", label: "区域" },
                { key: "value", label: "数值" },
              ],
              rows: [
                { metric: "营收", region: "华东", value: 120 },
                { metric: "利润", region: "华南", value: 46 },
              ],
            },
            {
              id: "img_mid",
              type: "custom_html",
              title: "趋势图",
              html: `<figure style="margin:0"><img src="${imageData}" alt="趋势图" style="width:100%;display:block;border-radius:8px" /></figure>`,
              fallback_text: "趋势图占位说明",
            },
            {
              id: "chart_mid",
              type: "echarts",
              title: "增长趋势",
              height: 260,
              option: {
                xAxis: { type: "category", data: ["Q1", "Q2", "Q3", "Q4"] },
                yAxis: { type: "value" },
                series: [{ type: "bar", data: [12, 18, 24, 31] }],
              },
              fallback_summary: "季度营收持续增长。",
            },
            {
              id: "p_end",
              type: "paragraph",
              text: "收尾段落：后续应继续关注现金流与区域结构。",
            },
          ],
        },
      ],
    });
    assert(receipt.committed === true, "fixture commit failed");

    const sessionResponse = await page.request.post(`${runtime}/api/sessions`, {
      headers: {
        ...auth(runtimeToken),
        "Content-Type": "application/json",
      },
      data: {
        title: `E2E 跨块批注 ${Date.now()}`,
        doc_id: docId,
        kb_branch: "main",
      },
    });
    assert(
      sessionResponse.ok(),
      `failed to create session: ${sessionResponse.status()}`,
    );
    sessionId = (await sessionResponse.json()).session_id;

    await page.goto(`${web}/`);
    await page.evaluate(
      ({ tokens, preferences }) => {
        window.localStorage.setItem(
          "quire.dev.tokens.v1",
          JSON.stringify(tokens),
        );
        window.localStorage.setItem(
          "quire.ui.preferences.v1",
          JSON.stringify(preferences),
        );
      },
      {
        tokens: {
          runtimeToken,
          quireRoToken: roToken,
          quireRwToken: rwToken,
        },
        preferences: {
          leftSidebarOpen: true,
          rightSidebarOpen: true,
          reportViewMode: "continuous",
          rightPanelPct: 38,
        },
      },
    );
    await page.goto(`${web}/sessions/${sessionId}`);
    await page
      .locator('[data-block-id="p_start"]')
      .waitFor({ state: "visible", timeout: 20000 });

    // Scenario 1: pure text across adjacent blocks. The popup steals focus, so
    // the CSS highlight must already be backed by saved ranges.
    await selectRange("p_start", "p_adjacent");
    await clickToolbar("批注");
    const textCommentBox = page.getByPlaceholder("添加批注...");
    await textCommentBox.waitFor({ state: "visible", timeout: 8000 });
    await textCommentBox.fill("请检查相邻段落衔接。");
    await page.waitForFunction(
      () =>
        (globalThis.CSS?.highlights?.get("quire-comment-active")?.size ?? 0) >
        0,
      null,
      { timeout: 8000 },
    );
    const beforeTextComment = await page.request.get(
      `${server}/api/docs/${docId}/comments?status=all`,
      { headers: auth(roToken) },
    );
    const beforeTextCount =
      (await beforeTextComment.json()).comments?.length ?? 0;
    await page.getByRole("button", { name: "添加", exact: true }).click();
    const textComments = await waitForThread(beforeTextCount);
    const textThread = textComments.find(
      (candidate) =>
        candidate.block_id === "p_start" &&
        candidate.end_block_id === "p_adjacent",
    );
    assert(textThread, "adjacent text thread not found");
    assert(
      textThread.anchor_blocks?.includes("p_adjacent"),
      `adjacent text anchors missing: ${JSON.stringify(textThread.anchor_blocks)}`,
    );

    await page.reload();
    await page
      .locator('[data-block-id="p_start"]')
      .waitFor({ state: "visible", timeout: 20000 });
    await page.waitForFunction(
      () => (globalThis.CSS?.highlights?.get("quire-comment")?.size ?? 0) > 0,
      null,
      { timeout: 15000 },
    );

    // Scenario 2: selection crossing table, image and ECharts blocks.
    const media = await page.evaluate(() => {
      const canvas = document.querySelector('[data-chart-canvas="true"]');
      const block = document.querySelector('[data-block-id="img_mid"]');
      const shadowHost = block?.querySelector("div") ?? null;
      const image = shadowHost?.shadowRoot?.querySelector("img") ?? null;
      return {
        canvasUserSelect: canvas
          ? window.getComputedStyle(canvas).userSelect
          : null,
        imageUserSelect: image
          ? window.getComputedStyle(image).userSelect
          : null,
        markCount: document.querySelectorAll("mark").length,
        tableMarkCount:
          document
            .querySelector('[data-block-id="t_mid"]')
            ?.querySelectorAll("mark").length ?? -1,
      };
    });
    assert(
      media.canvasUserSelect === "none",
      "ECharts canvas must be select-none",
    );
    assert(media.imageUserSelect === "none", "image must be select-none");
    assert(
      media.markCount === 0,
      "no DOM <mark> highlight injection is allowed",
    );
    assert(
      media.tableMarkCount === 0,
      "table must not contain injected <mark>",
    );

    await selectRange("p_start", "p_end");
    await clickToolbar("批注");
    const textarea = page.getByPlaceholder("添加批注...");
    await textarea.waitFor({ state: "visible", timeout: 8000 });
    await textarea.fill("请对齐跨块段落、表格和图表。");
    const firstComments = await page.request.get(
      `${server}/api/docs/${docId}/comments?status=all`,
      { headers: auth(roToken) },
    );
    const beforeCount = (await firstComments.json()).comments?.length ?? 0;
    await page.getByRole("button", { name: "添加", exact: true }).click();
    const comments = await waitForThread(beforeCount);
    const thread = comments.find(
      (candidate) =>
        candidate.block_id === "p_start" && candidate.end_block_id === "p_end",
    );
    assert(thread, "cross-block thread not found");
    assert(thread.end_block_id === "p_end", "end_block_id not persisted");
    assert(
      Array.isArray(thread.anchor_blocks) &&
        ["t_mid", "img_mid", "chart_mid"].every((blockId) =>
          thread.anchor_blocks.includes(blockId),
        ),
      `anchor_blocks missing media blocks: ${JSON.stringify(thread.anchor_blocks)}`,
    );
    assert(
      thread.anchor_snapshot?.includes("[表格: 核心指标]") &&
        thread.anchor_snapshot?.includes("[图片: 趋势图]") &&
        thread.anchor_snapshot?.includes("[图表: 增长趋势]"),
      `anchor_snapshot missing placeholders: ${thread.anchor_snapshot}`,
    );

    await page.waitForFunction(
      () => {
        const registry = globalThis.CSS?.highlights;
        return (
          (registry?.get("quire-comment-active")?.size ?? 0) > 0 ||
          (registry?.get("quire-comment")?.size ?? 0) > 0
        );
      },
      null,
      { timeout: 8000 },
    );

    await page.reload();
    await page
      .locator('[data-block-id="p_start"]')
      .waitFor({ state: "visible", timeout: 20000 });
    await page.waitForFunction(
      () => (globalThis.CSS?.highlights?.get("quire-comment")?.size ?? 0) > 0,
      null,
      { timeout: 15000 },
    );

    await page.getByRole("tab", { name: /批注/ }).click();
    await page.locator(`#comment-thread-${thread.thread_id}`).click();
    await page.waitForFunction(
      () =>
        (globalThis.CSS?.highlights?.get("quire-comment-active")?.size ?? 0) >
        0,
      null,
      { timeout: 8000 },
    );
    const startVisible = await page.evaluate(() => {
      const element = document.querySelector('[data-block-id="p_start"]');
      if (!element) return false;
      const rect = element.getBoundingClientRect();
      return rect.bottom > 0 && rect.top < window.innerHeight;
    });
    assert(startVisible, "comment jump must scroll to the start block");

    await selectRange("p_start", "p_end");
    await clickToolbar("问问 Agent");
    await page
      .getByText(/引用报告块 #p_start…#p_end/)
      .waitFor({ state: "visible", timeout: 8000 });
    assert(
      consoleErrors.length === 0,
      `browser console/page errors: ${consoleErrors.join(" | ")}`,
    );

    return {
      pass: true,
      scenario: "cross-block-annotation",
      docId,
      sessionId,
      results: {
        textSelection: true,
        adjacentTextEndBlock: textThread.end_block_id,
        persistedEndBlock: thread.end_block_id,
        anchorBlocks: thread.anchor_blocks,
        mediaProtected: true,
        highlightRestored: true,
        commentJump: true,
        askAgentRangeChip: true,
        consoleErrors: 0,
      },
    };
  } finally {
    if (sessionId) {
      await page.request
        .delete(`${runtime}/api/sessions/${sessionId}`, {
          headers: auth(runtimeToken),
        })
        .catch(() => undefined);
    }
  }
};
