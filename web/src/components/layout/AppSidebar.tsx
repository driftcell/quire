import { useEffect, useMemo, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Link, useNavigate, useParams } from "@tanstack/react-router";
import {
  CheckIcon,
  FileTextIcon,
  LoaderCircleIcon,
  MessageSquareIcon,
  MoreHorizontalIcon,
  PinIcon,
  PinOffIcon,
  PlusIcon,
  SearchIcon,
  Trash2Icon,
  XIcon,
} from "lucide-react";
import { toast } from "sonner";
import { deleteSession, listSessions, updateSession } from "@/api/runtime";
import { ProductBrand } from "@/components/layout/ProductBrand";
import { SidebarUserFooter } from "@/components/layout/SidebarUserFooter";
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
      aria-label="正在执行"
    >
      {Array.from({ length: 25 }, (_, index) => {
        const row = Math.floor(index / 5);
        const column = index % 5;
        return (
          <span
            key={index}
            className="grid-loader-cell size-[2px] rounded-[0.5px] bg-[#1D4ED8]"
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
    refetchOnMount: "always",
    refetchOnWindowFocus: true,
  });

  const [searchQuery, setSearchQuery] = useState("");
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
      toast.error(errorMessage(error, "删除对话失败"));
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
        errorMessage(error, variables.pinned ? "置顶对话失败" : "取消置顶失败"),
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

  const filteredSessions = useMemo(() => {
    const all = sessionsQuery.data ?? [];
    const q = searchQuery.trim().toLowerCase();
    if (!q) return all;
    return all.filter((s) =>
      (s.title || "未命名对话").toLowerCase().includes(q),
    );
  }, [sessionsQuery.data, searchQuery]);

  const groups = useMemo(
    () => groupSessionsForSidebar(filteredSessions),
    [filteredSessions],
  );

  const totalCount = sessionsQuery.data?.length ?? 0;

  return (
    <Sidebar collapsible="offcanvas" className="border-r border-sidebar-border">
      <SidebarHeader className="gap-2.5 border-b border-sidebar-border/80 px-3 pt-3.5 pb-3">
        <div className="flex items-center justify-between px-1">
          <Link
            to="/"
            className="min-w-0 rounded-md focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none"
          >
            <ProductBrand />
          </Link>
        </div>

        <Button
          type="button"
          variant="outline"
          onClick={() => {
            void navigate({ to: "/" });
          }}
          className="h-9 w-full justify-start gap-2 rounded-lg border-sidebar-border bg-card px-2.5 text-xs font-medium text-foreground shadow-2xs transition-colors hover:bg-background"
        >
          <PlusIcon className="size-3.5 text-[#1D4ED8]" aria-hidden="true" />
          <span>新建对话</span>
          <span
            className="ml-auto hidden items-center gap-0.5 md:flex"
            aria-label="Command or Control plus N"
          >
            <kbd className="flex h-5 items-center justify-center rounded border border-border/80 bg-muted/60 px-1 font-sans text-[10px] leading-none font-medium text-muted-foreground">
              ⌘&nbsp;N
            </kbd>
          </span>
        </Button>

        {totalCount > 2 && (
          <div className="relative">
            <SearchIcon
              className="pointer-events-none absolute top-1/2 left-2.5 size-3.5 -translate-y-1/2 text-muted-foreground/75"
              aria-hidden="true"
            />
            <input
              type="search"
              name="sidebar-session-filter"
              autoComplete="off"
              spellCheck={false}
              aria-label="搜索历史对话"
              placeholder="搜索历史对话或报告…"
              value={searchQuery}
              onChange={(e) => setSearchQuery(e.target.value)}
              className="h-8 w-full rounded-lg border border-sidebar-border/90 bg-background/70 pr-7 pl-8 text-xs text-foreground placeholder:text-muted-foreground/70 focus-visible:border-ring focus-visible:bg-background focus-visible:ring-2 focus-visible:ring-ring/25 focus-visible:outline-none"
            />
            {searchQuery && (
              <button
                type="button"
                aria-label="清空搜索"
                onClick={() => setSearchQuery("")}
                className="absolute top-1/2 right-1.5 -translate-y-1/2 rounded p-0.5 text-muted-foreground hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none"
              >
                <XIcon className="size-3" aria-hidden="true" />
              </button>
            )}
          </div>
        )}
      </SidebarHeader>

      <SidebarContent className="no-scrollbar overscroll-contain px-2 pt-2 pb-3">
        {groups.map((group) => (
          <SidebarGroup key={group.key} className="py-1.5">
            <SidebarGroupLabel className="flex items-center justify-between px-2.5 text-[11px] font-medium text-muted-foreground">
              <span>{group.label}</span>
              <span className="font-mono text-[10px] text-muted-foreground/70 tabular-nums">
                {group.items.length}
              </span>
            </SidebarGroupLabel>
            <SidebarGroupContent>
              <SidebarMenu className="gap-0.5">
                {group.items.map((session) => {
                  const confirming = confirmingSessionId === session.session_id;
                  const deleting = deletingSessionId === session.session_id;
                  const isPinned = Boolean(session.pinned_at);
                  const isPinning =
                    pinMutation.isPending &&
                    pinMutation.variables?.sessionId === session.session_id;
                  const isActive = activeSessionId === session.session_id;
                  const title = session.title || "未命名对话";
                  const hasDoc = Boolean(
                    session.doc_id || session.linked_doc_id,
                  );
                  const isExternalMcp = session.created_via === "mcp";

                  return (
                    <SidebarMenuItem
                      key={session.session_id}
                      className="group/session relative"
                    >
                      <Button
                        variant="ghost"
                        nativeButton={false}
                        className={`relative h-9 w-full justify-start gap-2 rounded-lg px-2.5 pr-8 text-xs transition-colors ${
                          isActive
                            ? "bg-card font-medium text-foreground shadow-2xs ring-1 ring-border/80 before:absolute before:top-2 before:bottom-2 before:left-0 before:w-[2.5px] before:rounded-r before:bg-[#1D4ED8]"
                            : "font-normal text-foreground/85 hover:bg-sidebar-accent/70 hover:text-foreground"
                        }`}
                        title={title}
                        render={
                          <Link
                            to="/sessions/$sessionId"
                            params={{ sessionId: session.session_id }}
                          />
                        }
                      >
                        {hasDoc ? (
                          <FileTextIcon
                            className={`size-3.5 shrink-0 ${
                              isActive
                                ? "text-[#1D4ED8]"
                                : "text-muted-foreground/75"
                            }`}
                            aria-hidden="true"
                          />
                        ) : (
                          <MessageSquareIcon
                            className={`size-3.5 shrink-0 ${
                              isActive
                                ? "text-[#1D4ED8]"
                                : "text-muted-foreground/75"
                            }`}
                            aria-hidden="true"
                          />
                        )}
                        <span className="min-w-0 flex-1 truncate">{title}</span>
                        {isExternalMcp && (
                          <span
                            translate="no"
                            title="由外部代理 (MCP) 创建并同步"
                            className="shrink-0 rounded border border-border/80 bg-muted/55 px-1 py-0.2 font-mono text-[9.5px] leading-tight font-medium text-muted-foreground group-hover/session:opacity-0"
                          >
                            MCP
                          </span>
                        )}
                        {isPinned &&
                          session.active_run_status !== "running" &&
                          session.active_run_status !== "awaiting_input" && (
                            <PinIcon
                              className="ml-auto size-3 shrink-0 text-muted-foreground/60 group-hover/session:opacity-0"
                              aria-hidden="true"
                            />
                          )}
                        {session.active_run_status === "running" && (
                          <GridLoader />
                        )}
                        {session.active_run_status === "awaiting_input" && (
                          <span
                            className="ml-auto size-2 shrink-0 rounded-full bg-amber-500"
                            title="等待你的确认"
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
                              className={`absolute inset-y-0 right-1 my-auto size-7 text-muted-foreground transition-opacity focus-visible:pointer-events-auto focus-visible:opacity-100 ${
                                deleting
                                  ? "pointer-events-auto opacity-100"
                                  : "pointer-events-none opacity-0 group-hover/session:pointer-events-auto group-hover/session:opacity-100 aria-expanded:pointer-events-auto aria-expanded:opacity-100 [@media(hover:none)]:pointer-events-auto [@media(hover:none)]:opacity-100"
                              }`}
                              aria-label={`对话操作 ${title}`}
                              title="对话操作"
                              disabled={deleting}
                            />
                          }
                        >
                          {deleting ? (
                            <LoaderCircleIcon
                              className="size-3.5 animate-spin"
                              aria-hidden="true"
                            />
                          ) : (
                            <MoreHorizontalIcon
                              className="size-3.5"
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
                              {isPinned ? "取消置顶" : "置顶对话"}
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
                                  : "删除对话"}
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
          <div className="mx-2 my-6 rounded-xl border border-dashed border-sidebar-border bg-background/50 px-3.5 py-5 text-center">
            <MessageSquareIcon
              className="mx-auto mb-2 size-4 text-muted-foreground/60"
              aria-hidden="true"
            />
            <p className="text-xs font-medium text-foreground/80">
              暂无历史对话
            </p>
            <p className="mt-1 text-[11px] leading-relaxed text-muted-foreground">
              在右侧输入高校数据查询或报告分析需求，即可开启新对话。
            </p>
          </div>
        )}

        {(sessionsQuery.data?.length ?? 0) > 0 && groups.length === 0 && (
          <div className="px-4 py-6 text-center text-xs text-muted-foreground">
            未找到匹配“{searchQuery}”的记录
          </div>
        )}
      </SidebarContent>
      <SidebarUserFooter />
    </Sidebar>
  );
}
