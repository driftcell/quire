import { useState, type FormEvent } from "react";
import { useNavigate } from "@tanstack/react-router";
import {
  ChevronsUpDownIcon,
  KeyRoundIcon,
  LoaderCircleIcon,
  LogOutIcon,
  ShieldCheckIcon,
} from "lucide-react";
import { toast } from "sonner";
import { changePassword } from "@/api/auth";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { SidebarFooter } from "@/components/ui/sidebar";
import { useAuth } from "@/hooks/useAuth";
import { errorMessage } from "@/lib/errors";

export function SidebarUserFooter() {
  const navigate = useNavigate();
  const { user, isAdmin, logout } = useAuth();
  const [pwdOpen, setPwdOpen] = useState(false);
  const [currentPwd, setCurrentPwd] = useState("");
  const [newPwd, setNewPwd] = useState("");
  const [confirmPwd, setConfirmPwd] = useState("");
  const [saving, setSaving] = useState(false);

  if (!user) {
    return null;
  }

  const monogram =
    Array.from(user.display_name.trim() || user.username)[0]?.toUpperCase() ||
    "U";

  const handlePasswordSubmit = async (event: FormEvent) => {
    event.preventDefault();
    if (!currentPwd || !newPwd) {
      toast.error("请填写当前密码和新密码");
      return;
    }
    if (newPwd.length < 6) {
      toast.error("新密码长度至少为 6 位");
      return;
    }
    if (newPwd !== confirmPwd) {
      toast.error("两次输入的新密码不一致");
      return;
    }
    setSaving(true);
    try {
      await changePassword(currentPwd, newPwd);
      toast.success("密码已更新");
      setPwdOpen(false);
      setCurrentPwd("");
      setNewPwd("");
      setConfirmPwd("");
    } catch (error) {
      toast.error(errorMessage(error, "修改密码失败"));
    } finally {
      setSaving(false);
    }
  };

  return (
    <>
      <SidebarFooter className="border-t border-sidebar-border/60 p-2">
        <DropdownMenu>
          <DropdownMenuTrigger
            render={
              <button
                type="button"
                aria-label="用户账户菜单"
                className="flex w-full items-center gap-2.5 rounded-lg px-2 py-1.5 text-left text-xs transition-colors hover:bg-sidebar-accent hover:text-sidebar-accent-foreground focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none"
              />
            }
          >
            <span className="flex size-7 shrink-0 items-center justify-center rounded-full bg-primary/10 text-xs font-semibold text-primary ring-1 ring-primary/20">
              {monogram}
            </span>
            <div className="min-w-0 flex-1">
              <div className="flex items-center gap-1.5">
                <span className="truncate font-medium text-foreground">
                  {user.display_name}
                </span>
                {isAdmin && (
                  <span className="inline-flex shrink-0 items-center gap-0.5 rounded bg-primary/10 px-1 py-0.2 text-[10px] font-medium text-primary">
                    <ShieldCheckIcon className="size-2.5" aria-hidden="true" />
                    管理员
                  </span>
                )}
              </div>
              <div className="truncate text-[11px] text-muted-foreground">
                {user.email || `@${user.username}`}
              </div>
            </div>
            <ChevronsUpDownIcon
              className="size-3.5 shrink-0 text-muted-foreground"
              aria-hidden="true"
            />
          </DropdownMenuTrigger>
          <DropdownMenuContent side="top" align="start" className="w-52">
            <DropdownMenuGroup>
              {isAdmin && (
                <DropdownMenuItem
                  onClick={() => {
                    void navigate({ to: "/admin" });
                  }}
                >
                  <ShieldCheckIcon aria-hidden="true" />
                  管理控制台
                </DropdownMenuItem>
              )}
              <DropdownMenuItem onClick={() => setPwdOpen(true)}>
                <KeyRoundIcon aria-hidden="true" />
                修改密码
              </DropdownMenuItem>
              <DropdownMenuSeparator />
              <DropdownMenuItem
                variant="destructive"
                onClick={() => {
                  logout();
                  void navigate({ to: "/login", replace: true });
                }}
              >
                <LogOutIcon aria-hidden="true" />
                退出登录
              </DropdownMenuItem>
            </DropdownMenuGroup>
          </DropdownMenuContent>
        </DropdownMenu>
      </SidebarFooter>

      <Dialog open={pwdOpen} onOpenChange={setPwdOpen}>
        <DialogContent className="sm:max-w-sm">
          <DialogHeader>
            <DialogTitle>修改密码</DialogTitle>
            <DialogDescription>
              修改后将立即保存到数据库，下次登录请使用新密码。
            </DialogDescription>
          </DialogHeader>
          <form
            onSubmit={(e) => void handlePasswordSubmit(e)}
            className="space-y-3"
          >
            <div className="space-y-1.5">
              <Label htmlFor="pwd-current" className="text-xs">
                当前密码
              </Label>
              <Input
                id="pwd-current"
                type="password"
                autoComplete="current-password"
                value={currentPwd}
                onChange={(e) => setCurrentPwd(e.target.value)}
                disabled={saving}
              />
            </div>
            <div className="space-y-1.5">
              <Label htmlFor="pwd-new" className="text-xs">
                新密码（至少 6 位）
              </Label>
              <Input
                id="pwd-new"
                type="password"
                autoComplete="new-password"
                value={newPwd}
                onChange={(e) => setNewPwd(e.target.value)}
                disabled={saving}
              />
            </div>
            <div className="space-y-1.5">
              <Label htmlFor="pwd-confirm" className="text-xs">
                确认新密码
              </Label>
              <Input
                id="pwd-confirm"
                type="password"
                autoComplete="new-password"
                value={confirmPwd}
                onChange={(e) => setConfirmPwd(e.target.value)}
                disabled={saving}
              />
            </div>
            <DialogFooter className="pt-2">
              <Button
                type="button"
                variant="ghost"
                onClick={() => setPwdOpen(false)}
                disabled={saving}
              >
                取消
              </Button>
              <Button type="submit" disabled={saving}>
                {saving && (
                  <LoaderCircleIcon className="mr-1.5 size-3.5 animate-spin" />
                )}
                保存新密码
              </Button>
            </DialogFooter>
          </form>
        </DialogContent>
      </Dialog>
    </>
  );
}
