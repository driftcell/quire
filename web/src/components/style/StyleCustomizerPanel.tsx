import { useState } from "react";
import {
  BookOpenCheckIcon,
  FileCode2Icon,
  Heading1Icon,
  PaletteIcon,
  RotateCcwIcon,
  Table2Icon,
  TypeIcon,
} from "lucide-react";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import type { PageChromeConfig } from "@/contracts/report-protocol";
import { PAGE_CHROME_PRESETS } from "@/lib/pageChrome";
import type { StylePresetBundle } from "@/lib/stylePresets";

export type CustomizerSectionId =
  "palette" | "typography" | "headings" | "data" | "structure" | "page";

export interface StyleCustomizerPanelProps {
  bundle: StylePresetBundle;
  onChange: (next: StylePresetBundle) => void;
  onReset: () => void;
}

const SECTIONS: ReadonlyArray<{
  id: CustomizerSectionId;
  label: string;
  icon: typeof PaletteIcon;
}> = [
  { id: "palette", label: "色彩", icon: PaletteIcon },
  { id: "typography", label: "排版", icon: TypeIcon },
  { id: "headings", label: "标题与块", icon: Heading1Icon },
  { id: "data", label: "表与图", icon: Table2Icon },
  { id: "structure", label: "封面目录", icon: BookOpenCheckIcon },
  { id: "page", label: "分页与CSS", icon: FileCode2Icon },
];

const ACCENT_SWATCHES = [
  { label: "松石绿", value: "#0F766E" },
  { label: "群青蓝", value: "#1D4ED8" },
  { label: "牛津蓝", value: "#1E3A5F" },
  { label: "深黛青", value: "#27374D" },
  { label: "朱砂红", value: "#991B1B" },
  { label: "信号橙", value: "#EA580C" },
  { label: "钴蓝图", value: "#0369A1" },
  { label: "琥珀棕", value: "#9A3412" },
  { label: "荧光翠", value: "#10B981" },
  { label: "竹青绿", value: "#2D5A4C" },
] as const;

const PAPER_SWATCHES = [
  {
    label: "纯白",
    paper: "#FFFFFF",
    surface: "#F8FAFC",
    ink: "#18181B",
    muted: "#575651",
    border: "#D6D4CD",
  },
  {
    label: "米白手札",
    paper: "#FAF8F3",
    surface: "#F2EFE7",
    ink: "#1C1917",
    muted: "#57534E",
    border: "#E7E5E4",
  },
  {
    label: "奶油画报",
    paper: "#FBF7F0",
    surface: "#F5EFE4",
    ink: "#27221D",
    muted: "#78716C",
    border: "#E7DEC8",
  },
  {
    label: "宣纸古籍",
    paper: "#F7F4EB",
    surface: "#EFECE2",
    ink: "#1F2421",
    muted: "#6B7068",
    border: "#D5CFC0",
  },
  {
    label: "工程蓝图",
    paper: "#F4F8FB",
    surface: "#E0F2FE",
    ink: "#0C4A6E",
    muted: "#475569",
    border: "#BAE6FD",
  },
  {
    label: "曜石暗夜",
    paper: "#0F141C",
    surface: "#16202E",
    ink: "#E2E8F0",
    muted: "#94A3B8",
    border: "#263345",
  },
] as const;

const CHART_PALETTE_PRESETS = [
  {
    label: "青塔牛津",
    colors: ["#1E3A5F", "#3B6E9C", "#6495B8", "#94B4CE", "#9A6B2F", "#475569"],
  },
  {
    label: "松石翠林",
    colors: ["#0F766E", "#0284C7", "#D97706", "#65A30D", "#7C3AED", "#64748B"],
  },
  {
    label: "朱砂丹宸",
    colors: ["#991B1B", "#1E3A5F", "#B45309", "#334155", "#DC2626", "#78716C"],
  },
  {
    label: "瑞士信号",
    colors: ["#EA580C", "#09090B", "#2563EB", "#71717A", "#D97706", "#0D9488"],
  },
  {
    label: "曜石霓虹",
    colors: ["#10B981", "#38BDF8", "#F59E0B", "#A78BFA", "#F43F5E", "#2DD4BF"],
  },
] as const;

const FONT_STACKS = {
  serif: '"Noto Serif SC", "Songti SC", "STSong", "SimSun", Georgia, serif',
  sans: '"Geist Variable", "PingFang SC", "Noto Sans SC", system-ui, sans-serif',
  kaiti: '"STKaiti", "Kaiti SC", "KaiTi", "Noto Serif SC", "Songti SC", serif',
  mono: '"JetBrains Mono", "Geist Mono", "PingFang SC", monospace',
} as const;

type FontCategory = keyof typeof FONT_STACKS;

function isFontCategory(value: string | undefined): value is FontCategory {
  return (
    value === "serif" ||
    value === "sans" ||
    value === "kaiti" ||
    value === "mono"
  );
}

function detectFontCategory(stack: string): FontCategory {
  if (stack.includes("Mono") || stack.includes("monospace")) return "mono";
  if (stack.includes("Kaiti") || stack.includes("KaiTi")) return "kaiti";
  if (stack.includes("Serif") || stack.includes("Song")) return "serif";
  return "sans";
}

function safeHexForPicker(raw: string, fallback: string): string {
  return /^#[0-9A-Fa-f]{6}$/.test(raw.trim()) ? raw.trim() : fallback;
}

const PLACEHOLDERS = [
  "{page}",
  "{total}",
  "{title}",
  "{kicker}",
  "{authors}",
  "{date}",
] as const;

export function StyleCustomizerPanel({
  bundle,
  onChange,
  onReset,
}: StyleCustomizerPanelProps) {
  const [activeSection, setActiveSection] =
    useState<CustomizerSectionId>("palette");
  const [activeChromeSlot, setActiveChromeSlot] = useState<{
    group: "header" | "footer";
    slot: "left" | "center" | "right";
  } | null>(null);

  const tokens = bundle.theme.tokens ?? {};
  const accent = tokens["--rpt-accent"] ?? "#1E3A5F";
  const ink = tokens["--rpt-ink"] ?? "#18181B";
  const muted = tokens["--rpt-muted"] ?? "#575651";
  const paper = tokens["--rpt-paper"] ?? "#FFFFFF";
  const surface = tokens["--rpt-surface"] ?? "#F7F6F2";
  const border = tokens["--rpt-border"] ?? "#D6D4CD";
  const quoteBg = tokens["--rpt-quote-bg"] ?? "#F8FAFC";
  const bodySize = tokens["--rpt-body-size"] ?? "14.5px";
  const h1Size = tokens["--rpt-h1-size"] ?? "22px";
  const headingWeight = tokens["--rpt-heading-weight"] ?? "700";
  const headingTracking = tokens["--rpt-heading-tracking"] ?? "0em";
  const lineHeight = tokens["--rpt-line-height"] ?? "1.82";
  const paraGap = tokens["--rpt-para-gap"] ?? "0.75rem";
  const paraIndent = tokens["--rpt-para-indent"] ?? "0em";
  const paraAlign = tokens["--rpt-para-align"] ?? "left";
  const radius = tokens["--rpt-radius"] ?? "6px";

  const h1Variant = tokens["--rpt-h1-variant"] ?? "left-pillar";
  const h2Variant = tokens["--rpt-h2-variant"] ?? "left-bar";
  const calloutVariant = tokens["--rpt-callout-variant"] ?? "left-accent";
  const quoteVariant = tokens["--rpt-quote-variant"] ?? "left-rule";
  const tableVariant = tokens["--rpt-table-variant"] ?? "three-line";
  const tableDensity = tokens["--rpt-table-density"] ?? "standard";
  const tableBorder = tokens["--rpt-table-border"] ?? "#1E293B";
  const tableHeaderBg = tokens["--rpt-table-header-bg"] ?? "#F1F5F9";
  const tableStripe = tokens["--rpt-table-stripe"] ?? "#F8FAFC";

  const headingFontCat = detectFontCategory(
    tokens["--rpt-font-heading"] ?? FONT_STACKS.serif,
  );
  const bodyFontCat = detectFontCategory(
    tokens["--rpt-font-body"] ?? FONT_STACKS.sans,
  );

  const patchTokens = (patch: Record<string, string>) => {
    onChange({
      ...bundle,
      theme: {
        ...bundle.theme,
        tokens: {
          ...tokens,
          ...patch,
        },
      },
    });
  };

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

  const patchPageChrome = (patch: Partial<PageChromeConfig>) => {
    onChange({
      ...bundle,
      theme: {
        ...bundle.theme,
        page_chrome: {
          ...pageChrome,
          ...patch,
          header: {
            ...pageChrome.header,
            ...(patch.header ?? {}),
          },
          footer: {
            ...pageChrome.footer,
            ...(patch.footer ?? {}),
          },
        },
      },
    });
  };

  return (
    <div className="space-y-3.5 text-xs">
      {/* Partition Segmented Navigation Header */}
      <div className="flex items-center justify-between gap-2">
        <div
          role="tablist"
          aria-label="样式调优分区"
          className="grid flex-1 grid-cols-3 gap-1 rounded-lg border border-border/85 bg-muted/45 p-1"
        >
          {SECTIONS.map((sec) => {
            const Icon = sec.icon;
            const active = activeSection === sec.id;
            return (
              <button
                key={sec.id}
                type="button"
                role="tab"
                aria-selected={active}
                onClick={() => setActiveSection(sec.id)}
                className={`flex items-center justify-center gap-1 rounded-md px-2 py-1.5 text-[11px] transition-colors focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none ${
                  active
                    ? "bg-card font-semibold text-foreground shadow-2xs"
                    : "text-muted-foreground hover:text-foreground"
                }`}
              >
                <Icon className="size-3 shrink-0" aria-hidden="true" />
                <span className="truncate">{sec.label}</span>
              </button>
            );
          })}
        </div>

        <button
          type="button"
          onClick={onReset}
          title="重置当前预设修改"
          className="inline-flex shrink-0 items-center gap-1 rounded-md border border-border/80 bg-card px-2 py-1.5 text-[11px] text-muted-foreground transition-colors hover:bg-muted hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none"
        >
          <RotateCcwIcon className="size-3" aria-hidden="true" />
          重置
        </button>
      </div>

      {/* Partition 1: Palette & Surface */}
      {activeSection === "palette" && (
        <div className="space-y-3.5">
          <section className="space-y-2 rounded-lg border border-border/75 bg-card/60 p-3">
            <div className="flex items-center justify-between">
              <Label htmlFor="style-accent-hex" className="text-xs font-medium">
                主强调色
              </Label>
              <div className="flex items-center gap-1.5">
                <input
                  id="style-accent-picker"
                  type="color"
                  aria-label="主强调色拾色器"
                  value={safeHexForPicker(accent, "#1E3A5F")}
                  onChange={(e) =>
                    patchTokens({
                      "--rpt-accent": e.target.value,
                      "--rpt-chart-1": e.target.value,
                    })
                  }
                  className="size-6 cursor-pointer rounded border border-border bg-transparent p-0.5"
                />
                <Input
                  id="style-accent-hex"
                  name="style-accent-hex"
                  autoComplete="off"
                  spellCheck={false}
                  value={accent}
                  onChange={(e) =>
                    patchTokens({
                      "--rpt-accent": e.target.value,
                      "--rpt-chart-1": e.target.value,
                    })
                  }
                  className="h-6 w-21 font-mono text-[11px] uppercase tabular-nums"
                />
              </div>
            </div>

            <div className="grid grid-cols-5 gap-1.5 pt-0.5">
              {ACCENT_SWATCHES.map((swatch) => {
                const active =
                  accent.toLowerCase() === swatch.value.toLowerCase();
                return (
                  <button
                    key={swatch.value}
                    type="button"
                    onClick={() =>
                      patchTokens({
                        "--rpt-accent": swatch.value,
                        "--rpt-chart-1": swatch.value,
                      })
                    }
                    className={`flex items-center gap-1.5 rounded-md border px-2 py-1 text-[11px] transition-colors ${
                      active
                        ? "border-foreground bg-secondary font-medium text-foreground"
                        : "border-border/80 bg-background text-muted-foreground hover:text-foreground"
                    }`}
                  >
                    <span
                      style={{ backgroundColor: swatch.value }}
                      className="size-2.5 shrink-0 rounded-full ring-1 ring-black/15"
                    />
                    <span className="truncate">{swatch.label}</span>
                  </button>
                );
              })}
            </div>
          </section>

          <section className="space-y-2 rounded-lg border border-border/75 bg-card/60 p-3">
            <Label className="text-xs font-medium">纸张氛围预设</Label>
            <div className="grid grid-cols-3 gap-1.5">
              {PAPER_SWATCHES.map((item) => {
                const active = paper.toLowerCase() === item.paper.toLowerCase();
                return (
                  <button
                    key={item.label}
                    type="button"
                    onClick={() =>
                      patchTokens({
                        "--rpt-paper": item.paper,
                        "--rpt-surface": item.surface,
                        "--rpt-ink": item.ink,
                        "--rpt-muted": item.muted,
                        "--rpt-border": item.border,
                      })
                    }
                    className={`flex items-center gap-1.5 rounded-md border px-2 py-1.5 text-[11px] transition-colors ${
                      active
                        ? "border-[#1D4ED8] bg-[#1D4ED8]/8 font-medium text-foreground"
                        : "border-border/80 bg-background text-muted-foreground hover:text-foreground"
                    }`}
                  >
                    <span
                      style={{ backgroundColor: item.paper }}
                      className="size-3 shrink-0 rounded-full ring-1 ring-black/25"
                    />
                    <span className="truncate">{item.label}</span>
                  </button>
                );
              })}
            </div>

            <div className="grid grid-cols-2 gap-2 pt-1.5">
              {(
                [
                  {
                    key: "--rpt-paper",
                    label: "纸面底色",
                    val: paper,
                    fb: "#FFFFFF",
                  },
                  {
                    key: "--rpt-surface",
                    label: "容器底色",
                    val: surface,
                    fb: "#F7F6F2",
                  },
                  {
                    key: "--rpt-ink",
                    label: "正文墨色",
                    val: ink,
                    fb: "#18181B",
                  },
                  {
                    key: "--rpt-muted",
                    label: "次要字色",
                    val: muted,
                    fb: "#575651",
                  },
                  {
                    key: "--rpt-border",
                    label: "分割线色",
                    val: border,
                    fb: "#D6D4CD",
                  },
                  {
                    key: "--rpt-quote-bg",
                    label: "引文底色",
                    val: quoteBg,
                    fb: "#F8FAFC",
                  },
                ] as const
              ).map((item) => (
                <div key={item.key} className="space-y-1">
                  <span className="text-[11px] text-muted-foreground">
                    {item.label}
                  </span>
                  <div className="flex items-center gap-1">
                    <input
                      type="color"
                      aria-label={item.label}
                      value={safeHexForPicker(item.val, item.fb)}
                      onChange={(e) =>
                        patchTokens({ [item.key]: e.target.value })
                      }
                      className="size-6 shrink-0 cursor-pointer rounded border border-border bg-transparent p-0.5"
                    />
                    <Input
                      aria-label={`${item.label} Hex`}
                      value={item.val}
                      onChange={(e) =>
                        patchTokens({ [item.key]: e.target.value })
                      }
                      className="h-6 flex-1 font-mono text-[11px] uppercase tabular-nums"
                    />
                  </div>
                </div>
              ))}
            </div>
          </section>
        </div>
      )}

      {/* Partition 2: Typography & Paragraph Layout */}
      {activeSection === "typography" && (
        <div className="space-y-3.5">
          <section className="space-y-2.5 rounded-lg border border-border/75 bg-card/60 p-3">
            <div className="space-y-1.5">
              <Label className="text-xs font-medium">标题字族</Label>
              <ToggleGroup
                multiple={false}
                value={[headingFontCat]}
                onValueChange={(values) => {
                  const v = values[0];
                  if (isFontCategory(v)) {
                    patchTokens({ "--rpt-font-heading": FONT_STACKS[v] });
                  }
                }}
                spacing={0}
                className="w-full rounded-md border bg-muted/40 p-0.5"
              >
                {[
                  { label: "宋体衬线", value: "serif" },
                  { label: "现代黑体", value: "sans" },
                  { label: "古典楷体", value: "kaiti" },
                  { label: "技术等宽", value: "mono" },
                ].map((opt) => (
                  <ToggleGroupItem
                    key={opt.value}
                    value={opt.value}
                    size="sm"
                    className="h-6 flex-1 rounded-xs text-[11px] aria-pressed:bg-background aria-pressed:font-medium aria-pressed:text-foreground aria-pressed:shadow-2xs"
                  >
                    {opt.label}
                  </ToggleGroupItem>
                ))}
              </ToggleGroup>
            </div>

            <div className="space-y-1.5">
              <Label className="text-xs font-medium">正文字族</Label>
              <ToggleGroup
                multiple={false}
                value={[bodyFontCat]}
                onValueChange={(values) => {
                  const v = values[0];
                  if (isFontCategory(v)) {
                    patchTokens({ "--rpt-font-body": FONT_STACKS[v] });
                  }
                }}
                spacing={0}
                className="w-full rounded-md border bg-muted/40 p-0.5"
              >
                {[
                  { label: "现代黑体", value: "sans" },
                  { label: "宋体衬线", value: "serif" },
                  { label: "古典楷体", value: "kaiti" },
                  { label: "技术等宽", value: "mono" },
                ].map((opt) => (
                  <ToggleGroupItem
                    key={opt.value}
                    value={opt.value}
                    size="sm"
                    className="h-6 flex-1 rounded-xs text-[11px] aria-pressed:bg-background aria-pressed:font-medium aria-pressed:text-foreground aria-pressed:shadow-2xs"
                  >
                    {opt.label}
                  </ToggleGroupItem>
                ))}
              </ToggleGroup>
            </div>

            <div className="grid grid-cols-2 gap-2.5">
              <div className="space-y-1.5">
                <Label className="text-xs font-medium">一级标题字号</Label>
                <ToggleGroup
                  multiple={false}
                  value={[h1Size]}
                  onValueChange={(values) => {
                    const next = values[0];
                    if (next) patchTokens({ "--rpt-h1-size": next });
                  }}
                  spacing={0}
                  className="w-full rounded-md border bg-muted/40 p-0.5"
                >
                  {["20px", "22px", "24px", "26px"].map((sz) => (
                    <ToggleGroupItem
                      key={sz}
                      value={sz}
                      size="sm"
                      className="h-6 flex-1 rounded-xs font-mono text-[10.5px] tabular-nums aria-pressed:bg-background aria-pressed:font-medium aria-pressed:text-foreground aria-pressed:shadow-2xs"
                    >
                      {sz.replace("px", "")}
                    </ToggleGroupItem>
                  ))}
                </ToggleGroup>
              </div>

              <div className="space-y-1.5">
                <Label className="text-xs font-medium">标题字重</Label>
                <ToggleGroup
                  multiple={false}
                  value={[headingWeight]}
                  onValueChange={(values) => {
                    const next = values[0];
                    if (next) patchTokens({ "--rpt-heading-weight": next });
                  }}
                  spacing={0}
                  className="w-full rounded-md border bg-muted/40 p-0.5"
                >
                  {[
                    { label: "半粗", value: "600" },
                    { label: "粗体", value: "700" },
                    { label: "特粗", value: "800" },
                  ].map((opt) => (
                    <ToggleGroupItem
                      key={opt.value}
                      value={opt.value}
                      size="sm"
                      className="h-6 flex-1 rounded-xs text-[11px] aria-pressed:bg-background aria-pressed:font-medium aria-pressed:text-foreground aria-pressed:shadow-2xs"
                    >
                      {opt.label}
                    </ToggleGroupItem>
                  ))}
                </ToggleGroup>
              </div>
            </div>

            <div className="space-y-1.5">
              <Label className="text-xs font-medium">标题字距</Label>
              <ToggleGroup
                multiple={false}
                value={[headingTracking]}
                onValueChange={(values) => {
                  const next = values[0];
                  if (next) patchTokens({ "--rpt-heading-tracking": next });
                }}
                spacing={0}
                className="w-full rounded-md border bg-muted/40 p-0.5"
              >
                {[
                  { label: "紧凑", value: "-0.02em" },
                  { label: "标准", value: "0em" },
                  { label: "舒展", value: "0.04em" },
                  { label: "宽距", value: "0.08em" },
                ].map((opt) => (
                  <ToggleGroupItem
                    key={opt.value}
                    value={opt.value}
                    size="sm"
                    className="h-6 flex-1 rounded-xs text-[11px] aria-pressed:bg-background aria-pressed:font-medium aria-pressed:text-foreground aria-pressed:shadow-2xs"
                  >
                    {opt.label}
                  </ToggleGroupItem>
                ))}
              </ToggleGroup>
            </div>
          </section>

          <section className="space-y-2.5 rounded-lg border border-border/75 bg-card/60 p-3">
            <div className="space-y-1.5">
              <Label className="text-xs font-medium">正文字号</Label>
              <ToggleGroup
                multiple={false}
                value={[bodySize]}
                onValueChange={(values) => {
                  const next = values[0];
                  if (next) patchTokens({ "--rpt-body-size": next });
                }}
                spacing={0}
                className="w-full rounded-md border bg-muted/40 p-0.5"
              >
                {[
                  { label: "13.5", value: "13.5px" },
                  { label: "14", value: "14px" },
                  { label: "14.5", value: "14.5px" },
                  { label: "15", value: "15px" },
                  { label: "16", value: "16px" },
                ].map((opt) => (
                  <ToggleGroupItem
                    key={opt.value}
                    value={opt.value}
                    size="sm"
                    className="h-6 flex-1 rounded-xs font-mono text-[11px] tabular-nums aria-pressed:bg-background aria-pressed:font-medium aria-pressed:text-foreground aria-pressed:shadow-2xs"
                  >
                    {opt.label}
                  </ToggleGroupItem>
                ))}
              </ToggleGroup>
            </div>

            <div className="space-y-1.5">
              <Label className="text-xs font-medium">段落行高</Label>
              <ToggleGroup
                multiple={false}
                value={[lineHeight]}
                onValueChange={(values) => {
                  const next = values[0];
                  if (next) patchTokens({ "--rpt-line-height": next });
                }}
                spacing={0}
                className="w-full rounded-md border bg-muted/40 p-0.5"
              >
                {["1.68", "1.76", "1.82", "1.92", "2.05"].map((lh) => (
                  <ToggleGroupItem
                    key={lh}
                    value={lh}
                    size="sm"
                    className="h-6 flex-1 rounded-xs font-mono text-[11px] tabular-nums aria-pressed:bg-background aria-pressed:font-medium aria-pressed:text-foreground aria-pressed:shadow-2xs"
                  >
                    {lh}
                  </ToggleGroupItem>
                ))}
              </ToggleGroup>
            </div>

            <div className="space-y-1.5">
              <Label className="text-xs font-medium">段落间距</Label>
              <ToggleGroup
                multiple={false}
                value={[paraGap]}
                onValueChange={(values) => {
                  const next = values[0];
                  if (next) patchTokens({ "--rpt-para-gap": next });
                }}
                spacing={0}
                className="w-full rounded-md border bg-muted/40 p-0.5"
              >
                {[
                  { label: "紧密", value: "0.55rem" },
                  { label: "标准", value: "0.75rem" },
                  { label: "疏朗", value: "1.05rem" },
                  { label: "呼吸", value: "1.35rem" },
                ].map((opt) => (
                  <ToggleGroupItem
                    key={opt.value}
                    value={opt.value}
                    size="sm"
                    className="h-6 flex-1 rounded-xs text-[11px] aria-pressed:bg-background aria-pressed:font-medium aria-pressed:text-foreground aria-pressed:shadow-2xs"
                  >
                    {opt.label}
                  </ToggleGroupItem>
                ))}
              </ToggleGroup>
            </div>

            <div className="grid grid-cols-2 gap-2.5">
              <div className="space-y-1.5">
                <Label className="text-xs font-medium">首行缩进</Label>
                <ToggleGroup
                  multiple={false}
                  value={[paraIndent]}
                  onValueChange={(values) => {
                    const next = values[0];
                    if (next) patchTokens({ "--rpt-para-indent": next });
                  }}
                  spacing={0}
                  className="w-full rounded-md border bg-muted/40 p-0.5"
                >
                  <ToggleGroupItem
                    value="0em"
                    size="sm"
                    className="h-6 flex-1 rounded-xs text-[11px] aria-pressed:bg-background aria-pressed:font-medium aria-pressed:text-foreground aria-pressed:shadow-2xs"
                  >
                    无缩进
                  </ToggleGroupItem>
                  <ToggleGroupItem
                    value="2em"
                    size="sm"
                    className="h-6 flex-1 rounded-xs text-[11px] aria-pressed:bg-background aria-pressed:font-medium aria-pressed:text-foreground aria-pressed:shadow-2xs"
                  >
                    缩进2字
                  </ToggleGroupItem>
                </ToggleGroup>
              </div>

              <div className="space-y-1.5">
                <Label className="text-xs font-medium">段落对齐</Label>
                <ToggleGroup
                  multiple={false}
                  value={[paraAlign]}
                  onValueChange={(values) => {
                    const next = values[0];
                    if (next) patchTokens({ "--rpt-para-align": next });
                  }}
                  spacing={0}
                  className="w-full rounded-md border bg-muted/40 p-0.5"
                >
                  <ToggleGroupItem
                    value="left"
                    size="sm"
                    className="h-6 flex-1 rounded-xs text-[11px] aria-pressed:bg-background aria-pressed:font-medium aria-pressed:text-foreground aria-pressed:shadow-2xs"
                  >
                    左对齐
                  </ToggleGroupItem>
                  <ToggleGroupItem
                    value="justify"
                    size="sm"
                    className="h-6 flex-1 rounded-xs text-[11px] aria-pressed:bg-background aria-pressed:font-medium aria-pressed:text-foreground aria-pressed:shadow-2xs"
                  >
                    两端对齐
                  </ToggleGroupItem>
                </ToggleGroup>
              </div>
            </div>
          </section>
        </div>
      )}

      {/* Partition 3: Heading Architecture & Block Ornamentation */}
      {activeSection === "headings" && (
        <div className="space-y-3.5">
          <section className="space-y-2 rounded-lg border border-border/75 bg-card/60 p-3">
            <Label className="text-xs font-medium">
              一级标题（H1）视觉架构
            </Label>
            <div className="grid grid-cols-2 gap-1.5">
              {[
                { label: "纯净无底线", value: "none" },
                { label: "顶冠短横线", value: "top-rule" },
                { label: "左侧厚立柱", value: "left-pillar" },
                { label: "满幅反白横幅", value: "pill-banner" },
                { label: "居中卷首刊头", value: "center-ornament" },
                { label: "公文文武双线", value: "double-decree" },
                { label: "经典单底线", value: "bottom-rule" },
              ].map((opt) => {
                const active = h1Variant === opt.value;
                return (
                  <button
                    key={opt.value}
                    type="button"
                    onClick={() =>
                      patchTokens({ "--rpt-h1-variant": opt.value })
                    }
                    className={`rounded-md border px-2.5 py-1.5 text-left text-[11px] transition-colors ${
                      active
                        ? "border-[#1D4ED8] bg-[#1D4ED8]/8 font-medium text-foreground"
                        : "border-border/80 bg-background text-muted-foreground hover:text-foreground"
                    }`}
                  >
                    {opt.label}
                  </button>
                );
              })}
            </div>
          </section>

          <section className="space-y-2 rounded-lg border border-border/75 bg-card/60 p-3">
            <Label className="text-xs font-medium">
              二级标题（H2）视觉架构
            </Label>
            <div className="grid grid-cols-2 gap-1.5">
              {[
                { label: "纯文字无框线", value: "none" },
                { label: "左侧单立线", value: "left-bar" },
                { label: "柔和胶囊色块", value: "soft-pill" },
                { label: "几何方标前缀", value: "dot-prefix" },
                { label: "贯穿粗顶线", value: "top-rule" },
                { label: "手札细虚线", value: "bottom-dashed" },
                { label: "经典细底线", value: "bottom-line" },
              ].map((opt) => {
                const active = h2Variant === opt.value;
                return (
                  <button
                    key={opt.value}
                    type="button"
                    onClick={() =>
                      patchTokens({ "--rpt-h2-variant": opt.value })
                    }
                    className={`rounded-md border px-2.5 py-1.5 text-left text-[11px] transition-colors ${
                      active
                        ? "border-[#1D4ED8] bg-[#1D4ED8]/8 font-medium text-foreground"
                        : "border-border/80 bg-background text-muted-foreground hover:text-foreground"
                    }`}
                  >
                    {opt.label}
                  </button>
                );
              })}
            </div>
          </section>

          <section className="space-y-2.5 rounded-lg border border-border/75 bg-card/60 p-3">
            <div className="space-y-1.5">
              <Label className="text-xs font-medium">
                结论提示块（Callout）
              </Label>
              <ToggleGroup
                multiple={false}
                value={[calloutVariant]}
                onValueChange={(values) => {
                  const next = values[0];
                  if (next) patchTokens({ "--rpt-callout-variant": next });
                }}
                spacing={0}
                className="w-full rounded-md border bg-muted/40 p-0.5"
              >
                {[
                  { label: "左立线", value: "left-accent" },
                  { label: "柔和块", value: "solid-tint" },
                  { label: "顶色条", value: "top-banner" },
                  { label: "精密框", value: "outline-frame" },
                ].map((opt) => (
                  <ToggleGroupItem
                    key={opt.value}
                    value={opt.value}
                    size="sm"
                    className="h-6 flex-1 rounded-xs text-[11px] aria-pressed:bg-background aria-pressed:font-medium aria-pressed:text-foreground aria-pressed:shadow-2xs"
                  >
                    {opt.label}
                  </ToggleGroupItem>
                ))}
              </ToggleGroup>
            </div>

            <div className="space-y-1.5">
              <Label className="text-xs font-medium">文献引文块（Quote）</Label>
              <ToggleGroup
                multiple={false}
                value={[quoteVariant]}
                onValueChange={(values) => {
                  const next = values[0];
                  if (next) patchTokens({ "--rpt-quote-variant": next });
                }}
                spacing={0}
                className="w-full rounded-md border bg-muted/40 p-0.5"
              >
                {[
                  { label: "经典左线", value: "left-rule" },
                  { label: "画报大引号", value: "editorial-mark" },
                  { label: "四周细线框", value: "boxed" },
                ].map((opt) => (
                  <ToggleGroupItem
                    key={opt.value}
                    value={opt.value}
                    size="sm"
                    className="h-6 flex-1 rounded-xs text-[11px] aria-pressed:bg-background aria-pressed:font-medium aria-pressed:text-foreground aria-pressed:shadow-2xs"
                  >
                    {opt.label}
                  </ToggleGroupItem>
                ))}
              </ToggleGroup>
            </div>

            <div className="space-y-1.5">
              <Label className="text-xs font-medium">容器与卡片圆角</Label>
              <ToggleGroup
                multiple={false}
                value={[radius]}
                onValueChange={(values) => {
                  const next = values[0];
                  if (next) patchTokens({ "--rpt-radius": next });
                }}
                spacing={0}
                className="w-full rounded-md border bg-muted/40 p-0.5"
              >
                {[
                  { label: "锐利直角 (0px)", value: "0px" },
                  { label: "现代微圆 (6px)", value: "6px" },
                  { label: "柔和胶囊 (12px)", value: "12px" },
                ].map((opt) => (
                  <ToggleGroupItem
                    key={opt.value}
                    value={opt.value}
                    size="sm"
                    className="h-6 flex-1 rounded-xs text-[11px] aria-pressed:bg-background aria-pressed:font-medium aria-pressed:text-foreground aria-pressed:shadow-2xs"
                  >
                    {opt.label}
                  </ToggleGroupItem>
                ))}
              </ToggleGroup>
            </div>
          </section>
        </div>
      )}

      {/* Partition 4: Table & ECharts Palette */}
      {activeSection === "data" && (
        <div className="space-y-3.5">
          <section className="space-y-2.5 rounded-lg border border-border/75 bg-card/60 p-3">
            <div className="space-y-1.5">
              <Label className="text-xs font-medium">表格框线形态</Label>
              <ToggleGroup
                multiple={false}
                value={[tableVariant]}
                onValueChange={(values) => {
                  const next = values[0];
                  if (!next) return;
                  if (next === "three-line") {
                    patchTokens({
                      "--rpt-table-variant": "three-line",
                      "--rpt-table-stripe": paper,
                    });
                  } else if (next === "striped") {
                    patchTokens({
                      "--rpt-table-variant": "striped",
                      "--rpt-table-header-bg": surface,
                      "--rpt-table-stripe": surface,
                    });
                  } else {
                    patchTokens({ "--rpt-table-variant": next });
                  }
                }}
                spacing={0}
                className="w-full rounded-md border bg-muted/40 p-0.5"
              >
                {[
                  { label: "标准三线", value: "three-line" },
                  { label: "斑马纹", value: "striped" },
                  { label: "全框网格", value: "grid" },
                  { label: "极简细线", value: "minimal" },
                ].map((opt) => (
                  <ToggleGroupItem
                    key={opt.value}
                    value={opt.value}
                    size="sm"
                    className="h-6 flex-1 rounded-xs text-[11px] aria-pressed:bg-background aria-pressed:font-medium aria-pressed:text-foreground aria-pressed:shadow-2xs"
                  >
                    {opt.label}
                  </ToggleGroupItem>
                ))}
              </ToggleGroup>
            </div>

            <div className="space-y-1.5">
              <Label className="text-xs font-medium">表格行高密度</Label>
              <ToggleGroup
                multiple={false}
                value={[tableDensity]}
                onValueChange={(values) => {
                  const next = values[0];
                  if (next) patchTokens({ "--rpt-table-density": next });
                }}
                spacing={0}
                className="w-full rounded-md border bg-muted/40 p-0.5"
              >
                {[
                  { label: "紧凑", value: "compact" },
                  { label: "标准", value: "standard" },
                  { label: "宽松", value: "relaxed" },
                ].map((opt) => (
                  <ToggleGroupItem
                    key={opt.value}
                    value={opt.value}
                    size="sm"
                    className="h-6 flex-1 rounded-xs text-[11px] aria-pressed:bg-background aria-pressed:font-medium aria-pressed:text-foreground aria-pressed:shadow-2xs"
                  >
                    {opt.label}
                  </ToggleGroupItem>
                ))}
              </ToggleGroup>
            </div>

            <div className="grid grid-cols-3 gap-2 pt-1">
              {(
                [
                  {
                    key: "--rpt-table-border",
                    label: "表框线色",
                    val: tableBorder,
                    fb: "#1E293B",
                  },
                  {
                    key: "--rpt-table-header-bg",
                    label: "表头底色",
                    val: tableHeaderBg,
                    fb: "#F1F5F9",
                  },
                  {
                    key: "--rpt-table-stripe",
                    label: "隔行底色",
                    val: tableStripe,
                    fb: "#F8FAFC",
                  },
                ] as const
              ).map((item) => (
                <div key={item.key} className="space-y-1">
                  <span className="text-[11px] text-muted-foreground">
                    {item.label}
                  </span>
                  <div className="flex items-center gap-1">
                    <input
                      type="color"
                      aria-label={item.label}
                      value={safeHexForPicker(item.val, item.fb)}
                      onChange={(e) =>
                        patchTokens({ [item.key]: e.target.value })
                      }
                      className="size-6 shrink-0 cursor-pointer rounded border border-border bg-transparent p-0.5"
                    />
                    <Input
                      aria-label={`${item.label} Hex`}
                      value={item.val}
                      onChange={(e) =>
                        patchTokens({ [item.key]: e.target.value })
                      }
                      className="h-6 min-w-0 flex-1 px-1.5 font-mono text-[10px] uppercase tabular-nums"
                    />
                  </div>
                </div>
              ))}
            </div>
          </section>

          <section className="space-y-2.5 rounded-lg border border-border/75 bg-card/60 p-3">
            <Label className="text-xs font-medium">
              ECharts 6 色图表调色盘
            </Label>
            <div className="flex flex-wrap gap-1.5">
              {CHART_PALETTE_PRESETS.map((pal) => (
                <button
                  key={pal.label}
                  type="button"
                  onClick={() =>
                    patchTokens({
                      "--rpt-chart-1": pal.colors[0],
                      "--rpt-chart-2": pal.colors[1],
                      "--rpt-chart-3": pal.colors[2],
                      "--rpt-chart-4": pal.colors[3],
                      "--rpt-chart-5": pal.colors[4],
                      "--rpt-chart-6": pal.colors[5],
                    })
                  }
                  className="inline-flex items-center gap-1.5 rounded-md border border-border/80 bg-background px-2 py-1 text-[11px] text-muted-foreground transition-colors hover:border-foreground/40 hover:text-foreground"
                >
                  <span className="flex -space-x-1">
                    {pal.colors.slice(0, 3).map((c) => (
                      <span
                        key={c}
                        style={{ backgroundColor: c }}
                        className="size-2.5 rounded-full ring-1 ring-black/15"
                      />
                    ))}
                  </span>
                  <span>{pal.label}</span>
                </button>
              ))}
            </div>

            <div className="grid grid-cols-3 gap-2 pt-1">
              {([1, 2, 3, 4, 5, 6] as const).map((idx) => {
                const key = `--rpt-chart-${idx}`;
                const val = tokens[key] ?? accent;
                return (
                  <div key={key} className="space-y-1">
                    <span className="font-mono text-[10.5px] text-muted-foreground">
                      序列 {idx}
                    </span>
                    <div className="flex items-center gap-1">
                      <input
                        type="color"
                        aria-label={`图表序列 ${idx} 颜色`}
                        value={safeHexForPicker(val, "#1E3A5F")}
                        onChange={(e) => patchTokens({ [key]: e.target.value })}
                        className="size-6 shrink-0 cursor-pointer rounded border border-border bg-transparent p-0.5"
                      />
                      <Input
                        aria-label={`图表序列 ${idx} Hex`}
                        value={val}
                        onChange={(e) => patchTokens({ [key]: e.target.value })}
                        className="h-6 min-w-0 flex-1 px-1.5 font-mono text-[10px] uppercase tabular-nums"
                      />
                    </div>
                  </div>
                );
              })}
            </div>
          </section>
        </div>
      )}

      {/* Partition 5: Cover, Inline TOC & References (Consolidated from Toolbar TOC Button) */}
      {activeSection === "structure" && (
        <div className="space-y-3.5">
          {/* Cover Settings */}
          <section className="space-y-2.5 rounded-lg border border-border/75 bg-card/60 p-3">
            <div className="flex items-center justify-between">
              <Label className="text-xs font-medium">报告封面</Label>
              <Switch
                size="sm"
                checked={bundle.cover_enabled ?? true}
                onCheckedChange={(checked) =>
                  onChange({ ...bundle, cover_enabled: checked })
                }
                aria-label="显示报告封面"
              />
            </div>

            {(bundle.cover_enabled ?? true) && (
              <div className="space-y-1.5">
                <span className="text-[11px] text-muted-foreground">
                  封面版式
                </span>
                <ToggleGroup
                  multiple={false}
                  value={[bundle.cover_variant ?? "split"]}
                  onValueChange={(values) => {
                    const next = values[0];
                    if (next) onChange({ ...bundle, cover_variant: next });
                  }}
                  spacing={0}
                  className="w-full rounded-md border bg-muted/40 p-0.5"
                >
                  {[
                    { label: "极简", value: "minimal" },
                    { label: "现代竖线", value: "modern" },
                    { label: "双栏构成", value: "split" },
                    { label: "庄重居中", value: "classic" },
                  ].map((opt) => (
                    <ToggleGroupItem
                      key={opt.value}
                      value={opt.value}
                      size="sm"
                      className="h-6 flex-1 rounded-xs text-[11px] aria-pressed:bg-background aria-pressed:font-medium aria-pressed:text-foreground aria-pressed:shadow-2xs"
                    >
                      {opt.label}
                    </ToggleGroupItem>
                  ))}
                </ToggleGroup>
              </div>
            )}
          </section>

          {/* Inline TOC Settings — Moved from Toolbar Outline Button */}
          <section className="space-y-2.5 rounded-lg border border-border/75 bg-card/60 p-3">
            <div className="flex items-center justify-between">
              <Label className="text-xs font-medium">内嵌目录</Label>
              <Switch
                size="sm"
                checked={bundle.toc_enabled ?? true}
                onCheckedChange={(checked) =>
                  onChange({ ...bundle, toc_enabled: checked })
                }
                aria-label="显示内嵌目录"
              />
            </div>

            {(bundle.toc_enabled ?? true) && (
              <>
                <div className="space-y-1.5">
                  <span className="text-[11px] text-muted-foreground">
                    目录样式
                  </span>
                  <ToggleGroup
                    multiple={false}
                    value={[bundle.toc_variant ?? "dotted"]}
                    onValueChange={(values) => {
                      const next = values[0];
                      if (next) onChange({ ...bundle, toc_variant: next });
                    }}
                    spacing={0}
                    className="w-full rounded-md border bg-muted/40 p-0.5"
                  >
                    {[
                      { label: "点线页码", value: "dotted" },
                      { label: "章节编号", value: "numbered" },
                      { label: "简洁无号", value: "standard" },
                    ].map((opt) => (
                      <ToggleGroupItem
                        key={opt.value}
                        value={opt.value}
                        size="sm"
                        className="h-6 flex-1 rounded-xs text-[11px] aria-pressed:bg-background aria-pressed:font-medium aria-pressed:text-foreground aria-pressed:shadow-2xs"
                      >
                        {opt.label}
                      </ToggleGroupItem>
                    ))}
                  </ToggleGroup>
                </div>

                <div className="space-y-1.5">
                  <span className="text-[11px] text-muted-foreground">
                    层级深度
                  </span>
                  <ToggleGroup
                    multiple={false}
                    value={[String(bundle.toc_max_depth ?? 3)]}
                    onValueChange={(values) => {
                      const v = Number(values[0]);
                      if (v === 1 || v === 2 || v === 3) {
                        onChange({ ...bundle, toc_max_depth: v });
                      }
                    }}
                    spacing={0}
                    className="w-full rounded-md border bg-muted/40 p-0.5"
                  >
                    {([1, 2, 3] as const).map((depth) => (
                      <ToggleGroupItem
                        key={depth}
                        value={String(depth)}
                        size="sm"
                        className="h-6 flex-1 rounded-xs text-[11px] tabular-nums aria-pressed:bg-background aria-pressed:font-medium aria-pressed:text-foreground aria-pressed:shadow-2xs"
                      >
                        {depth} 级标题
                      </ToggleGroupItem>
                    ))}
                  </ToggleGroup>
                </div>

                <div className="grid grid-cols-2 gap-2 pt-1">
                  <label
                    htmlFor="style-toc-page-break-before"
                    className="flex cursor-pointer items-center justify-between rounded-md border border-border/75 bg-background px-2.5 py-1.5 text-[11px]"
                  >
                    <span>目录前换新页</span>
                    <Switch
                      id="style-toc-page-break-before"
                      size="sm"
                      checked={bundle.toc_page_break_before ?? false}
                      onCheckedChange={(checked) =>
                        onChange({ ...bundle, toc_page_break_before: checked })
                      }
                    />
                  </label>
                  <label
                    htmlFor="style-toc-page-break-after"
                    className="flex cursor-pointer items-center justify-between rounded-md border border-border/75 bg-background px-2.5 py-1.5 text-[11px]"
                  >
                    <span>目录后换新页</span>
                    <Switch
                      id="style-toc-page-break-after"
                      size="sm"
                      checked={bundle.toc_page_break_after ?? false}
                      onCheckedChange={(checked) =>
                        onChange({ ...bundle, toc_page_break_after: checked })
                      }
                    />
                  </label>
                </div>
              </>
            )}
          </section>

          {/* Bibliography & Citation Style */}
          <section className="space-y-2.5 rounded-lg border border-border/75 bg-card/60 p-3">
            <div className="flex items-center justify-between">
              <Label className="text-xs font-medium">文末参考文献</Label>
              <Switch
                size="sm"
                checked={bundle.references_enabled ?? true}
                onCheckedChange={(checked) =>
                  onChange({ ...bundle, references_enabled: checked })
                }
                aria-label="显示文末参考文献"
              />
            </div>

            <div className="space-y-1.5">
              <span className="text-[11px] text-muted-foreground">
                正文引文角标体例
              </span>
              <ToggleGroup
                multiple={false}
                value={[bundle.citation_style ?? "numeric"]}
                onValueChange={(values) => {
                  const next = values[0];
                  if (next === "numeric" || next === "author_year") {
                    onChange({
                      ...bundle,
                      citation_style: next,
                    });
                  }
                }}
                spacing={0}
                className="w-full rounded-md border bg-muted/40 p-0.5"
              >
                <ToggleGroupItem
                  value="numeric"
                  size="sm"
                  className="h-6 flex-1 rounded-xs text-[11px] aria-pressed:bg-background aria-pressed:font-medium aria-pressed:text-foreground aria-pressed:shadow-2xs"
                >
                  [1] 顺序编码制
                </ToggleGroupItem>
                <ToggleGroupItem
                  value="author_year"
                  size="sm"
                  className="h-6 flex-1 rounded-xs text-[11px] aria-pressed:bg-background aria-pressed:font-medium aria-pressed:text-foreground aria-pressed:shadow-2xs"
                >
                  (著者, 年份) 制
                </ToggleGroupItem>
              </ToggleGroup>
            </div>

            {(bundle.references_enabled ?? true) && (
              <label
                htmlFor="style-references-page-break-before"
                className="flex cursor-pointer items-center justify-between rounded-md border border-border/75 bg-background px-2.5 py-1.5 text-[11px]"
              >
                <span>参考文献独立换新页</span>
                <Switch
                  id="style-references-page-break-before"
                  size="sm"
                  checked={bundle.references_page_break_before ?? false}
                  onCheckedChange={(checked) =>
                    onChange({
                      ...bundle,
                      references_page_break_before: checked,
                    })
                  }
                />
              </label>
            )}
          </section>
        </div>
      )}

      {/* Partition 6: Paged Headers/Footers, Heading Page Break & Scoped Custom CSS */}
      {activeSection === "page" && (
        <div className="space-y-3.5">
          <section className="space-y-2.5 rounded-lg border border-border/75 bg-card/60 p-3">
            <div className="space-y-1.5">
              <Label className="text-xs font-medium">A4 分页章节自动换页</Label>
              <ToggleGroup
                multiple={false}
                value={[pageChrome.heading_page_break ?? "none"]}
                onValueChange={(values) => {
                  const next = values[0];
                  if (
                    next === "none" ||
                    next === "h1" ||
                    next === "h1_and_h2"
                  ) {
                    patchPageChrome({ heading_page_break: next });
                  }
                }}
                spacing={0}
                className="w-full rounded-md border bg-muted/40 p-0.5"
              >
                <ToggleGroupItem
                  value="none"
                  size="sm"
                  className="h-6 flex-1 rounded-xs text-[11px] aria-pressed:bg-background aria-pressed:font-medium aria-pressed:text-foreground aria-pressed:shadow-2xs"
                >
                  不强制
                </ToggleGroupItem>
                <ToggleGroupItem
                  value="h1"
                  size="sm"
                  className="h-6 flex-1 rounded-xs text-[11px] aria-pressed:bg-background aria-pressed:font-medium aria-pressed:text-foreground aria-pressed:shadow-2xs"
                >
                  一级换页
                </ToggleGroupItem>
                <ToggleGroupItem
                  value="h1_and_h2"
                  size="sm"
                  className="h-6 flex-1 rounded-xs text-[11px] aria-pressed:bg-background aria-pressed:font-medium aria-pressed:text-foreground aria-pressed:shadow-2xs"
                >
                  一、二级换页
                </ToggleGroupItem>
              </ToggleGroup>
            </div>

            <div className="space-y-1.5">
              <span className="text-[11px] text-muted-foreground">
                页眉页脚模板预设
              </span>
              <div className="flex flex-wrap gap-1.5">
                {PAGE_CHROME_PRESETS.map((preset) => (
                  <button
                    key={preset.id}
                    type="button"
                    onClick={() =>
                      patchPageChrome({
                        header: { ...preset.config.header },
                        footer: { ...preset.config.footer },
                        hide_header_on_first_page:
                          preset.config.hide_header_on_first_page,
                        hide_footer_on_first_page:
                          preset.config.hide_footer_on_first_page,
                      })
                    }
                    className="rounded-md border border-border/80 bg-background px-2 py-1 text-[11px] text-muted-foreground transition-colors hover:border-foreground/40 hover:text-foreground"
                  >
                    {preset.label}
                  </button>
                ))}
              </div>
            </div>

            <div className="flex flex-wrap gap-1">
              {PLACEHOLDERS.map((ph) => (
                <button
                  key={ph}
                  type="button"
                  onClick={() => {
                    if (!activeChromeSlot) return;
                    const curr =
                      pageChrome[activeChromeSlot.group]?.[
                        activeChromeSlot.slot
                      ] ?? "";
                    patchPageChrome({
                      [activeChromeSlot.group]: {
                        ...pageChrome[activeChromeSlot.group],
                        [activeChromeSlot.slot]: `${curr}${ph}`,
                      },
                    });
                  }}
                  className="rounded border border-border bg-muted/40 px-1.5 py-0.5 font-mono text-[10px] text-muted-foreground hover:bg-muted hover:text-foreground"
                >
                  {ph}
                </button>
              ))}
            </div>

            {(["header", "footer"] as const).map((group) => {
              const isHeader = group === "header";
              const hideFirst = isHeader
                ? (pageChrome.hide_header_on_first_page ?? true)
                : (pageChrome.hide_footer_on_first_page ?? false);
              return (
                <div key={group} className="space-y-1.5 pt-1">
                  <div className="flex items-center justify-between">
                    <span className="text-[11px] font-medium">
                      {isHeader
                        ? "页眉（左 / 中 / 右）"
                        : "页脚（左 / 中 / 右）"}
                    </span>
                    <label className="flex cursor-pointer items-center gap-1.5 text-[11px] text-muted-foreground">
                      <input
                        type="checkbox"
                        checked={hideFirst}
                        onChange={(e) =>
                          patchPageChrome(
                            isHeader
                              ? { hide_header_on_first_page: e.target.checked }
                              : { hide_footer_on_first_page: e.target.checked },
                          )
                        }
                        className="size-3.5"
                      />
                      <span>首页隐藏</span>
                    </label>
                  </div>
                  <div className="grid grid-cols-3 gap-1.5">
                    {(["left", "center", "right"] as const).map((slot) => (
                      <Input
                        key={slot}
                        aria-label={`${isHeader ? "页眉" : "页脚"}-${slot}`}
                        placeholder={
                          slot === "left"
                            ? "左"
                            : slot === "center"
                              ? "中"
                              : "右"
                        }
                        value={pageChrome[group]?.[slot] ?? ""}
                        onFocus={() => setActiveChromeSlot({ group, slot })}
                        onChange={(e) =>
                          patchPageChrome({
                            [group]: {
                              ...pageChrome[group],
                              [slot]: e.target.value,
                            },
                          })
                        }
                        className="h-7 px-2 font-mono text-[10.5px]"
                      />
                    ))}
                  </div>
                </div>
              );
            })}
          </section>

          <section className="space-y-1.5 rounded-lg border border-border/75 bg-card/60 p-3">
            <Label htmlFor="style-custom-css" className="text-xs font-medium">
              附加 CSS（.report-root 作用域）
            </Label>
            <textarea
              id="style-custom-css"
              name="style-custom-css"
              autoComplete="off"
              spellCheck={false}
              rows={3}
              placeholder=".report-root h1 { letter-spacing: 0.04em; }…"
              value={bundle.theme.custom_css ?? ""}
              onChange={(e) =>
                onChange({
                  ...bundle,
                  theme: {
                    ...bundle.theme,
                    custom_css: e.target.value,
                  },
                })
              }
              className="w-full rounded-lg border border-border bg-background px-2.5 py-1.5 font-mono text-[11px] leading-relaxed text-foreground placeholder:text-muted-foreground/60 focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none"
            />
          </section>
        </div>
      )}
    </div>
  );
}
