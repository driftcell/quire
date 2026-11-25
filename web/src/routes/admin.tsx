import { useEffect, useState, type FormEvent } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { createFileRoute, useNavigate } from "@tanstack/react-router";
import {
  ArrowLeftIcon,
  BotIcon,
  CheckIcon,
  CopyIcon,
  KeyRoundIcon,
  LoaderCircleIcon,
  PanelLeftIcon,
  PlusIcon,
  RefreshCwIcon,
  ServerIcon,
  ShieldCheckIcon,
  Trash2Icon,
  UsersIcon,
} from "lucide-react";
import { toast } from "sonner";
import {
  createModel,
  createModelProvider,
  listAdminUsers,
  listMcpServers,
  listModelProviders,
  listModels,
  syncMcpServer,
  updateAdminUser,
  updateModel,
} from "@/api/admin";
import {
  createApiKey,
  deleteApiKey,
  listApiKeys,
  type CreatedApiKeyItem,
} from "@/api/auth";
import { getMcpEndpointUrl } from "@/components/layout/ExternalAgentDialog";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { useSidebar } from "@/components/ui/sidebar";
import { useIsMobile } from "@/hooks/use-mobile";
import { useAuth } from "@/hooks/useAuth";
import { errorMessage } from "@/lib/errors";

export const Route = createFileRoute("/admin")({
  component: AdminPage,
});

type AdminTab = "api_keys" | "users" | "models" | "mcp";

const dateTimeFormatter = new Intl.DateTimeFormat("zh-CN", {
  year: "numeric",
  month: "2-digit",
  day: "2-digit",
  hour: "2-digit",
  minute: "2-digit",
});

function formatTimestamp(iso?: string | null): string {
  if (!iso) return "从未使用";
  const parsed = new Date(iso);
  if (Number.isNaN(parsed.getTime())) return iso;
  return dateTimeFormatter.format(parsed);
}

function AdminPage() {
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const isMobile = useIsMobile();
  const { toggleSidebar } = useSidebar();
  const { user, isAdmin } = useAuth();
  const [tab, setTab] = useState<AdminTab>("api_keys");

  // API Key form state
  const [newKeyName, setNewKeyName] = useState("");
  const [latestCreatedKey, setLatestCreatedKey] =
    useState<CreatedApiKeyItem | null>(null);
  const [confirmingKeyId, setConfirmingKeyId] = useState<string | null>(null);
  const [copiedId, setCopiedId] = useState<string | null>(null);

  // Provider form state
  const [providerId, setProviderId] = useState("");
  const [providerName, setProviderName] = useState("");
  const [protocol, setProtocol] = useState<"anthropic" | "openai_compatible">(
    "anthropic",
  );
  const [baseUrl, setBaseUrl] = useState("");
  const [apiKeyRef, setApiKeyRef] = useState("");

  // Model form state
  const [modelRouteId, setModelRouteId] = useState("");
  const [modelProviderId, setModelProviderId] = useState("");
  const [modelName, setModelName] = useState("");
  const [displayName, setDisplayName] = useState("");
  const [contextLimit, setContextLimit] = useState("128000");

  useEffect(() => {
    if (!confirmingKeyId) return undefined;
    const timer = window.setTimeout(() => setConfirmingKeyId(null), 3500);
    return () => window.clearTimeout(timer);
  }, [confirmingKeyId]);

  const apiKeysQuery = useQuery({
    queryKey: ["auth", "api-keys"],
    queryFn: listApiKeys,
    enabled: tab === "api_keys",
    refetchOnMount: "always",
  });

  const usersQuery = useQuery({
    queryKey: ["admin", "users"],
    queryFn: listAdminUsers,
    enabled: isAdmin && tab === "users",
  });

  const providersQuery = useQuery({
    queryKey: ["admin", "providers"],
    queryFn: listModelProviders,
    enabled: isAdmin && tab === "models",
  });

  const modelsQuery = useQuery({
    queryKey: ["admin", "models"],
    queryFn: listModels,
    enabled: isAdmin && tab === "models",
  });

  const mcpQuery = useQuery({
    queryKey: ["admin", "mcp"],
    queryFn: listMcpServers,
    enabled: isAdmin && tab === "mcp",
  });

  const createApiKeyMutation = useMutation({
    mutationFn: (name: string) => createApiKey(name),
    onSuccess: (created) => {
      setLatestCreatedKey(created);
      setNewKeyName("");
      toast.success("API 密钥已创建，请立即复制保存");
      void queryClient.invalidateQueries({ queryKey: ["auth", "api-keys"] });
    },
    onError: (err) => {
      toast.error(errorMessage(err, "创建 API 密钥失败"));
    },
  });

  const deleteApiKeyMutation = useMutation({
    mutationFn: (keyId: string) => deleteApiKey(keyId),
    onSuccess: (_data, deletedId) => {
      setConfirmingKeyId(null);
      if (latestCreatedKey?.key_id === deletedId) {
        setLatestCreatedKey(null);
      }
      toast.success("API 密钥已撤销");
      void queryClient.invalidateQueries({ queryKey: ["auth", "api-keys"] });
    },
    onError: (err) => {
      toast.error(errorMessage(err, "撤销 API 密钥失败"));
    },
  });

  const userMutation = useMutation({
    mutationFn: ({
      userId,
      patch,
    }: {
      userId: string;
      patch: { role?: "admin" | "user"; enabled?: boolean };
    }) => updateAdminUser(userId, patch),
    onSuccess: () => {
      toast.success("用户状态已更新");
      void queryClient.invalidateQueries({ queryKey: ["admin", "users"] });
    },
    onError: (err) => {
      toast.error(errorMessage(err, "更新用户失败"));
    },
  });

  const providerMutation = useMutation({
    mutationFn: createModelProvider,
    onSuccess: () => {
      toast.success("供应商已保存");
      setProviderId("");
      setProviderName("");
      setBaseUrl("");
      setApiKeyRef("");
      void queryClient.invalidateQueries({ queryKey: ["admin", "providers"] });
    },
    onError: (err) => {
      toast.error(errorMessage(err, "保存供应商失败"));
    },
  });

  const modelMutation = useMutation({
    mutationFn: createModel,
    onSuccess: () => {
      toast.success("模型路由已添加");
      setModelRouteId("");
      setModelName("");
      setDisplayName("");
      void queryClient.invalidateQueries({ queryKey: ["admin", "models"] });
    },
    onError: (err) => {
      toast.error(errorMessage(err, "添加模型失败"));
    },
  });

  const toggleModelMutation = useMutation({
    mutationFn: ({ id, enabled }: { id: string; enabled: boolean }) =>
      updateModel(id, { enabled }),
    onSuccess: () => {
      toast.success("模型状态已更新");
      void queryClient.invalidateQueries({ queryKey: ["admin", "models"] });
    },
    onError: (err) => {
      toast.error(errorMessage(err, "更新模型状态失败"));
    },
  });

  const syncMcpMutation = useMutation({
    mutationFn: syncMcpServer,
    onSuccess: () => {
      toast.success("MCP 工具已同步");
      void queryClient.invalidateQueries({ queryKey: ["admin", "mcp"] });
    },
    onError: (err) => {
      toast.error(errorMessage(err, "同步 MCP 失败"));
    },
  });

  const handleCreateApiKey = (e: FormEvent) => {
    e.preventDefault();
    createApiKeyMutation.mutate(newKeyName.trim() || "外部代理密钥");
  };

  const copyValue = async (text: string, id: string, label: string) => {
    try {
      await navigator.clipboard.writeText(text);
      setCopiedId(id);
      toast.success(`已复制${label}`);
      window.setTimeout(() => {
        setCopiedId((curr) => (curr === id ? null : curr));
      }, 2000);
    } catch {
      toast.error(`复制${label}失败，请手动选中复制`);
    }
  };

  const handleCreateProvider = (e: FormEvent) => {
    e.preventDefault();
    if (
      !providerId.trim() ||
      !providerName.trim() ||
      !baseUrl.trim() ||
      !apiKeyRef.trim()
    ) {
      toast.error("请完整填写供应商信息");
      return;
    }
    providerMutation.mutate({
      provider_id: providerId.trim(),
      name: providerName.trim(),
      protocol,
      base_url: baseUrl.trim(),
      api_key_ref: apiKeyRef.trim(),
    });
  };

  const handleCreateModel = (e: FormEvent) => {
    e.preventDefault();
    const limit = Number(contextLimit);
    if (
      !modelRouteId.trim() ||
      !modelProviderId.trim() ||
      !modelName.trim() ||
      !displayName.trim() ||
      !Number.isFinite(limit) ||
      limit <= 0
    ) {
      toast.error("请完整填写模型路由信息");
      return;
    }
    modelMutation.mutate({
      id: modelRouteId.trim(),
      provider_id: modelProviderId.trim(),
      model_id: modelName.trim(),
      display_name: displayName.trim(),
      context_limit: Math.floor(limit),
    });
  };

  const mcpEndpointUrl = getMcpEndpointUrl();

  return (
    <div className="flex h-full min-w-0 flex-col overflow-x-hidden overflow-y-auto bg-background">
      <header className="flex shrink-0 flex-col gap-3 border-b border-border/60 px-4 py-3 sm:flex-row sm:items-center sm:justify-between sm:px-6 sm:py-4">
        <div className="flex min-w-0 items-center gap-2">
          {isMobile && (
            <Button
              type="button"
              size="icon-sm"
              variant="ghost"
              aria-label="切换左侧会话栏"
              onClick={toggleSidebar}
            >
              <PanelLeftIcon className="size-4" />
            </Button>
          )}
          <Button
            type="button"
            variant="ghost"
            size="sm"
            className="shrink-0 px-2 sm:px-3"
            onClick={() => void navigate({ to: "/" })}
          >
            <ArrowLeftIcon className="mr-1 size-4" />
            返回工作台
          </Button>
          <div className="flex min-w-0 items-center gap-1.5">
            <ShieldCheckIcon className="size-4 shrink-0 text-primary" />
            <h1 className="truncate text-sm font-semibold sm:text-base">
              {isAdmin ? "Quire 控制台" : "API 密钥控制台"}
            </h1>
          </div>
        </div>
        <div
          className={`grid w-full gap-1 rounded-lg border border-border/60 bg-muted/40 p-1 sm:flex sm:w-auto sm:items-center sm:gap-1.5 ${
            isAdmin ? "grid-cols-2 sm:grid-cols-4" : "grid-cols-1"
          }`}
        >
          <Button
            type="button"
            size="sm"
            variant={tab === "api_keys" ? "default" : "ghost"}
            className="w-full justify-center px-2 text-xs sm:w-auto sm:px-3"
            onClick={() => setTab("api_keys")}
          >
            <KeyRoundIcon className="mr-1 size-3.5 shrink-0" />
            <span className="truncate">API 密钥</span>
          </Button>
          {isAdmin && (
            <>
              <Button
                type="button"
                size="sm"
                variant={tab === "users" ? "default" : "ghost"}
                className="w-full justify-center px-2 text-xs sm:w-auto sm:px-3"
                onClick={() => setTab("users")}
              >
                <UsersIcon className="mr-1 size-3.5 shrink-0" />
                <span className="truncate">用户管理</span>
              </Button>
              <Button
                type="button"
                size="sm"
                variant={tab === "models" ? "default" : "ghost"}
                className="w-full justify-center px-2 text-xs sm:w-auto sm:px-3"
                onClick={() => setTab("models")}
              >
                <BotIcon className="mr-1 size-3.5 shrink-0" />
                <span className="truncate">模型与供应商</span>
              </Button>
              <Button
                type="button"
                size="sm"
                variant={tab === "mcp" ? "default" : "ghost"}
                className="w-full justify-center px-2 text-xs sm:w-auto sm:px-3"
                onClick={() => setTab("mcp")}
              >
                <ServerIcon className="mr-1 size-3.5 shrink-0" />
                <span className="truncate">MCP 服务</span>
              </Button>
            </>
          )}
        </div>
      </header>

      <main className="mx-auto w-full max-w-5xl min-w-0 space-y-6 p-4 sm:p-6">
        {tab === "api_keys" && (
          <section className="space-y-6">
            <div className="grid gap-4 md:grid-cols-2">
              {/* Create new API key */}
              <form
                onSubmit={handleCreateApiKey}
                className="space-y-3 rounded-xl border border-border/70 bg-card p-4"
              >
                <div className="space-y-1">
                  <h2 className="flex items-center gap-1.5 text-sm font-semibold text-foreground">
                    <PlusIcon className="size-4 text-[#1D4ED8]" />
                    <span>创建外部代理 API 密钥</span>
                  </h2>
                  <p className="text-xs leading-relaxed text-muted-foreground">
                    创建的密钥可用于访问{" "}
                    <code className="rounded bg-muted px-1 py-0.5 font-mono text-[11px]">
                      /mcp
                    </code>{" "}
                    报告工具端点。外部代理创建报告时将自动绑定到你的左侧会话栏。
                  </p>
                </div>

                <div className="space-y-1.5">
                  <Label htmlFor="console-key-name" className="text-xs">
                    密钥名称 / 备注
                  </Label>
                  <div className="flex items-center gap-2">
                    <Input
                      id="console-key-name"
                      name="console-key-name"
                      autoComplete="off"
                      spellCheck={false}
                      placeholder="例如：Claude Code / Cursor 本地代理…"
                      value={newKeyName}
                      onChange={(e) => setNewKeyName(e.target.value)}
                      disabled={createApiKeyMutation.isPending}
                      className="text-xs"
                    />
                    <Button
                      type="submit"
                      size="sm"
                      disabled={createApiKeyMutation.isPending}
                      className="shrink-0 gap-1.5"
                    >
                      {createApiKeyMutation.isPending ? (
                        <LoaderCircleIcon
                          className="size-3.5 animate-spin"
                          aria-hidden="true"
                        />
                      ) : (
                        <PlusIcon className="size-3.5" aria-hidden="true" />
                      )}
                      <span>创建密钥</span>
                    </Button>
                  </div>
                </div>

                {latestCreatedKey && (
                  <div
                    aria-live="polite"
                    className="space-y-2 rounded-lg border border-emerald-500/35 bg-emerald-500/10 p-3"
                  >
                    <div className="flex items-center justify-between gap-2 text-xs font-medium text-emerald-800 dark:text-emerald-300">
                      <span>
                        密钥“{latestCreatedKey.name}”已生成（明文仅显示一次）：
                      </span>
                      <Button
                        type="button"
                        size="xs"
                        variant="outline"
                        className="h-6 gap-1 border-emerald-500/40 bg-background/90 px-2 text-[11px]"
                        onClick={() =>
                          void copyValue(
                            latestCreatedKey.secret_key,
                            "created-secret",
                            " API 密钥",
                          )
                        }
                      >
                        {copiedId === "created-secret" ? (
                          <CheckIcon
                            className="size-3 text-emerald-600"
                            aria-hidden="true"
                          />
                        ) : (
                          <CopyIcon className="size-3" aria-hidden="true" />
                        )}
                        <span>
                          {copiedId === "created-secret"
                            ? "已复制"
                            : "复制密钥"}
                        </span>
                      </Button>
                    </div>
                    <div
                      translate="no"
                      className="overflow-x-auto rounded border border-emerald-500/25 bg-background/95 px-2.5 py-1.5 font-mono text-xs font-medium text-foreground select-all"
                    >
                      {latestCreatedKey.secret_key}
                    </div>
                  </div>
                )}
              </form>

              {/* MCP endpoint info */}
              <div className="flex flex-col justify-between space-y-3 rounded-xl border border-border/70 bg-card p-4">
                <div className="space-y-1.5">
                  <h2 className="text-sm font-semibold text-foreground">
                    外部 Harness 接入地址
                  </h2>
                  <p className="text-xs leading-relaxed text-muted-foreground">
                    在支持 MCP Streamable HTTP 协议的客户端中填入以下端点，并在{" "}
                    <code className="rounded bg-muted px-1 py-0.5 font-mono text-[11px]">
                      Authorization
                    </code>{" "}
                    请求头中携带{" "}
                    <code className="rounded bg-muted px-1 py-0.5 font-mono text-[11px]">
                      Bearer qk_...
                    </code>
                    。
                  </p>
                </div>

                <div className="space-y-1.5">
                  <Label htmlFor="console-mcp-url" className="text-xs">
                    MCP 端点 URL
                  </Label>
                  <div className="flex items-center gap-2">
                    <Input
                      id="console-mcp-url"
                      name="console-mcp-url"
                      readOnly
                      autoComplete="off"
                      spellCheck={false}
                      value={mcpEndpointUrl}
                      className="font-mono text-xs"
                    />
                    <Button
                      type="button"
                      size="sm"
                      variant="outline"
                      className="shrink-0 gap-1.5"
                      onClick={() =>
                        void copyValue(
                          mcpEndpointUrl,
                          "mcp-endpoint",
                          " MCP 地址",
                        )
                      }
                    >
                      {copiedId === "mcp-endpoint" ? (
                        <CheckIcon
                          className="size-3.5 text-emerald-600"
                          aria-hidden="true"
                        />
                      ) : (
                        <CopyIcon className="size-3.5" aria-hidden="true" />
                      )}
                      <span>
                        {copiedId === "mcp-endpoint" ? "已复制" : "复制地址"}
                      </span>
                    </Button>
                  </div>
                </div>
              </div>
            </div>

            {/* Key list */}
            <div className="space-y-3">
              <div className="flex items-center justify-between gap-2">
                <div>
                  <h3 className="text-sm font-semibold text-foreground">
                    我的 API 密钥列表
                  </h3>
                  <p className="text-xs text-muted-foreground">
                    出于安全考虑，平台仅存储密钥 SHA-256 哈希与前缀摘要
                  </p>
                </div>
                <Button
                  type="button"
                  size="sm"
                  variant="outline"
                  onClick={() => void apiKeysQuery.refetch()}
                >
                  <RefreshCwIcon className="mr-1.5 size-3.5" />
                  刷新
                </Button>
              </div>

              {apiKeysQuery.isLoading && (
                <div className="flex items-center justify-center rounded-xl border border-border/70 py-10 text-muted-foreground">
                  <LoaderCircleIcon className="size-4 animate-spin" />
                </div>
              )}

              {/* Mobile stack cards */}
              <div className="space-y-2.5 sm:hidden">
                {(apiKeysQuery.data ?? []).map((item) => {
                  const confirming = confirmingKeyId === item.key_id;
                  return (
                    <div
                      key={item.key_id}
                      className="space-y-2.5 rounded-xl border border-border/70 bg-card p-3.5 text-xs"
                    >
                      <div className="flex items-start justify-between gap-2">
                        <div className="min-w-0">
                          <div className="truncate font-semibold text-foreground">
                            {item.name}
                          </div>
                          <div
                            translate="no"
                            className="mt-0.5 font-mono text-[11px] text-muted-foreground"
                          >
                            {item.key_prefix}
                          </div>
                        </div>
                        <Button
                          type="button"
                          size="xs"
                          variant={confirming ? "destructive" : "outline"}
                          disabled={deleteApiKeyMutation.isPending}
                          onClick={() => {
                            if (!confirming) {
                              setConfirmingKeyId(item.key_id);
                              return;
                            }
                            deleteApiKeyMutation.mutate(item.key_id);
                          }}
                        >
                          <Trash2Icon className="mr-1 size-3" />
                          {confirming ? "确认撤销" : "撤销"}
                        </Button>
                      </div>
                      <div className="flex items-center justify-between text-[11px] text-muted-foreground tabular-nums">
                        <span>创建：{formatTimestamp(item.created_at)}</span>
                        <span>
                          最近使用：{formatTimestamp(item.last_used_at)}
                        </span>
                      </div>
                    </div>
                  );
                })}
                {!apiKeysQuery.isLoading &&
                  (apiKeysQuery.data ?? []).length === 0 && (
                    <div className="rounded-xl border border-border/70 py-6 text-center text-xs text-muted-foreground">
                      暂无 API 密钥，请在上方创建一个密钥以连接外部代理
                    </div>
                  )}
              </div>

              {/* Desktop table */}
              <div className="hidden overflow-x-auto rounded-xl border border-border/70 sm:block">
                <table className="w-full text-left text-xs">
                  <thead className="border-b border-border/70 bg-muted/40 text-muted-foreground">
                    <tr>
                      <th className="px-4 py-2.5 font-medium">名称</th>
                      <th className="px-4 py-2.5 font-medium">密钥前缀</th>
                      <th className="px-4 py-2.5 font-medium">创建时间</th>
                      <th className="px-4 py-2.5 font-medium">最近使用</th>
                      <th className="px-4 py-2.5 text-right font-medium">
                        操作
                      </th>
                    </tr>
                  </thead>
                  <tbody className="divide-y divide-border/50">
                    {(apiKeysQuery.data ?? []).map((item) => {
                      const confirming = confirmingKeyId === item.key_id;
                      return (
                        <tr key={item.key_id} className="hover:bg-muted/20">
                          <td className="px-4 py-2.5 font-medium text-foreground">
                            {item.name}
                          </td>
                          <td
                            translate="no"
                            className="px-4 py-2.5 font-mono text-muted-foreground"
                          >
                            {item.key_prefix}
                          </td>
                          <td className="px-4 py-2.5 text-muted-foreground tabular-nums">
                            {formatTimestamp(item.created_at)}
                          </td>
                          <td className="px-4 py-2.5 text-muted-foreground tabular-nums">
                            {formatTimestamp(item.last_used_at)}
                          </td>
                          <td className="px-4 py-2.5 text-right">
                            <Button
                              type="button"
                              size="sm"
                              variant={confirming ? "destructive" : "outline"}
                              disabled={deleteApiKeyMutation.isPending}
                              onClick={() => {
                                if (!confirming) {
                                  setConfirmingKeyId(item.key_id);
                                  return;
                                }
                                deleteApiKeyMutation.mutate(item.key_id);
                              }}
                            >
                              <Trash2Icon className="mr-1 size-3.5" />
                              {confirming ? "再次点击确认撤销" : "撤销密钥"}
                            </Button>
                          </td>
                        </tr>
                      );
                    })}
                    {!apiKeysQuery.isLoading &&
                      (apiKeysQuery.data ?? []).length === 0 && (
                        <tr>
                          <td
                            colSpan={5}
                            className="px-4 py-8 text-center text-muted-foreground"
                          >
                            暂无 API 密钥，请在上方创建一个密钥以连接外部代理
                          </td>
                        </tr>
                      )}
                  </tbody>
                </table>
              </div>
            </div>
          </section>
        )}

        {isAdmin && tab === "users" && (
          <section className="space-y-4">
            <div className="flex items-center justify-between gap-2">
              <div className="min-w-0">
                <h2 className="text-sm font-semibold">注册用户列表</h2>
                <p className="text-xs text-muted-foreground">
                  管理 SaaS 平台成员角色与账号启用状态
                </p>
              </div>
              <Button
                type="button"
                size="sm"
                variant="outline"
                className="shrink-0"
                onClick={() => void usersQuery.refetch()}
              >
                <RefreshCwIcon className="mr-1.5 size-3.5" />
                刷新
              </Button>
            </div>

            {usersQuery.isLoading && (
              <div className="flex items-center justify-center rounded-xl border border-border/70 py-10 text-muted-foreground">
                <LoaderCircleIcon className="size-4 animate-spin" />
              </div>
            )}

            {/* Mobile stack cards */}
            <div className="space-y-2.5 sm:hidden">
              {(usersQuery.data ?? []).map((u) => {
                const isSelf = u.user_id === user?.user_id;
                return (
                  <div
                    key={u.user_id}
                    className="space-y-2.5 rounded-xl border border-border/70 bg-card p-3.5 text-xs"
                  >
                    <div className="flex items-start justify-between gap-2">
                      <div className="min-w-0">
                        <div className="truncate font-semibold text-foreground">
                          {u.display_name} (@{u.username})
                        </div>
                        <div className="truncate text-[11px] text-muted-foreground">
                          {u.email || "未绑定邮箱"}
                        </div>
                      </div>
                      <div className="flex shrink-0 items-center gap-1">
                        <span
                          className={`inline-flex rounded px-1.5 py-0.5 text-[10px] font-medium ${
                            u.role === "admin"
                              ? "bg-primary/10 text-primary"
                              : "bg-muted text-muted-foreground"
                          }`}
                        >
                          {u.role === "admin" ? "管理员" : "普通用户"}
                        </span>
                        <span
                          className={`inline-flex rounded px-1.5 py-0.5 text-[10px] font-medium ${
                            u.enabled
                              ? "bg-emerald-500/10 text-emerald-600"
                              : "bg-destructive/10 text-destructive"
                          }`}
                        >
                          {u.enabled ? "已启用" : "已禁用"}
                        </span>
                      </div>
                    </div>
                    <div className="flex items-center gap-2 pt-1">
                      <Button
                        type="button"
                        size="sm"
                        variant="outline"
                        className="flex-1 text-xs"
                        disabled={isSelf || userMutation.isPending}
                        onClick={() =>
                          userMutation.mutate({
                            userId: u.user_id,
                            patch: {
                              role: u.role === "admin" ? "user" : "admin",
                            },
                          })
                        }
                      >
                        {u.role === "admin" ? "设为普通用户" : "设为管理员"}
                      </Button>
                      <Button
                        type="button"
                        size="sm"
                        variant={u.enabled ? "destructive" : "default"}
                        className="flex-1 text-xs"
                        disabled={isSelf || userMutation.isPending}
                        onClick={() =>
                          userMutation.mutate({
                            userId: u.user_id,
                            patch: { enabled: !u.enabled },
                          })
                        }
                      >
                        {u.enabled ? "禁用" : "启用"}
                      </Button>
                    </div>
                  </div>
                );
              })}
            </div>

            {/* Desktop table */}
            <div className="hidden overflow-x-auto rounded-xl border border-border/70 sm:block">
              <table className="w-full text-left text-xs">
                <thead className="border-b border-border/70 bg-muted/40 text-muted-foreground">
                  <tr>
                    <th className="px-4 py-2.5 font-medium">用户名</th>
                    <th className="px-4 py-2.5 font-medium">昵称 / 邮箱</th>
                    <th className="px-4 py-2.5 font-medium">角色</th>
                    <th className="px-4 py-2.5 font-medium">状态</th>
                    <th className="px-4 py-2.5 text-right font-medium">操作</th>
                  </tr>
                </thead>
                <tbody className="divide-y divide-border/50">
                  {(usersQuery.data ?? []).map((u) => {
                    const isSelf = u.user_id === user?.user_id;
                    return (
                      <tr key={u.user_id} className="hover:bg-muted/20">
                        <td className="px-4 py-2.5 font-medium">
                          @{u.username}
                        </td>
                        <td className="px-4 py-2.5">
                          <div>{u.display_name}</div>
                          <div className="text-[11px] text-muted-foreground">
                            {u.email || "未绑定邮箱"}
                          </div>
                        </td>
                        <td className="px-4 py-2.5">
                          <span
                            className={`inline-flex rounded px-1.5 py-0.5 text-[11px] font-medium ${
                              u.role === "admin"
                                ? "bg-primary/10 text-primary"
                                : "bg-muted text-muted-foreground"
                            }`}
                          >
                            {u.role === "admin" ? "管理员" : "普通用户"}
                          </span>
                        </td>
                        <td className="px-4 py-2.5">
                          <span
                            className={`inline-flex rounded px-1.5 py-0.5 text-[11px] font-medium ${
                              u.enabled
                                ? "bg-emerald-500/10 text-emerald-600"
                                : "bg-destructive/10 text-destructive"
                            }`}
                          >
                            {u.enabled ? "已启用" : "已禁用"}
                          </span>
                        </td>
                        <td
                          className="px-4 py-2.5 text-right"
                          aria-label="操作"
                        >
                          <div className="inline-flex gap-2">
                            <Button
                              type="button"
                              size="sm"
                              variant="outline"
                              disabled={isSelf || userMutation.isPending}
                              onClick={() =>
                                userMutation.mutate({
                                  userId: u.user_id,
                                  patch: {
                                    role: u.role === "admin" ? "user" : "admin",
                                  },
                                })
                              }
                            >
                              {u.role === "admin"
                                ? "设为普通用户"
                                : "设为管理员"}
                            </Button>
                            <Button
                              type="button"
                              size="sm"
                              variant={u.enabled ? "destructive" : "default"}
                              disabled={isSelf || userMutation.isPending}
                              onClick={() =>
                                userMutation.mutate({
                                  userId: u.user_id,
                                  patch: { enabled: !u.enabled },
                                })
                              }
                            >
                              {u.enabled ? "禁用" : "启用"}
                            </Button>
                          </div>
                        </td>
                      </tr>
                    );
                  })}
                </tbody>
              </table>
            </div>
          </section>
        )}

        {isAdmin && tab === "models" && (
          <section className="space-y-6">
            <div className="grid gap-4 md:grid-cols-2 md:gap-6">
              <form
                onSubmit={handleCreateProvider}
                className="space-y-3 rounded-xl border border-border/70 p-4"
              >
                <h3 className="flex items-center gap-1.5 text-sm font-semibold">
                  <PlusIcon className="size-4" />
                  注册模型供应商
                </h3>
                <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
                  <div className="space-y-1">
                    <Label htmlFor="prov-id" className="text-xs">
                      供应商 ID
                    </Label>
                    <Input
                      id="prov-id"
                      placeholder="anthropic"
                      value={providerId}
                      onChange={(e) => setProviderId(e.target.value)}
                    />
                  </div>
                  <div className="space-y-1">
                    <Label htmlFor="prov-name" className="text-xs">
                      名称
                    </Label>
                    <Input
                      id="prov-name"
                      placeholder="DeepSeek Anthropic"
                      value={providerName}
                      onChange={(e) => setProviderName(e.target.value)}
                    />
                  </div>
                </div>
                <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
                  <div className="space-y-1">
                    <Label htmlFor="prov-proto" className="text-xs">
                      协议类型
                    </Label>
                    <select
                      id="prov-proto"
                      value={protocol}
                      onChange={(e) =>
                        setProtocol(
                          e.target.value === "openai_compatible"
                            ? "openai_compatible"
                            : "anthropic",
                        )
                      }
                      className="flex h-9 w-full rounded-md border border-input bg-background px-3 py-1 text-xs text-foreground shadow-xs"
                    >
                      <option value="anthropic">anthropic</option>
                      <option value="openai_compatible">
                        openai_compatible
                      </option>
                    </select>
                  </div>
                  <div className="space-y-1">
                    <Label htmlFor="prov-url" className="text-xs">
                      Base URL
                    </Label>
                    <Input
                      id="prov-url"
                      placeholder="https://api.deepseek.com/anthropic"
                      value={baseUrl}
                      onChange={(e) => setBaseUrl(e.target.value)}
                    />
                  </div>
                </div>
                <div className="space-y-1">
                  <Label htmlFor="prov-key" className="text-xs">
                    API Key 引用（env:VAR / enc:... / 明文密钥）
                  </Label>
                  <Input
                    id="prov-key"
                    placeholder="enc:... 或 env:DEEPSEEK_API_KEY"
                    value={apiKeyRef}
                    onChange={(e) => setApiKeyRef(e.target.value)}
                  />
                </div>
                <Button
                  type="submit"
                  size="sm"
                  className="w-full sm:w-auto"
                  disabled={providerMutation.isPending}
                >
                  保存供应商
                </Button>
              </form>

              <form
                onSubmit={handleCreateModel}
                className="space-y-3 rounded-xl border border-border/70 p-4"
              >
                <h3 className="flex items-center gap-1.5 text-sm font-semibold">
                  <PlusIcon className="size-4" />
                  注册模型路由
                </h3>
                <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
                  <div className="space-y-1">
                    <Label htmlFor="mod-id" className="text-xs">
                      路由 ID
                    </Label>
                    <Input
                      id="mod-id"
                      placeholder="deepseek-flash"
                      value={modelRouteId}
                      onChange={(e) => setModelRouteId(e.target.value)}
                    />
                  </div>
                  <div className="space-y-1">
                    <Label htmlFor="mod-prov" className="text-xs">
                      所属供应商 ID
                    </Label>
                    <Input
                      id="mod-prov"
                      placeholder="anthropic"
                      value={modelProviderId}
                      onChange={(e) => setModelProviderId(e.target.value)}
                    />
                  </div>
                </div>
                <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
                  <div className="space-y-1">
                    <Label htmlFor="mod-name" className="text-xs">
                      上游模型名
                    </Label>
                    <Input
                      id="mod-name"
                      placeholder="deepseek-chat"
                      value={modelName}
                      onChange={(e) => setModelName(e.target.value)}
                    />
                  </div>
                  <div className="space-y-1">
                    <Label htmlFor="mod-disp" className="text-xs">
                      展示名称
                    </Label>
                    <Input
                      id="mod-disp"
                      placeholder="DeepSeek Flash"
                      value={displayName}
                      onChange={(e) => setDisplayName(e.target.value)}
                    />
                  </div>
                </div>
                <div className="space-y-1">
                  <Label htmlFor="mod-ctx" className="text-xs">
                    上下文窗口上限
                  </Label>
                  <Input
                    id="mod-ctx"
                    type="number"
                    value={contextLimit}
                    onChange={(e) => setContextLimit(e.target.value)}
                  />
                </div>
                <Button
                  type="submit"
                  size="sm"
                  className="w-full sm:w-auto"
                  disabled={modelMutation.isPending}
                >
                  添加模型
                </Button>
              </form>
            </div>

            <div className="space-y-2">
              <h3 className="text-sm font-semibold">已注册供应商与模型</h3>

              {/* Mobile stack cards */}
              <div className="space-y-2.5 sm:hidden">
                {(modelsQuery.data ?? []).map((m) => {
                  const prov = (providersQuery.data ?? []).find(
                    (p) => p.provider_id === m.provider_id,
                  );
                  return (
                    <div
                      key={m.id}
                      className="flex items-center justify-between gap-2 rounded-xl border border-border/70 bg-card p-3.5 text-xs"
                    >
                      <div className="min-w-0">
                        <div className="truncate font-semibold text-foreground">
                          {m.id}
                        </div>
                        <div className="truncate text-[11px] text-muted-foreground">
                          {m.provider_id} ({prov?.protocol ?? "-"}) ·{" "}
                          {m.model_id}
                        </div>
                      </div>
                      <Button
                        type="button"
                        size="sm"
                        variant="outline"
                        className="shrink-0"
                        onClick={() =>
                          toggleModelMutation.mutate({
                            id: m.id,
                            enabled: !m.enabled,
                          })
                        }
                      >
                        {m.enabled ? "停用" : "启用"}
                      </Button>
                    </div>
                  );
                })}
                {(modelsQuery.data ?? []).length === 0 && (
                  <div className="rounded-xl border border-border/70 py-6 text-center text-xs text-muted-foreground">
                    暂无已注册模型
                  </div>
                )}
              </div>

              {/* Desktop table */}
              <div className="hidden overflow-x-auto rounded-xl border border-border/70 sm:block">
                <table className="w-full text-left text-xs">
                  <thead className="border-b border-border/70 bg-muted/40 text-muted-foreground">
                    <tr>
                      <th className="px-4 py-2.5 font-medium">路由 ID</th>
                      <th className="px-4 py-2.5 font-medium">供应商 / 协议</th>
                      <th className="px-4 py-2.5 font-medium">上游模型</th>
                      <th className="px-4 py-2.5 font-medium">状态</th>
                      <th className="px-4 py-2.5 text-right font-medium">
                        操作
                      </th>
                    </tr>
                  </thead>
                  <tbody className="divide-y divide-border/50">
                    {(modelsQuery.data ?? []).map((m) => {
                      const prov = (providersQuery.data ?? []).find(
                        (p) => p.provider_id === m.provider_id,
                      );
                      return (
                        <tr key={m.id} className="hover:bg-muted/20">
                          <td className="px-4 py-2.5 font-medium">{m.id}</td>
                          <td className="px-4 py-2.5">
                            {m.provider_id} ({prov?.protocol ?? "-"})
                          </td>
                          <td className="px-4 py-2.5">{m.model_id}</td>
                          <td className="px-4 py-2.5">
                            {m.enabled ? "已启用" : "已停用"}
                          </td>
                          <td
                            className="px-4 py-2.5 text-right"
                            aria-label="操作"
                          >
                            <Button
                              type="button"
                              size="sm"
                              variant="outline"
                              onClick={() =>
                                toggleModelMutation.mutate({
                                  id: m.id,
                                  enabled: !m.enabled,
                                })
                              }
                            >
                              {m.enabled ? "停用" : "启用"}
                            </Button>
                          </td>
                        </tr>
                      );
                    })}
                    {(modelsQuery.data ?? []).length === 0 && (
                      <tr>
                        <td
                          colSpan={5}
                          className="px-4 py-6 text-center text-muted-foreground"
                        >
                          暂无已注册模型
                        </td>
                      </tr>
                    )}
                  </tbody>
                </table>
              </div>
            </div>
          </section>
        )}

        {isAdmin && tab === "mcp" && (
          <section className="space-y-4">
            <h2 className="text-sm font-semibold">已配置 MCP 服务</h2>

            {/* Mobile stack cards */}
            <div className="space-y-2.5 sm:hidden">
              {(mcpQuery.data ?? []).map((s) => (
                <div
                  key={s.server_id}
                  className="flex items-center justify-between gap-2 rounded-xl border border-border/70 bg-card p-3.5 text-xs"
                >
                  <div className="min-w-0">
                    <div className="truncate font-semibold text-foreground">
                      {s.name} ({s.server_id})
                    </div>
                    <div className="truncate text-[11px] text-muted-foreground">
                      {s.url}
                    </div>
                  </div>
                  <Button
                    type="button"
                    size="sm"
                    variant="outline"
                    className="shrink-0"
                    onClick={() => syncMcpMutation.mutate(s.server_id)}
                  >
                    同步工具
                  </Button>
                </div>
              ))}
              {(mcpQuery.data ?? []).length === 0 && (
                <div className="rounded-xl border border-border/70 py-6 text-center text-xs text-muted-foreground">
                  暂无已配置 MCP 服务
                </div>
              )}
            </div>

            {/* Desktop table */}
            <div className="hidden overflow-x-auto rounded-xl border border-border/70 sm:block">
              <table className="w-full text-left text-xs">
                <thead className="border-b border-border/70 bg-muted/40 text-muted-foreground">
                  <tr>
                    <th className="px-4 py-2.5 font-medium">ID</th>
                    <th className="px-4 py-2.5 font-medium">名称</th>
                    <th className="px-4 py-2.5 font-medium">地址</th>
                    <th className="px-4 py-2.5 font-medium">状态</th>
                    <th className="px-4 py-2.5 text-right font-medium">操作</th>
                  </tr>
                </thead>
                <tbody className="divide-y divide-border/50">
                  {(mcpQuery.data ?? []).map((s) => (
                    <tr key={s.server_id}>
                      <td className="px-4 py-2.5 font-medium">{s.server_id}</td>
                      <td className="px-4 py-2.5">{s.name}</td>
                      <td className="px-4 py-2.5">{s.url}</td>
                      <td className="px-4 py-2.5">
                        {s.enabled ? "已启用" : "已禁用"}
                      </td>
                      <td className="px-4 py-2.5 text-right" aria-label="操作">
                        <Button
                          type="button"
                          size="sm"
                          variant="outline"
                          onClick={() => syncMcpMutation.mutate(s.server_id)}
                        >
                          同步工具
                        </Button>
                      </td>
                    </tr>
                  ))}
                  {(mcpQuery.data ?? []).length === 0 && (
                    <tr>
                      <td
                        colSpan={5}
                        className="px-4 py-6 text-center text-muted-foreground"
                      >
                        暂无已配置 MCP 服务
                      </td>
                    </tr>
                  )}
                </tbody>
              </table>
            </div>
          </section>
        )}
      </main>
    </div>
  );
}
