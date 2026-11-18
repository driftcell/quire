import { useState } from "react";
import { RotateCcwIcon, Settings2Icon } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import type { PageChromeConfig } from "@/contracts/report-protocol";
import {
  PAGE_CHROME_PRESETS,
  clearPageChromeConfig,
  resolvePageChromeConfig,
  savePageChromeConfig,
  type ResolvedPageChrome,
} from "@/lib/pageChrome";

type SlotKey = "left" | "center" | "right";
type SlotGroup = "header" | "footer";

const SLOT_KEYS: SlotKey[] = ["left", "center", "right"];
const SLOT_LABELS: Record<SlotKey, string> = {
  left: "左",
  center: "中",
  right: "右",
};
const PLACEHOLDERS = [
  "{page}",
  "{total}",
  "{title}",
  "{subtitle}",
  "{date}",
  "{kicker}",
  "{authors}",
];

export interface PageChromeDialogProps {
  docId: string;
  themePageChrome?: PageChromeConfig | null;
}

export function PageChromeDialog({
  docId,
  themePageChrome,
}: PageChromeDialogProps) {
  const [open, setOpen] = useState(false);
  const [draft, setDraft] = useState<ResolvedPageChrome>(() =>
    resolvePageChromeConfig(docId, themePageChrome),
  );
  const [activeSlot, setActiveSlot] = useState<{
    group: SlotGroup;
    slot: SlotKey;
  } | null>(null);

  const handleOpenChange = (nextOpen: boolean) => {
    if (nextOpen) {
      setDraft(resolvePageChromeConfig(docId, themePageChrome));
      setActiveSlot(null);
    }
    setOpen(nextOpen);
  };

  const applyPreset = (preset: ResolvedPageChrome) => {
    setDraft({
      header: { ...preset.header },
      footer: { ...preset.footer },
      hide_header_on_first_page: preset.hide_header_on_first_page,
      hide_footer_on_first_page: preset.hide_footer_on_first_page,
    });
  };

  const updateSlot = (group: SlotGroup, slot: SlotKey, value: string) => {
    setDraft((prev) => ({
      ...prev,
      [group]: { ...prev[group], [slot]: value },
    }));
  };

  const insertPlaceholder = (placeholder: string) => {
    if (!activeSlot) {
      return;
    }
    setDraft((prev) => {
      const current = prev[activeSlot.group][activeSlot.slot];
      return {
        ...prev,
        [activeSlot.group]: {
          ...prev[activeSlot.group],
          [activeSlot.slot]: `${current}${placeholder}`,
        },
      };
    });
  };

  const restoreDocumentDefaults = () => {
    clearPageChromeConfig(docId);
    setDraft(resolvePageChromeConfig(docId, themePageChrome));
  };

  const save = () => {
    savePageChromeConfig(docId, draft);
    setOpen(false);
  };

  return (
    <Dialog open={open} onOpenChange={handleOpenChange}>
      <DialogTrigger
        render={
          <Button
            type="button"
            size="icon-sm"
            variant="ghost"
            aria-label="页眉与页脚设置"
          />
        }
      >
        <Settings2Icon className="size-4" />
      </DialogTrigger>
      <DialogContent className="max-h-[88vh] overflow-y-auto sm:max-w-2xl">
        <DialogHeader>
          <DialogTitle>分页页眉与页脚</DialogTitle>
          <DialogDescription>
            模板占位符支持页码、标题、副标题、日期、眉题与作者；本地设置优先于
            Agent 写入的文档配置。
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-2">
          <div className="text-xs font-medium text-muted-foreground">
            一键预设
          </div>
          <div className="flex flex-wrap gap-1.5">
            {PAGE_CHROME_PRESETS.map((preset) => (
              <Button
                key={preset.id}
                type="button"
                size="xs"
                variant="outline"
                onClick={() => applyPreset(preset.config)}
              >
                {preset.label}
              </Button>
            ))}
          </div>
        </div>

        <div className="space-y-1.5">
          <div className="text-xs font-medium text-muted-foreground">
            可用占位符（插入到最后聚焦的槽位）
          </div>
          <div className="flex flex-wrap gap-1.5">
            {PLACEHOLDERS.map((placeholder) => (
              <button
                key={placeholder}
                type="button"
                onClick={() => insertPlaceholder(placeholder)}
                className="rounded-full border border-border bg-muted/40 px-2 py-0.5 font-mono text-[10px] text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
              >
                {placeholder}
              </button>
            ))}
          </div>
        </div>

        {(["header", "footer"] as const).map((group) => {
          const isHeader = group === "header";
          const hideFirst = isHeader
            ? draft.hide_header_on_first_page
            : draft.hide_footer_on_first_page;
          return (
            <div key={group} className="space-y-2 rounded-lg border p-3">
              <div className="flex items-center justify-between gap-3">
                <span className="text-xs font-semibold">
                  {isHeader ? "页眉（上）" : "页脚（下）"}
                </span>
                <label className="flex items-center gap-1.5 text-[11px] text-muted-foreground">
                  <input
                    type="checkbox"
                    className="size-3.5 accent-[var(--rpt-accent)]"
                    checked={hideFirst}
                    onChange={(event) => {
                      const checked = event.target.checked;
                      setDraft((prev) => ({
                        ...prev,
                        ...(isHeader
                          ? { hide_header_on_first_page: checked }
                          : { hide_footer_on_first_page: checked }),
                      }));
                    }}
                  />
                  首页隐藏{isHeader ? "页眉" : "页脚"}
                </label>
              </div>
              <div className="grid grid-cols-3 gap-2">
                {SLOT_KEYS.map((slot) => (
                  <div key={slot} className="space-y-1">
                    <div className="text-[10px] text-muted-foreground">
                      {SLOT_LABELS[slot]}
                    </div>
                    <Input
                      value={draft[group][slot]}
                      placeholder={SLOT_LABELS[slot]}
                      aria-label={`${isHeader ? "页眉" : "页脚"}-${SLOT_LABELS[slot]}`}
                      onFocus={() => setActiveSlot({ group, slot })}
                      onChange={(event) =>
                        updateSlot(group, slot, event.target.value)
                      }
                      className="h-8 font-mono text-xs"
                    />
                  </div>
                ))}
              </div>
            </div>
          );
        })}

        <div className="flex flex-wrap items-center justify-between gap-2 border-t pt-3">
          <Button
            type="button"
            size="sm"
            variant="ghost"
            onClick={restoreDocumentDefaults}
          >
            <RotateCcwIcon className="mr-1 size-3.5" />
            恢复文档/Agent 默认
          </Button>
          <div className="flex gap-2">
            <Button
              type="button"
              size="sm"
              variant="outline"
              onClick={() => setOpen(false)}
            >
              取消
            </Button>
            <Button type="button" size="sm" onClick={save}>
              应用并保存
            </Button>
          </div>
        </div>
      </DialogContent>
    </Dialog>
  );
}
