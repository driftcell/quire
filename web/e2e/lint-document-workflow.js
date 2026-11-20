/* eslint-disable no-unused-expressions -- Playwright CLI loads this file as a function expression. */
async (page) => {
  const env = globalThis.process?.env ?? {};
  const web = env.E2E_WEB_URL || "http://localhost:5173";
  const server = env.E2E_SERVER_URL || "http://127.0.0.1:8082";
  const rwToken = env.E2E_QUIRE_RW_TOKEN || "dev-rw-token-change-me";
  const roToken = env.E2E_QUIRE_RO_TOKEN || "dev-ro-token-change-me";
  const runtimeToken =
    env.E2E_RUNTIME_TOKEN || "dev-runtime-token-change-me";
  const docId = "e2e-lint-workflow";
  const xSessionId = `e2e_lint_${Date.now()}`;
  const protocolVersion = "2025-11-25";

  const assert = (condition, message) => {
    if (!condition) throw new Error(message);
  };

  const baseHeaders = {
    Authorization: `Bearer ${rwToken}`,
    Accept: "application/json, text/event-stream",
    "Content-Type": "application/json",
    "X-Session-Id": xSessionId,
    "X-Actor": "e2e-lint",
    "MCP-Protocol-Version": protocolVersion,
  };

  const parseRpc = async (response) => {
    const contentType = response.headers()["content-type"] || "";
    const text = await response.text();
    if (!text.trim()) return {};
    if (contentType.includes("text/event-stream")) {
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
          // Ignore non-JSON SSE comments/heartbeats.
        }
      }
      return found ?? {};
    }
    return JSON.parse(text);
  };

  const initialize = await page.request.post(`${server}/mcp`, {
    headers: baseHeaders,
    data: {
      jsonrpc: "2.0",
      id: 1,
      method: "initialize",
      params: {
        protocolVersion,
        capabilities: {},
        clientInfo: { name: "e2e-lint", version: "1.0" },
      },
    },
  });
  assert(initialize.ok(), `MCP initialize failed: ${initialize.status()}`);
  const mcpSessionId = initialize.headers()["mcp-session-id"];
  assert(Boolean(mcpSessionId), "MCP initialize did not return a session id");
  const initializeBody = await parseRpc(initialize);
  assert(
    initializeBody.result !== undefined,
    `MCP initialize error: ${JSON.stringify(initializeBody)}`,
  );

  const mcpHeaders = { ...baseHeaders, "Mcp-Session-Id": mcpSessionId };

  await page.request.post(`${server}/mcp`, {
    headers: mcpHeaders,
    data: { jsonrpc: "2.0", method: "notifications/initialized" },
  });

  let nextId = 2;
  const rpc = async (method, params) => {
    const response = await page.request.post(`${server}/mcp`, {
      headers: mcpHeaders,
      data: { jsonrpc: "2.0", id: nextId++, method, params },
    });
    assert(response.ok(), `MCP ${method} failed: ${response.status()}`);
    const body = await parseRpc(response);
    if (body.error) {
      throw new Error(`MCP ${method} error: ${JSON.stringify(body.error)}`);
    }
    return body;
  };

  const callTool = async (name, args) => {
    const body = await rpc("tools/call", { name, arguments: args });
    const result = body.result;
    assert(result !== undefined, `tool ${name} returned no result`);
    if (result.isError === true) {
      throw new Error(`tool ${name} returned isError: ${JSON.stringify(result)}`);
    }
    if (result.structuredContent !== undefined) {
      return result.structuredContent;
    }
    const text = result.content?.[0]?.text;
    assert(typeof text === "string", `tool ${name} returned no content`);
    try {
      return JSON.parse(text);
    } catch {
      return text;
    }
  };

  const lint = async (extra = {}) =>
    callTool("lint_document", { doc_id: docId, ...extra });

  const codeSet = (receipt) =>
    new Set((receipt.diagnostics ?? []).map((diagnostic) => diagnostic.code));

  const created = await callTool("create_report", {
    doc_id: docId,
    title: "E2E Lint Workflow",
    enable_cover: true,
    enable_toc: true,
  });
  let revision = created.revision;
  assert(revision === 1, `new report must start at revision 1, got ${revision}`);

  const receipt = await callTool("apply_operations", {
    doc_id: docId,
    expected_revision: revision,
    summary: "inject lint fixtures",
    ops: [
      {
        type: "append",
        blocks: [
          { id: "h_intro", type: "heading", level: 1, text: "报告概览" },
          {
            id: "p_lint_placeholder",
            type: "paragraph",
            text: "本段包含 TBD 占位符，尚未完成。",
          },
          {
            id: "tbl_lint",
            type: "table",
            title: "指标对照",
            style: { column_widths: { metric: "48px" } },
            columns: [
              { key: "metric", label: "指标名称" },
              { key: "region", label: "地区" },
              { key: "spare", label: "备注" },
            ],
            rows: [
              { metric: "营收", region: "华东", spare: null },
              { metric: "利润", region: "华南", spare: null },
            ],
          },
          { id: "h_empty", type: "heading", level: 1, text: "空章节" },
          { id: "h_next", type: "heading", level: 1, text: "后续章节" },
          {
            id: "p_lint_dangling",
            type: "paragraph",
            text: "结论依据 [^src_missing]。",
          },
        ],
      },
    ],
  });
  assert(receipt.committed === true, "fixture operations must commit");
  revision = receipt.new_revision;

  const first = await lint();
  assert(first.ok === false, "fixture report must start with errors");
  for (const code of ["QR101", "QR201", "QR202", "QR401", "QR403"]) {
    assert(codeSet(first).has(code), `missing ${code}: ${JSON.stringify(first)}`);
  }
  const dangling = first.diagnostics.find(
    (diagnostic) => diagnostic.code === "QR101",
  );
  assert(dangling.rule === "dangling-source-id", "QR101 rule name mismatch");
  assert(dangling.suppressible === false, "QR101 must not be suppressible");
  assert(
    dangling.block_id === "p_lint_dangling",
    "QR101 must anchor to its paragraph",
  );
  const emptyColumn = first.diagnostics.find(
    (diagnostic) => diagnostic.code === "QR202",
  );
  assert(emptyColumn.rule === "table-empty-column", "QR202 rule name mismatch");
  assert(emptyColumn.suppressible === true, "QR202 must be suppressible");
  assert(emptyColumn.block_id === "tbl_lint", "QR202 must anchor to the table");

  await callTool("apply_operations", {
    doc_id: docId,
    expected_revision: revision,
    summary: "ignore the intentional empty column",
    ops: [
      {
        type: "patch_block",
        block_id: "tbl_lint",
        patch: { lint_ignore: ["QR202", "QR101"] },
      },
    ],
  });

  const ignored = await lint();
  assert(
    !codeSet(ignored).has("QR202"),
    "QR202 must be suppressed by block lint_ignore",
  );
  assert(
    codeSet(ignored).has("QR101"),
    "non-suppressible QR101 must survive lint_ignore",
  );
  assert(
    (ignored.summary?.suppressed ?? 0) >= 1,
    "suppressed diagnostics must be counted",
  );

  const fixReceipt = await callTool("apply_operations", {
    doc_id: docId,
    expected_revision: ignored.rev,
    summary: "fix placeholder and dangling citation",
    ops: [
      {
        type: "patch_block",
        block_id: "p_lint_placeholder",
        patch: { text: "本段已完成，数据来自正式来源。" },
      },
      {
        type: "patch_block",
        block_id: "p_lint_dangling",
        patch: { text: "结论已完成。" },
      },
    ],
  });
  assert(fixReceipt.committed === true, "fix operations must commit");

  const fixed = await lint();
  assert(fixed.ok === true, `report must have zero errors: ${JSON.stringify(fixed)}`);
  assert(!codeSet(fixed).has("QR101"), "QR101 must be gone after fixing");
  assert(!codeSet(fixed).has("QR401"), "QR401 must be gone after fixing");

  const tableOnly = await lint({ select: ["QR2"] });
  assert(
    tableOnly.diagnostics.every((diagnostic) =>
      diagnostic.code.startsWith("QR2"),
    ),
    `select QR2 returned non-table rules: ${JSON.stringify(tableOnly)}`,
  );
  assert(tableOnly.diagnostics.length >= 1, "select QR2 must return QR201");

  const withoutStructure = await lint({ ignore: ["QR4"] });
  assert(
    withoutStructure.diagnostics.every(
      (diagnostic) => !diagnostic.code.startsWith("QR4"),
    ),
    `ignore QR4 still returned structure rules: ${JSON.stringify(withoutStructure)}`,
  );
  assert(
    (withoutStructure.summary?.suppressed ?? 0) >= 1,
    "ignored QR4 diagnostics must count as suppressed",
  );

  const consoleErrors = [];
  page.on("console", (message) => {
    if (message.type() === "error") consoleErrors.push(message.text());
  });
  page.on("pageerror", (error) => consoleErrors.push(error.message));

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
        rightPanelPct: 34,
      },
    },
  );
  await page.goto(`${web}/docs/${docId}`);

  const table = page.locator('[data-block-id="tbl_lint"]');
  await table.waitFor({ state: "visible", timeout: 20000 });
  await page.getByText("本段已完成，数据来自正式来源。").waitFor({
    state: "visible",
    timeout: 20000,
  });
  assert(
    (await table.getByText("指标名称", { exact: true }).count()) === 1,
    "table header must render after lint_ignore",
  );
  assert(
    (await table.getByText("营收", { exact: true }).count()) === 1,
    "table body must render after lint_ignore",
  );
  assert(
    (await table.getByText("备注", { exact: true }).count()) === 1,
    "ignored empty column must still render",
  );
  await page.waitForTimeout(500);
  assert(
    consoleErrors.length === 0,
    `browser console/page errors: ${consoleErrors.join(" | ")}`,
  );

  return {
    pass: true,
    scenario: "lint-document-workflow",
    docId,
    results: {
      detected: ["QR101", "QR201", "QR202", "QR401", "QR403"],
      suppressed: ignored.summary.suppressed,
      fixedOk: fixed.ok,
      selectFiltered: true,
      ignoreFiltered: true,
      rendered: true,
      consoleErrors: 0,
    },
  };
}
