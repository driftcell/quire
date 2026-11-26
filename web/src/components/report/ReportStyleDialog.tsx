import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import {
  ArrowUpRightIcon,
  CheckIcon,
  GlobeIcon,
  LoaderCircleIcon,
  PaletteIcon,
  PlusIcon,
} from "lucide-react";
import { toast } from "sonner";
import {
  applyStyleToDocument,
  createStylePreset,
  listStylePresets,
} from "@/api/styles";
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
import type {
  CoverConfig,
  ReferencesConfig,
  ThemeConfig,
  TocConfig,
} from "@/contracts/report-protocol";
import { useAuth } from "@/hooks/useAuth";
import { errorMessage } from "@/lib/errors";
import { updatePreferences, useUiPreferences } from "@/lib/preferences";
import type { StylePresetRecord } from "@/lib/stylePresets";

export interface ReportStyleDialogProps {
  docId: string;
  currentTheme: ThemeConfig;
  currentCover?: CoverConfig;
  currentToc?: TocConfig;
  currentReferences?: ReferencesConfig;
}

export function ReportStyleDialog({
  docId,
  currentTheme,
  currentCover,
  currentToc,
  currentReferences,
}: ReportStyleDialogProps) {
  const queryClient = useQueryClient();
  const { user } = useAuth();
  const prefs = useUiPreferences();
  const [open, setOpen] = useState(false);
  const [saveMode, setSaveMode] = useState(false);
  const [newName, setNewName] = useState("");
  const [newTag, setNewTag] = useState("自定义");
  const [isPublic, setIsPublic] = useState(false);

  const stylesQuery = useQuery({
    queryKey: ["style-presets"],
    queryFn: listStylePresets,
    enabled: open,
  });

  const applyMutation = useMutation({
    mutationFn: (preset: StylePresetRecord) =>
      applyStyleToDocument(docId, {
        style_id: preset.style_id,
        bundle: preset.bundle,
        sync_layout_variants: true,
      }),
    onSuccess: (_res, preset) => {
      updatePreferences({ defaultStylePresetId: preset.style_id });
      void queryClient.invalidateQueries({ queryKey: ["doc", docId] });
      toast.success(`已应用「${preset.name}」`);
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
          base_preset_id: currentTheme.preset ?? "quire-folio",
          bundle: {
            theme: currentTheme,
            cover_variant: currentCover?.variant ?? "split",
            toc_variant: currentToc?.variant ?? "dotted",
            citation_style: currentReferences?.style ?? "numeric",
          },
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

  const presets = stylesQuery.data ?? [];
  const activePresetId = currentTheme.preset || "quire-folio";

  return (
    <Dialog open={open} onOpenChange={setOpen}>
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

      <DialogContent className="no-scrollbar max-h-[86vh] overflow-y-auto overscroll-contain sm:max-w-3xl">
        <DialogHeader className="flex flex-row items-center justify-between pr-6">
          <DialogTitle>报告样式</DialogTitle>
          <div className="flex items-center gap-2">
            <Button
              type="button"
              size="xs"
              variant={saveMode ? "secondary" : "outline"}
              onClick={() => setSaveMode((prev) => !prev)}
            >
              <PlusIcon className="mr-1 size-3" aria-hidden="true" />
              将当前报告样式存为预设
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

        <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 md:grid-cols-3">
          {presets.map((preset) => {
            const isSelected =
              preset.style_id === activePresetId ||
              preset.bundle.theme.preset === activePresetId;
            const isMine =
              Boolean(user?.user_id) && preset.owner_user_id === user?.user_id;
            return (
              <StyleThumbnailCard
                key={preset.style_id}
                preset={preset}
                selected={isSelected}
                isDefault={prefs.defaultStylePresetId === preset.style_id}
                isMine={isMine}
                onSelect={() => applyMutation.mutate(preset)}
              />
            );
          })}
        </div>
      </DialogContent>
    </Dialog>
  );
}
