/* eslint-disable no-unused-expressions -- Playwright CLI loads this file as a function expression. */
async (page) => {
  const env = globalThis.process?.env ?? {};
  const web = env.E2E_WEB_URL || "http://localhost:5173";
  const docId = "e2e-cover-breaks";
  const now = new Date().toISOString();

  const documentPayload = {
    doc_id: docId,
    session_id: null,
    revision: 3,
    current_rev: 3,
    cover: {
      enabled: true,
      variant: "custom",
      title: "高校学科竞争力对标研究",
      custom_html: `
        <div style="height:300px;display:flex;flex-direction:column;align-items:center;justify-content:center;background:#f4f7fb;border:1px solid #d7e0ea;">
          <p style="letter-spacing:0.3em;font-size:11px;color:#5b7190;">QINGTA RESEARCH</p>
          <h1 style="margin:12px 0;font-size:34px;font-weight:700;color:#17324f;">高校学科竞争力对标研究</h1>
          <p style="font-size:13px;color:#5b7190;">2026 年度 · 内部评审稿</p>
          <script>window.coverXssExecuted = true;</script>
          <img src="x" onerror="window.coverXssExecuted = true" style="display:none" />
        </div>
      `,
    },
    toc: {
      enabled: true,
      title: "目录",
      max_depth: 3,
      variant: "standard",
    },
    theme: {
      preset: "default",
      tokens: {},
      custom_css: "",
      page_chrome: {
        header: {
          left: "{kicker}",
          center: "",
          right: "{title}",
        },
        footer: {
          left: "",
          center: "第 {page} 页 / 共 {total} 页",
          right: "",
        },
        hide_header_on_first_page: false,
        hide_footer_on_first_page: false,
        heading_page_break: "none",
      },
    },
    references: {
      enabled: false,
      page_break_before: true,
      style: "numeric",
      title: "参考文献",
    },
    blocks: [
      { id: "h1_a", type: "heading", level: 1, text: "第一章 发展基础" },
      { id: "p1", type: "paragraph", text: "学校整体办学水平稳步提升。" },
      { id: "h2_a", type: "heading", level: 2, text: "1.1 学科基础" },
      { id: "p2", type: "paragraph", text: "优势学科保持领先。" },
      { id: "h1_b", type: "heading", level: 1, text: "第二章 对标分析" },
      { id: "p3", type: "paragraph", text: "对标高校差距持续收窄。" },
    ],
    sources: {},
    computed: {
      bibliography: [],
      block_framing: {},
      citation_map: {},
      table_layouts: {},
      toc_tree: [
        {
          block_id: "h1_a",
          level: 1,
          text: "第一章 发展基础",
          number: "一",
          children: [
            {
              block_id: "h2_a",
              level: 2,
              text: "1.1 学科基础",
              number: "1.1",
              children: [],
            },
          ],
        },
        {
          block_id: "h1_b",
          level: 1,
          text: "第二章 对标分析",
          number: "二",
          children: [],
        },
      ],
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
        reportViewMode: "continuous",
        rightPanelPct: 34,
      }),
    );
  });

  const assert = (condition, message) => {
    if (!condition) throw new Error(message);
  };

  await page.goto(`${web}/docs/${docId}`);

  const article = page.locator("article").first();
  const continuousCover = article.locator('[data-report-cover="custom"]');
  await continuousCover.waitFor({ state: "visible", timeout: 15000 });
  const continuousHeight = await continuousCover.evaluate(
    (element) => element.getBoundingClientRect().height,
  );
  assert(
    continuousHeight > 250 && continuousHeight < 500,
    `continuous cover must keep natural height, got ${continuousHeight}`,
  );
  assert(
    (await page.locator("[data-page-number]").count()) === 0,
    "continuous mode must not render A4 pages",
  );
  const continuousWrapper = article.locator(
    '[data-slot="continuous-custom-cover"]',
  );
  const hasContinuousWrapperLayout = await continuousWrapper.evaluate(
    (element) => {
      const style = window.getComputedStyle(element);
      return (
        style.overflow === "hidden" &&
        Number.parseFloat(style.marginTop) < 0 &&
        Number.parseFloat(style.marginBottom) > 0
      );
    },
  );
  assert(
    hasContinuousWrapperLayout,
    "continuous custom cover must use its full-bleed spacing wrapper",
  );
  assert(
    await page.evaluate(() => window.coverXssExecuted === undefined),
    "sanitizer must strip scripts and inline handlers in the browser",
  );

  const tocTitle = article.getByText("目录", { exact: true }).first();
  await tocTitle.waitFor({ state: "visible", timeout: 5000 });
  const flowMetrics = await article.evaluate((element) => {
    const cover = element.querySelector('[data-report-cover="custom"]');
    const toc = element.querySelector('[data-toc-block-id="h1_a"]');
    if (!cover || !toc) return null;
    const coverBox = cover.getBoundingClientRect();
    const tocBox = toc.getBoundingClientRect();
    return { coverBottom: coverBox.bottom, tocTop: tocBox.top };
  });
  assert(flowMetrics !== null, "continuous TOC must follow the cover");
  assert(
    flowMetrics.tocTop >= flowMetrics.coverBottom - 1,
    "continuous cover must flow naturally into the TOC",
  );

  await page.getByRole("button", { name: "分页" }).click();
  const firstPage = page.locator('[data-page-number="1"]');
  await page
    .locator('[data-page-number="1"][data-full-page-cover="true"]')
    .waitFor({ state: "visible", timeout: 15000 });
  assert(
    (await firstPage.locator("[data-page-header]").count()) === 0,
    "full-page cover must hide the page header",
  );
  assert(
    (await firstPage.locator("[data-page-footer]").count()) === 0,
    "full-page cover must hide the page footer",
  );
  assert(
    (await firstPage.getAttribute("data-include-toc")) === null,
    "full-page cover page must not include the TOC",
  );
  assert(
    (await firstPage.locator("[data-block-id]").count()) === 0,
    "full-page cover page must not include body blocks",
  );
  assert(
    (await firstPage.locator('[data-report-cover="custom"]').count()) === 1,
    "page 1 must contain the custom cover",
  );

  const centering = await firstPage.evaluate((element) => {
    const cover = element.querySelector('[data-report-cover="custom"]');
    if (!cover) return null;
    const pageBox = element.getBoundingClientRect();
    const coverBox = cover.getBoundingClientRect();
    return {
      topGap: coverBox.top - pageBox.top,
      bottomGap: pageBox.bottom - coverBox.bottom,
    };
  });
  assert(centering !== null, "cover centering metrics must exist");
  assert(
    Math.abs(centering.topGap - centering.bottomGap) < 60,
    `full-page cover must be vertically centered, got ${JSON.stringify(centering)}`,
  );

  const secondPage = page.locator('[data-page-number="2"]');
  await secondPage.waitFor({ state: "visible", timeout: 15000 });
  assert(
    (await secondPage.getAttribute("data-include-toc")) === "true",
    "TOC must start on page 2",
  );
  assert(
    (await secondPage.locator('[data-block-id="h1_a"]').count()) === 1,
    "first body block must start on page 2",
  );

  const pageOf = async (blockId) => {
    const attribute = await page
      .locator(`[data-page-number] [data-block-id="${blockId}"]`)
      .first()
      .evaluate((element) =>
        element.closest("[data-page-number]")?.getAttribute("data-page-number"),
      );
    return Number(attribute);
  };

  const beforeH1 = await pageOf("h1_a");
  const beforeH1B = await pageOf("h1_b");
  assert(
    beforeH1 === beforeH1B,
    `default flow should keep both H1 headings together, got ${beforeH1} and ${beforeH1B}`,
  );

  await page.getByRole("button", { name: "页眉与页脚设置" }).click();
  await page.getByRole("button", { name: "章节自动换页：一级标题" }).click();
  await page.getByRole("button", { name: "应用并保存" }).click();

  await page.waitForFunction(() => {
    const readPage = (blockId) =>
      document
        .querySelector(`[data-page-number] [data-block-id="${blockId}"]`)
        ?.closest("[data-page-number]")
        ?.getAttribute("data-page-number") ?? "";
    const first = readPage("h1_a");
    const second = readPage("h1_b");
    return first !== "" && second !== "" && first !== second;
  });

  const afterH1 = await pageOf("h1_a");
  const afterH1B = await pageOf("h1_b");
  assert(
    afterH1 !== afterH1B,
    "H1 page-break policy must move the second H1 to its own page",
  );

  const audit = await page.locator("[data-page-number]").evaluateAll((pages) =>
    pages.map((element) => ({
      pageNumber: element.getAttribute("data-page-number"),
      hasContent:
        element.hasAttribute("data-include-cover") ||
        element.hasAttribute("data-include-toc") ||
        element.hasAttribute("data-include-bibliography") ||
        Boolean(element.querySelector("[data-block-id]")),
    })),
  );
  assert(
    audit.every((entry) => entry.hasContent),
    `auto heading breaks produced a blank page: ${JSON.stringify(audit)}`,
  );

  return {
    pass: true,
    scenario: "cover-and-heading-breaks",
    docId,
    results: {
      continuousCoverHeight: Math.round(continuousHeight),
      continuousWrapperLayout: hasContinuousWrapperLayout,
      continuousTocFollowsCover: true,
      coverScriptStripped: true,
      page1OnlyCover: true,
      page1HeaderHidden: true,
      page1FooterHidden: true,
      page2StartsWithToc: true,
      fullPageCoverCentered: true,
      defaultH1Pages: [beforeH1, beforeH1B],
      autoH1Pages: [afterH1, afterH1B],
      noBlankPages: true,
      totalPages: audit.length,
    },
  };
}
