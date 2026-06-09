"use client";

import { FolderClosed, FolderOpen, LoaderCircle } from "lucide-react";

import { Button } from "@/components/ui/button";
import {
  type WorkspaceListEntry,
  type WorkspaceSessionSummary,
  workspaceEntryKey,
  workspaceFolderName,
} from "@/lib/desktop-workspaces";

type WorkspaceSessionTreeProps = {
  workspaces: WorkspaceListEntry[];
  activeWorkspaceKey: string;
  currentSessionId: string | null;
  workspaceStatusLabel: string;
  onAddWorkspace: () => void;
  onToggleWorkspace: (workspace: WorkspaceListEntry) => void;
  onSelectWorkspace: (workspace: WorkspaceListEntry) => void;
  onSelectSession: (workspace: WorkspaceListEntry, session: WorkspaceSessionSummary) => void;
};

function truncate(value: string | null | undefined, maxLength: number): string {
  if (!value) {
    return "";
  }
  if (value.length <= maxLength) {
    return value;
  }
  return `${value.slice(0, Math.max(0, maxLength - 1))}…`;
}

function sessionDisplayTitle(session: WorkspaceSessionSummary): string {
  const value = session.title || session.session_id;
  return truncate(value.replace(/\s+/g, " ").trim(), 26);
}

function formatCompactTimestamp(value: number): string {
  return new Date(value * 1000).toLocaleString("zh-CN", {
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  });
}

function formatSessionTimestamp(value: number): string {
  const seconds = Math.max(0, Math.floor(Date.now() / 1000 - value));
  if (seconds < 60) {
    return "刚刚";
  }
  if (seconds < 60 * 60) {
    return `${Math.floor(seconds / 60)}分钟`;
  }
  if (seconds < 60 * 60 * 24) {
    return `${Math.floor(seconds / 3600)}小时`;
  }
  if (seconds < 60 * 60 * 24 * 7) {
    return `${Math.floor(seconds / 86400)}天`;
  }
  return formatCompactTimestamp(value);
}

export function WorkspaceSessionTree({
  workspaces,
  activeWorkspaceKey,
  currentSessionId,
  workspaceStatusLabel,
  onAddWorkspace,
  onToggleWorkspace,
  onSelectWorkspace,
  onSelectSession,
}: WorkspaceSessionTreeProps) {
  return (
    <div className="min-h-0 flex-1">
      <div className="mb-2 flex items-center justify-between px-2">
        <span className="text-[11px] uppercase tracking-[0.18em] text-slate-400">Workspaces</span>
        <Button
          type="button"
          variant="ghost"
          size="icon"
          className="h-6 w-6 rounded-md text-slate-400 hover:bg-slate-100 hover:text-slate-700"
          onClick={onAddWorkspace}
          title="添加或切换 workspace"
        >
          <FolderOpen className="size-3.5" />
        </Button>
      </div>
      <div className="min-h-0 flex-1 space-y-0.5 overflow-auto px-1">
        {workspaces.length ? (
          workspaces.map((workspace) => {
            const workspaceKey = workspaceEntryKey(workspace.workspaceRoot, workspace.dataDir);
            const activeWorkspace = workspaceKey === activeWorkspaceKey;
            return (
              <div key={workspaceKey}>
                <div className="flex min-w-0 items-center rounded-md px-1 py-0.5 transition hover:bg-slate-100/55">
                  <button
                    type="button"
                    className="flex min-w-0 flex-1 items-center gap-1.5 rounded-md px-0 py-0.5 text-left"
                    onClick={() => {
                      if (activeWorkspace) {
                        onToggleWorkspace(workspace);
                        return;
                      }
                      onSelectWorkspace(workspace);
                    }}
                    title={workspace.expanded ? "折叠 workspace" : "展开 workspace"}
                  >
                    {workspace.expanded ? (
                      <FolderOpen
                        className={`size-3.5 shrink-0 ${activeWorkspace ? "text-slate-600" : "text-slate-400"}`}
                      />
                    ) : (
                      <FolderClosed
                        className={`size-3.5 shrink-0 ${activeWorkspace ? "text-slate-600" : "text-slate-400"}`}
                      />
                    )}
                    <span
                      className={`min-w-0 flex-1 truncate text-[12px] font-medium leading-5 ${
                        activeWorkspace ? "text-slate-800" : "text-slate-600"
                      }`}
                    >
                      {workspaceFolderName(workspace.workspaceRoot)}
                    </span>
                    {workspace.loading ? (
                      <LoaderCircle className="size-3 shrink-0 animate-spin text-slate-400" />
                    ) : null}
                  </button>
                </div>
                {activeWorkspace && workspaceStatusLabel ? (
                  <div className="ml-6 truncate text-[11px] leading-4 text-slate-400">{workspaceStatusLabel}</div>
                ) : null}
                {workspace.expanded ? (
                  <div className="mt-0.5 space-y-0.5 pl-6">
                    {workspace.sessions.length ? (
                      workspace.sessions.map((session) => (
                        <button
                          key={`${workspaceKey}:${session.session_id}`}
                          type="button"
                          onClick={() => onSelectSession(workspace, session)}
                          className={`w-full rounded-md border px-0 py-0.5 text-left transition ${
                            activeWorkspace && session.session_id === currentSessionId
                              ? "border-transparent text-slate-800"
                              : "border-transparent text-slate-500 hover:text-slate-700"
                          }`}
                        >
                          <div className="flex min-w-0 items-center gap-1.5">
                            <span
                              className={`min-w-0 flex-1 truncate text-[12px] leading-5 ${
                                activeWorkspace && session.session_id === currentSessionId
                                  ? "font-medium text-slate-800"
                                  : "text-slate-500"
                              }`}
                            >
                              {sessionDisplayTitle(session)}
                            </span>
                            <span className="shrink-0 text-[11px] leading-5 text-slate-400">
                              {formatSessionTimestamp(session.updated_at_unix)}
                            </span>
                          </div>
                        </button>
                      ))
                    ) : (
                      <div className="px-2 py-1 text-xs leading-5 text-slate-400">
                        {workspace.error ? "加载失败" : workspace.loading ? "加载中" : "暂无会话"}
                      </div>
                    )}
                  </div>
                ) : null}
              </div>
            );
          })
        ) : (
          <div className="rounded-xl border border-dashed border-slate-200 bg-white/65 px-3 py-3 text-[13px] leading-[1.5] text-slate-500">
            选择一个 workspace 后，会在这里按文件夹展示它的会话。
          </div>
        )}
      </div>
    </div>
  );
}
