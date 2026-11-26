import { memo } from "react";
import { CheckIcon, GlobeIcon, LockIcon } from "lucide-react";
import type { StylePresetRecord } from "@/lib/stylePresets";

export interface StyleThumbnailCardProps {
  preset: StylePresetRecord;
  selected: boolean;
  isDefault: boolean;
  isMine: boolean;
  onSelect: () => void;
}

export const StyleThumbnailCard = memo(function StyleThumbnailCard({
  preset,
  selected,
  isDefault,
  isMine,
  onSelect,
}: StyleThumbnailCardProps) {
  const tokens = preset.bundle.theme.tokens ?? {};
  const accent = tokens["--rpt-accent"] ?? "#1E3A5F";
  const ink = tokens["--rpt-ink"] ?? "#18181B";
  const muted = tokens["--rpt-muted"] ?? "#575651";
  const border = tokens["--rpt-border"] ?? "#D6D4CD";
  const surface = tokens["--rpt-surface"] ?? "#F7F6F2";
  const paper = tokens["--rpt-paper"] ?? "#FFFFFF";
  const tableHeaderBg = tokens["--rpt-table-header-bg"] ?? surface;
  const tableBorder = tokens["--rpt-table-border"] ?? ink;
  const chart1 = tokens["--rpt-chart-1"] ?? accent;
  const chart2 = tokens["--rpt-chart-2"] ?? "#3B6E9C";
  const chart3 = tokens["--rpt-chart-3"] ?? "#6495B8";
  const fontHeading =
    tokens["--rpt-font-heading"] ??
    '"Noto Serif SC", "Songti SC", Georgia, serif';
  const fontBody =
    tokens["--rpt-font-body"] ??
    '"Geist Variable", "PingFang SC", system-ui, sans-serif';
  const coverVariant = preset.bundle.cover_variant ?? "classic";
  const presetKey = preset.bundle.theme.preset ?? preset.base_preset_id;

  const isCrimson = presetKey === "crimson-decree" || presetKey === "briefing";
  const isAcademic = presetKey === "academic-press";
  const isFieldNotes = presetKey === "field-notes";

  return (
    <button
      type="button"
      onClick={onSelect}
      aria-pressed={selected}
      aria-label={`预览样式：${preset.name}`}
      className={`group relative flex w-full flex-col overflow-hidden rounded-xl border text-left transition-[border-color,box-shadow,background-color] duration-150 focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none ${
        selected
          ? "border-[#1D4ED8] bg-card shadow-xs ring-1 ring-[#1D4ED8]"
          : "border-border/85 bg-card/75 hover:border-foreground/30 hover:bg-card"
      }`}
    >
      {/* Top Miniature A4 Specimen Canvas */}
      <div className="relative flex h-36 w-full items-center justify-center overflow-hidden border-b border-border/60 bg-[#EEECE6]/75 px-4 pt-3">
        {/* Miniature A4 Page Sheet */}
        <div
          aria-hidden="true"
          style={{
            backgroundColor: paper,
            color: ink,
            fontFamily: fontBody,
          }}
          className="flex h-full w-full max-w-[230px] flex-col rounded-t-md border border-black/10 px-3.5 pt-2.5 pb-2 shadow-2xs transition-transform duration-150 group-hover:-translate-y-0.5"
        >
          {/* Miniature Cover / Header Treatment */}
          {coverVariant === "split" ? (
            <div
              style={{ borderColor: accent }}
              className="mb-2 grid grid-cols-[1.5fr_1fr] items-end gap-2 border-b-[1.5px] pb-1.5"
            >
              <div>
                <div
                  style={{ color: accent }}
                  className="text-[6.5px] font-semibold tracking-widest"
                >
                  高教发展参阅
                </div>
                <div
                  style={{ fontFamily: fontHeading }}
                  className="mt-0.5 truncate text-[10px] leading-tight font-bold"
                >
                  学科建设竞争力报告
                </div>
              </div>
              <div
                style={{ borderColor: border, color: muted }}
                className="border-l pl-1.5 text-[6.5px] leading-tight"
              >
                <div>规划与学科处</div>
                <div>2026.11</div>
              </div>
            </div>
          ) : coverVariant === "modern" ? (
            <div
              style={{ borderColor: border }}
              className="mb-2 flex gap-1.5 border-b pb-1.5"
            >
              <div
                style={{ backgroundColor: accent }}
                className="w-0.5 shrink-0 rounded-full"
              />
              <div className="min-w-0 flex-1">
                <div className="flex items-center justify-between gap-1">
                  <span
                    style={{
                      fontFamily: fontHeading,
                    }}
                    className="truncate text-[10px] leading-tight font-bold"
                  >
                    学科建设竞争力报告
                  </span>
                  <span
                    style={{
                      backgroundColor: surface,
                      borderColor: border,
                      color: accent,
                    }}
                    className="shrink-0 rounded-full border px-1 py-0.2 text-[6px] font-medium"
                  >
                    简报
                  </span>
                </div>
              </div>
            </div>
          ) : coverVariant === "minimal" ? (
            <div
              style={{
                borderColor: border,
                borderBottomStyle: isFieldNotes ? "dashed" : "solid",
              }}
              className="mb-2 border-b pb-1.5"
            >
              <div
                style={{ fontFamily: fontHeading }}
                className="truncate text-[10px] leading-tight font-semibold"
              >
                学科建设竞争力对标手札
              </div>
            </div>
          ) : (
            <div
              style={{
                borderColor: isCrimson ? accent : border,
                borderBottomStyle: isCrimson ? "double" : "solid",
                borderBottomWidth: isCrimson ? "2.5px" : "1px",
              }}
              className="mb-2 pb-1.5 text-center"
            >
              <div
                style={{ color: accent }}
                className="text-[6.5px] font-semibold tracking-[0.18em]"
              >
                高等教育发展战略参阅
              </div>
              <div
                style={{ fontFamily: fontHeading }}
                className="mt-0.5 truncate text-[10px] leading-tight font-bold"
              >
                学科建设与科研创新报告
              </div>
            </div>
          )}

          {/* Miniature Section Heading + Prose Lines */}
          <div
            style={{
              fontFamily: fontHeading,
              borderLeft: isAcademic ? "none" : `2px solid ${accent}`,
              borderBottom: isAcademic ? `1px solid ${ink}` : "none",
              backgroundColor: isFieldNotes ? surface : "transparent",
              paddingLeft: isAcademic ? "0px" : "4px",
            }}
            className="mb-1.5 text-[7.5px] leading-tight font-semibold"
          >
            1.1 国自然立项与经费结构
          </div>

          {/* Miniature Split: 3-Line Table + Bar Chart */}
          <div className="mt-auto grid grid-cols-[1.35fr_1fr] items-end gap-2 pt-0.5">
            {/* Miniature Three-Line Table */}
            <div
              style={{
                borderTop: `1px solid ${tableBorder}`,
                borderBottom: `1px solid ${tableBorder}`,
              }}
              className="text-[6px] leading-tight tabular-nums"
            >
              <div
                style={{
                  backgroundColor: tableHeaderBg,
                  borderBottom: `0.5px solid ${tableBorder}`,
                }}
                className="flex justify-between px-1 py-0.5 font-semibold"
              >
                <span>高校</span>
                <span>立项</span>
                <span>增速</span>
              </div>
              <div
                style={{ borderBottom: `0.5px solid ${border}` }}
                className="flex justify-between px-1 py-0.5"
              >
                <span>A校</span>
                <span>742</span>
                <span style={{ color: accent }}>+14%</span>
              </div>
              <div className="flex justify-between px-1 py-0.5">
                <span>C校</span>
                <span>486</span>
                <span style={{ color: accent }}>+15%</span>
              </div>
            </div>

            {/* Miniature Bar Chart */}
            <div
              style={{
                backgroundColor: surface,
                borderColor: border,
              }}
              className="flex h-10 items-end justify-around rounded-xs border px-1.5 pt-1.5 pb-1"
            >
              <span
                style={{ backgroundColor: chart1, height: "85%" }}
                className="w-2 rounded-t-[1px]"
              />
              <span
                style={{ backgroundColor: chart2, height: "62%" }}
                className="w-2 rounded-t-[1px]"
              />
              <span
                style={{ backgroundColor: chart3, height: "94%" }}
                className="w-2 rounded-t-[1px]"
              />
              <span
                style={{ backgroundColor: chart1, height: "48%" }}
                className="w-2 rounded-t-[1px] opacity-75"
              />
            </div>
          </div>
        </div>
      </div>

      {/* Bottom Metadata Bar — Terse, No Redundant Explanations */}
      <div className="flex items-center justify-between gap-2 px-3 py-2.5">
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-1.5">
            <span className="truncate text-[13px] font-medium text-foreground">
              {preset.name}
            </span>
            {isDefault && (
              <span className="inline-flex shrink-0 items-center gap-0.5 rounded bg-[#1D4ED8]/10 px-1.5 py-0.2 text-[10px] font-medium text-[#1D4ED8]">
                <CheckIcon className="size-2.5" aria-hidden="true" />
                默认
              </span>
            )}
          </div>
          <div className="mt-0.5 flex items-center gap-1.5 text-[11px] text-muted-foreground">
            <span>{preset.category_tag}</span>
            {!preset.is_builtin && (
              <>
                <span aria-hidden="true">·</span>
                <span className="inline-flex items-center gap-0.5 truncate">
                  {preset.is_public ? (
                    <GlobeIcon
                      className="size-2.5 shrink-0"
                      aria-hidden="true"
                    />
                  ) : (
                    <LockIcon
                      className="size-2.5 shrink-0"
                      aria-hidden="true"
                    />
                  )}
                  <span className="truncate">
                    {isMine
                      ? preset.is_public
                        ? "已公开"
                        : "仅自己"
                      : preset.owner_display_name}
                  </span>
                </span>
              </>
            )}
          </div>
        </div>

        {/* Palette Swatch Dots */}
        <div
          className="flex shrink-0 items-center -space-x-1"
          aria-hidden="true"
        >
          <span
            style={{ backgroundColor: accent }}
            className="size-3.5 rounded-full ring-1 ring-black/15"
          />
          <span
            style={{ backgroundColor: chart2 }}
            className="size-3.5 rounded-full ring-1 ring-black/15"
          />
          <span
            style={{ backgroundColor: paper }}
            className="size-3.5 rounded-full ring-1 ring-black/20"
          />
        </div>
      </div>
    </button>
  );
});
