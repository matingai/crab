"use client";

import Link from "next/link";
import { useMemo, useState } from "react";
import { useRouter } from "next/navigation";
import { ArrowRight, LoaderCircle } from "lucide-react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { registerUser, loginUser, saveSession } from "@/lib/newapi";

type Mode = "login" | "register";

export function AuthForm({ mode }: { mode: Mode }) {
  const router = useRouter();
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [result, setResult] = useState<string | null>(null);
  const [form, setForm] = useState({
    username: "",
    email: "",
    account: "",
    password: "",
    confirmPassword: "",
  });

  const content = useMemo(() => {
    if (mode === "register") {
      return {
        title: "创建账号",
        description: "先把注册页和 newapi 服务层稳定下来，后面再补验证码、邀请码或短信登录。",
        submitLabel: "注册并进入详情",
        alternateHref: "/login",
        alternateLabel: "已有账号，去登录",
      };
    }
    return {
      title: "账号登录",
      description: "登录成功后会缓存 token，并直接跳转到用户详情页，方便你继续联调接口返回结构。",
      submitLabel: "登录并查看详情",
      alternateHref: "/register",
      alternateLabel: "没有账号，先注册",
    };
  }, [mode]);

  async function handleSubmit(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setSubmitting(true);
    setError(null);
    setResult(null);

    try {
      if (mode === "register") {
        if (form.password !== form.confirmPassword) {
          throw new Error("两次输入的密码不一致");
        }
        const session = await registerUser({
          username: form.username,
          email: form.email,
          password: form.password,
        });
        saveSession(session);
        setResult("注册成功，正在跳转到用户详情页。");
        if (session.user?.id) {
          router.push(`/users?userId=${encodeURIComponent(session.user.id)}`);
          return;
        }
      } else {
        const session = await loginUser({
          account: form.account,
          password: form.password,
        });
        saveSession(session);
        setResult("登录成功，正在读取用户详情。");
        if (session.user?.id) {
          router.push(`/users?userId=${encodeURIComponent(session.user.id)}`);
          return;
        }
      }
      setResult("接口调用成功，但返回里没有用户 ID。你可以先根据 newapi 实际结构补字段映射。");
    } catch (nextError) {
      setError(nextError instanceof Error ? nextError.message : "提交失败");
    } finally {
      setSubmitting(false);
    }
  }

  return (
    <Card className="overflow-hidden rounded-[32px] border-white/80 bg-white/84 shadow-[0_26px_80px_rgba(15,23,42,0.08)] backdrop-blur">
      <CardHeader className="border-b border-slate-200/80 bg-[linear-gradient(135deg,rgba(15,23,42,0.98),rgba(30,41,59,0.94))] text-white">
        <Badge variant="outline" className="w-fit rounded-full border-white/20 bg-white/10 text-white/80">
          {mode === "register" ? "Register" : "Login"}
        </Badge>
        <CardTitle className="text-2xl font-semibold tracking-[-0.04em] text-white">
          {content.title}
        </CardTitle>
        <CardDescription className="max-w-xl text-slate-300">{content.description}</CardDescription>
      </CardHeader>

      <CardContent className="grid gap-6 p-6 lg:p-8">
        <form className="grid gap-4" onSubmit={handleSubmit}>
          {mode === "register" ? (
            <>
              <FieldLabel label="用户名" hint="建议和 newapi 的唯一标识字段保持一致" />
              <Input
                value={form.username}
                onChange={(event) => setForm((prev) => ({ ...prev, username: event.target.value }))}
                placeholder="demo-user"
                required
              />

              <FieldLabel label="邮箱" hint="后续可扩成邮箱验证码流程" />
              <Input
                type="email"
                value={form.email}
                onChange={(event) => setForm((prev) => ({ ...prev, email: event.target.value }))}
                placeholder="demo@example.com"
                required
              />
            </>
          ) : (
            <>
              <FieldLabel label="账号" hint="这里先统一成 account，后面按接口决定用用户名还是邮箱" />
              <Input
                value={form.account}
                onChange={(event) => setForm((prev) => ({ ...prev, account: event.target.value }))}
                placeholder="邮箱或用户名"
                required
              />
            </>
          )}

          <FieldLabel label="密码" hint="后面如果 newapi 支持强度校验，再补规则提示" />
          <Input
            type="password"
            value={form.password}
            onChange={(event) => setForm((prev) => ({ ...prev, password: event.target.value }))}
            placeholder="••••••••"
            required
          />

          {mode === "register" ? (
            <>
              <FieldLabel label="确认密码" hint="页面先做本地一致性校验" />
              <Input
                type="password"
                value={form.confirmPassword}
                onChange={(event) => setForm((prev) => ({ ...prev, confirmPassword: event.target.value }))}
                placeholder="再次输入密码"
                required
              />
            </>
          ) : null}

          {error ? (
            <div className="rounded-2xl border border-rose-200 bg-rose-50 px-4 py-3 text-sm leading-6 text-rose-700">
              {error}
            </div>
          ) : null}

          {result ? (
            <div className="rounded-2xl border border-emerald-200 bg-emerald-50 px-4 py-3 text-sm leading-6 text-emerald-700">
              {result}
            </div>
          ) : null}

          <div className="flex flex-col gap-3 pt-2 sm:flex-row sm:items-center sm:justify-between">
            <Button type="submit" size="lg" className="rounded-2xl px-6" disabled={submitting}>
              {submitting ? <LoaderCircle className="mr-2 size-4 animate-spin" /> : <ArrowRight className="mr-2 size-4" />}
              {content.submitLabel}
            </Button>

            <Button asChild variant="ghost" className="rounded-2xl">
              <Link href={content.alternateHref}>{content.alternateLabel}</Link>
            </Button>
          </div>
        </form>
      </CardContent>
    </Card>
  );
}

function FieldLabel({ label, hint }: { label: string; hint: string }) {
  return (
    <div className="mt-1 flex items-baseline justify-between gap-3">
      <label className="text-sm font-medium text-slate-800">{label}</label>
      <span className="text-[11px] leading-5 text-slate-400">{hint}</span>
    </div>
  );
}
