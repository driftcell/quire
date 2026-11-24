import { useRef, useState, type FormEvent } from "react";
import { createFileRoute, Link, useNavigate } from "@tanstack/react-router";
import {
  AlertCircleIcon,
  BookOpenCheckIcon,
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
      toast.success("已进入 Quire 工作台");
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
    <div className="grid min-h-svh bg-background lg:grid-cols-[1.08fr_1fr]">
      {/* Left Column: Editorial Publication Colophon & Manifesto */}
      <aside className="hidden flex-col justify-between border-r border-border/90 bg-sidebar p-10 lg:flex xl:p-14">
        <div className="flex items-center gap-3">
          <BrandMark className="size-8 shrink-0" />
          <div>
            <div className="font-serif text-lg leading-none font-bold tracking-tight text-foreground">
              Quire
            </div>
            <div className="mt-1 text-xs text-muted-foreground">
              深度研究与结构化报告出版系统
            </div>
          </div>
        </div>

        <div className="my-auto max-w-lg space-y-6">
          <div className="inline-flex items-center gap-2 rounded-md border border-border bg-card px-2.5 py-1 text-xs font-medium text-[#1D4ED8]">
            <BookOpenCheckIcon className="size-3.5" aria-hidden="true" />
            <span>Multi-Agent Editorial Architecture</span>
          </div>
          <h1 className="font-serif text-3xl leading-snug font-bold tracking-tight text-balance text-foreground xl:text-4xl">
            把碎片化的数据检索与多维对标，凝练为可付印的严肃研究报告
          </h1>
          <p className="text-sm leading-relaxed text-muted-foreground">
            Quire
            以印刷术中的“折页手稿（Quire）”命名。多智能体在后台协同完成数据核验、ECharts
            出版级制图与章节撰写，你只需在 A4 画布上划词批注、审定成稿。
          </p>

          <dl className="grid grid-cols-3 gap-4 border-t border-border/90 pt-6">
            <div>
              <dt className="text-xs text-muted-foreground">排版引擎</dt>
              <dd className="mt-1 font-serif text-sm font-semibold text-foreground">
                A4 像素级分页
              </dd>
            </div>
            <div>
              <dt className="text-xs text-muted-foreground">证据链规范</dt>
              <dd className="mt-1 font-serif text-sm font-semibold text-foreground">
                块级溯源引证
              </dd>
            </div>
            <div>
              <dt className="text-xs text-muted-foreground">人机协同</dt>
              <dd className="mt-1 font-serif text-sm font-semibold text-foreground">
                跨块划词批注
              </dd>
            </div>
          </dl>
        </div>

        <footer className="flex items-center justify-between border-t border-border/80 pt-4 text-xs text-muted-foreground">
          <span>Quire Editorial Suite</span>
          <span className="font-mono text-[11px] tabular-nums">
            FOLIO EDITION · 2026
          </span>
        </footer>
      </aside>

      {/* Right Column: Focused Sign-In Form */}
      <main className="flex flex-col justify-center px-5 py-10 sm:px-10">
        <div className="mx-auto w-full max-w-sm space-y-6">
          <div className="flex items-center gap-2.5 lg:hidden">
            <BrandMark className="size-7 shrink-0" />
            <span className="font-serif text-lg font-bold tracking-tight text-foreground">
              Quire
            </span>
          </div>

          <header className="space-y-1.5">
            <h2 className="font-serif text-2xl font-bold tracking-tight text-foreground">
              登录 Quire 工作台
            </h2>
            <p className="text-xs leading-relaxed text-muted-foreground">
              输入你的用户名或工作邮箱以访问研究卷宗与报告画布
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
                  placeholder="admin 或 name@example.com…"
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
                      setFieldError((prev) => ({
                        ...prev,
                        password: undefined,
                      }));
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
              {submitting ? "正在进入工作台…" : "进入工作台"}
            </Button>
          </form>

          <p className="text-center text-xs text-muted-foreground">
            还没有工作台账号？{" "}
            <Link
              to="/register"
              className="font-medium text-foreground underline underline-offset-4 hover:text-[#1D4ED8]"
            >
              创建新账号
            </Link>
          </p>
        </div>
      </main>
    </div>
  );
}
