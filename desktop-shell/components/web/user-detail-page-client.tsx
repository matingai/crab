"use client";

import { useSearchParams } from "next/navigation";

import { UserDetailView } from "@/components/web/user-detail-view";
import { WebShell } from "@/components/web/web-shell";

export function UserDetailPageClient() {
  const searchParams = useSearchParams();
  const userId = searchParams.get("userId")?.trim() || "demo";

  return (
    <WebShell
      activePath="/users"
      eyebrow="newapi profile"
      title="用户详情页单独拆开"
      description="详情页只负责展示和刷新用户信息。这样后面补编辑、绑定角色、修改密码时，不会再挤进登录注册的状态流里。"
    >
      <UserDetailView userId={userId} />
    </WebShell>
  );
}
