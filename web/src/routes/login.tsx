import { useRef, useState, type FormEvent } from "react";
import { createFileRoute, Link, useNavigate } from "@tanstack/react-router";
import {
  AlertCircleIcon,
  LoaderCircleIcon,
  LockIcon,
  UserIcon,
} from "lucide-react";
import { toast } from "sonner";
import { BrandMark } from "@/components/layout/BrandMark";
import { Button } from "@/components/ui/button";
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
  const [fieldError, setFieldError] = useState<{
    identifier?: string;
    password?: string;
    general?: string;
  }>({});

  const identifierRef = useRef<HTMLInputElement>(null);
  const passwordRef = useRef<HTMLInputElement>(null);

  const handleSubmit = async (event: FormEvent) => {
    event.preventDefault();
    const trimmedId = identifier.trim();
    const errors: { identifier?: string; password?: string } = {};
    if (!trimmedId) {
      errors.identifier = "请填写你的用户名或工作邮箱";
    }
    if (!password) {
      errors.password = "请填写账户登录密码";
    }
    if (errors.identifier || errors.password) {
      setFieldError(errors);
      if (errors.identifier) {
        identifierRef.current?.focus();
      } else {
        passwordRef.current?.focus();
      }
      return;
    }

    setFieldError({});
    setSubmitting(true);
    try {
      await login(trimmedId, password);
      toast.success("已登录 Quire");
      await navigate({ to: "/" });
    } catch (error) {
      const msg = errorMessage(error, "账号或密码不匹配，请核对后重试");
      setFieldError({ general: msg });
      toast.error(msg);
      identifierRef.current?.focus();
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <main className="flex min-h-svh flex-col items-center justify-center bg-background px-5 py-10">
      <div className="w-full max-w-sm space-y-6">
        <header className="flex flex-col items-center space-y-2 text-center">
          <div className="inline-flex items-center gap-2.5">
            <BrandMark className="size-7 shrink-0" />
            <span className="font-serif text-xl font-bold tracking-tight text-foreground">
              Quire
            </span>
          </div>
          <p className="text-xs text-muted-foreground">
            使用工作账号登录，继续你的数据查询与报告编写
          </p>
        </header>

        <form
          noValidate
          onSubmit={(e) => void handleSubmit(e)}
          className="space-y-4 rounded-xl border border-border/90 bg-card p-6 shadow-2xs"
        >
          {fieldError.general && (
            <div
              role="alert"
              aria-live="polite"
              className="flex items-start gap-2 rounded-lg border border-destructive/35 bg-destructive/10 px-3 py-2 text-xs text-destructive"
            >
              <AlertCircleIcon className="mt-0.5 size-3.5 shrink-0" />
              <span>{fieldError.general}</span>
            </div>
          )}

          <div className="space-y-1.5">
            <Label htmlFor="login-identifier" className="text-xs font-medium">
              用户名或邮箱
            </Label>
            <div className="relative">
              <UserIcon
                className="pointer-events-none absolute top-1/2 left-3 size-3.5 -translate-y-1/2 text-muted-foreground"
                aria-hidden="true"
              />
              <Input
                ref={identifierRef}
                id="login-identifier"
                name="username"
                type="text"
                autoComplete="username"
                spellCheck={false}
                placeholder="输入用户名或工作邮箱…"
                aria-invalid={Boolean(fieldError.identifier)}
                aria-describedby={
                  fieldError.identifier ? "login-identifier-error" : undefined
                }
                className="pl-8"
                value={identifier}
                onChange={(e) => {
                  setIdentifier(e.target.value);
                  if (fieldError.identifier) {
                    setFieldError((prev) => ({
                      ...prev,
                      identifier: undefined,
                    }));
                  }
                }}
                disabled={submitting}
              />
            </div>
            {fieldError.identifier && (
              <p
                id="login-identifier-error"
                role="alert"
                className="text-[11px] text-destructive"
              >
                {fieldError.identifier}
              </p>
            )}
          </div>

          <div className="space-y-1.5">
            <Label htmlFor="login-password" className="text-xs font-medium">
              登录密码
            </Label>
            <div className="relative">
              <LockIcon
                className="pointer-events-none absolute top-1/2 left-3 size-3.5 -translate-y-1/2 text-muted-foreground"
                aria-hidden="true"
              />
              <Input
                ref={passwordRef}
                id="login-password"
                name="password"
                type="password"
                autoComplete="current-password"
                placeholder="请输入登录密码…"
                aria-invalid={Boolean(fieldError.password)}
                aria-describedby={
                  fieldError.password ? "login-password-error" : undefined
                }
                className="pl-8"
                value={password}
                onChange={(e) => {
                  setPassword(e.target.value);
                  if (fieldError.password) {
                    setFieldError((prev) => ({ ...prev, password: undefined }));
                  }
                }}
                disabled={submitting}
              />
            </div>
            {fieldError.password && (
              <p
                id="login-password-error"
                role="alert"
                className="text-[11px] text-destructive"
              >
                {fieldError.password}
              </p>
            )}
          </div>

          <Button type="submit" className="w-full" disabled={submitting}>
            {submitting && (
              <LoaderCircleIcon
                className="mr-1.5 size-4 animate-spin"
                aria-hidden="true"
              />
            )}
            {submitting ? "正在登录…" : "登录"}
          </Button>
        </form>

        <p className="text-center text-xs text-muted-foreground">
          还没有账号？{" "}
          <Link
            to="/register"
            className="font-medium text-foreground underline underline-offset-4 hover:text-[#1D4ED8]"
          >
            注册新账号
          </Link>
        </p>
      </div>
    </main>
  );
}
