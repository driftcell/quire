import { useEffect, useMemo, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Link, useNavigate, useParams } from "@tanstack/react-router";
import {
  CheckIcon,
  FileTextIcon,
  KeyRoundIcon,
  PlusIcon,
  Trash2Icon,
} from "lucide-react";
import { deleteSession, listSessions } from "@/api/runtime";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Sidebar,
  SidebarContent,
  SidebarFooter,
  SidebarGroup,
  SidebarGroupContent,
  SidebarGroupLabel,
  SidebarHeader,
  SidebarMenu,
  SidebarMenuAction,
  SidebarMenuButton,
  SidebarMenuItem,
} from "@/components/ui/sidebar";
import type {
  ActiveRunStatus,
  SessionSummary,
} from "@/contracts/session-protocol";
import { getAuthTokens, saveAuthTokens } from "@/lib/auth";

function statusDotClass(status: ActiveRunStatus): string {
  switch (status) {
    case "running":
      return "bg-emerald-500 animate-pulse";
    case "awaiting_input":
      return "bg-amber-500 animate-pulse";
    case "interrupted":
    case "failed":
      return "bg-destructive";
    case "idle":
    default:
      return "bg-muted-foreground/30";
  }
}

function groupSessionsByDate(sessions: SessionSummary[]) {
  const now = new Date();
  const startOfToday = new Date(
    now.getFullYear(),
    now.getMonth(),
    now.getDate(),
  ).getTime();
  const startOfYesterday = startOfToday - 86_400_000;

  const today: SessionSummary[] = [];
  const yesterday: SessionSummary[] = [];
  const earlier: SessionSummary[] = [];

  for (const s of sessions) {
    const ts = Date.parse(s.updated_at || s.created_at);
    if (Number.isNaN(ts) || ts >= startOfToday) {
      today.push(s);
    } else if (ts >= startOfYesterday) {
      yesterday.push(s);
    } else {
      earlier.push(s);
    }
  }

  return [
    { label: "今天", items: today },
    { label: "昨天", items: yesterday },
    { label: "更早", items: earlier },
  ].filter((g) => g.items.length > 0);
}

export function AppSidebar() {
  const queryClient = useQueryClient();
  const navigate = useNavigate();
  const params = useParams({ strict: false });
  const activeSessionId = params.sessionId;

  const sessionsQuery = useQuery({
    queryKey: ["sessions"],
    queryFn: () => listSessions(50, 0),
    refetchInterval: 10_000,
  });

  const [pendingDeleteId, setPendingDeleteId] = useState<string | null>(null);

  useEffect(() => {
    if (!pendingDeleteId) return undefined;
    const timer = window.setTimeout(() => setPendingDeleteId(null), 3000);
    return () => window.clearTimeout(timer);
  }, [pendingDeleteId]);

  const deleteMutation = useMutation({
    mutationFn: (sessionId: string) => deleteSession(sessionId),
    onSuccess: (_data, deletedId) => {
      setPendingDeleteId(null);
      void queryClient.invalidateQueries({ queryKey: ["sessions"] });
      if (activeSessionId === deletedId) {
        void navigate({ to: "/" });
      }
    },
  });

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "n") {
        event.preventDefault();
        void navigate({ to: "/" });
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [navigate]);

  const groups = useMemo(
    () => groupSessionsByDate(sessionsQuery.data ?? []),
    [sessionsQuery.data],
  );

  const [tokenDialogOpen, setTokenDialogOpen] = useState(false);
  const [tokens, setTokens] = useState(() => getAuthTokens());

  return (
    <Sidebar collapsible="offcanvas">
      <SidebarHeader className="gap-2.5 border-b px-3 py-2.5">
        <div className="flex items-center justify-between">
          <Link to="/" className="flex items-center gap-2 font-semibold">
            <span className="flex size-6 items-center justify-center rounded-md bg-primary text-xs font-bold text-primary-foreground">
              Q
            </span>
            <span className="text-sm tracking-tight">Quire Studio</span>
          </Link>
        </div>

        <Button
          type="button"
          size="sm"
          variant="outline"
          className="w-full justify-between bg-background font-normal shadow-2xs"
          onClick={() => {
            void navigate({ to: "/" });
          }}
        >
          <span className="flex items-center gap-1.5 text-xs">
            <PlusIcon className="size-3.5" />
            <span>新建会话</span>
          </span>
          <kbd className="rounded bg-muted px-1.5 py-0.5 font-sans text-[10px] text-muted-foreground">
            ⌘ N
          </kbd>
        </Button>
      </SidebarHeader>

      <SidebarContent className="overscroll-contain">
        {groups.map((group) => (
          <SidebarGroup key={group.label}>
            <SidebarGroupLabel>{group.label}</SidebarGroupLabel>
            <SidebarGroupContent>
              <SidebarMenu>
                {group.items.map((session) => {
                  const isConfirming = pendingDeleteId === session.session_id;
                  return (
                    <SidebarMenuItem key={session.session_id}>
                      <SidebarMenuButton
                        isActive={activeSessionId === session.session_id}
                        render={
                          <Link
                            to="/sessions/$sessionId"
                            params={{ sessionId: session.session_id }}
                          />
                        }
                      >
                        <span
                          className={`size-2 shrink-0 rounded-full ${statusDotClass(session.active_run_status)}`}
                        />
                        <span className="truncate">{session.title}</span>
                        {session.doc_id && (
                          <FileTextIcon className="ml-auto size-3.5 shrink-0 text-muted-foreground" />
                        )}
                      </SidebarMenuButton>
                      <SidebarMenuAction
                        showOnHover={!isConfirming}
                        aria-label={
                          isConfirming
                            ? `确认删除 ${session.title}`
                            : `删除 ${session.title}`
                        }
                        title={isConfirming ? "再次点击确认删除" : "删除会话"}
                        className={
                          isConfirming
                            ? "text-destructive opacity-100 hover:bg-destructive/10"
                            : undefined
                        }
                        onClick={() => {
                          if (isConfirming) {
                            deleteMutation.mutate(session.session_id);
                          } else {
                            setPendingDeleteId(session.session_id);
                          }
                        }}
                      >
                        {isConfirming ? (
                          <CheckIcon className="size-3.5" />
                        ) : (
                          <Trash2Icon className="size-3.5" />
                        )}
                      </SidebarMenuAction>
                    </SidebarMenuItem>
                  );
                })}
              </SidebarMenu>
            </SidebarGroupContent>
          </SidebarGroup>
        ))}

        {sessionsQuery.data?.length === 0 && (
          <div className="px-4 py-6 text-xs text-muted-foreground">
            暂无会话，发送第一条消息即可创建。
          </div>
        )}
      </SidebarContent>

      <SidebarFooter className="border-t p-2">
        <Dialog open={tokenDialogOpen} onOpenChange={setTokenDialogOpen}>
          <DialogTrigger
            render={
              <Button
                type="button"
                size="sm"
                variant="ghost"
                className="w-full justify-start gap-2 text-xs text-muted-foreground"
              />
            }
          >
            <KeyRoundIcon className="size-3.5" />
            <span>API Token 设置</span>
          </DialogTrigger>
          <DialogContent>
            <DialogHeader>
              <DialogTitle>本地开发 Token 配置</DialogTitle>
            </DialogHeader>
            <div className="space-y-3 py-2 text-xs">
              <div className="space-y-1">
                <Label htmlFor="runtime-token">RUNTIME_API_TOKEN</Label>
                <Input
                  id="runtime-token"
                  type="password"
                  value={tokens.runtimeToken}
                  onChange={(e) =>
                    setTokens((p) => ({ ...p, runtimeToken: e.target.value }))
                  }
                />
              </div>
              <div className="space-y-1">
                <Label htmlFor="quire-ro-token">QUIRE_RO_TOKEN</Label>
                <Input
                  id="quire-ro-token"
                  type="password"
                  value={tokens.quireRoToken}
                  onChange={(e) =>
                    setTokens((p) => ({ ...p, quireRoToken: e.target.value }))
                  }
                />
              </div>
              <div className="space-y-1">
                <Label htmlFor="quire-rw-token">QUIRE_RW_TOKEN（批注用）</Label>
                <Input
                  id="quire-rw-token"
                  type="password"
                  value={tokens.quireRwToken}
                  onChange={(e) =>
                    setTokens((p) => ({ ...p, quireRwToken: e.target.value }))
                  }
                />
              </div>
            </div>
            <DialogFooter>
              <Button
                type="button"
                size="sm"
                onClick={() => {
                  saveAuthTokens(tokens);
                  setTokenDialogOpen(false);
                  void queryClient.invalidateQueries();
                }}
              >
                保存并刷新
              </Button>
            </DialogFooter>
          </DialogContent>
        </Dialog>
      </SidebarFooter>
    </Sidebar>
  );
}
