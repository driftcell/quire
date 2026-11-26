import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import {
  ArrowUpRightIcon,
  CheckIcon,
  GlobeIcon,
  LayoutGridIcon,
  LoaderCircleIcon,
  PaletteIcon,
  PlusIcon,
  RotateCcwIcon,
  SlidersHorizontalIcon,
} from "lucide-react";
import { toast } from "sonner";
import {
  applyStyleToDocument,
  createStylePreset,
  listStylePresets,
  setDefaultStylePreset,
} from "@/api/styles";
import { StyleCustomizerPanel } from "@/components/style/StyleCustomizerPanel";
import { StyleThumbnailCard } from "@/components/style/StyleThumbnailCard";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import type {
  CoverConfig,
  PageChromeConfig,
  ReferencesConfig,
  ThemeConfig,
  TocConfig,
} from "@/contracts/report-protocol";
import { useAuth } from "@/hooks/useAuth";
import { errorMessage } from "@/lib/errors";
import { clearPageChromeConfig } from "@/lib/pageChrome";
import { updatePreferences, useUiPreferences } from "@/lib/preferences";
import {
  BUILTIN_STYLE_PRESETS,
  type StylePresetBundle,
  type StylePresetRecord,
} from "@/lib/stylePresets";
import { clampTocMaxDepth, clearTocConfig } from "@/lib/tocConfig";

export interface ReportStyleDialogProps {
  docId: string;
  currentTheme: ThemeConfig;
  currentCover?: CoverConfig;
  currentToc?: TocConfig;
  currentReferences?: ReferencesConfig;
  themePageChrome?: PageChromeConfig | null;
}

function buildBundleFromDocument(
  currentTheme: ThemeConfig,
  currentCover?: CoverConfig,
  currentToc?: TocConfig,
  currentReferences?: ReferencesConfig,
  themePageChrome?: PageChromeConfig | null,
): StylePresetBundle {
  const presetId = currentTheme.preset || "quire-folio";
  const basePreset =
    BUILTIN_STYLE_PRESETS.find((p) => p.style_id === presetId) ??
    BUILTIN_STYLE_PRESETS.find((p) => p.style_id === "quire-folio");
  if (!basePreset) {
    throw new Error("Missing quire-folio built-in preset");
  }
  return {
    cover_enabled:
      currentCover?.enabled ?? basePreset.bundle.cover_enabled ?? true,
    cover_variant:
      currentCover?.variant ?? basePreset.bundle.cover_variant ?? "split",
    toc_enabled: currentToc?.enabled ?? basePreset.bundle.toc_enabled ?? true,
    toc_max_depth: clampTocMaxDepth(
      currentToc?.max_depth ?? basePreset.bundle.toc_max_depth ?? 3,
    ),
    toc_variant:
      currentToc?.variant ?? basePreset.bundle.toc_variant ?? "dotted",
    toc_page_break_before:
      currentToc?.page_break_before ??
      basePreset.bundle.toc_page_break_before ??
      false,
    toc_page_break_after:
      currentToc?.page_break_after ??
      basePreset.bundle.toc_page_break_after ??
      false,
    citation_style:
      currentReferences?.style ?? basePreset.bundle.citation_style ?? "numeric",
    references_enabled:
      currentReferences?.enabled ??
      basePreset.bundle.references_enabled ??
      true,
    references_page_break_before:
      currentReferences?.page_break_before ??
      basePreset.bundle.references_page_break_before ??
      false,
    theme: {
      preset: presetId,
      tokens: {
        ...(basePreset.bundle.theme.tokens ?? {}),
        ...(currentTheme.tokens ?? {}),
      },
      custom_css: currentTheme.custom_css ?? "",
      page_chrome:
        themePageChrome ??
        currentTheme.page_chrome ??
        basePreset.bundle.theme.page_chrome ??
        null,
    },
  };
}

export function ReportStyleDialog({
  docId,
  currentTheme,
  currentCover,
  currentToc,
  currentReferences,
  themePageChrome,
}: ReportStyleDialogProps) {
  const queryClient = useQueryClient();
  const { user } = useAuth();
  const prefs = useUiPreferences();
  const [open, setOpen] = useState(false);
  const [dialogTab, setDialogTab] = useState<"gallery" | "customize">(
    "gallery",
  );
  const [draftBundle, setDraftBundle] = useState<StylePresetBundle>(() =>
    buildBundleFromDocument(
      currentTheme,
      currentCover,
      currentToc,
      currentReferences,
      themePageChrome,
    ),
  );
  const [saveMode, setSaveMode] = useState(false);
  const [newName, setNewName] = useState("");
  const [newTag, setNewTag] = useState("自定义");
  const [isPublic, setIsPublic] = useState(false);

  const handleOpenChange = (nextOpen: boolean) => {
    setOpen(nextOpen);
    if (nextOpen) {
      setDraftBundle(
        buildBundleFromDocument(
          currentTheme,
          currentCover,
          currentToc,
          currentReferences,
          themePageChrome,
        ),
      );
    }
  };

  const stylesQuery = useQuery({
    queryKey: ["style-presets"],
    queryFn: listStylePresets,
    enabled: open,
  });

  const applyPresetMutation = useMutation({
    mutationFn: async (preset: StylePresetRecord) => {
      const response = await applyStyleToDocument(docId, {
        style_id: preset.style_id,
        bundle: preset.bundle,
        sync_layout_variants: true,
      });
      await setDefaultStylePreset(preset.style_id);
      return response;
    },
    onSuccess: (_res, preset) => {
      clearTocConfig(docId);
      clearPageChromeConfig(docId);
      updatePreferences({ defaultStylePresetId: preset.style_id });
      void queryClient.invalidateQueries({ queryKey: ["doc", docId] });
      toast.success(`已应用「${preset.name}」`);
      setOpen(false);
    },
    onError: (err) => {
      toast.error(errorMessage(err, "应用样式失败"));
    },
  });

  const applyCustomBundleMutation = useMutation({
    mutationFn: (bundle: StylePresetBundle) =>
      applyStyleToDocument(docId, {
        bundle,
        sync_layout_variants: true,
      }),
    onSuccess: () => {
      clearTocConfig(docId);
      clearPageChromeConfig(docId);
      void queryClient.invalidateQueries({ queryKey: ["doc", docId] });
      toast.success("已应用精调样式");
      setOpen(false);
    },
    onError: (err) => {
      toast.error(errorMessage(err, "应用样式失败"));
    },
  });

  const saveCurrentMutation = useMutation({
    mutationFn: () =>
      createStylePreset(
        {
          name: newName.trim(),
          category_tag: newTag.trim() || "自定义",
          is_public: isPublic,
          base_preset_id: draftBundle.theme.preset ?? "quire-folio",
          bundle: draftBundle,
        },
        {
          userId: user?.user_id,
          displayName: user?.display_name || user?.username,
        },
      ),
    onSuccess: (created) => {
      void queryClient.invalidateQueries({ queryKey: ["style-presets"] });
      toast.success(`已保存为预设「${created.name}」`);
      setSaveMode(false);
      setNewName("");
    },
    onError: (err) => {
      toast.error(errorMessage(err, "保存样式预设失败"));
    },
  });

  const presets = stylesQuery.data ?? [...BUILTIN_STYLE_PRESETS];
  const activePresetId = currentTheme.preset || "quire-folio";
  const resetDraft = () =>
    setDraftBundle(
      buildBundleFromDocument(
        currentTheme,
        currentCover,
        currentToc,
        currentReferences,
        themePageChrome,
      ),
    );

  return (
    <Dialog open={open} onOpenChange={handleOpenChange}>
      <DialogTrigger
        render={
          <Button
            type="button"
            size="sm"
            variant="ghost"
            aria-label="报告样式"
            className="h-7 gap-1.5 rounded-md px-2 text-xs font-medium text-foreground/85 hover:bg-secondary hover:text-foreground"
          />
        }
      >
        <PaletteIcon
          className="size-3.5 text-muted-foreground"
          aria-hidden="true"
        />
        <span className="hidden @min-[540px]:inline">样式</span>
      </DialogTrigger>

      <DialogContent className="no-scrollbar max-h-[88vh] overflow-y-auto overscroll-contain sm:max-w-3xl">
        <DialogHeader className="flex flex-row flex-wrap items-center justify-between gap-2 pr-6">
          <div className="flex items-center gap-2.5">
            <DialogTitle>报告样式</DialogTitle>
            <ToggleGroup
              multiple={false}
              value={[dialogTab]}
              onValueChange={(values) => {
                const next = values[0];
                if (next === "gallery" || next === "customize") {
                  setDialogTab(next);
                }
              }}
              spacing={0}
              className="rounded-lg border border-border/85 bg-secondary/75 p-0.5"
            >
              <ToggleGroupItem
                value="gallery"
                size="sm"
                className="h-6 gap-1 rounded-md px-2.5 text-xs text-muted-foreground aria-pressed:bg-card aria-pressed:font-medium aria-pressed:text-foreground aria-pressed:shadow-2xs"
              >
                <LayoutGridIcon className="size-3" aria-hidden="true" />
                <span>预设画廊</span>
              </ToggleGroupItem>
              <ToggleGroupItem
                value="customize"
                size="sm"
                className="h-6 gap-1 rounded-md px-2.5 text-xs text-muted-foreground aria-pressed:bg-card aria-pressed:font-medium aria-pressed:text-foreground aria-pressed:shadow-2xs"
              >
                <SlidersHorizontalIcon className="size-3" aria-hidden="true" />
                <span>分区精调（含目录/封面）</span>
              </ToggleGroupItem>
            </ToggleGroup>
          </div>

          <div className="flex items-center gap-2">
            {dialogTab === "customize" && (
              <Button
                type="button"
                size="xs"
                variant="ghost"
                title="重置当前预设修改"
                onClick={resetDraft}
              >
                <RotateCcwIcon data-icon="inline-start" />
                重置
              </Button>
            )}
            <Button
              type="button"
              size="xs"
              variant={saveMode ? "secondary" : "outline"}
              onClick={() => setSaveMode((prev) => !prev)}
            >
              <PlusIcon className="mr-1 size-3" aria-hidden="true" />
              存为预设
            </Button>
            <Link
              to="/styles"
              onClick={() => setOpen(false)}
              className="inline-flex items-center gap-1 rounded-md border border-border bg-card px-2 py-1 text-xs font-medium text-foreground transition-colors hover:bg-secondary focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none"
            >
              <span>样式中心</span>
              <ArrowUpRightIcon className="size-3" aria-hidden="true" />
            </Link>
          </div>
        </DialogHeader>

        {saveMode && (
          <form
            onSubmit={(e) => {
              e.preventDefault();
              if (!newName.trim()) {
                toast.error("请输入样式名称");
                return;
              }
              saveCurrentMutation.mutate();
            }}
            className="space-y-3 rounded-xl border border-border bg-muted/35 p-3.5"
          >
            <div className="grid grid-cols-1 gap-3 sm:grid-cols-[1.6fr_1fr]">
              <div className="space-y-1">
                <Label htmlFor="quick-style-name" className="text-xs">
                  预设名称
                </Label>
                <Input
                  id="quick-style-name"
                  name="quick-style-name"
                  autoComplete="off"
                  placeholder="例如：校党委会专项汇报样式…"
                  value={newName}
                  onChange={(e) => setNewName(e.target.value)}
                  className="h-8 text-xs"
                />
              </div>
              <div className="space-y-1">
                <Label htmlFor="quick-style-tag" className="text-xs">
                  风格分类
                </Label>
                <Input
                  id="quick-style-tag"
                  name="quick-style-tag"
                  autoComplete="off"
                  placeholder="正式 / 简报 / 学术…"
                  value={newTag}
                  onChange={(e) => setNewTag(e.target.value)}
                  className="h-8 text-xs"
                />
              </div>
            </div>

            <div className="flex flex-wrap items-center justify-between gap-2">
              <label
                htmlFor="quick-style-public"
                className="flex cursor-pointer items-center gap-2 text-xs text-foreground"
              >
                <Switch
                  id="quick-style-public"
                  checked={isPublic}
                  onCheckedChange={setIsPublic}
                  aria-label="公开至样式中心"
                />
                <GlobeIcon
                  className="size-3.5 text-muted-foreground"
                  aria-hidden="true"
                />
                <span>公开给团队其他成员使用</span>
              </label>

              <div className="flex items-center gap-2">
                <Button
                  type="button"
                  size="sm"
                  variant="ghost"
                  onClick={() => setSaveMode(false)}
                >
                  取消
                </Button>
                <Button
                  type="submit"
                  size="sm"
                  disabled={saveCurrentMutation.isPending}
                >
                  {saveCurrentMutation.isPending ? (
                    <LoaderCircleIcon className="mr-1.5 size-3.5 animate-spin" />
                  ) : (
                    <CheckIcon className="mr-1.5 size-3.5" />
                  )}
                  保存预设
                </Button>
              </div>
            </div>
          </form>
        )}

        {dialogTab === "gallery" ? (
          <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 md:grid-cols-3">
            {presets.map((preset) => {
              const isSelected =
                preset.style_id === activePresetId ||
                preset.bundle.theme.preset === activePresetId;
              const isMine =
                Boolean(user?.user_id) &&
                preset.owner_user_id === user?.user_id;
              return (
                <StyleThumbnailCard
                  key={preset.style_id}
                  preset={preset}
                  selected={isSelected}
                  isDefault={prefs.defaultStylePresetId === preset.style_id}
                  isMine={isMine}
                  onSelect={() => applyPresetMutation.mutate(preset)}
                />
              );
            })}
          </div>
        ) : (
          <div className="space-y-4">
            <StyleCustomizerPanel
              bundle={draftBundle}
              onChange={setDraftBundle}
            />
            <div className="flex items-center justify-end gap-2 border-t pt-3">
              <Button
                type="button"
                size="sm"
                variant="outline"
                onClick={() => setOpen(false)}
              >
                取消
              </Button>
              <Button
                type="button"
                size="sm"
                disabled={applyCustomBundleMutation.isPending}
                onClick={() => applyCustomBundleMutation.mutate(draftBundle)}
              >
                {applyCustomBundleMutation.isPending && (
                  <LoaderCircleIcon className="mr-1.5 size-3.5 animate-spin" />
                )}
                应用到当前报告
              </Button>
            </div>
          </div>
        )}
      </DialogContent>
    </Dialog>
  );
}
