import type {
  BibliographyEntry,
  CitationMapEntry,
  ComputedDocumentMeta,
  PageChromeConfig,
  SourceEntry,
  ThemeConfig,
} from "@/contracts/report-protocol";
import type { ReportDocumentView } from "@/lib/document";

export type CitationStyleMode = "numeric" | "author_year";

export interface StylePresetBundle {
  theme: ThemeConfig;
  cover_enabled?: boolean;
  cover_variant?: string;
  toc_enabled?: boolean;
  toc_max_depth?: 1 | 2 | 3;
  toc_variant?: string;
  toc_page_break_before?: boolean;
  toc_page_break_after?: boolean;
  citation_style?: CitationStyleMode;
  references_enabled?: boolean;
  references_page_break_before?: boolean;
}

export interface StylePresetRecord {
  style_id: string;
  name: string;
  description: string;
  category_tag: string;
  is_builtin: boolean;
  is_public: boolean;
  owner_user_id?: string | null;
  owner_display_name: string;
  base_preset_id: string;
  bundle: StylePresetBundle;
  created_at: string;
  updated_at: string;
}

const BUILTIN_TIMESTAMP = "2026-01-01T00:00:00Z";

export const BUILTIN_STYLE_PRESETS: readonly StylePresetRecord[] = [
  // 1. 灵思手札 — Borderless Notion-like exploratory notes, soft pill H2, minimal dividers
  {
    style_id: "field-notes",
    name: "灵思手札",
    description:
      "无下划线纯净大标题与松石绿柔和胶囊，暖米纸面，适配课题探索与随笔",
    category_tag: "自由 · 随笔",
    is_builtin: true,
    is_public: true,
    owner_user_id: null,
    owner_display_name: "Quire",
    base_preset_id: "field-notes",
    created_at: BUILTIN_TIMESTAMP,
    updated_at: BUILTIN_TIMESTAMP,
    bundle: {
      cover_enabled: true,
      cover_variant: "minimal",
      toc_enabled: true,
      toc_max_depth: 2,
      toc_variant: "standard",
      toc_page_break_before: false,
      toc_page_break_after: false,
      citation_style: "numeric",
      references_enabled: true,
      references_page_break_before: false,
      theme: {
        preset: "field-notes",
        tokens: {
          "--rpt-accent": "#0F766E",
          "--rpt-ink": "#1C1917",
          "--rpt-muted": "#57534E",
          "--rpt-border": "#E7E5E4",
          "--rpt-surface": "#EDF7F5",
          "--rpt-paper": "#FAF8F3",
          "--rpt-quote-bg": "#F0FDF4",
          "--rpt-table-border": "#A7F3D0",
          "--rpt-table-header-bg": "#ECFDF5",
          "--rpt-table-stripe": "#F5F3EE",
          "--rpt-line-height": "1.92",
          "--rpt-body-size": "14.5px",
          "--rpt-h1-size": "22px",
          "--rpt-heading-weight": "700",
          "--rpt-heading-tracking": "-0.01em",
          "--rpt-para-gap": "1.05rem",
          "--rpt-para-indent": "0em",
          "--rpt-para-align": "left",
          "--rpt-h1-variant": "none",
          "--rpt-h2-variant": "soft-pill",
          "--rpt-callout-variant": "solid-tint",
          "--rpt-quote-variant": "editorial-mark",
          "--rpt-table-variant": "minimal",
          "--rpt-table-density": "relaxed",
          "--rpt-radius": "12px",
          "--rpt-font-heading":
            '"Geist Variable", "PingFang SC", "Noto Sans SC", system-ui, sans-serif',
          "--rpt-font-body":
            '"Geist Variable", "PingFang SC", "Noto Sans SC", system-ui, sans-serif',
          "--rpt-chart-1": "#0F766E",
          "--rpt-chart-2": "#0284C7",
          "--rpt-chart-3": "#D97706",
          "--rpt-chart-4": "#65A30D",
          "--rpt-chart-5": "#7C3AED",
          "--rpt-chart-6": "#64748B",
        },
        custom_css: "",
        page_chrome: {
          header: { left: "{kicker}", center: "", right: "{date}" },
          footer: { left: "{title}", center: "", right: "{page} / {total}" },
          hide_header_on_first_page: true,
          hide_footer_on_first_page: false,
          heading_page_break: "none",
        },
      },
    },
  },
  // 2. 智库简报 — McKinsey-style top-rule crown H1, geometric dot H2, top-banner callouts
  {
    style_id: "modern-brief",
    name: "智库简报",
    description:
      "顶冠短横线标题与几何方标导航，顶部色条提示卡，适配战略咨询与数据快报",
    category_tag: "轻快 · 咨询",
    is_builtin: true,
    is_public: true,
    owner_user_id: null,
    owner_display_name: "Quire",
    base_preset_id: "modern-brief",
    created_at: BUILTIN_TIMESTAMP,
    updated_at: BUILTIN_TIMESTAMP,
    bundle: {
      cover_enabled: true,
      cover_variant: "modern",
      toc_enabled: true,
      toc_max_depth: 3,
      toc_variant: "numbered",
      toc_page_break_before: false,
      toc_page_break_after: false,
      citation_style: "numeric",
      references_enabled: true,
      references_page_break_before: false,
      theme: {
        preset: "modern-brief",
        tokens: {
          "--rpt-accent": "#1D4ED8",
          "--rpt-ink": "#0F172A",
          "--rpt-muted": "#475569",
          "--rpt-border": "#CBD5E1",
          "--rpt-surface": "#EFF6FF",
          "--rpt-paper": "#FFFFFF",
          "--rpt-quote-bg": "#F8FAFC",
          "--rpt-table-border": "#1E293B",
          "--rpt-table-header-bg": "#EFF6FF",
          "--rpt-table-stripe": "#F8FAFC",
          "--rpt-line-height": "1.76",
          "--rpt-body-size": "14px",
          "--rpt-h1-size": "22px",
          "--rpt-heading-weight": "700",
          "--rpt-heading-tracking": "-0.015em",
          "--rpt-para-gap": "0.75rem",
          "--rpt-para-indent": "0em",
          "--rpt-para-align": "left",
          "--rpt-h1-variant": "top-rule",
          "--rpt-h2-variant": "dot-prefix",
          "--rpt-callout-variant": "top-banner",
          "--rpt-quote-variant": "left-rule",
          "--rpt-table-variant": "striped",
          "--rpt-table-density": "standard",
          "--rpt-radius": "6px",
          "--rpt-font-heading":
            '"Geist Variable", "PingFang SC", "Noto Sans SC", system-ui, sans-serif',
          "--rpt-font-body":
            '"Geist Variable", "PingFang SC", "Noto Sans SC", system-ui, sans-serif',
          "--rpt-chart-1": "#1D4ED8",
          "--rpt-chart-2": "#0D9488",
          "--rpt-chart-3": "#3B82F6",
          "--rpt-chart-4": "#F59E0B",
          "--rpt-chart-5": "#6366F1",
          "--rpt-chart-6": "#64748B",
        },
        custom_css: "",
        page_chrome: {
          header: { left: "{kicker}", center: "", right: "{title}" },
          footer: {
            left: "{authors}",
            center: "",
            right: "P.{page} / {total}",
          },
          hide_header_on_first_page: true,
          hide_footer_on_first_page: false,
          heading_page_break: "none",
        },
      },
    },
  },
  // 3. 青塔研报 — Default flagship style: Left pillar H1, serif heading + sans body, split cover
  {
    style_id: "quire-folio",
    name: "青塔研报",
    description:
      "牛津蓝厚立柱章节头、人文衬线标题配无衬线正文，兼顾数字阅读与 A4 归档",
    category_tag: "均衡 · 标杆",
    is_builtin: true,
    is_public: true,
    owner_user_id: null,
    owner_display_name: "Quire",
    base_preset_id: "quire-folio",
    created_at: BUILTIN_TIMESTAMP,
    updated_at: BUILTIN_TIMESTAMP,
    bundle: {
      cover_enabled: true,
      cover_variant: "split",
      toc_enabled: true,
      toc_max_depth: 3,
      toc_variant: "dotted",
      toc_page_break_before: false,
      toc_page_break_after: false,
      citation_style: "numeric",
      references_enabled: true,
      references_page_break_before: false,
      theme: {
        preset: "quire-folio",
        tokens: {
          "--rpt-accent": "#1E3A5F",
          "--rpt-ink": "#18181B",
          "--rpt-muted": "#575651",
          "--rpt-border": "#D6D4CD",
          "--rpt-surface": "#F7F6F2",
          "--rpt-paper": "#FFFFFF",
          "--rpt-quote-bg": "#F8FAFC",
          "--rpt-table-border": "#1E293B",
          "--rpt-table-header-bg": "#F1F5F9",
          "--rpt-table-stripe": "#F8FAFC",
          "--rpt-line-height": "1.82",
          "--rpt-body-size": "14.5px",
          "--rpt-h1-size": "22px",
          "--rpt-heading-weight": "700",
          "--rpt-heading-tracking": "0em",
          "--rpt-para-gap": "0.75rem",
          "--rpt-para-indent": "0em",
          "--rpt-para-align": "left",
          "--rpt-h1-variant": "left-pillar",
          "--rpt-h2-variant": "left-bar",
          "--rpt-callout-variant": "left-accent",
          "--rpt-quote-variant": "left-rule",
          "--rpt-table-variant": "three-line",
          "--rpt-table-density": "standard",
          "--rpt-radius": "6px",
          "--rpt-font-heading":
            '"Noto Serif SC", "Songti SC", "STSong", "SimSun", Georgia, serif',
          "--rpt-font-body":
            '"Geist Variable", "PingFang SC", "Noto Sans SC", system-ui, sans-serif',
          "--rpt-chart-1": "#1E3A5F",
          "--rpt-chart-2": "#3B6E9C",
          "--rpt-chart-3": "#6495B8",
          "--rpt-chart-4": "#94B4CE",
          "--rpt-chart-5": "#9A6B2F",
          "--rpt-chart-6": "#475569",
        },
        custom_css: "",
        page_chrome: {
          header: { left: "{kicker}", center: "", right: "{title}" },
          footer: {
            left: "{authors}",
            center: "第 {page} 页 / 共 {total} 页",
            right: "{date}",
          },
          hide_header_on_first_page: true,
          hide_footer_on_first_page: false,
          heading_page_break: "none",
        },
      },
    },
  },
  // 4. 学林刊印 — Academic Monograph: Centered ornament H1, unadorned H2, 2em indent, justify, pure 3-line table, author_year
  {
    style_id: "academic-press",
    name: "学林刊印",
    description:
      "居中刊头章节、首行缩进两字符与两端对齐宋体，纯白三线表与著者年份引文",
    category_tag: "严谨 · 期刊",
    is_builtin: true,
    is_public: true,
    owner_user_id: null,
    owner_display_name: "Quire",
    base_preset_id: "academic-press",
    created_at: BUILTIN_TIMESTAMP,
    updated_at: BUILTIN_TIMESTAMP,
    bundle: {
      cover_enabled: true,
      cover_variant: "classic",
      toc_enabled: true,
      toc_max_depth: 3,
      toc_variant: "dotted",
      toc_page_break_before: false,
      toc_page_break_after: false,
      citation_style: "author_year",
      references_enabled: true,
      references_page_break_before: true,
      theme: {
        preset: "academic-press",
        tokens: {
          "--rpt-accent": "#27374D",
          "--rpt-ink": "#111111",
          "--rpt-muted": "#4B5563",
          "--rpt-border": "#9CA3AF",
          "--rpt-surface": "#F9FAFB",
          "--rpt-paper": "#FFFFFF",
          "--rpt-quote-bg": "#FFFFFF",
          "--rpt-table-border": "#111111",
          "--rpt-table-header-bg": "#FFFFFF",
          "--rpt-table-stripe": "#FFFFFF",
          "--rpt-line-height": "1.76",
          "--rpt-body-size": "14px",
          "--rpt-h1-size": "20px",
          "--rpt-heading-weight": "700",
          "--rpt-heading-tracking": "0.02em",
          "--rpt-para-gap": "0.55rem",
          "--rpt-para-indent": "2em",
          "--rpt-para-align": "justify",
          "--rpt-h1-variant": "center-ornament",
          "--rpt-h2-variant": "none",
          "--rpt-callout-variant": "outline-frame",
          "--rpt-quote-variant": "boxed",
          "--rpt-table-variant": "three-line",
          "--rpt-table-density": "compact",
          "--rpt-radius": "0px",
          "--rpt-font-heading":
            '"Noto Serif SC", "Songti SC", "STSong", "SimSun", Georgia, serif',
          "--rpt-font-body":
            '"Noto Serif SC", "Songti SC", "STSong", "SimSun", Georgia, serif',
          "--rpt-chart-1": "#27374D",
          "--rpt-chart-2": "#526D82",
          "--rpt-chart-3": "#8E7AB5",
          "--rpt-chart-4": "#3F72AF",
          "--rpt-chart-5": "#7F8487",
          "--rpt-chart-6": "#9DB2BF",
        },
        custom_css: "",
        page_chrome: {
          header: { left: "{title}", center: "", right: "{kicker}" },
          footer: { left: "", center: "— {page} —", right: "" },
          hide_header_on_first_page: true,
          hide_footer_on_first_page: false,
          heading_page_break: "h1",
        },
      },
    },
  },
  // 5. 丹宸公文 — Formal Red-Header Decree: Double red rule H1, pure serif H2, 2em indent, 15px
  {
    style_id: "crimson-decree",
    name: "丹宸公文",
    description:
      "朱砂红文武双线、首行缩进两字符与 15px 宋体，适配呈报校领导决策参阅件",
    category_tag: "正式 · 红头",
    is_builtin: true,
    is_public: true,
    owner_user_id: null,
    owner_display_name: "Quire",
    base_preset_id: "crimson-decree",
    created_at: BUILTIN_TIMESTAMP,
    updated_at: BUILTIN_TIMESTAMP,
    bundle: {
      cover_enabled: true,
      cover_variant: "classic",
      toc_enabled: true,
      toc_max_depth: 3,
      toc_variant: "dotted",
      toc_page_break_before: false,
      toc_page_break_after: true,
      citation_style: "numeric",
      references_enabled: true,
      references_page_break_before: true,
      theme: {
        preset: "crimson-decree",
        tokens: {
          "--rpt-accent": "#991B1B",
          "--rpt-ink": "#0A0A0A",
          "--rpt-muted": "#525252",
          "--rpt-border": "#D4D4D4",
          "--rpt-surface": "#FEF2F2",
          "--rpt-paper": "#FFFFFF",
          "--rpt-quote-bg": "#FFFBFB",
          "--rpt-table-border": "#171717",
          "--rpt-table-header-bg": "#FAFAFA",
          "--rpt-table-stripe": "#FFFFFF",
          "--rpt-line-height": "1.92",
          "--rpt-body-size": "15px",
          "--rpt-h1-size": "22px",
          "--rpt-heading-weight": "700",
          "--rpt-heading-tracking": "0.02em",
          "--rpt-para-gap": "0.75rem",
          "--rpt-para-indent": "2em",
          "--rpt-para-align": "justify",
          "--rpt-h1-variant": "double-decree",
          "--rpt-h2-variant": "none",
          "--rpt-callout-variant": "left-accent",
          "--rpt-quote-variant": "boxed",
          "--rpt-table-variant": "three-line",
          "--rpt-table-density": "relaxed",
          "--rpt-radius": "0px",
          "--rpt-font-heading":
            '"Noto Serif SC", "Songti SC", "STSong", "SimSun", Georgia, serif',
          "--rpt-font-body":
            '"Noto Serif SC", "Songti SC", "STSong", "PingFang SC", Georgia, serif',
          "--rpt-chart-1": "#991B1B",
          "--rpt-chart-2": "#1E3A5F",
          "--rpt-chart-3": "#B45309",
          "--rpt-chart-4": "#334155",
          "--rpt-chart-5": "#DC2626",
          "--rpt-chart-6": "#78716C",
        },
        custom_css: "",
        page_chrome: {
          header: { left: "{kicker}", center: "", right: "{title}" },
          footer: {
            left: "{authors}",
            center: "— 第 {page} 页 / 共 {total} 页 —",
            right: "{date}",
          },
          hide_header_on_first_page: true,
          hide_footer_on_first_page: false,
          heading_page_break: "h1",
        },
      },
    },
  },
  // 6. 瑞士网格 — Swiss International Brutalist Grid: Full-bleed dark banner H1, signal orange, 0px radius, full grid table
  {
    style_id: "swiss-grid",
    name: "瑞士网格",
    description:
      "黑底反白满幅章节横幅、信号橙几何构成与零圆角全网格表，现代主义视觉张力",
    category_tag: "先锋 · 构成",
    is_builtin: true,
    is_public: true,
    owner_user_id: null,
    owner_display_name: "Quire",
    base_preset_id: "swiss-grid",
    created_at: BUILTIN_TIMESTAMP,
    updated_at: BUILTIN_TIMESTAMP,
    bundle: {
      cover_enabled: true,
      cover_variant: "split",
      toc_enabled: true,
      toc_max_depth: 2,
      toc_variant: "numbered",
      toc_page_break_before: false,
      toc_page_break_after: false,
      citation_style: "numeric",
      references_enabled: true,
      references_page_break_before: false,
      theme: {
        preset: "swiss-grid",
        tokens: {
          "--rpt-accent": "#EA580C",
          "--rpt-ink": "#09090B",
          "--rpt-muted": "#52525B",
          "--rpt-border": "#18181B",
          "--rpt-surface": "#F4F4F5",
          "--rpt-paper": "#FFFFFF",
          "--rpt-quote-bg": "#FFF7ED",
          "--rpt-table-border": "#09090B",
          "--rpt-table-header-bg": "#F4F4F5",
          "--rpt-table-stripe": "#FAFAFA",
          "--rpt-line-height": "1.68",
          "--rpt-body-size": "14px",
          "--rpt-h1-size": "22px",
          "--rpt-heading-weight": "800",
          "--rpt-heading-tracking": "-0.02em",
          "--rpt-para-gap": "0.75rem",
          "--rpt-para-indent": "0em",
          "--rpt-para-align": "left",
          "--rpt-h1-variant": "pill-banner",
          "--rpt-h2-variant": "top-rule",
          "--rpt-callout-variant": "outline-frame",
          "--rpt-quote-variant": "left-rule",
          "--rpt-table-variant": "grid",
          "--rpt-table-density": "compact",
          "--rpt-radius": "0px",
          "--rpt-font-heading":
            '"Geist Variable", "Helvetica Neue", "PingFang SC", sans-serif',
          "--rpt-font-body":
            '"Geist Variable", "Helvetica Neue", "PingFang SC", sans-serif',
          "--rpt-chart-1": "#EA580C",
          "--rpt-chart-2": "#09090B",
          "--rpt-chart-3": "#2563EB",
          "--rpt-chart-4": "#71717A",
          "--rpt-chart-5": "#D97706",
          "--rpt-chart-6": "#0D9488",
        },
        custom_css: "",
        page_chrome: {
          header: {
            left: "SWISS GRID // {kicker}",
            center: "",
            right: "{date}",
          },
          footer: {
            left: "{title}",
            center: "",
            right: "[ {page} / {total} ]",
          },
          hide_header_on_first_page: false,
          hide_footer_on_first_page: false,
          heading_page_break: "none",
        },
      },
    },
  },
  // 7. 工程蓝图 — Technical Blueprint & Quantitative Ledger
  {
    style_id: "blueprint-ledger",
    name: "工程蓝图",
    description:
      "冷钢蓝图纸底色、技术等宽字族与全网格精密账本，适配经费决算与实验室指标盘点",
    category_tag: "精密 · 档案",
    is_builtin: true,
    is_public: true,
    owner_user_id: null,
    owner_display_name: "Quire",
    base_preset_id: "blueprint-ledger",
    created_at: BUILTIN_TIMESTAMP,
    updated_at: BUILTIN_TIMESTAMP,
    bundle: {
      cover_enabled: true,
      cover_variant: "modern",
      toc_enabled: true,
      toc_max_depth: 3,
      toc_variant: "numbered",
      toc_page_break_before: false,
      toc_page_break_after: false,
      citation_style: "numeric",
      references_enabled: true,
      references_page_break_before: false,
      theme: {
        preset: "blueprint-ledger",
        tokens: {
          "--rpt-accent": "#0369A1",
          "--rpt-ink": "#0C4A6E",
          "--rpt-muted": "#475569",
          "--rpt-border": "#BAE6FD",
          "--rpt-surface": "#E0F2FE",
          "--rpt-paper": "#F4F8FB",
          "--rpt-quote-bg": "#E0F2FE",
          "--rpt-table-border": "#0284C7",
          "--rpt-table-header-bg": "#E0F2FE",
          "--rpt-table-stripe": "#EDF5FA",
          "--rpt-line-height": "1.76",
          "--rpt-body-size": "13.5px",
          "--rpt-h1-size": "20px",
          "--rpt-heading-weight": "700",
          "--rpt-heading-tracking": "0.02em",
          "--rpt-para-gap": "0.75rem",
          "--rpt-para-indent": "0em",
          "--rpt-para-align": "left",
          "--rpt-h1-variant": "pill-banner",
          "--rpt-h2-variant": "dot-prefix",
          "--rpt-callout-variant": "outline-frame",
          "--rpt-quote-variant": "boxed",
          "--rpt-table-variant": "grid",
          "--rpt-table-density": "compact",
          "--rpt-radius": "4px",
          "--rpt-font-heading":
            '"JetBrains Mono", "Geist Mono", "PingFang SC", monospace',
          "--rpt-font-body":
            '"JetBrains Mono", "Geist Variable", "PingFang SC", monospace',
          "--rpt-chart-1": "#0369A1",
          "--rpt-chart-2": "#0284C7",
          "--rpt-chart-3": "#0D9488",
          "--rpt-chart-4": "#6366F1",
          "--rpt-chart-5": "#D97706",
          "--rpt-chart-6": "#64748B",
        },
        custom_css: "",
        page_chrome: {
          header: { left: "SPEC // {kicker}", center: "", right: "{title}" },
          footer: {
            left: "{authors}",
            center: "SHEET {page} OF {total}",
            right: "{date}",
          },
          hide_header_on_first_page: false,
          hide_footer_on_first_page: false,
          heading_page_break: "none",
        },
      },
    },
  },
  // 8. 琥珀画报 — Monocle / Economist Magazine Feature
  {
    style_id: "monocle-magazine",
    name: "琥珀画报",
    description:
      "羊皮奶油画报纸面、琥珀金居中特稿标题与巨型装饰引号，极具人文杂志叙事感",
    category_tag: "杂志 · 叙事",
    is_builtin: true,
    is_public: true,
    owner_user_id: null,
    owner_display_name: "Quire",
    base_preset_id: "monocle-magazine",
    created_at: BUILTIN_TIMESTAMP,
    updated_at: BUILTIN_TIMESTAMP,
    bundle: {
      cover_enabled: true,
      cover_variant: "classic",
      toc_enabled: true,
      toc_max_depth: 2,
      toc_variant: "dotted",
      toc_page_break_before: false,
      toc_page_break_after: false,
      citation_style: "author_year",
      references_enabled: true,
      references_page_break_before: false,
      theme: {
        preset: "monocle-magazine",
        tokens: {
          "--rpt-accent": "#9A3412",
          "--rpt-ink": "#27221D",
          "--rpt-muted": "#78716C",
          "--rpt-border": "#E7DEC8",
          "--rpt-surface": "#F5EFE4",
          "--rpt-paper": "#FBF7F0",
          "--rpt-quote-bg": "#F5EFE4",
          "--rpt-table-border": "#9A3412",
          "--rpt-table-header-bg": "#F5EFE4",
          "--rpt-table-stripe": "#F8F3E9",
          "--rpt-line-height": "1.92",
          "--rpt-body-size": "15px",
          "--rpt-h1-size": "24px",
          "--rpt-heading-weight": "700",
          "--rpt-heading-tracking": "0.04em",
          "--rpt-para-gap": "1.05rem",
          "--rpt-para-indent": "0em",
          "--rpt-para-align": "justify",
          "--rpt-h1-variant": "center-ornament",
          "--rpt-h2-variant": "none",
          "--rpt-callout-variant": "top-banner",
          "--rpt-quote-variant": "editorial-mark",
          "--rpt-table-variant": "minimal",
          "--rpt-table-density": "relaxed",
          "--rpt-radius": "6px",
          "--rpt-font-heading":
            '"Noto Serif SC", "Songti SC", "Playfair Display", Georgia, serif',
          "--rpt-font-body": '"Noto Serif SC", "Songti SC", Georgia, serif',
          "--rpt-chart-1": "#9A3412",
          "--rpt-chart-2": "#D97706",
          "--rpt-chart-3": "#1E3A5F",
          "--rpt-chart-4": "#0F766E",
          "--rpt-chart-5": "#78716C",
          "--rpt-chart-6": "#B45309",
        },
        custom_css: "",
        page_chrome: {
          header: { left: "", center: "{kicker}", right: "" },
          footer: { left: "{authors}", center: "No. {page}", right: "{date}" },
          hide_header_on_first_page: true,
          hide_footer_on_first_page: false,
          heading_page_break: "none",
        },
      },
    },
  },
  // 9. 曜石终端 — Dark Obsidian Terminal
  {
    style_id: "cyber-terminal",
    name: "曜石终端",
    description:
      "突破白纸局限的深邃曜石暗色画布、荧光翠绿立柱与霓虹多维图表，适配大屏演示与数字看板",
    category_tag: "暗色 · 极客",
    is_builtin: true,
    is_public: true,
    owner_user_id: null,
    owner_display_name: "Quire",
    base_preset_id: "cyber-terminal",
    created_at: BUILTIN_TIMESTAMP,
    updated_at: BUILTIN_TIMESTAMP,
    bundle: {
      cover_enabled: true,
      cover_variant: "modern",
      toc_enabled: true,
      toc_max_depth: 3,
      toc_variant: "numbered",
      toc_page_break_before: false,
      toc_page_break_after: false,
      citation_style: "numeric",
      references_enabled: true,
      references_page_break_before: false,
      theme: {
        preset: "cyber-terminal",
        tokens: {
          "--rpt-accent": "#10B981",
          "--rpt-ink": "#E2E8F0",
          "--rpt-muted": "#94A3B8",
          "--rpt-border": "#263345",
          "--rpt-surface": "#16202E",
          "--rpt-paper": "#0F141C",
          "--rpt-quote-bg": "#131C28",
          "--rpt-table-border": "#334155",
          "--rpt-table-header-bg": "#1E293B",
          "--rpt-table-stripe": "#131B26",
          "--rpt-line-height": "1.82",
          "--rpt-body-size": "14px",
          "--rpt-h1-size": "22px",
          "--rpt-heading-weight": "700",
          "--rpt-heading-tracking": "0.01em",
          "--rpt-para-gap": "0.75rem",
          "--rpt-para-indent": "0em",
          "--rpt-para-align": "left",
          "--rpt-h1-variant": "left-pillar",
          "--rpt-h2-variant": "dot-prefix",
          "--rpt-callout-variant": "outline-frame",
          "--rpt-quote-variant": "left-rule",
          "--rpt-table-variant": "grid",
          "--rpt-table-density": "compact",
          "--rpt-radius": "6px",
          "--rpt-font-heading":
            '"JetBrains Mono", "Geist Variable", "PingFang SC", monospace',
          "--rpt-font-body":
            '"Geist Variable", "PingFang SC", "Noto Sans SC", system-ui, sans-serif',
          "--rpt-chart-1": "#10B981",
          "--rpt-chart-2": "#38BDF8",
          "--rpt-chart-3": "#F59E0B",
          "--rpt-chart-4": "#A78BFA",
          "--rpt-chart-5": "#F43F5E",
          "--rpt-chart-6": "#2DD4BF",
        },
        custom_css: "",
        page_chrome: {
          header: {
            left: "TERMINAL // {kicker}",
            center: "",
            right: "{date}",
          },
          footer: {
            left: "{title}",
            center: "",
            right: "PAGE {page} / {total}",
          },
          hide_header_on_first_page: true,
          hide_footer_on_first_page: false,
          heading_page_break: "none",
        },
      },
    },
  },
  // 10. 竹韵书院 — Oriental Academy & Bamboo Scroll
  {
    style_id: "bamboo-scroll",
    name: "竹韵书院",
    description:
      "温润宣纸底色、竹青黛绿卷首刊头与楷宋人文排版，适配文史哲学科评估与书院专题",
    category_tag: "东方 · 雅集",
    is_builtin: true,
    is_public: true,
    owner_user_id: null,
    owner_display_name: "Quire",
    base_preset_id: "bamboo-scroll",
    created_at: BUILTIN_TIMESTAMP,
    updated_at: BUILTIN_TIMESTAMP,
    bundle: {
      cover_enabled: true,
      cover_variant: "classic",
      toc_enabled: true,
      toc_max_depth: 3,
      toc_variant: "dotted",
      toc_page_break_before: false,
      toc_page_break_after: false,
      citation_style: "numeric",
      references_enabled: true,
      references_page_break_before: false,
      theme: {
        preset: "bamboo-scroll",
        tokens: {
          "--rpt-accent": "#2D5A4C",
          "--rpt-ink": "#1F2421",
          "--rpt-muted": "#6B7068",
          "--rpt-border": "#D5CFC0",
          "--rpt-surface": "#EFECE2",
          "--rpt-paper": "#F7F4EB",
          "--rpt-quote-bg": "#EFECE2",
          "--rpt-table-border": "#2D5A4C",
          "--rpt-table-header-bg": "#EFECE2",
          "--rpt-table-stripe": "#F3F0E6",
          "--rpt-line-height": "2.05",
          "--rpt-body-size": "15px",
          "--rpt-h1-size": "24px",
          "--rpt-heading-weight": "700",
          "--rpt-heading-tracking": "0.08em",
          "--rpt-para-gap": "1.05rem",
          "--rpt-para-indent": "2em",
          "--rpt-para-align": "justify",
          "--rpt-h1-variant": "center-ornament",
          "--rpt-h2-variant": "soft-pill",
          "--rpt-callout-variant": "outline-frame",
          "--rpt-quote-variant": "boxed",
          "--rpt-table-variant": "three-line",
          "--rpt-table-density": "relaxed",
          "--rpt-radius": "6px",
          "--rpt-font-heading":
            '"STKaiti", "Kaiti SC", "KaiTi", "Noto Serif SC", "Songti SC", serif',
          "--rpt-font-body":
            '"Noto Serif SC", "Songti SC", "STSong", "STKaiti", Georgia, serif',
          "--rpt-chart-1": "#2D5A4C",
          "--rpt-chart-2": "#B45309",
          "--rpt-chart-3": "#3B6E9C",
          "--rpt-chart-4": "#6D4C41",
          "--rpt-chart-5": "#557A6E",
          "--rpt-chart-6": "#8D6E63",
        },
        custom_css: "",
        page_chrome: {
          header: { left: "{kicker}", center: "", right: "{title}" },
          footer: {
            left: "{authors}",
            center: "卷叶 · 第 {page} 页 / 共 {total} 页",
            right: "{date}",
          },
          hide_header_on_first_page: true,
          hide_footer_on_first_page: false,
          heading_page_break: "h1",
        },
      },
    },
  },
];

const SPECIMEN_SOURCES: Record<
  "moe_2025" | "nsfc_2025" | "esi_2026" | "qingta_2026",
  SourceEntry
> = {
  moe_2025: {
    title: "关于深入推进世界一流大学和一流学科建设的若干意见",
    publisher: "教育部、财政部、国家发展改革委",
    authors: ["教育部学位管理与研究生教育司"],
    date: "2025-03-18",
    accessed: "2026-11-20",
    url: "https://www.moe.gov.cn/srcsite/A22/s7065/202503/t20250318_1183920.html",
    excerpt:
      "坚持以学科为基础，强化原始创新与关键核心技术攻关，构建基础研究与应用转化贯通的学科生态体系。",
  },
  nsfc_2025: {
    title: "2025年度国家自然科学基金集中接收申请项目评审结果通告",
    publisher: "国家自然科学基金委员会",
    authors: ["国家自然科学基金委员会计划与政策局"],
    date: "2025-08-26",
    accessed: "2026-11-22",
    url: "https://www.nsfc.gov.cn/publish/portal0/tab434/info93125.htm",
    note: "统计口径：集中接收期面上项目、青年科学基金项目与重点项目，不含联合基金后期批复。",
  },
  esi_2026: {
    title:
      "Essential Science Indicators (ESI) 高被引学科统计数据库（2026年11月期）",
    publisher: "Clarivate Analytics",
    authors: ["Clarivate"],
    date: "2026-11-14",
    accessed: "2026-11-24",
    url: "https://esi.clarivate.com/",
  },
  qingta_2026: {
    title: "长三角重点高校“十四五”科研创新竞争力与学科产出全景对标数据集",
    publisher: "青塔学科云",
    authors: ["青塔高等教育研究中心"],
    date: "2026-10-30",
    accessed: "2026-11-25",
    url: "https://www.cingta.com/",
    note: "含华东五校及省属重点建设高校 2022—2025 年获批立项、直接经费与高层次人才增量数据。",
  },
};

export const SPECIMEN_DOC_ID = "specimen-style-template";

export function buildSpecimenDocument(bundle: StylePresetBundle): {
  document: ReportDocumentView;
  computed: ComputedDocumentMeta;
  pageChrome: PageChromeConfig;
} {
  const citationStyle: CitationStyleMode = bundle.citation_style ?? "numeric";
  const coverEnabled = bundle.cover_enabled ?? true;
  const coverVariant = bundle.cover_variant ?? "split";
  const tocEnabled = bundle.toc_enabled ?? true;
  const tocMaxDepth = bundle.toc_max_depth ?? 3;
  const tocVariant = bundle.toc_variant ?? "dotted";
  const tocBreakBefore = bundle.toc_page_break_before ?? false;
  const tocBreakAfter = bundle.toc_page_break_after ?? false;
  const referencesEnabled = bundle.references_enabled ?? true;
  const referencesBreakBefore = bundle.references_page_break_before ?? false;

  const pageChrome: PageChromeConfig = bundle.theme.page_chrome ?? {
    header: { left: "{kicker}", center: "", right: "{title}" },
    footer: {
      left: "{authors}",
      center: "第 {page} 页 / 共 {total} 页",
      right: "{date}",
    },
    hide_header_on_first_page: true,
    hide_footer_on_first_page: false,
    heading_page_break: "none",
  };

  const document: ReportDocumentView = {
    doc_id: SPECIMEN_DOC_ID,
    session_id: null,
    title: "2026 年“双一流”高校学科建设与科研创新竞争力对标样本报告",
    revision: 1,
    target_prose_chars: 2800,
    cover: {
      enabled: coverEnabled,
      variant: coverVariant,
      title: "2026 年“双一流”高校学科建设与科研创新竞争力对标样本报告",
      subtitle:
        "基于国家自然科学基金立项、ESI 千分之一学科演进与标志性平台产出的多维实证",
      authors: ["规划与学科建设处", "高等教育数据研究中心"],
      date: "2026年11月",
      meta: {
        kicker: "高等教育发展战略参阅 · 样式基准标本",
        unit: "规划与学科建设处",
      },
    },
    toc: {
      enabled: tocEnabled,
      title: "目录",
      max_depth: tocMaxDepth,
      variant: tocVariant,
      page_break_before: tocBreakBefore,
      page_break_after: tocBreakAfter,
    },
    theme: {
      preset: bundle.theme.preset ?? "quire-folio",
      tokens: { ...(bundle.theme.tokens ?? {}) },
      custom_css: bundle.theme.custom_css ?? "",
      page_chrome: pageChrome,
    },
    sources: { ...SPECIMEN_SOURCES },
    references: {
      enabled: referencesEnabled,
      title: "参考文献",
      style: citationStyle,
      page_break_before: referencesBreakBefore,
    },
    lint: {
      ignore: [],
    },
    blocks: [
      {
        id: "sec-overview",
        type: "heading",
        level: 1,
        text: "一、核心指标对标与总体态势",
        source_ids: [],
      },
      {
        id: "p-intro",
        type: "paragraph",
        text: "围绕新一轮“双一流”建设中期评估要求[^moe_2025]，本报告选取华东地区五所标杆高校，对 **2025 年度国家自然科学基金立项规模**、*直接经费结构*、以及 `0812 计算机科学与技术` 等优势学科集群的国际学术影响力进行系统对标[^nsfc_2025][^esi_2026]。统计期内，五校合计获批集中接收期项目 **2,438 项**，直接经费达 **15.42 亿元**，较上一年度增长 **11.8%**，呈现出基础研究向交叉学科加速集聚的显著特征（详见 [国家自然科学基金委员会年度通告](https://www.nsfc.gov.cn/)）。",
        source_ids: ["moe_2025", "nsfc_2025", "esi_2026"],
      },
      {
        id: "callout-summary",
        type: "paragraph",
        style: { variant: "callout", keep_together: true },
        text: "**核心结论提示**：头部综合性大学在重点项目与重大科研仪器研制项目上的经费占比已突破 **34.5%**；工科优势高校通过“大团队+大平台”有组织科研模式，青年科学基金项目资助率较全国均值高出 **4.2 个百分点**[^nsfc_2025][^qingta_2026]。",
        source_ids: ["nsfc_2025", "qingta_2026"],
      },
      {
        id: "html-kpi-grid",
        type: "custom_html",
        title: "样本高校集群 2025—2026 年度四大核心观测指标摘要",
        numbering: "none",
        note: "数据汇总自五所样本高校公开统计公报及 ESI 2026 年 11 月最新一期数据[^esi_2026][^qingta_2026]。",
        source_ids: ["esi_2026", "qingta_2026"],
        fallback_text:
          "国自然获批总数 2,438 项（+11.8%）；直接经费总额 15.42 亿元（+14.2%）；ESI 前 1‰ 学科 38 个（新增 5 个）；全国重点实验室牵头/共建 46 个。",
        css: `
          .kpi-grid {
            display: grid;
            grid-template-columns: repeat(4, minmax(0, 1fr));
            gap: 12px;
            margin: 4px 0;
          }
          @media (max-width: 600px) {
            .kpi-grid {
              grid-template-columns: repeat(2, minmax(0, 1fr));
            }
          }
          .kpi-card {
            border: 1px solid var(--rpt-border, #d6d4cd);
            background: var(--rpt-surface, #f7f6f2);
            border-top: 3px solid var(--rpt-accent, #1e3a5f);
            border-radius: var(--rpt-radius, 6px);
            padding: 12px 14px;
          }
          .kpi-label {
            font-size: 11.5px;
            color: var(--rpt-muted, #575651);
            margin-bottom: 4px;
          }
          .kpi-val {
            font-size: 20px;
            font-weight: 700;
            color: var(--rpt-ink, #18181b);
            font-variant-numeric: tabular-nums;
            letter-spacing: -0.02em;
          }
          .kpi-delta {
            margin-top: 4px;
            font-size: 11px;
            color: var(--rpt-accent, #1e3a5f);
            font-weight: 600;
          }
        `,
        html: `
          <div class="kpi-grid">
            <div class="kpi-card">
              <div class="kpi-label">国自然集中期获批</div>
              <div class="kpi-val">2,438 项</div>
              <div class="kpi-delta">同比 +11.8%</div>
            </div>
            <div class="kpi-card">
              <div class="kpi-label">获批直接经费总额</div>
              <div class="kpi-val">15.42 亿</div>
              <div class="kpi-delta">同比 +14.2%</div>
            </div>
            <div class="kpi-card">
              <div class="kpi-label">ESI 全球前 1‰ 学科</div>
              <div class="kpi-val">38 个</div>
              <div class="kpi-delta">期内新增 5 个</div>
            </div>
            <div class="kpi-card">
              <div class="kpi-label">全国重点实验室</div>
              <div class="kpi-val">46 个</div>
              <div class="kpi-delta">重组通过率 100%</div>
            </div>
          </div>
        `,
      },
      {
        id: "sec-funding",
        type: "heading",
        level: 2,
        text: "1.1 国家自然科学基金立项与经费结构对标",
        source_ids: [],
      },
      {
        id: "quote-policy",
        type: "quote",
        text: "“坚持面向世界科技前沿与国家重大需求，强化高校基础研究主力军和重大科技突破策源地作用，持续优化面上、青年与重点重大项目资助梯度。” ——《关于深入推进世界一流大学和一流学科建设的若干意见》[^moe_2025]",
        source_ids: ["moe_2025"],
      },
      {
        id: "tbl-benchmark",
        type: "table",
        title: "五所样本高校 2025 年国家自然科学基金立项与学科产出对标",
        numbering: "table",
        note: "单位：项、万元；资助率 = 集中期获批项数 / 有效申报项数 × 100%；含面上、青年与重点项目。",
        source_note:
          "国家自然科学基金委员会、青塔学科云[^nsfc_2025][^qingta_2026]",
        source_ids: ["nsfc_2025", "qingta_2026"],
        style: { keep_together: true },
        columns: [
          {
            key: "school",
            label: "对标高校",
            data_type: "text",
            sizing: "grow",
            weight: 1.3,
          },
          {
            key: "tier",
            label: "建设类别",
            data_type: "badge",
            sizing: "fit",
            align: "center",
          },
          {
            label: "2025年国自然立项",
            children: [
              {
                key: "projects",
                label: "获批项数",
                data_type: "number",
                sizing: "fit",
                align: "right",
              },
              {
                key: "general_youth",
                label: "面上/青年",
                data_type: "text",
                sizing: "fit",
                align: "right",
              },
              {
                key: "funding",
                label: "直接经费（万元）",
                data_type: "currency",
                sizing: "fit",
                align: "right",
              },
            ],
          },
          {
            label: "竞争效率与学科支撑",
            children: [
              {
                key: "rate",
                label: "资助率",
                data_type: "percent",
                sizing: "fit",
                align: "right",
              },
              {
                key: "growth",
                label: "经费增速",
                data_type: "percent",
                sizing: "fit",
                align: "right",
              },
            ],
          },
        ],
        rows: [
          {
            school: "A 大学（综合标杆）",
            tier: "一流大学",
            projects: 742,
            general_youth: "398 / 286",
            funding: 49850,
            rate: "22.4%",
            growth: "+14.8%",
          },
          {
            school: "B 大学（理工标杆）",
            tier: "一流大学",
            projects: 618,
            general_youth: "332 / 241",
            funding: 41260,
            rate: "21.1%",
            growth: "+13.2%",
          },
          {
            school: "C 大学（本校样本）",
            tier: "一流大学",
            projects: 486,
            general_youth: "264 / 192",
            funding: 30840,
            rate: "19.6%",
            growth: "+15.4%",
          },
          {
            school: "D 工业大学（省重点）",
            tier: "一流学科",
            projects: 312,
            general_youth: "165 / 134",
            funding: 17620,
            rate: "17.8%",
            growth: "+9.6%",
          },
          {
            school: "E 理工大学（特色校）",
            tier: "省高水平",
            projects: 280,
            general_youth: "142 / 128",
            funding: 14630,
            rate: "16.9%",
            growth: "+8.5%",
          },
        ],
        summary_rows: [
          {
            school: "五校合计 / 均值",
            tier: "样本总体",
            projects: 2438,
            general_youth: "1301 / 981",
            funding: 154200,
            rate: "19.6%",
            growth: "+12.3%",
          },
        ],
      },
      {
        id: "sec-chart-analysis",
        type: "heading",
        level: 2,
        text: "1.2 立项规模与资助率双轴演化趋势",
        source_ids: [],
      },
      {
        id: "p-chart-lead",
        type: "paragraph",
        text: "从表 1 与图 1 的联动对比可见，C 大学（本校样本）在保持青年基金高获批基数的同时，2025 年经费增速达到 **+15.4%**，位列五校第一；但在单项平均经费强度（63.46 万元/项）上，与 A 大学（67.18 万元/项）仍存在约 **5.5%** 的结构性差距[^nsfc_2025][^qingta_2026]。",
        source_ids: ["nsfc_2025", "qingta_2026"],
      },
      {
        id: "chart-funding-trend",
        type: "echarts",
        title: "五所样本高校 2025 年国自然面上/青年立项结构与整体资助率对比",
        numbering: "figure",
        height: 310,
        note: "左轴为面上项目与青年科学基金立项数（单位：项），右轴为全校集中接收期综合资助率（单位：%）。",
        source_note:
          "青塔学科云、国家自然科学基金委员会[^nsfc_2025][^qingta_2026]",
        source_ids: ["nsfc_2025", "qingta_2026"],
        fallback_summary:
          "A大学面上398项、青年286项、资助率22.4%；C大学面上264项、青年192项、资助率19.6%。",
        option: {
          tooltip: { trigger: "axis" },
          legend: {
            top: 6,
            left: "center",
            data: ["面上项目（项）", "青年基金（项）", "综合资助率（%）"],
          },
          grid: {
            containLabel: true,
            left: 16,
            right: 20,
            top: 46,
            bottom: 12,
          },
          xAxis: {
            type: "category",
            data: [
              "A 大学",
              "B 大学",
              "C 大学（本校）",
              "D 工业大学",
              "E 理工大学",
            ],
          },
          yAxis: [
            {
              type: "value",
              name: "立项数（项）",
              min: 0,
              max: 450,
            },
            {
              type: "value",
              name: "资助率（%）",
              min: 10,
              max: 26,
            },
          ],
          series: [
            {
              name: "面上项目（项）",
              type: "bar",
              barMaxWidth: 26,
              data: [398, 332, 264, 165, 142],
            },
            {
              name: "青年基金（项）",
              type: "bar",
              barMaxWidth: 26,
              data: [286, 241, 192, 134, 128],
            },
            {
              name: "综合资助率（%）",
              type: "line",
              yAxisIndex: 1,
              symbolSize: 7,
              lineStyle: { width: 2.5 },
              data: [22.4, 21.1, 19.6, 17.8, 16.9],
            },
          ],
        },
      },
      {
        id: "sec-discipline",
        type: "heading",
        level: 1,
        text: "二、高峰学科建设路径与分梯度推进策略",
        source_ids: [],
      },
      {
        id: "sec-esi",
        type: "heading",
        level: 2,
        text: "2.1 ESI 千分之一学科突破梯队与资源配置",
        source_ids: [],
      },
      {
        id: "sec-esi-detail",
        type: "heading",
        level: 3,
        text: "2.1.1 优势工科学科群与基础理科交叉机制",
        source_ids: [],
      },
      {
        id: "p-esi-body",
        type: "paragraph",
        text: "根据 Clarivate 最新一期 ESI 统计数据[^esi_2026]，本校工程学、材料科学、化学 3 个学科已稳居全球前 **0.5‰**，计算机科学、临床医学进入前 **1.2‰** 冲刺区间（学科潜力值分别达 `0.94` 与 `0.89`）。下一步需依托全国重点实验室重组契机，建立“学科特区+青年长聘准聘”双轨支持机制[^moe_2025][^qingta_2026]。",
        source_ids: ["esi_2026", "moe_2025", "qingta_2026"],
      },
      {
        id: "sec-esi-micro",
        type: "heading",
        level: 4,
        text: "（1）青年拔尖人才引育与标志性成果考核优化",
        source_ids: [],
      },
      {
        id: "p-action-plan",
        type: "paragraph",
        text: "针对青年科研人员在交叉学科领域的培育周期特点，将首个聘期考核窗口由 3 年延长至 **5 年长周期代表作评估**，并对获批国家自然科学基金原创探索计划项目、重点项目的团队给予 **1:1.5 校级学科建设配套经费**支持，确保“十五五”开局阶段实现高峰学科量质齐升[^moe_2025][^nsfc_2025]。",
        source_ids: ["moe_2025", "nsfc_2025"],
      },
    ],
  };

  const citationOrder: Array<keyof typeof SPECIMEN_SOURCES> = [
    "moe_2025",
    "nsfc_2025",
    "esi_2026",
    "qingta_2026",
  ];

  const authorYearLabels: Record<keyof typeof SPECIMEN_SOURCES, string> = {
    moe_2025: "(教育部等, 2025)",
    nsfc_2025: "(国家自然科学基金委员会, 2025)",
    esi_2026: "(Clarivate, 2026)",
    qingta_2026: "(青塔高等教育研究中心, 2026)",
  };

  const citation_map: Record<string, CitationMapEntry> = {};
  const bibliography: BibliographyEntry[] = [];

  citationOrder.forEach((sourceId, index) => {
    const number = index + 1;
    const source = SPECIMEN_SOURCES[sourceId];
    const isNumeric = citationStyle === "numeric";
    const label = isNumeric ? `[${number}]` : authorYearLabels[sourceId];
    citation_map[sourceId] = {
      source_id: sourceId,
      number,
      label,
      title: source.title ?? sourceId,
    };
    bibliography.push({
      source_id: sourceId,
      number,
      label,
      source,
      plain_text: `${source.title} — ${source.publisher ?? ""}`.trim(),
      excerpt: source.excerpt ?? null,
    });
  });

  const computed: ComputedDocumentMeta = {
    table_layouts: {
      "tbl-benchmark": {
        table_block_id: "tbl-benchmark",
        header_depth: 2,
        total_width_pct: 100,
        header_rows: [
          [
            {
              key: "school",
              label: "对标高校",
              colspan: 1,
              rowspan: 2,
              data_type: "text",
              align: "left",
              is_group: false,
            },
            {
              key: "tier",
              label: "建设类别",
              colspan: 1,
              rowspan: 2,
              data_type: "badge",
              align: "center",
              is_group: false,
            },
            {
              key: "grp-2025",
              label: "2025年国自然立项",
              colspan: 3,
              rowspan: 1,
              data_type: "text",
              align: "center",
              is_group: true,
            },
            {
              key: "grp-eff",
              label: "竞争效率与学科支撑",
              colspan: 2,
              rowspan: 1,
              data_type: "text",
              align: "center",
              is_group: true,
            },
          ],
          [
            {
              key: "projects",
              label: "获批项数",
              colspan: 1,
              rowspan: 1,
              data_type: "number",
              align: "right",
              is_group: false,
            },
            {
              key: "general_youth",
              label: "面上/青年",
              colspan: 1,
              rowspan: 1,
              data_type: "text",
              align: "right",
              is_group: false,
            },
            {
              key: "funding",
              label: "直接经费（万元）",
              colspan: 1,
              rowspan: 1,
              data_type: "currency",
              align: "right",
              is_group: false,
            },
            {
              key: "rate",
              label: "资助率",
              colspan: 1,
              rowspan: 1,
              data_type: "percent",
              align: "right",
              is_group: false,
            },
            {
              key: "growth",
              label: "经费增速",
              colspan: 1,
              rowspan: 1,
              data_type: "percent",
              align: "right",
              is_group: false,
            },
          ],
        ],
        leaf_columns: [
          {
            key: "school",
            label: "对标高校",
            path: ["对标高校"],
            index: 0,
            data_type: "text",
            sizing: "grow",
            weight: 1.3,
            align: "left",
            header_align: "left",
            width_pct: 22,
            width_css: "22%",
          },
          {
            key: "tier",
            label: "建设类别",
            path: ["建设类别"],
            index: 1,
            data_type: "badge",
            sizing: "fit",
            weight: 1,
            align: "center",
            header_align: "center",
            width_pct: 12,
            width_css: "12%",
          },
          {
            key: "projects",
            label: "获批项数",
            path: ["2025年国自然立项", "获批项数"],
            index: 2,
            data_type: "number",
            sizing: "fit",
            weight: 1,
            align: "right",
            header_align: "right",
            width_pct: 12,
            width_css: "12%",
          },
          {
            key: "general_youth",
            label: "面上/青年",
            path: ["2025年国自然立项", "面上/青年"],
            index: 3,
            data_type: "text",
            sizing: "fit",
            weight: 1,
            align: "right",
            header_align: "right",
            width_pct: 15,
            width_css: "15%",
          },
          {
            key: "funding",
            label: "直接经费（万元）",
            path: ["2025年国自然立项", "直接经费（万元）"],
            index: 4,
            data_type: "currency",
            sizing: "fit",
            weight: 1,
            align: "right",
            header_align: "right",
            width_pct: 17,
            width_css: "17%",
          },
          {
            key: "rate",
            label: "资助率",
            path: ["竞争效率与学科支撑", "资助率"],
            index: 5,
            data_type: "percent",
            sizing: "fit",
            weight: 1,
            align: "right",
            header_align: "right",
            width_pct: 11,
            width_css: "11%",
          },
          {
            key: "growth",
            label: "经费增速",
            path: ["竞争效率与学科支撑", "经费增速"],
            index: 6,
            data_type: "percent",
            sizing: "fit",
            weight: 1,
            align: "right",
            header_align: "right",
            width_pct: 11,
            width_css: "11%",
          },
        ],
      },
    },
    citation_map,
    bibliography,
    toc_tree: [
      {
        block_id: "sec-overview",
        level: 1,
        number: "1",
        text: "一、核心指标对标与总体态势",
        children: [
          {
            block_id: "sec-funding",
            level: 2,
            number: "1.1",
            text: "1.1 国家自然科学基金立项与经费结构对标",
            children: [],
          },
          {
            block_id: "sec-chart-analysis",
            level: 2,
            number: "1.2",
            text: "1.2 立项规模与资助率双轴演化趋势",
            children: [],
          },
        ],
      },
      {
        block_id: "sec-discipline",
        level: 1,
        number: "2",
        text: "二、高峰学科建设路径与分梯度推进策略",
        children: [
          {
            block_id: "sec-esi",
            level: 2,
            number: "2.1",
            text: "2.1 ESI 千分之一学科突破梯队与资源配置",
            children: [
              {
                block_id: "sec-esi-detail",
                level: 3,
                number: "2.1.1",
                text: "2.1.1 优势工科学科群与基础理科交叉机制",
                children: [],
              },
            ],
          },
        ],
      },
    ],
    block_framing: {
      "tbl-benchmark": {
        block_id: "tbl-benchmark",
        numbering: "table",
        number: 1,
        label: "表 1",
        title: "五所样本高校 2025 年国家自然科学基金立项与学科产出对标",
        attribution: null,
      },
      "chart-funding-trend": {
        block_id: "chart-funding-trend",
        numbering: "figure",
        number: 1,
        label: "图 1",
        title: "五所样本高校 2025 年国自然面上/青年立项结构与整体资助率对比",
        attribution: null,
      },
    },
    chart_tables: {
      "chart-funding-trend": [
        {
          title: "五所样本高校 2025 年国自然面上/青年立项结构与整体资助率对比",
          headers: [
            "高校",
            "面上项目（项）",
            "青年基金（项）",
            "综合资助率（%）",
          ],
          rows: [
            ["A 大学", "398", "286", "22.4"],
            ["B 大学", "332", "241", "21.1"],
            ["C 大学（本校）", "264", "192", "19.6"],
            ["D 工业大学", "165", "134", "17.8"],
            ["E 理工大学", "142", "128", "16.9"],
          ],
        },
      ],
    },
  };

  return { document, computed, pageChrome };
}
