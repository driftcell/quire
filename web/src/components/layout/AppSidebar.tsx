import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Link, useNavigate, useParams } from "@tanstack/react-router";
import {
  FileTextIcon,
  KeyRoundIcon,
  MessageSquarePlusIcon,
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
import type { ActiveRunStatus } from "@/contracts/session-protocol";
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

  const deleteMutation = useMutation({
    mutationFn: (sessionId: string) => deleteSession(sessionId),
    onSuccess: (_data, deletedId) => {
      void queryClient.invalidateQueries({ queryKey: ["sessions"] });
      if (activeSessionId === deletedId) {
        void navigate({ to: "/" });
      }
    },
  });

  const [tokenDialogOpen, setTokenDialogOpen] = useState(false);
  const [tokens, setTokens] = useState(() => getAuthTokens());

  return (
    <Sidebar collapsible="offcanvas">
      <SidebarHeader className="border-b px-3 py-2.5">
        <div className="flex items-center justify-between">
          <Link to="/" className="flex items-center gap-2 font-semibold">
            <span className="flex size-6 items-center justify-center rounded-md bg-primary text-xs font-bold text-primary-foreground">
              Q
            </span>
            <span className="text-sm tracking-tight">Quire Studio</span>
          </Link>
          <Button
            type="button"
            size="icon-sm"
            variant="ghost"
            aria-label="新建报告会话"
            onClick={() => {
              void navigate({ to: "/" });
            }}
          >
            <MessageSquarePlusIcon className="size-4" />
          </Button>
        </div>
      </SidebarHeader>

      <SidebarContent>
        <SidebarGroup>
          <SidebarGroupLabel>会话列表</SidebarGroupLabel>
          <SidebarGroupContent>
            <SidebarMenu>
              {sessionsQuery.data?.map((session) => (
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
                    showOnHover
                    aria-label={`删除 ${session.title}`}
                    onClick={() => {
                      deleteMutation.mutate(session.session_id);
                    }}
                  >
                    <Trash2Icon className="size-3.5" />
                  </SidebarMenuAction>
                </SidebarMenuItem>
              ))}
              {sessionsQuery.data?.length === 0 && (
                <div className="px-3 py-4 text-xs text-muted-foreground">
                  暂无会话，发送第一条消息即可创建。
                </div>
              )}
            </SidebarMenu>
          </SidebarGroupContent>
        </SidebarGroup>
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
