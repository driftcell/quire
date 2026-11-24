import { useRef, useState, type FormEvent } from "react";
import { createFileRoute, Link, useNavigate } from "@tanstack/react-router";
import {
  AlertCircleIcon,
  LoaderCircleIcon,
  LockIcon,
  MailIcon,
  UserIcon,
} from "lucide-react";
import { toast } from "sonner";
import { BrandMark } from "@/components/layout/BrandMark";
import { Button } from "@/components/ui/button";
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
  const [formError, setFormError] = useState<string | null>(null);

  const emailRef = useRef<HTMLInputElement>(null);
  const usernameRef = useRef<HTMLInputElement>(null);
  const passwordRef = useRef<HTMLInputElement>(null);
  const confirmRef = useRef<HTMLInputElement>(null);

  const handleSubmit = async (event: FormEvent) => {
    event.preventDefault();
    if (!email.trim()) {
      setFormError("请填写有效的工作邮箱");
      emailRef.current?.focus();
      return;
    }
    if (!username.trim()) {
      setFormError("请设置登录用户名（3–32 位字母、数字或下划线）");
      usernameRef.current?.focus();
      return;
    }
    if (password.length < 6) {
      setFormError("登录密码长度至少需要 6 位字符");
      passwordRef.current?.focus();
      return;
    }
    if (password !== confirmPassword) {
      setFormError("两次输入的密码不一致，请重新核对");
      confirmRef.current?.focus();
      return;
    }
    setFormError(null);
    setSubmitting(true);
    try {
      await register({
        username: username.trim(),
        email: email.trim(),
        display_name: displayName.trim() || undefined,
        password,
      });
      toast.success("账号创建成功");
      await navigate({ to: "/" });
    } catch (error) {
      const msg = errorMessage(error, "创建账号失败，请检查填写内容");
      setFormError(msg);
      toast.error(msg);
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <main className="flex min-h-svh flex-col items-center justify-center bg-background px-5 py-10">
      <div className="w-full max-w-sm space-y-5">
        <header className="flex flex-col items-center space-y-2 text-center">
          <div className="inline-flex items-center gap-2.5">
            <BrandMark className="size-7 shrink-0" />
            <span className="font-serif text-xl font-bold tracking-tight text-foreground">
              Quire
            </span>
          </div>
          <p className="text-xs text-muted-foreground">
            注册工作账号以开始数据查询与报告编写
          </p>
        </header>

        <form
          noValidate
          onSubmit={(e) => void handleSubmit(e)}
          className="space-y-3.5 rounded-xl border border-border/90 bg-card p-6 shadow-2xs"
        >
          {formError && (
            <div
              role="alert"
              aria-live="polite"
              className="flex items-start gap-2 rounded-lg border border-destructive/35 bg-destructive/10 px-3 py-2 text-xs text-destructive"
            >
              <AlertCircleIcon className="mt-0.5 size-3.5 shrink-0" />
              <span>{formError}</span>
            </div>
          )}

          <div className="space-y-1.5">
            <Label htmlFor="reg-email" className="text-xs font-medium">
              工作邮箱
            </Label>
            <div className="relative">
              <MailIcon
                className="pointer-events-none absolute top-1/2 left-3 size-3.5 -translate-y-1/2 text-muted-foreground"
                aria-hidden="true"
              />
              <Input
                ref={emailRef}
                id="reg-email"
                name="email"
                type="email"
                autoComplete="email"
                spellCheck={false}
                placeholder="name@example.com…"
                className="pl-8"
                value={email}
                onChange={(e) => setEmail(e.target.value)}
                disabled={submitting}
              />
            </div>
          </div>

          <div className="space-y-1.5">
            <Label htmlFor="reg-username" className="text-xs font-medium">
              用户名
            </Label>
            <div className="relative">
              <UserIcon
                className="pointer-events-none absolute top-1/2 left-3 size-3.5 -translate-y-1/2 text-muted-foreground"
                aria-hidden="true"
              />
              <Input
                ref={usernameRef}
                id="reg-username"
                name="username"
                type="text"
                autoComplete="username"
                spellCheck={false}
                placeholder="3–32 位字母、数字或下划线…"
                className="pl-8"
                value={username}
                onChange={(e) => setUsername(e.target.value)}
                disabled={submitting}
              />
            </div>
          </div>

          <div className="space-y-1.5">
            <Label htmlFor="reg-display" className="text-xs font-medium">
              姓名 / 显示名称（可选）
            </Label>
            <Input
              id="reg-display"
              name="display_name"
              type="text"
              autoComplete="name"
              placeholder="例如：张老师…"
              value={displayName}
              onChange={(e) => setDisplayName(e.target.value)}
              disabled={submitting}
            />
          </div>

          <div className="space-y-1.5">
            <Label htmlFor="reg-password" className="text-xs font-medium">
              设置密码
            </Label>
            <div className="relative">
              <LockIcon
                className="pointer-events-none absolute top-1/2 left-3 size-3.5 -translate-y-1/2 text-muted-foreground"
                aria-hidden="true"
              />
              <Input
                ref={passwordRef}
                id="reg-password"
                name="password"
                type="password"
                autoComplete="new-password"
                placeholder="至少 6 位字符…"
                className="pl-8"
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                disabled={submitting}
              />
            </div>
          </div>

          <div className="space-y-1.5">
            <Label htmlFor="reg-confirm" className="text-xs font-medium">
              确认密码
            </Label>
            <div className="relative">
              <LockIcon
                className="pointer-events-none absolute top-1/2 left-3 size-3.5 -translate-y-1/2 text-muted-foreground"
                aria-hidden="true"
              />
              <Input
                ref={confirmRef}
                id="reg-confirm"
                name="confirm_password"
                type="password"
                autoComplete="new-password"
                placeholder="再次输入登录密码…"
                className="pl-8"
                value={confirmPassword}
                onChange={(e) => setConfirmPassword(e.target.value)}
                disabled={submitting}
              />
            </div>
          </div>

          <Button type="submit" className="w-full" disabled={submitting}>
            {submitting && (
              <LoaderCircleIcon
                className="mr-1.5 size-4 animate-spin"
                aria-hidden="true"
              />
            )}
            {submitting ? "正在注册…" : "注册并登录"}
          </Button>
        </form>

        <p className="text-center text-xs text-muted-foreground">
          已有账号？{" "}
          <Link
            to="/login"
            className="font-medium text-foreground underline underline-offset-4 hover:text-[#1D4ED8]"
          >
            返回登录
          </Link>
        </p>
      </div>
    </main>
  );
}
