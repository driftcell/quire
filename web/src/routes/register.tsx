import { useState, type FormEvent } from "react";
import { createFileRoute, Link, useNavigate } from "@tanstack/react-router";
import {
  LoaderCircleIcon,
  LockIcon,
  MailIcon,
  UserIcon,
} from "lucide-react";
import { toast } from "sonner";
import { ProductBrand } from "@/components/layout/ProductBrand";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { useAuth } from "@/hooks/useAuth";
import { errorMessage } from "@/lib/errors";

export const Route = createFileRoute("/register")({
  component: RegisterPage,
});

function RegisterPage() {
  const navigate = useNavigate();
  const { register } = useAuth();
  const [username, setUsername] = useState("");
  const [email, setEmail] = useState("");
  const [displayName, setDisplayName] = useState("");
  const [password, setPassword] = useState("");
  const [confirmPassword, setConfirmPassword] = useState("");
  const [submitting, setSubmitting] = useState(false);

  const handleSubmit = async (event: FormEvent) => {
    event.preventDefault();
    if (!username.trim() || !password) {
      toast.error("请填写用户名和密码");
      return;
    }
    if (password.length < 6) {
      toast.error("密码长度至少为 6 位");
      return;
    }
    if (password !== confirmPassword) {
      toast.error("两次输入的密码不一致");
      return;
    }
    setSubmitting(true);
    try {
      await register({
        username,
        email: email.trim() || undefined,
        display_name: displayName.trim() || undefined,
        password,
      });
      toast.success("注册成功，已自动登录");
      await navigate({ to: "/" });
    } catch (error) {
      toast.error(errorMessage(error, "注册失败，请检查输入"));
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <div className="flex min-h-svh items-center justify-center bg-muted/30 px-4 py-10">
      <div className="w-full max-w-sm space-y-6">
        <div className="flex justify-center">
          <ProductBrand />
        </div>
        <Card className="border-border/80 shadow-sm">
          <CardHeader className="space-y-1 pb-4">
            <CardTitle className="text-lg font-semibold">注册账号</CardTitle>
            <p className="text-xs text-muted-foreground">
              创建你的 Quire 工作台账号
            </p>
          </CardHeader>
          <CardContent>
            <form onSubmit={(e) => void handleSubmit(e)} className="space-y-3.5">
              <div className="space-y-1.5">
                <Label htmlFor="reg-username" className="text-xs">
                  用户名 *
                </Label>
                <div className="relative">
                  <UserIcon className="pointer-events-none absolute top-1/2 left-3 size-3.5 -translate-y-1/2 text-muted-foreground" />
                  <Input
                    id="reg-username"
                    type="text"
                    autoComplete="username"
                    placeholder="3-32 位字母、数字、_ 或 -"
                    className="pl-8"
                    value={username}
                    onChange={(e) => setUsername(e.target.value)}
                    disabled={submitting}
                  />
                </div>
              </div>
              <div className="space-y-1.5">
                <Label htmlFor="reg-email" className="text-xs">
                  邮箱（可选）
                </Label>
                <div className="relative">
                  <MailIcon className="pointer-events-none absolute top-1/2 left-3 size-3.5 -translate-y-1/2 text-muted-foreground" />
                  <Input
                    id="reg-email"
                    type="email"
                    autoComplete="email"
                    placeholder="name@example.com"
                    className="pl-8"
                    value={email}
                    onChange={(e) => setEmail(e.target.value)}
                    disabled={submitting}
                  />
                </div>
              </div>
              <div className="space-y-1.5">
                <Label htmlFor="reg-display" className="text-xs">
                  显示名称（可选）
                </Label>
                <Input
                  id="reg-display"
                  type="text"
                  placeholder="例如：张研究员"
                  value={displayName}
                  onChange={(e) => setDisplayName(e.target.value)}
                  disabled={submitting}
                />
              </div>
              <div className="space-y-1.5">
                <Label htmlFor="reg-password" className="text-xs">
                  密码 *
                </Label>
                <div className="relative">
                  <LockIcon className="pointer-events-none absolute top-1/2 left-3 size-3.5 -translate-y-1/2 text-muted-foreground" />
                  <Input
                    id="reg-password"
                    type="password"
                    autoComplete="new-password"
                    placeholder="至少 6 位"
                    className="pl-8"
                    value={password}
                    onChange={(e) => setPassword(e.target.value)}
                    disabled={submitting}
                  />
                </div>
              </div>
              <div className="space-y-1.5">
                <Label htmlFor="reg-confirm" className="text-xs">
                  确认密码 *
                </Label>
                <div className="relative">
                  <LockIcon className="pointer-events-none absolute top-1/2 left-3 size-3.5 -translate-y-1/2 text-muted-foreground" />
                  <Input
                    id="reg-confirm"
                    type="password"
                    autoComplete="new-password"
                    placeholder="再次输入密码"
                    className="pl-8"
                    value={confirmPassword}
                    onChange={(e) => setConfirmPassword(e.target.value)}
                    disabled={submitting}
                  />
                </div>
              </div>
              <Button type="submit" className="w-full" disabled={submitting}>
                {submitting && (
                  <LoaderCircleIcon className="mr-1.5 size-4 animate-spin" />
                )}
                注册并登录
              </Button>
            </form>
            <div className="mt-4 text-center text-xs text-muted-foreground">
              已有账号？{" "}
              <Link
                to="/login"
                className="font-medium text-foreground underline underline-offset-4 hover:text-primary"
              >
                返回登录
              </Link>
            </div>
          </CardContent>
        </Card>
      </div>
    </div>
  );
}
