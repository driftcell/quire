import { useEffect, useMemo, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Link, useNavigate, useParams } from "@tanstack/react-router";
import {
  CheckIcon,
  KeyRoundIcon,
  LoaderCircleIcon,
  MoreHorizontalIcon,
  PanelLeftCloseIcon,
  PlusIcon,
  Trash2Icon,
} from "lucide-react";
import { deleteSession, listSessions } from "@/api/runtime";
import { ProductBrand } from "@/components/layout/ProductBrand";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
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
  SidebarMenuItem,
  useSidebar,
} from "@/components/ui/sidebar";
import type { SessionSummary } from "@/contracts/session-protocol";
import { getAuthTokens, saveAuthTokens } from "@/lib/auth";

function GridLoader() {
  return (
    <output
      className="ml-auto grid shrink-0 grid-cols-5 gap-[1.5px]"
      aria-label="Agent 正在运行"
    >
      {Array.from({ length: 25 }, (_, index) => {
        const row = Math.floor(index / 5);
        const column = index % 5;
        return (
          <span
            key={index}
            className="grid-loader-cell size-[2px] rounded-[0.5px] bg-muted-foreground"
            style={{ animationDelay: `${(row + column) * 90}ms` }}
          />
        );
      })}
    </output>
  );
}

type SessionGroupKey = "today" | "yesterday" | "thisWeek" | "earlier";

const GROUP_ORDER: readonly SessionGroupKey[] = [
  "today",
  "yesterday",
  "thisWeek",
  "earlier",
];

const GROUP_LABELS: Record<SessionGroupKey, string> = {
  today: "今天",
  yesterday: "昨天",
  thisWeek: "本周",
  earlier: "更早",
};

const DAY_MS = 86_400_000;

const shanghaiDayFormatter = new Intl.DateTimeFormat("en-US", {
  timeZone: "Asia/Shanghai",
  year: "numeric",
  month: "2-digit",
  day: "2-digit",
});

function shanghaiCalendarDay(value: Date): number | undefined {
  if (!Number.isFinite(value.getTime())) return undefined;
  const parts = shanghaiDayFormatter.formatToParts(value);
  const year = Number(parts.find((part) => part.type === "year")?.value);
  const month = Number(parts.find((part) => part.type === "month")?.value);
  const day = Number(parts.find((part) => part.type === "day")?.value);
  if (!year || !month || !day) return undefined;
  return Math.floor(Date.UTC(year, month - 1, day) / DAY_MS);
}

function startOfWeek(day: number): number {
  const weekday = new Date(day * DAY_MS).getUTCDay();
  return day - ((weekday + 6) % 7);
}

function groupSessionsByDate(sessions: SessionSummary[]) {
  const today = shanghaiCalendarDay(new Date());
  const weekStart = today === undefined ? undefined : startOfWeek(today);
  const groups: Record<SessionGroupKey, SessionSummary[]> = {
    today: [],
    yesterday: [],
    thisWeek: [],
    earlier: [],
  };

  const sorted = [...sessions].sort((left, right) => {
    const diff =
      Date.parse(right.updated_at || right.created_at) -
      Date.parse(left.updated_at || left.created_at);
    return diff || right.session_id.localeCompare(left.session_id);
  });

  for (const session of sorted) {
    const day = shanghaiCalendarDay(
      new Date(session.updated_at || session.created_at),
    );
    let key: SessionGroupKey = "earlier";
    if (day !== undefined && today !== undefined && weekStart !== undefined) {
      if (day >= today) {
        key = "today";
      } else if (day === today - 1) {
        key = "yesterday";
      } else if (day >= weekStart) {
        key = "thisWeek";
      }
    }
    groups[key].push(session);
  }

  return GROUP_ORDER.flatMap((key) =>
    groups[key].length > 0
      ? [{ key, label: GROUP_LABELS[key], items: groups[key] }]
      : [],
  );
}

export function AppSidebar() {
  const queryClient = useQueryClient();
  const navigate = useNavigate();
  const params = useParams({ strict: false });
  const activeSessionId = params.sessionId;
  const { toggleSidebar } = useSidebar();

  const sessionsQuery = useQuery({
    queryKey: ["sessions"],
    queryFn: () => listSessions(50, 0),
    refetchInterval: 10_000,
  });

  const [confirmingSessionId, setConfirmingSessionId] = useState<string | null>(
    null,
  );
  const [deletingSessionId, setDeletingSessionId] = useState<string | null>(
    null,
  );

  useEffect(() => {
    if (!confirmingSessionId) return undefined;
    const timer = window.setTimeout(() => setConfirmingSessionId(null), 3000);
    return () => window.clearTimeout(timer);
  }, [confirmingSessionId]);

  const deleteMutation = useMutation({
    mutationFn: (sessionId: string) => deleteSession(sessionId),
    onMutate: (sessionId) => {
      setDeletingSessionId(sessionId);
    },
    onSuccess: (_data, deletedId) => {
      setConfirmingSessionId(null);
      void queryClient.invalidateQueries({ queryKey: ["sessions"] });
      if (activeSessionId === deletedId) {
        void navigate({ to: "/" });
      }
    },
    onSettled: () => {
      setDeletingSessionId(null);
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
      <SidebarHeader className="gap-2 px-3 pt-3 pb-1">
        <div className="flex h-10 items-center justify-between px-1">
          <Link to="/" className="min-w-0">
            <ProductBrand />
          </Link>
          <Button
            type="button"
            variant="ghost"
            size="icon-sm"
            aria-label="收起侧边栏"
            title="收起侧边栏"
            onClick={toggleSidebar}
          >
            <PanelLeftCloseIcon className="size-[18px]" aria-hidden="true" />
          </Button>
        </div>

        <Button
          type="button"
          variant="outline"
          onClick={() => {
            void navigate({ to: "/" });
          }}
          className="h-11 w-full justify-start gap-2 rounded-xl bg-background px-3 font-normal"
        >
          <PlusIcon className="size-4" aria-hidden="true" />
          <span>新建会话</span>
          <span
            className="ml-auto hidden items-center gap-1 md:flex"
            aria-label="Command or Control plus N"
          >
            <kbd className="flex h-6 w-6 items-center justify-center rounded border bg-muted/60 text-base leading-none font-normal text-muted-foreground">
              ⌘
            </kbd>
            <kbd className="flex h-6 w-6 items-center justify-center rounded border bg-muted/60 text-xs font-normal text-muted-foreground">
              N
            </kbd>
          </span>
        </Button>
      </SidebarHeader>

      <SidebarContent className="overscroll-contain px-1 pt-2 [scrollbar-width:none] [&::-webkit-scrollbar]:hidden">
        {groups.map((group) => (
          <SidebarGroup key={group.key} className="py-1">
            <SidebarGroupLabel className="px-3 text-[11px] font-medium text-muted-foreground/70">
              {group.label}
            </SidebarGroupLabel>
            <SidebarGroupContent>
              <SidebarMenu className="gap-1">
                {group.items.map((session) => {
                  const confirming = confirmingSessionId === session.session_id;
                  const deleting = deletingSessionId === session.session_id;
                  const isActive = activeSessionId === session.session_id;
                  const title = session.title || "未命名会话";

                  return (
                    <SidebarMenuItem
                      key={session.session_id}
                      className="group/session relative"
                    >
                      <Button
                        variant="ghost"
                        nativeButton={false}
                        className={`h-10 w-full justify-start gap-2 rounded-lg px-3 pr-9 font-normal ${
                          isActive ? "bg-background shadow-sm" : ""
                        }`}
                        title={title}
                        render={
                          <Link
                            to="/sessions/$sessionId"
                            params={{ sessionId: session.session_id }}
                          />
                        }
                      >
                        <span className="truncate">{title}</span>
                        {session.active_run_status === "running" && (
                          <GridLoader />
                        )}
                        {session.active_run_status === "awaiting_input" && (
                          <span
                            className="ml-auto size-2 shrink-0 rounded-full bg-amber-500"
                            title="等待确认"
                          />
                        )}
                      </Button>

                      <DropdownMenu
                        onOpenChange={(open) => {
                          if (!open && confirming) {
                            setConfirmingSessionId(null);
                          }
                        }}
                      >
                        <DropdownMenuTrigger
                          render={
                            <Button
                              type="button"
                              variant="ghost"
                              size="icon-sm"
                              className={`absolute inset-y-0 right-1 my-auto text-muted-foreground transition-opacity focus-visible:pointer-events-auto focus-visible:opacity-100 ${
                                deleting
                                  ? "pointer-events-auto opacity-100"
                                  : "pointer-events-none opacity-0 group-hover/session:pointer-events-auto group-hover/session:opacity-100 aria-expanded:pointer-events-auto aria-expanded:opacity-100 [@media(hover:none)]:pointer-events-auto [@media(hover:none)]:opacity-100"
                              }`}
                              aria-label={`会话操作 ${title}`}
                              title="会话操作"
                              disabled={deleting}
                            />
                          }
                        >
                          {deleting ? (
                            <LoaderCircleIcon
                              className="size-4 animate-spin"
                              aria-hidden="true"
                            />
                          ) : (
                            <MoreHorizontalIcon
                              className="size-4"
                              aria-hidden="true"
                            />
                          )}
                        </DropdownMenuTrigger>
                        <DropdownMenuContent
                          side="right"
                          align="start"
                          className="w-40"
                        >
                          <DropdownMenuGroup>
                            <DropdownMenuItem
                              variant="destructive"
                              disabled={deleting}
                              closeOnClick={confirming}
                              onClick={() => {
                                if (!confirming) {
                                  setConfirmingSessionId(session.session_id);
                                  return;
                                }
                                setConfirmingSessionId(null);
                                deleteMutation.mutate(session.session_id);
                              }}
                            >
                              {deleting ? (
                                <LoaderCircleIcon
                                  className="animate-spin"
                                  aria-hidden="true"
                                />
                              ) : confirming ? (
                                <CheckIcon aria-hidden="true" />
                              ) : (
                                <Trash2Icon aria-hidden="true" />
                              )}
                              {deleting
                                ? "正在删除…"
                                : confirming
                                  ? "再次点击确认删除"
                                  : "删除会话"}
                            </DropdownMenuItem>
                          </DropdownMenuGroup>
                        </DropdownMenuContent>
                      </DropdownMenu>
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

      <SidebarFooter className="p-2">
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
