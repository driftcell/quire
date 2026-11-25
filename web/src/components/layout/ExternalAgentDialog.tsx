import { useState, type FormEvent } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import {
  CheckIcon,
  CopyIcon,
  ExternalLinkIcon,
  KeyRoundIcon,
  LoaderCircleIcon,
  PlusIcon,
  TerminalIcon,
} from "lucide-react";
import { toast } from "sonner";
import { createApiKey, listApiKeys, type CreatedApiKeyItem } from "@/api/auth";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { errorMessage } from "@/lib/errors";

export interface ExternalAgentDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

export function getMcpEndpointUrl(): string {
  if (typeof window === "undefined") {
    return "http://localhost:8080/mcp";
  }
  return `${window.location.origin}/mcp`;
}

export function ExternalAgentDialog({
  open,
  onOpenChange,
}: ExternalAgentDialogProps) {
  const queryClient = useQueryClient();
  const [keyName, setKeyName] = useState("Claude Code / Cursor");
  const [createdKey, setCreatedKey] = useState<CreatedApiKeyItem | null>(null);
  const [copiedTarget, setCopiedTarget] = useState<
    "url" | "key" | "config" | null
  >(null);

  const apiKeysQuery = useQuery({
    queryKey: ["auth", "api-keys"],
    queryFn: listApiKeys,
    enabled: open,
    refetchOnMount: "always",
  });
  const createKeyMutation = useMutation({
    mutationFn: (name: string) => createApiKey(name),
    onSuccess: (created) => {
      setCreatedKey(created);
      toast.success("已创建 API 密钥，请妥善保存单次显示的明文密钥");
      void queryClient.invalidateQueries({ queryKey: ["auth", "api-keys"] });
    },
    onError: (error) => {
      toast.error(errorMessage(error, "创建 API 密钥失败"));
    },
  });

  const mcpUrl = getMcpEndpointUrl();
  const activeTokenDisplay = createdKey?.secret_key ?? "qk_<你的_API_KEY>";

  const mcpJsonSnippet = JSON.stringify(
    {
      mcpServers: {
        "quire-report": {
          type: "streamable-http",
          url: mcpUrl,
          headers: {
            Authorization: `Bearer ${activeTokenDisplay}`,
          },
        },
      },
    },
    null,
    2,
  );

  async function copyToClipboard(
    text: string,
    target: "url" | "key" | "config",
    label: string,
  ) {
    try {
      await navigator.clipboard.writeText(text);
      setCopiedTarget(target);
      toast.success(`已复制${label}`);
      window.setTimeout(() => {
        setCopiedTarget((current) => (current === target ? null : current));
      }, 2000);
    } catch {
      toast.error("复制失败，请手动选择复制");
    }
  }

  function handleQuickCreate(event: FormEvent) {
    event.preventDefault();
    if (!keyName.trim()) {
      toast.error("请输入密钥名称");
      return;
    }
    createKeyMutation.mutate(keyName.trim());
  }

  const existingKeysCount = apiKeysQuery.data?.length ?? 0;

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <div className="flex items-center gap-2">
            <span className="inline-flex size-7 items-center justify-center rounded-md border border-border/80 bg-muted/40 text-foreground">
              <TerminalIcon className="size-3.5" aria-hidden="true" />
            </span>
            <DialogTitle className="text-base font-semibold tracking-tight">
              使用外部代理（MCP 接入）
            </DialogTitle>
          </div>
          <DialogDescription className="text-xs leading-relaxed text-muted-foreground">
            将 Quire 的结构化报告创作工具通过{" "}
            <code className="rounded bg-muted px-1 py-0.5 font-mono text-[11px] text-foreground">
              /mcp
            </code>{" "}
            端点暴露给 Claude Code、Cursor、OpenCode 等外部
            Harness。外部代理创建报告后，左侧边栏会自动绑定一个带{" "}
            <span className="inline-flex items-center rounded border border-border/80 bg-muted/50 px-1 py-0 text-[10px] font-medium tracking-wider text-muted-foreground">
              MCP
            </span>{" "}
            标签的独立会话，供随时审阅、批注或由内置助手接力润色。
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-4 py-1">
          {/* Step 1: MCP Endpoint URL */}
          <div className="space-y-1.5">
            <div className="flex items-center justify-between">
              <Label
                htmlFor="external-mcp-endpoint-url"
                className="text-xs font-medium text-foreground"
              >
                1. MCP 服务端点（Streamable HTTP）
              </Label>
              <span className="font-mono text-[11px] text-muted-foreground">
                POST / GET / DELETE
              </span>
            </div>
            <div className="flex items-center gap-1.5">
              <Input
                id="external-mcp-endpoint-url"
                readOnly
                value={mcpUrl}
                aria-label="MCP 服务端点地址"
                className="h-8 font-mono text-xs"
              />
              <Button
                type="button"
                variant="outline"
                size="sm"
                className="h-8 shrink-0 gap-1.5 px-2.5 text-xs"
                onClick={() =>
                  void copyToClipboard(mcpUrl, "url", " MCP 端点地址")
                }
              >
                {copiedTarget === "url" ? (
                  <CheckIcon
                    className="size-3.5 text-emerald-600"
                    aria-hidden="true"
                  />
                ) : (
                  <CopyIcon className="size-3.5" aria-hidden="true" />
                )}
                {copiedTarget === "url" ? "已复制" : "复制地址"}
              </Button>
            </div>
          </div>

          {/* Step 2: Quick Create API Key */}
          <div className="space-y-2 rounded-lg border border-border/70 bg-muted/20 p-3">
            <div className="flex items-center justify-between gap-2">
              <div className="flex items-center gap-1.5">
                <KeyRoundIcon
                  className="size-3.5 text-muted-foreground"
                  aria-hidden="true"
                />
                <span className="text-xs font-medium text-foreground">
                  2. 认证 API Key
                </span>
              </div>
              <span className="inline-flex items-center gap-1.5 text-[11px] text-muted-foreground tabular-nums">
                {apiKeysQuery.isFetching && (
                  <LoaderCircleIcon
                    className="size-3 animate-spin"
                    aria-hidden="true"
                  />
                )}
                <span>当前已有 {existingKeysCount} 个有效密钥</span>
              </span>
            </div>

            <form
              onSubmit={handleQuickCreate}
              className="flex items-center gap-1.5"
            >
              <Input
                id="quick-create-apikey-name"
                value={keyName}
                onChange={(e) => setKeyName(e.target.value)}
                placeholder="输入外部代理名称，如 Claude Code…"
                aria-label="新 API 密钥名称"
                autoComplete="off"
                className="h-8 bg-background text-xs"
              />
              <Button
                type="submit"
                size="sm"
                disabled={createKeyMutation.isPending || !keyName.trim()}
                className="h-8 shrink-0 gap-1.5 px-2.5 text-xs"
              >
                {createKeyMutation.isPending ? (
                  <LoaderCircleIcon
                    className="size-3.5 animate-spin"
                    aria-hidden="true"
                  />
                ) : (
                  <PlusIcon className="size-3.5" aria-hidden="true" />
                )}
                快捷创建密钥
              </Button>
            </form>

            {createdKey ? (
              <div className="mt-2 space-y-1.5 rounded-md border border-emerald-500/30 bg-emerald-500/5 p-2.5">
                <div className="flex items-center justify-between gap-2">
                  <span className="text-[11px] font-medium text-emerald-700 dark:text-emerald-300">
                    明文密钥仅显示一次，请立即复制保存：
                  </span>
                  <Button
                    type="button"
                    variant="ghost"
                    size="xs"
                    className="h-6 gap-1 px-2 text-[11px]"
                    onClick={() =>
                      void copyToClipboard(
                        createdKey.secret_key,
                        "key",
                        " API 密钥",
                      )
                    }
                  >
                    {copiedTarget === "key" ? (
                      <CheckIcon
                        className="size-3 text-emerald-600"
                        aria-hidden="true"
                      />
                    ) : (
                      <CopyIcon className="size-3" aria-hidden="true" />
                    )}
                    {copiedTarget === "key" ? "已复制" : "复制密钥"}
                  </Button>
                </div>
                <div className="overflow-x-auto rounded border border-border/60 bg-background px-2 py-1 font-mono text-[11px] break-all select-all text-foreground">
                  {createdKey.secret_key}
                </div>
              </div>
            ) : null}
          </div>

          {/* Step 3: Configuration snippet */}
          <div className="space-y-1.5">
            <div className="flex items-center justify-between">
              <span className="text-xs font-medium text-foreground">
                3. 外部代理配置示例（mcp.json）
              </span>
              <Button
                type="button"
                variant="ghost"
                size="xs"
                className="h-6 gap-1 px-2 text-[11px] text-muted-foreground hover:text-foreground"
                onClick={() =>
                  void copyToClipboard(
                    mcpJsonSnippet,
                    "config",
                    " MCP 配置片段",
                  )
                }
              >
                {copiedTarget === "config" ? (
                  <CheckIcon
                    className="size-3 text-emerald-600"
                    aria-hidden="true"
                  />
                ) : (
                  <CopyIcon className="size-3" aria-hidden="true" />
                )}
                {copiedTarget === "config" ? "已复制配置" : "复制 JSON"}
              </Button>
            </div>
            <pre className="max-h-40 overflow-auto rounded-lg border border-border/70 bg-muted/30 p-2.5 font-mono text-[11px] leading-relaxed text-foreground">
              <code>{mcpJsonSnippet}</code>
            </pre>
          </div>
        </div>

        <DialogFooter className="flex-row items-center justify-between sm:justify-between">
          <Link
            to="/admin"
            onClick={() => onOpenChange(false)}
            className="inline-flex items-center gap-1 text-xs text-muted-foreground transition-colors hover:text-foreground"
          >
            前往 API Key 控制台管理全部密钥
            <ExternalLinkIcon className="size-3" aria-hidden="true" />
          </Link>
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={() => onOpenChange(false)}
          >
            完成
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
