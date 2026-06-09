"use client";

import { X } from "lucide-react";
import type { ComponentProps } from "react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";

export type NoticeEntry = {
  id: string;
  kind: string;
  message: string;
};

function noticeBadgeVariant(kind: string): ComponentProps<typeof Badge>["variant"] {
  switch (kind) {
    case "error":
      return "danger";
    case "success":
      return "success";
    case "approval":
      return "secondary";
    case "nudge":
    case "skill":
    default:
      return "soft";
  }
}

function noticeLabel(kind: string): string {
  switch (kind) {
    case "error":
      return "错误";
    case "success":
      return "完成";
    case "approval":
      return "审批";
    case "skill":
      return "技能";
    case "nudge":
    default:
      return "提示";
  }
}

export function NoticeStack({
  notices,
  onDismiss,
}: {
  notices: NoticeEntry[];
  onDismiss: (id: string) => void;
}) {
  if (!notices.length) {
    return null;
  }

  return (
    <div className="pointer-events-none absolute inset-x-0 top-12 z-40 mx-auto flex w-full max-w-[760px] flex-col items-center gap-1.5 px-4">
      {notices.map((notice) => (
        <div
          key={notice.id}
          className="pointer-events-auto flex max-w-full items-center gap-2 rounded-full border border-slate-200/60 bg-white/78 px-3 py-1.5 text-slate-700 shadow-[0_8px_22px_rgba(15,23,42,0.08)] backdrop-blur-md"
        >
          <Badge variant={noticeBadgeVariant(notice.kind)}>{noticeLabel(notice.kind)}</Badge>
          <div className="min-w-0 truncate text-xs leading-5 text-slate-600/90">{notice.message}</div>
          <Button
            type="button"
            variant="ghost"
            size="icon"
            className="h-6 w-6 shrink-0 rounded-full text-slate-400 hover:bg-slate-100/80 hover:text-slate-700"
            onClick={() => onDismiss(notice.id)}
            title="关闭通知"
          >
            <X className="size-3.5" />
          </Button>
        </div>
      ))}
    </div>
  );
}
