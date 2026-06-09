import type { ReactNode } from "react";
import Link from "next/link";
import { ChevronRight, ShieldCheck, UserRound, Waypoints } from "lucide-react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";

const navItems = [
  { href: "/login", label: "登录" },
  { href: "/register", label: "注册" },
  { href: "/users?userId=demo", label: "用户详情" },
];

export function WebShell({
  title,
  eyebrow,
  description,
  activePath,
  children,
}: {
  title: string;
  eyebrow: string;
  description: string;
  activePath: string;
  children: ReactNode;
}) {
  return (
    <main className="h-screen overflow-auto bg-[radial-gradient(circle_at_top_left,#e0f2fe_0%,rgba(224,242,254,0.35)_28%,transparent_52%),radial-gradient(circle_at_top_right,#fef3c7_0%,rgba(254,243,199,0.35)_22%,transparent_48%),linear-gradient(180deg,#fffdf8_0%,#f4f7fb_100%)] text-slate-900">
      <div className="mx-auto grid min-h-full max-w-7xl gap-5 px-4 py-5 lg:grid-cols-[260px_minmax(0,1fr)] lg:px-6">
        <aside className="flex min-h-0 flex-col rounded-[30px] border border-white/70 bg-[rgba(248,249,252,0.9)] px-3 pb-4 pt-5 shadow-[0_20px_60px_rgba(15,23,42,0.08)] backdrop-blur">
          <div className="px-3 pb-5">
            <div className="flex items-center gap-3">
              <div className="flex size-11 items-center justify-center rounded-2xl bg-slate-950 text-white shadow-[0_14px_34px_rgba(15,23,42,0.24)]">
                <Waypoints className="size-5" />
              </div>
              <div>
                <div className="text-sm font-semibold tracking-[-0.03em] text-slate-950">User Center</div>
                <div className="text-xs text-slate-500">newapi 接入页</div>
              </div>
            </div>
          </div>

          <nav className="grid gap-1">
            {navItems.map((item) => {
              const itemActive = activePath === item.href.split("?")[0];
              return (
                <Button
                  key={item.href}
                  asChild
                  variant="ghost"
                  className={cn(
                    "h-auto justify-start rounded-2xl px-3 py-3 text-left",
                    itemActive
                      ? "bg-slate-900 text-white hover:bg-slate-900 hover:text-white"
                      : "text-slate-600 hover:bg-white hover:text-slate-950",
                  )}
                >
                  <Link href={item.href} className="flex w-full items-center justify-between gap-3">
                    <span className="text-sm font-medium">{item.label}</span>
                    <ChevronRight className={cn("size-4", itemActive ? "opacity-100" : "opacity-35")} />
                  </Link>
                </Button>
              );
            })}
          </nav>

          <div className="mt-6 space-y-4 px-2">
            <div className="rounded-[24px] border border-slate-200/80 bg-white/92 p-4">
              <div className="mb-2 flex items-center gap-2 text-slate-900">
                <ShieldCheck className="size-4" />
                <div className="text-sm font-semibold">接入说明</div>
              </div>
              <div className="space-y-2 text-xs leading-6 text-slate-600">
                <p>注册、登录、用户详情已经拆成独立页面。</p>
                <p>
                  接口映射统一收口在{" "}
                  <code className="rounded bg-slate-100 px-1.5 py-0.5 text-[11px]">lib/newapi.ts</code>。
                </p>
              </div>
            </div>

            <div className="rounded-[24px] border border-slate-200/70 bg-slate-950 p-4 text-slate-50">
              <div className="mb-2 flex items-center gap-2">
                <UserRound className="size-4" />
                <div className="text-sm font-semibold">默认约定</div>
              </div>
              <ul className="space-y-1.5 text-xs leading-6 text-slate-300">
                <li>注册：`POST /api/user/register`</li>
                <li>登录：`POST /api/user/login`</li>
                <li>详情：`GET /api/user/:id`</li>
              </ul>
            </div>
          </div>
        </aside>

        <section className="min-h-0 py-1">
          <header className="rounded-[30px] border border-white/70 bg-white/72 px-6 py-6 shadow-[0_20px_60px_rgba(15,23,42,0.08)] backdrop-blur">
            <div className="max-w-3xl">
              <Badge variant="soft" className="rounded-full px-3 py-1 text-[11px] tracking-[0.14em] uppercase">
                {eyebrow}
              </Badge>
              <h1 className="mt-4 text-3xl font-semibold tracking-[-0.05em] text-slate-950 lg:text-5xl">
                {title}
              </h1>
              <p className="mt-3 max-w-2xl text-sm leading-7 text-slate-600 lg:text-base">
                {description}
              </p>
            </div>
          </header>

          <div className="pt-5">{children}</div>
        </section>
      </div>
    </main>
  );
}
