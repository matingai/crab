"use client";

import type { ReactNode } from "react";
import Link from "next/link";
import { useEffect, useState } from "react";
import { LoaderCircle, RefreshCw, ShieldEllipsis } from "lucide-react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Textarea } from "@/components/ui/textarea";
import { clearSession, fetchUserDetail, readSession, type NewApiUser } from "@/lib/newapi";

export function UserDetailView({ userId }: { userId: string }) {
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [user, setUser] = useState<NewApiUser | null>(null);
  const [tokenState, setTokenState] = useState<"missing" | "present">("missing");

  async function loadUser() {
    setLoading(true);
    setError(null);
    try {
      const session = readSession();
      setTokenState(session?.accessToken ? "present" : "missing");
      const nextUser = await fetchUserDetail(userId, session?.accessToken || null);
      setUser(nextUser);
    } catch (nextError) {
      setError(nextError instanceof Error ? nextError.message : "读取失败");
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    void loadUser();
  }, [userId]);

  return (
    <div className="grid gap-6">
      <Card className="rounded-[32px] overflow-hidden border-white/80 bg-white/86 shadow-[0_26px_80px_rgba(15,23,42,0.08)]">
        <CardHeader className="border-b border-slate-200/80 bg-[linear-gradient(135deg,rgba(14,116,144,0.98),rgba(15,23,42,0.92))] text-white">
          <div className="flex flex-wrap items-center gap-2">
            <Badge variant="outline" className="rounded-full border-white/20 bg-white/10 text-white/85">
              User Detail
            </Badge>
            <Badge variant="outline" className="rounded-full border-white/20 bg-white/10 text-white/85">
              {userId}
            </Badge>
            <Badge variant="outline" className="rounded-full border-white/20 bg-white/10 text-white/85">
              token: {tokenState}
            </Badge>
          </div>
          <CardTitle className="text-2xl font-semibold tracking-[-0.04em] text-white">
            用户详情页
          </CardTitle>
          <CardDescription className="text-slate-200">
            这里直接验证 `newapi` 返回的用户详情结构，并把原始 JSON 保留出来，方便你对字段做最终定型。
          </CardDescription>
        </CardHeader>

        <CardContent className="grid gap-6 p-6 lg:grid-cols-[minmax(0,1.1fr)_360px] lg:p-8">
          <section className="space-y-4">
            {loading ? (
              <StateBox icon={<LoaderCircle className="size-4 animate-spin" />} text="正在读取用户详情..." />
            ) : error ? (
              <StateBox
                icon={<ShieldEllipsis className="size-4" />}
                text={error}
                tone="error"
              />
            ) : user ? (
              <div className="grid gap-4 md:grid-cols-2">
                <InfoCard label="用户 ID" value={user.id} />
                <InfoCard label="用户名" value={user.username} />
                <InfoCard label="邮箱" value={user.email} />
                <InfoCard label="手机号" value={user.phone} />
                <InfoCard label="角色" value={user.role} />
                <InfoCard label="状态" value={user.status} />
                <InfoCard label="创建时间" value={user.createdAt} />
                <InfoCard label="更新时间" value={user.updatedAt} />
              </div>
            ) : (
              <StateBox icon={<ShieldEllipsis className="size-4" />} text="接口没有返回可展示的用户信息。" />
            )}

            <div className="flex flex-wrap gap-3">
              <Button type="button" onClick={() => void loadUser()} className="rounded-2xl">
                <RefreshCw className="mr-2 size-4" />
                刷新详情
              </Button>
              <Button
                type="button"
                variant="outline"
                className="rounded-2xl"
                onClick={() => {
                  clearSession();
                  setTokenState("missing");
                }}
              >
                清除本地 Session
              </Button>
            </div>
          </section>

          <section className="space-y-4">
            <div className="rounded-[28px] border border-slate-200/80 bg-slate-50/80 p-5">
              <div className="mb-2 text-sm font-semibold text-slate-900">联调建议</div>
              <ul className="space-y-2 text-sm leading-7 text-slate-600">
                <li>先确认登录接口是否真的返回用户 ID。</li>
                <li>如果详情接口需要 token，保持 `Authorization: Bearer ...` 这条约定不变。</li>
                <li>字段名如果和当前映射不同，只改 `lib/newapi.ts` 的 normalize 逻辑。</li>
              </ul>
            </div>

            <div>
              <div className="mb-2 text-sm font-semibold text-slate-900">原始 JSON</div>
              <Textarea
                readOnly
                value={user ? JSON.stringify(user.raw, null, 2) : ""}
                className="min-h-[320px] resize-none rounded-[28px] border-slate-200 bg-white/90 font-mono text-[12px] leading-6"
              />
            </div>

            <Button asChild variant="ghost" className="rounded-2xl">
              <Link href="/login">返回登录页</Link>
            </Button>
          </section>
        </CardContent>
      </Card>
    </div>
  );
}

function InfoCard({ label, value }: { label: string; value?: string | null }) {
  return (
    <div className="rounded-[24px] border border-slate-200/80 bg-white p-5 shadow-[0_16px_30px_rgba(15,23,42,0.04)]">
      <div className="text-[11px] font-medium uppercase tracking-[0.12em] text-slate-400">{label}</div>
      <div className="mt-2 break-words text-sm leading-7 text-slate-900">{value || "暂无"}</div>
    </div>
  );
}

function StateBox({
  icon,
  text,
  tone = "normal",
}: {
  icon: ReactNode;
  text: string;
  tone?: "normal" | "error";
}) {
  return (
    <div
      className={`flex items-center gap-3 rounded-[24px] border px-4 py-4 text-sm leading-6 ${
        tone === "error"
          ? "border-rose-200 bg-rose-50 text-rose-700"
          : "border-slate-200 bg-slate-50 text-slate-600"
      }`}
    >
      {icon}
      <span>{text}</span>
    </div>
  );
}
