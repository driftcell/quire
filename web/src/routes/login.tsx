import { useState, type FormEvent } from "react";
import { createFileRoute, Link, useNavigate } from "@tanstack/react-router";
import { LoaderCircleIcon, LockIcon, UserIcon } from "lucide-react";
import { toast } from "sonner";
import { ProductBrand } from "@/components/layout/ProductBrand";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { useAuth } from "@/hooks/useAuth";
import { errorMessage } from "@/lib/errors";

export const Route = createFileRoute("/login")({
  component: LoginPage,
});

function LoginPage() {
  const navigate = useNavigate();
  const { login } = useAuth();
  const [identifier, setIdentifier] = useState("");
  const [password, setPassword] = useState("");
  const [submitting, setSubmitting] = useState(false);

  const handleSubmit = async (event: FormEvent) => {
    event.preventDefault();
    if (!identifier.trim() || !password) {
      toast.error("请输入用户名或邮箱以及密码");
      return;
    }
    setSubmitting(true);
    try {
      await login(identifier, password);
      toast.success("登录成功");
      await navigate({ to: "/" });
    } catch (error) {
      toast.error(errorMessage(error, "登录失败，请检查账号或密码"));
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
            <CardTitle className="text-lg font-semibold">登录 Quire</CardTitle>
            <p className="text-xs text-muted-foreground">
              使用管理员或个人账号登录工作台
            </p>
          </CardHeader>
          <CardContent>
            <form onSubmit={(e) => void handleSubmit(e)} className="space-y-4">
              <div className="space-y-1.5">
                <Label htmlFor="login-identifier" className="text-xs">
                  用户名或邮箱
                </Label>
                <div className="relative">
                  <UserIcon className="pointer-events-none absolute top-1/2 left-3 size-3.5 -translate-y-1/2 text-muted-foreground" />
                  <Input
                    id="login-identifier"
                    type="text"
                    autoComplete="username"
                    placeholder="admin 或 name@example.com"
                    className="pl-8"
                    value={identifier}
                    onChange={(e) => setIdentifier(e.target.value)}
                    disabled={submitting}
                  />
                </div>
              </div>
              <div className="space-y-1.5">
                <Label htmlFor="login-password" className="text-xs">
                  密码
                </Label>
                <div className="relative">
                  <LockIcon className="pointer-events-none absolute top-1/2 left-3 size-3.5 -translate-y-1/2 text-muted-foreground" />
                  <Input
                    id="login-password"
                    type="password"
                    autoComplete="current-password"
                    placeholder="请输入密码"
                    className="pl-8"
                    value={password}
                    onChange={(e) => setPassword(e.target.value)}
                    disabled={submitting}
                  />
                </div>
              </div>
              <Button type="submit" className="w-full" disabled={submitting}>
                {submitting && (
                  <LoaderCircleIcon className="mr-1.5 size-4 animate-spin" />
                )}
                登录
              </Button>
            </form>
            <div className="mt-4 text-center text-xs text-muted-foreground">
              还没有账号？{" "}
              <Link
                to="/register"
                className="font-medium text-foreground underline underline-offset-4 hover:text-primary"
              >
                立即注册
              </Link>
            </div>
          </CardContent>
        </Card>
      </div>
    </div>
  );
}
