import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import type { StylePresetBundle } from "@/lib/stylePresets";
import { RotateCcwIcon } from "lucide-react";

export interface StyleCustomizerPanelProps {
  bundle: StylePresetBundle;
  onChange: (next: StylePresetBundle) => void;
  onReset: () => void;
}

const ACCENT_SWATCHES = [
  { label: "松石绿", value: "#0F766E", chart2: "#0284C7", chart3: "#D97706" },
  { label: "群青蓝", value: "#1D4ED8", chart2: "#0D9488", chart3: "#3B82F6" },
  { label: "牛津蓝", value: "#1E3A5F", chart2: "#3B6E9C", chart3: "#6495B8" },
  { label: "深黛青", value: "#27374D", chart2: "#526D82", chart3: "#8E7AB5" },
  { label: "朱砂红", value: "#991B1B", chart2: "#1E3A5F", chart3: "#B45309" },
  { label: "紫金棕", value: "#7C2D12", chart2: "#B45309", chart3: "#0F766E" },
] as const;

const PAPER_SWATCHES = [
  { label: "纯白", paper: "#FFFFFF", surface: "#F8FAFC" },
  { label: "典藏米白", paper: "#FAF9F5", surface: "#F3F1EA" },
  { label: "宣纸暖白", paper: "#FFFDF9", surface: "#F7F4EC" },
] as const;

const SERIF_STACK =
  '"Noto Serif SC", "Songti SC", "STSong", "SimSun", Georgia, serif';
const SANS_STACK =
  '"Geist Variable", "PingFang SC", "Noto Sans SC", system-ui, sans-serif';

export function StyleCustomizerPanel({
  bundle,
  onChange,
  onReset,
}: StyleCustomizerPanelProps) {
  const tokens = bundle.theme.tokens ?? {};
  const accent = tokens["--rpt-accent"] ?? "#1E3A5F";
  const ink = tokens["--rpt-ink"] ?? "#18181B";
  const paper = tokens["--rpt-paper"] ?? "#FFFFFF";
  const surface = tokens["--rpt-surface"] ?? "#F7F6F2";
  const bodySize = tokens["--rpt-body-size"] ?? "14.5px";
  const lineHeight = tokens["--rpt-line-height"] ?? "1.82";
  const headingFont = tokens["--rpt-font-heading"] ?? SERIF_STACK;
  const bodyFont = tokens["--rpt-font-body"] ?? SANS_STACK;

  const isHeadingSerif =
    headingFont.includes("Serif") || headingFont.includes("Song");
  const isBodySerif = bodyFont.includes("Serif") || bodyFont.includes("Song");

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

  const headingBreak = bundle.theme.page_chrome?.heading_page_break ?? "none";

  return (
    <div className="space-y-4 text-xs">
      {/* Palette */}
      <section className="space-y-2">
        <div className="flex items-center justify-between">
          <Label htmlFor="style-accent-hex" className="text-xs font-medium">
            主色与图表色盘
          </Label>
          <button
            type="button"
            onClick={onReset}
            className="inline-flex items-center gap-1 rounded px-1.5 py-0.5 text-[11px] text-muted-foreground transition-colors hover:bg-muted hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none"
          >
            <RotateCcwIcon className="size-3" aria-hidden="true" />
            重置
          </button>
        </div>

        <div className="flex flex-wrap items-center gap-1.5">
          {ACCENT_SWATCHES.map((swatch) => {
            const active = accent.toLowerCase() === swatch.value.toLowerCase();
            return (
              <button
                key={swatch.value}
                type="button"
                aria-label={`主色：${swatch.label}`}
                title={swatch.label}
                onClick={() =>
                  patchTokens({
                    "--rpt-accent": swatch.value,
                    "--rpt-chart-1": swatch.value,
                    "--rpt-chart-2": swatch.chart2,
                    "--rpt-chart-3": swatch.chart3,
                  })
                }
                className={`flex size-6 items-center justify-center rounded-full border transition-transform focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none ${
                  active
                    ? "scale-105 border-foreground ring-2 ring-foreground/20"
                    : "border-black/15 hover:scale-105"
                }`}
                style={{ backgroundColor: swatch.value }}
              />
            );
          })}

          <div className="ml-auto flex items-center gap-1.5">
            <input
              id="style-accent-picker"
              type="color"
              aria-label="自定义主色拾色器"
              value={
                accent.startsWith("#") && accent.length === 7
                  ? accent
                  : "#1E3A5F"
              }
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
              aria-label="主色 Hex 值"
              value={accent}
              onChange={(e) =>
                patchTokens({
                  "--rpt-accent": e.target.value,
                  "--rpt-chart-1": e.target.value,
                })
              }
              className="h-7 w-22 font-mono text-xs uppercase tabular-nums"
            />
          </div>
        </div>
      </section>

      {/* Paper & Ink */}
      <section className="grid grid-cols-2 gap-2.5">
        <div className="space-y-1.5">
          <Label className="text-xs font-medium">纸面底色</Label>
          <div className="flex gap-1">
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
                    })
                  }
                  className={`flex-1 rounded-md border px-2 py-1 text-[11px] transition-colors focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none ${
                    active
                      ? "border-[#1D4ED8] bg-[#1D4ED8]/8 font-medium text-foreground"
                      : "border-border bg-card text-muted-foreground hover:text-foreground"
                  }`}
                >
                  {item.label}
                </button>
              );
            })}
          </div>
        </div>

        <div className="space-y-1.5">
          <Label htmlFor="style-ink-hex" className="text-xs font-medium">
            正文墨色
          </Label>
          <div className="flex items-center gap-1.5">
            <input
              type="color"
              aria-label="正文墨色拾色器"
              value={ink.startsWith("#") && ink.length === 7 ? ink : "#18181B"}
              onChange={(e) => patchTokens({ "--rpt-ink": e.target.value })}
              className="size-7 cursor-pointer rounded border border-border bg-transparent p-0.5"
            />
            <Input
              id="style-ink-hex"
              name="style-ink-hex"
              autoComplete="off"
              spellCheck={false}
              value={ink}
              onChange={(e) => patchTokens({ "--rpt-ink": e.target.value })}
              className="h-7 flex-1 font-mono text-xs uppercase tabular-nums"
            />
          </div>
        </div>
      </section>

      {/* Typography Pairing */}
      <section className="grid grid-cols-2 gap-2.5">
        <div className="space-y-1.5">
          <Label className="text-xs font-medium">标题字体</Label>
          <ToggleGroup
            multiple={false}
            value={[isHeadingSerif ? "serif" : "sans"]}
            onValueChange={(values) => {
              const v = values[0];
              if (v === "serif" || v === "sans") {
                patchTokens({
                  "--rpt-font-heading":
                    v === "serif" ? SERIF_STACK : SANS_STACK,
                });
              }
            }}
            spacing={0}
            className="w-full rounded-md border bg-muted/40 p-0.5"
          >
            <ToggleGroupItem
              value="serif"
              size="sm"
              className="h-6 flex-1 rounded-xs text-[11px] aria-pressed:bg-background aria-pressed:font-medium aria-pressed:text-foreground aria-pressed:shadow-2xs"
            >
              宋体衬线
            </ToggleGroupItem>
            <ToggleGroupItem
              value="sans"
              size="sm"
              className="h-6 flex-1 rounded-xs text-[11px] aria-pressed:bg-background aria-pressed:font-medium aria-pressed:text-foreground aria-pressed:shadow-2xs"
            >
              现代黑体
            </ToggleGroupItem>
          </ToggleGroup>
        </div>

        <div className="space-y-1.5">
          <Label className="text-xs font-medium">正文字体</Label>
          <ToggleGroup
            multiple={false}
            value={[isBodySerif ? "serif" : "sans"]}
            onValueChange={(values) => {
              const v = values[0];
              if (v === "serif" || v === "sans") {
                patchTokens({
                  "--rpt-font-body": v === "serif" ? SERIF_STACK : SANS_STACK,
                });
              }
            }}
            spacing={0}
            className="w-full rounded-md border bg-muted/40 p-0.5"
          >
            <ToggleGroupItem
              value="sans"
              size="sm"
              className="h-6 flex-1 rounded-xs text-[11px] aria-pressed:bg-background aria-pressed:font-medium aria-pressed:text-foreground aria-pressed:shadow-2xs"
            >
              现代黑体
            </ToggleGroupItem>
            <ToggleGroupItem
              value="serif"
              size="sm"
              className="h-6 flex-1 rounded-xs text-[11px] aria-pressed:bg-background aria-pressed:font-medium aria-pressed:text-foreground aria-pressed:shadow-2xs"
            >
              宋体衬线
            </ToggleGroupItem>
          </ToggleGroup>
        </div>
      </section>

      {/* Body Font Size & Line Height */}
      <section className="grid grid-cols-2 gap-2.5">
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
              { label: "紧凑", value: "14px" },
              { label: "标准", value: "14.5px" },
              { label: "公文", value: "15px" },
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
          <Label className="text-xs font-medium">段落行距</Label>
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
            {[
              { label: "1.76", value: "1.76" },
              { label: "1.82", value: "1.82" },
              { label: "1.92", value: "1.92" },
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
      </section>

      {/* Cover & TOC Variants */}
      <section className="space-y-1.5">
        <Label className="text-xs font-medium">封面版式</Label>
        <ToggleGroup
          multiple={false}
          value={[bundle.cover_variant ?? "split"]}
          onValueChange={(values) => {
            const next = values[0];
            if (next) {
              onChange({ ...bundle, cover_variant: next });
            }
          }}
          spacing={0}
          className="w-full rounded-md border bg-muted/40 p-0.5"
        >
          {[
            { label: "极简", value: "minimal" },
            { label: "现代", value: "modern" },
            { label: "双栏", value: "split" },
            { label: "庄重", value: "classic" },
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
      </section>

      <section className="grid grid-cols-2 gap-2.5">
        <div className="space-y-1.5">
          <Label className="text-xs font-medium">目录引导</Label>
          <ToggleGroup
            multiple={false}
            value={[bundle.toc_variant ?? "dotted"]}
            onValueChange={(values) => {
              const next = values[0];
              if (next) {
                onChange({ ...bundle, toc_variant: next });
              }
            }}
            spacing={0}
            className="w-full rounded-md border bg-muted/40 p-0.5"
          >
            {[
              { label: "点线", value: "dotted" },
              { label: "编号", value: "numbered" },
              { label: "无引导", value: "standard" },
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
          <Label className="text-xs font-medium">文献角标</Label>
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
              [1] 顺序
            </ToggleGroupItem>
            <ToggleGroupItem
              value="author_year"
              size="sm"
              className="h-6 flex-1 rounded-xs text-[11px] aria-pressed:bg-background aria-pressed:font-medium aria-pressed:text-foreground aria-pressed:shadow-2xs"
            >
              著者年份
            </ToggleGroupItem>
          </ToggleGroup>
        </div>
      </section>

      {/* Table Stripe & Heading Break */}
      <section className="grid grid-cols-2 gap-2.5">
        <div className="space-y-1.5">
          <Label className="text-xs font-medium">表格样式</Label>
          <ToggleGroup
            multiple={false}
            value={[
              (tokens["--rpt-table-stripe"] ?? "#F8FAFC").toLowerCase() ===
              "#ffffff"
                ? "clean"
                : "striped",
            ]}
            onValueChange={(values) => {
              const next = values[0];
              if (next === "clean") {
                patchTokens({
                  "--rpt-table-stripe": "#FFFFFF",
                  "--rpt-table-header-bg": "#FFFFFF",
                });
              } else if (next === "striped") {
                patchTokens({
                  "--rpt-table-stripe": surface,
                  "--rpt-table-header-bg": surface,
                });
              }
            }}
            spacing={0}
            className="w-full rounded-md border bg-muted/40 p-0.5"
          >
            <ToggleGroupItem
              value="striped"
              size="sm"
              className="h-6 flex-1 rounded-xs text-[11px] aria-pressed:bg-background aria-pressed:font-medium aria-pressed:text-foreground aria-pressed:shadow-2xs"
            >
              浅底斑马纹
            </ToggleGroupItem>
            <ToggleGroupItem
              value="clean"
              size="sm"
              className="h-6 flex-1 rounded-xs text-[11px] aria-pressed:bg-background aria-pressed:font-medium aria-pressed:text-foreground aria-pressed:shadow-2xs"
            >
              纯白三线表
            </ToggleGroupItem>
          </ToggleGroup>
        </div>

        <div className="space-y-1.5">
          <Label className="text-xs font-medium">分页章节换页</Label>
          <ToggleGroup
            multiple={false}
            value={[headingBreak]}
            onValueChange={(values) => {
              const next = values[0];
              if (next === "none" || next === "h1" || next === "h1_and_h2") {
                onChange({
                  ...bundle,
                  theme: {
                    ...bundle.theme,
                    page_chrome: {
                      header: bundle.theme.page_chrome?.header ?? {
                        left: "{kicker}",
                        center: "",
                        right: "{title}",
                      },
                      footer: bundle.theme.page_chrome?.footer ?? {
                        left: "{authors}",
                        center: "第 {page} 页 / 共 {total} 页",
                        right: "{date}",
                      },
                      hide_header_on_first_page:
                        bundle.theme.page_chrome?.hide_header_on_first_page ??
                        true,
                      hide_footer_on_first_page:
                        bundle.theme.page_chrome?.hide_footer_on_first_page ??
                        false,
                      heading_page_break: next,
                    },
                  },
                });
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
              连续
            </ToggleGroupItem>
            <ToggleGroupItem
              value="h1"
              size="sm"
              className="h-6 flex-1 rounded-xs text-[11px] aria-pressed:bg-background aria-pressed:font-medium aria-pressed:text-foreground aria-pressed:shadow-2xs"
            >
              一级换页
            </ToggleGroupItem>
          </ToggleGroup>
        </div>
      </section>

      {/* Custom Scoped CSS */}
      <section className="space-y-1.5">
        <Label htmlFor="style-custom-css" className="text-xs font-medium">
          附加 CSS（自动作用于 .report-root）
        </Label>
        <textarea
          id="style-custom-css"
          name="style-custom-css"
          autoComplete="off"
          spellCheck={false}
          rows={3}
          placeholder=".report-root h1 { letter-spacing: 0.02em; }…"
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
  );
}
