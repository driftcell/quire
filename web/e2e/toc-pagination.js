/* eslint-disable no-unused-expressions -- Playwright CLI loads this file as a function expression. */
async (page) => {
  const env = globalThis.process?.env ?? {};
  const web = env.E2E_WEB_URL || "http://localhost:5173";
  const docId = "e2e-toc-pagination";
  const now = new Date().toISOString();
  const flatNodes = Array.from({ length: 47 }, (_, index) => {
    const level = index % 7 === 0 ? 1 : index % 3 === 0 ? 3 : 2;
    return {
      block_id: `heading-${index}`,
      level,
      number: `${index + 1}`,
      text: `第${index + 1}节：面向中长期特色学科建设的超长目录标题与关键指标分析 ${index + 1}`,
      children: [],
    };
  });
  const tocTree = [];
  const stack = [];
  for (const node of flatNodes) {
    while (stack.length > 0 && stack.at(-1).level >= node.level) {
      stack.pop();
    }
    if (stack.length === 0) {
      tocTree.push(node);
    } else {
      stack.at(-1).children.push(node);
    }
    stack.push(node);
  }
  const blocks = flatNodes.flatMap((node, index) => [
    {
      id: node.block_id,
      type: "heading",
      level: node.level,
      text: node.text,
    },
    {
      id: `paragraph-${index}`,
      type: "paragraph",
      text: `第${index + 1}节正文，用于核对目录页码与最终正文分页是否一致。`,
    },
  ]);
  const documentPayload = {
    doc_id: docId,
    session_id: null,
    revision: 1,
    current_rev: 1,
    cover: { enabled: false, title: "超长目录分页测试" },
    toc: {
      enabled: true,
      title: "目录",
      max_depth: 3,
      variant: "dotted",
      page_break_before: false,
      page_break_after: false,
    },
    theme: { preset: "default", tokens: {}, custom_css: "" },
    references: { enabled: false, page_break_before: true },
    blocks,
    sources: {},
    computed: {
      bibliography: [],
      block_framing: {},
      citation_map: {},
      table_layouts: {},
      toc_tree: tocTree,
    },
    created_at: now,
    updated_at: now,
  };

  await page.route("**/api/docs/**", async (route) => {
    const path = new URL(route.request().url()).pathname;
    if (path === `/api/docs/${docId}`) {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify(documentPayload),
      });
      return;
    }
    if (path === `/api/docs/${docId}/comments`) {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: "[]",
      });
      return;
    }
    await route.continue();
  });
  await page.route("**/api/sessions**", async (route) => {
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: "[]",
    });
  });
  await page.addInitScript(() => {
    window.localStorage.clear();
    window.localStorage.setItem(
      "quire.ui.preferences.v1",
      JSON.stringify({
        leftSidebarOpen: false,
        rightSidebarOpen: true,
        reportViewMode: "paged",
        rightPanelPct: 34,
      }),
    );
  });

  await page.goto(`${web}/docs/${docId}`);
  await page.waitForFunction(
    () =>
      document.querySelectorAll("[data-page-number] [data-report-toc]")
        .length >= 2,
    null,
    { timeout: 20000 },
  );

  const result = await page.evaluate(() => {
    const tocPages = [...document.querySelectorAll("[data-page-number]")]
      .filter((pageElement) => pageElement.querySelector("[data-report-toc]"))
      .map((pageElement) => {
        const pageRect = pageElement.getBoundingClientRect();
        const root = pageElement.querySelector("[data-report-toc]");
        const entries = [...root.querySelectorAll("[data-toc-block-id]")];
        return {
          page: pageElement.getAttribute("data-page-number"),
          start: Number(root.getAttribute("data-toc-range-start")),
          end: Number(root.getAttribute("data-toc-range-end")),
          continued:
            root.querySelector("h2")?.textContent?.includes("（续）") ?? false,
          entries: entries.map((entry) => {
            const rect = entry.getBoundingClientRect();
            const id = entry.getAttribute("data-toc-block-id");
            return {
              id,
              clipped: rect.top < pageRect.top || rect.bottom > pageRect.bottom,
              tocPage: entry.getAttribute("data-toc-page"),
              actualPage: document
                .getElementById(`block-${id}`)
                ?.closest("[data-page-number]")
                ?.getAttribute("data-page-number"),
            };
          }),
        };
      });
    return {
      totalPages: document.querySelectorAll("[data-page-number]").length,
      tocPages,
    };
  });

  const assert = (condition, message) => {
    if (!condition) throw new Error(message);
  };
  const entries = result.tocPages.flatMap((tocPage) => tocPage.entries);
  const ids = entries.map((entry) => entry.id);
  assert(result.tocPages.length >= 2, "long TOC must span at least two pages");
  assert(ids.length === 47, `expected 47 entries, got ${ids.length}`);
  assert(new Set(ids).size === 47, "TOC entries must not repeat");
  assert(result.tocPages[0].start === 0, "first TOC range must start at zero");
  assert(result.tocPages.at(-1).end === 47, "last TOC range must end at 47");
  for (let index = 0; index < result.tocPages.length; index += 1) {
    const tocPage = result.tocPages[index];
    assert(tocPage.end > tocPage.start, "every TOC range must be non-empty");
    if (index > 0) {
      assert(tocPage.continued, "continuation pages must show 目录（续）");
      assert(
        tocPage.start === result.tocPages[index - 1].end,
        "TOC ranges must be contiguous",
      );
    }
  }
  assert(
    !entries.some((entry) => entry.clipped),
    "no TOC entry may be clipped",
  );
  assert(
    !entries.some((entry) => entry.tocPage !== entry.actualPage),
    "every TOC page number must match the final body page",
  );

  return {
    totalPages: result.totalPages,
    tocPages: result.tocPages.map((tocPage) => ({
      page: tocPage.page,
      start: tocPage.start,
      end: tocPage.end,
      entries: tocPage.entries.length,
      continued: tocPage.continued,
    })),
    entries: ids.length,
    clipped: 0,
    pageNumberMismatches: 0,
  };
}
