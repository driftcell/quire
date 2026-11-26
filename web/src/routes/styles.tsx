import { useMemo, useState, type FormEvent } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { createFileRoute } from "@tanstack/react-router";
import {
  BookOpenIcon,
  CheckIcon,
  GlobeIcon,
  LoaderCircleIcon,
  LockIcon,
  PanelLeftIcon,
  PlusIcon,
  ScrollTextIcon,
  SlidersHorizontalIcon,
  Trash2Icon,
  XIcon,
} from "lucide-react";
import { toast } from "sonner";
import {
  createStylePreset,
  deleteStylePreset,
  listStylePresets,
  updateStylePreset,
} from "@/api/styles";
import { SidebarEdgeHandle } from "@/components/layout/SidebarEdgeHandle";
import { ReportViewer } from "@/components/report/ReportViewer";
import { StyleCustomizerPanel } from "@/components/style/StyleCustomizerPanel";
import { StyleThumbnailCard } from "@/components/style/StyleThumbnailCard";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { useSidebar } from "@/components/ui/sidebar";
import { Switch } from "@/components/ui/switch";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { useIsMobile } from "@/hooks/use-mobile";
import { useAuth } from "@/hooks/useAuth";
import { errorMessage } from "@/lib/errors";
import {
  updatePreferences,
  useUiPreferences,
  type ReportViewMode,
} from "@/lib/preferences";
import {
  BUILTIN_STYLE_PRESETS,
  buildSpecimenDocument,
  type StylePresetBundle,
  type StylePresetRecord,
} from "@/lib/stylePresets";

type StyleTabFilter = "all" | "builtin" | "mine" | "public";

export const Route = createFileRoute("/styles")({
  component: StyleCenterPage,
});

function cloneBundle(bundle: StylePresetBundle): StylePresetBundle {
  return {
    cover_variant: bundle.cover_variant,
    toc_variant: bundle.toc_variant,
    citation_style: bundle.citation_style,
    theme: {
      preset: bundle.theme.preset,
      tokens: { ...(bundle.theme.tokens ?? {}) },
      custom_css: bundle.theme.custom_css ?? "",
      page_chrome: bundle.theme.page_chrome
        ? {
            header: { ...bundle.theme.page_chrome.header },
            footer: { ...bundle.theme.page_chrome.footer },
            hide_header_on_first_page:
              bundle.theme.page_chrome.hide_header_on_first_page,
            hide_footer_on_first_page:
              bundle.theme.page_chrome.hide_footer_on_first_page,
            heading_page_break: bundle.theme.page_chrome.heading_page_break,
          }
        : null,
    },
  };
}

function readUrlState(): {
  styleId: string | null;
  tab: StyleTabFilter;
  mode: ReportViewMode | null;
  customize: boolean;
} {
  if (typeof window === "undefined") {
    return { styleId: null, tab: "all", mode: null, customize: false };
  }
  const params = new URLSearchParams(window.location.search);
  const rawTab = params.get("tab");
  const tab: StyleTabFilter =
    rawTab === "builtin" || rawTab === "mine" || rawTab === "public"
      ? rawTab
      : "all";
  const rawMode = params.get("mode");
  const mode: ReportViewMode | null =
    rawMode === "paged" || rawMode === "continuous" ? rawMode : null;
  const customize = params.get("customize") === "1";
  return {
    styleId: params.get("style"),
    tab,
    mode,
    customize,
  };
}

function writeUrlState(next: {
  styleId?: string;
  tab?: StyleTabFilter;
  mode?: ReportViewMode;
  customize?: boolean;
}) {
  if (typeof window === "undefined") {
    return;
  }
  const url = new URL(window.location.href);
  if (next.styleId) {
    url.searchParams.set("style", next.styleId);
  }
  if (next.tab) {
    if (next.tab === "all") {
      url.searchParams.delete("tab");
    } else {
      url.searchParams.set("tab", next.tab);
    }
  }
  if (next.mode) {
    url.searchParams.set("mode", next.mode);
  }
  if (next.customize !== undefined) {
    if (next.customize) {
      url.searchParams.set("customize", "1");
    } else {
      url.searchParams.delete("customize");
    }
  }
  window.history.replaceState(null, "", `${url.pathname}${url.search}`);
}

function getDefaultStylePreset(): StylePresetRecord {
  const preset = BUILTIN_STYLE_PRESETS.find(
    (item) => item.style_id === "quire-folio",
  );
  if (!preset) {
    throw new Error("Missing quire-folio built-in preset");
  }
  return preset;
}

export function StyleCenterPage() {
  const queryClient = useQueryClient();
  const isMobile = useIsMobile();
  const { open: leftSidebarOpen, toggleSidebar } = useSidebar();
  const { user, isAdmin } = useAuth();
  const prefs = useUiPreferences();

  const initialUrl = useMemo(() => readUrlState(), []);
  const [tab, setTab] = useState<StyleTabFilter>(initialUrl.tab);
  const [selectedId, setSelectedId] = useState<string>(
    initialUrl.styleId || prefs.defaultStylePresetId || "quire-folio",
  );
  const [viewMode, setViewMode] = useState<ReportViewMode>(
    initialUrl.mode || prefs.reportViewMode,
  );
  const [customizerOpen, setCustomizerOpen] = useState<boolean>(
    initialUrl.customize,
  );

  // Save as new preset dialog state
  const [saveDialogOpen, setSaveDialogOpen] = useState(false);
  const [presetName, setPresetName] = useState("");
  const [presetDesc, setPresetDesc] = useState("");
  const [presetTag, setPresetTag] = useState("自定义");
  const [presetPublic, setPresetPublic] = useState(false);
  const [confirmDeleteId, setConfirmDeleteId] = useState<string | null>(null);

  const stylesQuery = useQuery({
    queryKey: ["style-presets"],
    queryFn: listStylePresets,
  });

  const allPresets = useMemo(
    () => stylesQuery.data ?? [...BUILTIN_STYLE_PRESETS],
    [stylesQuery.data],
  );

  const activePreset = useMemo(
    () =>
      allPresets.find((item) => item.style_id === selectedId) ??
      allPresets.find((item) => item.style_id === "quire-folio") ??
      allPresets[0] ??
      getDefaultStylePreset(),
    [allPresets, selectedId],
  );
  const [draftBundle, setDraftBundle] = useState<StylePresetBundle>(() =>
    cloneBundle(activePreset.bundle),
  );
  const [draftPresetId, setDraftPresetId] = useState(activePreset.style_id);
  const [isDirty, setIsDirty] = useState(false);

  if (draftPresetId !== activePreset.style_id) {
    setDraftPresetId(activePreset.style_id);
    setDraftBundle(cloneBundle(activePreset.bundle));
    setIsDirty(false);
    setConfirmDeleteId(null);
  }

  const selectPreset = (styleId: string) => {
    setSelectedId(styleId);
    writeUrlState({ styleId, tab, mode: viewMode, customize: customizerOpen });
  };

  const handleTabChange = (nextTab: StyleTabFilter) => {
    setTab(nextTab);
    writeUrlState({
      styleId: activePreset.style_id,
      tab: nextTab,
      mode: viewMode,
      customize: customizerOpen,
    });
  };

  const handleModeChange = (nextMode: ReportViewMode) => {
    setViewMode(nextMode);
    updatePreferences({ reportViewMode: nextMode });
    writeUrlState({
      styleId: activePreset.style_id,
      tab,
      mode: nextMode,
      customize: customizerOpen,
    });
  };

  const toggleCustomizer = () => {
    const next = !customizerOpen;
    setCustomizerOpen(next);
    writeUrlState({
      styleId: activePreset.style_id,
      tab,
      mode: viewMode,
      customize: next,
    });
  };

  const filteredPresets = useMemo(() => {
    return allPresets.filter((item) => {
      const isMine =
        Boolean(user?.user_id) && item.owner_user_id === user?.user_id;
      if (tab === "builtin") return item.is_builtin;
      if (tab === "mine") return !item.is_builtin && isMine;
      if (tab === "public") return !item.is_builtin && item.is_public;
      return true;
    });
  }, [allPresets, tab, user?.user_id]);

  const counts = useMemo(() => {
    const builtin = allPresets.filter((p) => p.is_builtin).length;
    const mine = allPresets.filter(
      (p) =>
        !p.is_builtin &&
        Boolean(user?.user_id) &&
        p.owner_user_id === user?.user_id,
    ).length;
    const pub = allPresets.filter((p) => !p.is_builtin && p.is_public).length;
    return {
      all: allPresets.length,
      builtin,
      mine,
      public: pub,
    };
  }, [allPresets, user?.user_id]);

  const specimen = useMemo(
    () => buildSpecimenDocument(draftBundle),
    [draftBundle],
  );

  const createMutation = useMutation({
    mutationFn: () =>
      createStylePreset(
        {
          name: presetName.trim(),
          description: presetDesc.trim(),
          category_tag: presetTag.trim() || "自定义",
          is_public: presetPublic,
          base_preset_id: activePreset.base_preset_id || activePreset.style_id,
          bundle: draftBundle,
        },
        {
          userId: user?.user_id,
          displayName: user?.display_name || user?.username,
        },
      ),
    onSuccess: (created) => {
      void queryClient.invalidateQueries({ queryKey: ["style-presets"] });
      setSaveDialogOpen(false);
      setPresetName("");
      setPresetDesc("");
      setIsDirty(false);
      selectPreset(created.style_id);
      toast.success(
        created.is_public
          ? `已保存并公开「${created.name}」`
          : `已保存预设「${created.name}」`,
      );
    },
    onError: (err) => {
      toast.error(errorMessage(err, "保存预设失败"));
    },
  });

  const updateMutation = useMutation({
    mutationFn: (payload: {
      styleId: string;
      is_public?: boolean;
      bundle?: StylePresetBundle;
    }) =>
      updateStylePreset(payload.styleId, {
        is_public: payload.is_public,
        bundle: payload.bundle,
      }),
    onSuccess: (updated) => {
      void queryClient.invalidateQueries({ queryKey: ["style-presets"] });
      setIsDirty(false);
      toast.success(`已更新「${updated.name}」`);
    },
    onError: (err) => {
      toast.error(errorMessage(err, "更新预设失败"));
    },
  });

  const deleteMutation = useMutation({
    mutationFn: (styleId: string) => deleteStylePreset(styleId),
    onSuccess: (_data, deletedId) => {
      void queryClient.invalidateQueries({ queryKey: ["style-presets"] });
      setConfirmDeleteId(null);
      if (prefs.defaultStylePresetId === deletedId) {
        updatePreferences({ defaultStylePresetId: "quire-folio" });
      }
      selectPreset("quire-folio");
      toast.success("已删除自定义样式");
    },
    onError: (err) => {
      toast.error(errorMessage(err, "删除样式失败"));
    },
  });

  const isCurrentMine =
    !activePreset.is_builtin &&
    (isAdmin ||
      (Boolean(user?.user_id) &&
        activePreset.owner_user_id === user?.user_id) ||
      activePreset.owner_user_id === "local");

  const isCurrentDefault = prefs.defaultStylePresetId === activePreset.style_id;

  const openSaveDialog = () => {
    setPresetName(
      activePreset.is_builtin
        ? `${activePreset.name} · 定制`
        : `${activePreset.name} 副本`,
    );
    setPresetDesc(activePreset.description);
    setPresetTag("自定义");
    setPresetPublic(false);
    setSaveDialogOpen(true);
  };

  const handleSaveSubmit = (e: FormEvent) => {
    e.preventDefault();
    if (!presetName.trim()) {
      toast.error("请输入样式名称");
      return;
    }
    createMutation.mutate();
  };

  return (
    <div className="relative flex h-full min-w-0 flex-col overflow-hidden bg-background">
      {!isMobile && (
        <SidebarEdgeHandle
          side="left"
          open={leftSidebarOpen}
          label={leftSidebarOpen ? "收起左侧栏" : "展开左侧栏"}
          onToggle={toggleSidebar}
        />
      )}

      {/* Top Bar */}
      <header className="flex h-12 shrink-0 items-center justify-between gap-3 border-b border-border/90 bg-card/95 px-3.5 backdrop-blur-xs">
        <div className="flex min-w-0 items-center gap-2.5">
          {isMobile && (
            <Button
              type="button"
              size="icon-sm"
              variant="ghost"
              aria-label="切换左侧栏"
              onClick={toggleSidebar}
            >
              <PanelLeftIcon className="size-4" />
            </Button>
          )}
          <h1 className="font-serif text-[15px] font-semibold tracking-tight text-foreground">
            样式中心
          </h1>

          {/* Category Filter Tabs */}
          <ToggleGroup
            multiple={false}
            value={[tab]}
            onValueChange={(values) => {
              const next = values[0];
              if (
                next === "all" ||
                next === "builtin" ||
                next === "mine" ||
                next === "public"
              ) {
                handleTabChange(next);
              }
            }}
            spacing={0}
            className="ml-1 rounded-lg border border-border/85 bg-secondary/75 p-0.5"
          >
            {(
              [
                { key: "all", label: "全部", count: counts.all },
                { key: "builtin", label: "内置", count: counts.builtin },
                { key: "mine", label: "我的", count: counts.mine },
                { key: "public", label: "公开", count: counts.public },
              ] as const
            ).map((item) => (
              <ToggleGroupItem
                key={item.key}
                value={item.key}
                size="sm"
                className="h-6 min-w-0 gap-1 rounded-md px-2 text-xs text-muted-foreground transition-colors aria-pressed:bg-card aria-pressed:font-medium aria-pressed:text-foreground aria-pressed:shadow-2xs"
              >
                <span>{item.label}</span>
                <span className="font-mono text-[10px] opacity-70 tabular-nums">
                  {item.count}
                </span>
              </ToggleGroupItem>
            ))}
          </ToggleGroup>
        </div>

        {/* Right Actions: Active Preset Actions & View Mode */}
        <div className="flex shrink-0 items-center gap-1.5">
          {!activePreset.is_builtin && isCurrentMine && (
            <>
              <Button
                type="button"
                size="xs"
                variant="outline"
                disabled={updateMutation.isPending}
                onClick={() =>
                  updateMutation.mutate({
                    styleId: activePreset.style_id,
                    is_public: !activePreset.is_public,
                  })
                }
                className="gap-1"
              >
                {activePreset.is_public ? (
                  <>
                    <GlobeIcon className="size-3 text-[#1D4ED8]" />
                    <span className="hidden sm:inline">已公开</span>
                  </>
                ) : (
                  <>
                    <LockIcon className="size-3 text-muted-foreground" />
                    <span className="hidden sm:inline">设为公开</span>
                  </>
                )}
              </Button>

              {isDirty && (
                <Button
                  type="button"
                  size="xs"
                  disabled={updateMutation.isPending}
                  onClick={() =>
                    updateMutation.mutate({
                      styleId: activePreset.style_id,
                      bundle: draftBundle,
                    })
                  }
                >
                  保存修改
                </Button>
              )}

              <Button
                type="button"
                size="xs"
                variant={
                  confirmDeleteId === activePreset.style_id
                    ? "destructive"
                    : "ghost"
                }
                aria-label={
                  confirmDeleteId === activePreset.style_id
                    ? "确认删除此样式"
                    : "删除此样式"
                }
                disabled={deleteMutation.isPending}
                onClick={() => {
                  if (confirmDeleteId !== activePreset.style_id) {
                    setConfirmDeleteId(activePreset.style_id);
                    return;
                  }
                  deleteMutation.mutate(activePreset.style_id);
                }}
              >
                <Trash2Icon className="size-3.5" aria-hidden="true" />
                {confirmDeleteId === activePreset.style_id && (
                  <span className="ml-1">确认删除</span>
                )}
              </Button>
            </>
          )}

          <Button
            type="button"
            size="xs"
            variant={isCurrentDefault ? "secondary" : "outline"}
            onClick={() => {
              updatePreferences({
                defaultStylePresetId: activePreset.style_id,
              });
              toast.success(`已将「${activePreset.name}」设为默认样式`);
            }}
          >
            {isCurrentDefault ? (
              <>
                <CheckIcon
                  className="mr-1 size-3 text-[#1D4ED8]"
                  aria-hidden="true"
                />
                当前默认
              </>
            ) : (
              "设为默认"
            )}
          </Button>

          <Button
            type="button"
            size="xs"
            variant={customizerOpen ? "secondary" : "outline"}
            onClick={toggleCustomizer}
            className="gap-1"
          >
            <SlidersHorizontalIcon className="size-3" aria-hidden="true" />
            <span>调优</span>
            {isDirty && (
              <span
                className="size-1.5 rounded-full bg-[#1D4ED8]"
                aria-hidden="true"
              />
            )}
          </Button>

          <Button
            type="button"
            size="xs"
            onClick={openSaveDialog}
            className="gap-1"
          >
            <PlusIcon className="size-3" aria-hidden="true" />
            <span className="hidden sm:inline">保存为预设</span>
          </Button>

          <span className="mx-0.5 h-4 w-px bg-border/80" aria-hidden="true" />

          <ToggleGroup
            multiple={false}
            value={[viewMode]}
            onValueChange={(values) => {
              const next = values[0];
              if (next === "continuous" || next === "paged") {
                handleModeChange(next);
              }
            }}
            spacing={0}
            className="rounded-lg border border-border/90 bg-secondary/75 p-0.5"
          >
            <ToggleGroupItem
              value="continuous"
              size="sm"
              className="h-6 min-w-0 rounded-md px-2 text-xs text-muted-foreground transition-colors aria-pressed:bg-card aria-pressed:font-medium aria-pressed:text-foreground aria-pressed:shadow-2xs"
            >
              <ScrollTextIcon className="size-3.5 sm:mr-1" aria-hidden="true" />
              <span className="hidden sm:inline">连续</span>
            </ToggleGroupItem>
            <ToggleGroupItem
              value="paged"
              size="sm"
              className="h-6 min-w-0 rounded-md px-2 text-xs text-muted-foreground transition-colors aria-pressed:bg-card aria-pressed:font-medium aria-pressed:text-foreground aria-pressed:shadow-2xs"
            >
              <BookOpenIcon className="size-3.5 sm:mr-1" aria-hidden="true" />
              <span className="hidden sm:inline">分页</span>
            </ToggleGroupItem>
          </ToggleGroup>
        </div>
      </header>

      {/* Main Split Workspace */}
      <div className="flex min-h-0 flex-1 flex-col md:flex-row">
        {/* Left Gallery Column */}
        <aside
          aria-label="样式列表"
          className="no-scrollbar flex w-full shrink-0 flex-col gap-2.5 overflow-x-auto border-b border-border/80 bg-sidebar/55 p-3 md:w-[300px] md:overflow-y-auto md:border-r md:border-b-0 lg:w-[320px]"
        >
          <div className="flex gap-2.5 md:flex-col">
            {filteredPresets.map((preset: StylePresetRecord) => {
              const isMine =
                Boolean(user?.user_id) &&
                preset.owner_user_id === user?.user_id;
              return (
                <div
                  key={preset.style_id}
                  className="w-[240px] shrink-0 md:w-full"
                >
                  <StyleThumbnailCard
                    preset={preset}
                    selected={preset.style_id === activePreset.style_id}
                    isDefault={prefs.defaultStylePresetId === preset.style_id}
                    isMine={isMine}
                    onSelect={() => selectPreset(preset.style_id)}
                  />
                </div>
              );
            })}
          </div>

          {filteredPresets.length === 0 && (
            <div className="my-8 rounded-xl border border-dashed border-border px-4 py-6 text-center text-xs text-muted-foreground">
              <p>暂无自定义样式</p>
              <Button
                type="button"
                size="xs"
                variant="outline"
                onClick={() => {
                  setCustomizerOpen(true);
                }}
                className="mt-2.5"
              >
                新建预设样式
              </Button>
            </div>
          )}
        </aside>

        {/* Center Live Specimen Report Canvas */}
        <main
          aria-label="样式实时标本预览"
          className="quire-desk-surface no-scrollbar relative min-h-0 min-w-0 flex-1 overflow-x-hidden overflow-y-auto overscroll-contain"
        >
          <div className="py-4">
            <ReportViewer
              document={specimen.document}
              computed={specimen.computed}
              viewMode={viewMode}
              pageChrome={specimen.pageChrome}
            />
          </div>
        </main>

        {/* Right Customizer Drawer */}
        {customizerOpen && (
          <aside
            aria-label="样式调优面板"
            className="no-scrollbar flex w-full shrink-0 flex-col border-t border-border/90 bg-card md:w-[320px] md:border-t-0 md:border-l lg:w-[340px]"
          >
            <div className="flex h-10 shrink-0 items-center justify-between border-b border-border/80 px-3.5">
              <div className="flex items-center gap-2 min-w-0">
                <span className="truncate text-xs font-semibold text-foreground">
                  {activePreset.name}
                </span>
                <span className="rounded bg-secondary px-1.5 py-0.2 text-[10px] text-muted-foreground">
                  {activePreset.category_tag}
                </span>
              </div>
              <button
                type="button"
                aria-label="关闭调优面板"
                onClick={toggleCustomizer}
                className="rounded p-1 text-muted-foreground hover:bg-secondary hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none"
              >
                <XIcon className="size-3.5" aria-hidden="true" />
              </button>
            </div>

            <div className="no-scrollbar flex-1 overflow-y-auto overscroll-contain p-3.5">
              <StyleCustomizerPanel
                bundle={draftBundle}
                onChange={(next) => {
                  setDraftBundle(next);
                  setIsDirty(true);
                }}
                onReset={() => {
                  setDraftBundle(cloneBundle(activePreset.bundle));
                  setIsDirty(false);
                }}
              />
            </div>

            <div className="flex items-center justify-between gap-2 border-t border-border/80 bg-muted/25 px-3.5 py-2.5">
              {!activePreset.is_builtin && isCurrentMine ? (
                <Button
                  type="button"
                  size="sm"
                  variant="outline"
                  disabled={!isDirty || updateMutation.isPending}
                  onClick={() =>
                    updateMutation.mutate({
                      styleId: activePreset.style_id,
                      bundle: draftBundle,
                    })
                  }
                  className="flex-1 text-xs"
                >
                  更新当前预设
                </Button>
              ) : null}
              <Button
                type="button"
                size="sm"
                onClick={openSaveDialog}
                className="flex-1 text-xs"
              >
                <PlusIcon className="mr-1 size-3.5" aria-hidden="true" />
                另存为新预设
              </Button>
            </div>
          </aside>
        )}
      </div>

      {/* Save Preset Modal */}
      <Dialog open={saveDialogOpen} onOpenChange={setSaveDialogOpen}>
        <DialogContent className="sm:max-w-md">
          <DialogHeader>
            <DialogTitle>保存为样式预设</DialogTitle>
          </DialogHeader>
          <form onSubmit={handleSaveSubmit} className="space-y-3.5">
            <div className="space-y-1.5">
              <Label htmlFor="save-preset-name" className="text-xs">
                样式名称
              </Label>
              <Input
                id="save-preset-name"
                name="save-preset-name"
                autoComplete="off"
                placeholder="例如：学科建设汇报公文样式…"
                value={presetName}
                onChange={(e) => setPresetName(e.target.value)}
              />
            </div>

            <div className="grid grid-cols-2 gap-2.5">
              <div className="space-y-1.5">
                <Label htmlFor="save-preset-tag" className="text-xs">
                  分类标签
                </Label>
                <Input
                  id="save-preset-tag"
                  name="save-preset-tag"
                  autoComplete="off"
                  placeholder="自由 / 轻快 / 正式…"
                  value={presetTag}
                  onChange={(e) => setPresetTag(e.target.value)}
                />
              </div>
              <div className="space-y-1.5">
                <Label htmlFor="save-preset-desc" className="text-xs">
                  备注（选填）
                </Label>
                <Input
                  id="save-preset-desc"
                  name="save-preset-desc"
                  autoComplete="off"
                  placeholder="适用场景说明…"
                  value={presetDesc}
                  onChange={(e) => setPresetDesc(e.target.value)}
                />
              </div>
            </div>

            <label
              htmlFor="save-preset-public"
              className="flex cursor-pointer items-center justify-between rounded-lg border border-border bg-muted/30 px-3 py-2.5"
            >
              <div className="flex items-center gap-2">
                <GlobeIcon
                  className="size-4 text-[#1D4ED8]"
                  aria-hidden="true"
                />
                <span className="text-xs font-medium text-foreground">
                  公开至样式中心（允许他人预览与使用）
                </span>
              </div>
              <Switch
                id="save-preset-public"
                checked={presetPublic}
                onCheckedChange={setPresetPublic}
                aria-label="公开至样式中心"
              />
            </label>

            <DialogFooter className="pt-1">
              <Button
                type="button"
                variant="ghost"
                onClick={() => setSaveDialogOpen(false)}
              >
                取消
              </Button>
              <Button type="submit" disabled={createMutation.isPending}>
                {createMutation.isPending && (
                  <LoaderCircleIcon className="mr-1.5 size-3.5 animate-spin" />
                )}
                保存预设
              </Button>
            </DialogFooter>
          </form>
        </DialogContent>
      </Dialog>
    </div>
  );
}
