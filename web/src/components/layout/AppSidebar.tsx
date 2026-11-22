import { useEffect, useMemo, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Link, useNavigate, useParams } from "@tanstack/react-router";
import {
  CheckIcon,
  LoaderCircleIcon,
  MoreHorizontalIcon,
  PinIcon,
  PinOffIcon,
  PlusIcon,
  Trash2Icon,
} from "lucide-react";
import { toast } from "sonner";
import { deleteSession, listSessions, updateSession } from "@/api/runtime";
import { ProductBrand } from "@/components/layout/ProductBrand";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import {
  Sidebar,
  SidebarContent,
  SidebarGroup,
  SidebarGroupContent,
  SidebarGroupLabel,
  SidebarHeader,
  SidebarMenu,
  SidebarMenuItem,
} from "@/components/ui/sidebar";
import type { SessionSummary } from "@/contracts/session-protocol";
import { errorMessage } from "@/lib/errors";
import {
  groupSessionsForSidebar,
  type SessionGroup,
} from "@/components/layout/sessionGroups";

export { groupSessionsForSidebar };
export type { SessionGroup };

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
    onError: (error) => {
      toast.error(errorMessage(error, "删除会话失败"));
    },
    onSettled: () => {
      setDeletingSessionId(null);
    },
  });

  const pinMutation = useMutation({
    mutationFn: ({
      sessionId,
      pinned,
    }: {
      sessionId: string;
      pinned: boolean;
    }) => updateSession(sessionId, { pinned }),
    onMutate: async ({ sessionId, pinned }) => {
      setConfirmingSessionId(null);
      await queryClient.cancelQueries({ queryKey: ["sessions"] });
      const previous = queryClient.getQueryData<SessionSummary[]>(["sessions"]);
      queryClient.setQueryData<SessionSummary[]>(["sessions"], (current) =>
        current?.map((session) =>
          session.session_id === sessionId
            ? {
                ...session,
                pinned_at: pinned ? new Date().toISOString() : null,
              }
            : session,
        ),
      );
      return { previous };
    },
    onError: (error, variables, context) => {
      if (context?.previous !== undefined) {
        queryClient.setQueryData(["sessions"], context.previous);
      }
      toast.error(
        errorMessage(error, variables.pinned ? "置顶会话失败" : "取消置顶失败"),
      );
    },
    onSettled: () => {
      void queryClient.invalidateQueries({ queryKey: ["sessions"] });
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
    () => groupSessionsForSidebar(sessionsQuery.data ?? []),
    [sessionsQuery.data],
  );

  return (
    <Sidebar collapsible="offcanvas">
      <SidebarHeader className="gap-2 px-3 pt-3 pb-1">
        <div className="flex h-10 items-center px-1">
          <Link to="/" className="min-w-0">
            <ProductBrand />
          </Link>
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

      <SidebarContent className="no-scrollbar overscroll-contain px-1 pt-2 pb-3">
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
                  const isPinned = Boolean(session.pinned_at);
                  const isPinning =
                    pinMutation.isPending &&
                    pinMutation.variables?.sessionId === session.session_id;
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
                              disabled={isPinning || deleting}
                              onClick={() => {
                                setConfirmingSessionId(null);
                                pinMutation.mutate({
                                  sessionId: session.session_id,
                                  pinned: !isPinned,
                                });
                              }}
                            >
                              {isPinning ? (
                                <LoaderCircleIcon
                                  className="animate-spin"
                                  aria-hidden="true"
                                />
                              ) : isPinned ? (
                                <PinOffIcon aria-hidden="true" />
                              ) : (
                                <PinIcon aria-hidden="true" />
                              )}
                              {isPinned ? "取消置顶" : "置顶会话"}
                            </DropdownMenuItem>
                            <DropdownMenuSeparator />
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
    </Sidebar>
  );
}
