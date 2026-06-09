"use client";

import { FileSearch, FileText, Globe, PanelRightClose, PanelRightOpen } from "lucide-react";
import type { ComponentProps, HTMLAttributes, MouseEvent as ReactMouseEvent } from "react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";

type SharedViewerMode = "file" | "browser";

type AppTopBarProps = {
  dragRegionProps: Record<string, unknown>;
  isMacElectron: boolean;
  viewerPanelVisible: boolean;
  title: string;
  showConversationMeta: boolean;
  showConversationState: boolean;
  conversationState: string;
  conversationStateVariant: ComponentProps<typeof Badge>["variant"];
  runningToolCount: number;
  pendingApprovalCount: number;
  queuedRedirectPrompt: string | null;
  showViewerToggle: boolean;
  sharedViewerMode: SharedViewerMode;
  workspaceConfigured: boolean;
  browserViewerAvailable: boolean;
  fileViewerOpen: boolean;
  showContextDebugButton?: boolean;
  onOpenContextDebug?: () => void;
  onMouseDown: (event: ReactMouseEvent<HTMLElement>) => void;
  onOpenFileViewer: () => void;
  onOpenBrowserViewer: () => void;
  onToggleFileViewer: () => void;
};

export function AppTopBar({
  dragRegionProps,
  isMacElectron,
  viewerPanelVisible,
  title,
  showConversationMeta,
  showConversationState,
  conversationState,
  conversationStateVariant,
  runningToolCount,
  pendingApprovalCount,
  queuedRedirectPrompt,
  showViewerToggle,
  sharedViewerMode,
  workspaceConfigured,
  browserViewerAvailable,
  fileViewerOpen,
  showContextDebugButton = false,
  onOpenContextDebug,
  onMouseDown,
  onOpenFileViewer,
  onOpenBrowserViewer,
  onToggleFileViewer,
}: AppTopBarProps) {
  return (
    <header
      {...(dragRegionProps as HTMLAttributes<HTMLElement>)}
      className="flex h-11 shrink-0 select-none items-center px-4 text-slate-700"
      onMouseDown={onMouseDown}
    >
      <div
        className={`mx-auto flex w-full items-center justify-between gap-3 ${
          viewerPanelVisible ? "max-w-[1760px]" : "max-w-[1040px]"
        } ${isMacElectron ? "pl-[76px]" : ""}`}
      >
        <div className="flex min-w-0 items-center gap-1.5">
          {title ? <h2 className="truncate text-[15px] leading-6">{title}</h2> : <div />}
          {showConversationMeta ? (
            <>
              {showConversationState ? <Badge variant={conversationStateVariant}>{conversationState}</Badge> : null}
              {runningToolCount ? <Badge variant="outline">{runningToolCount} 个工具</Badge> : null}
              {pendingApprovalCount ? <Badge variant="secondary">{pendingApprovalCount} 条审批</Badge> : null}
              {queuedRedirectPrompt ? <Badge variant="soft">下一条已排队</Badge> : null}
            </>
          ) : null}
        </div>
        <div className="flex items-center gap-1.5">
          {showContextDebugButton ? (
            <Button
              type="button"
              variant="ghost"
              size="sm"
              data-no-drag
              className="h-8 gap-1.5 rounded-full px-2.5 text-xs text-slate-500 hover:bg-slate-100 hover:text-slate-900"
              onClick={onOpenContextDebug}
              title="打开当前会话最新的上下文快照"
            >
              <FileSearch className="size-3.5" />
              上下文快照
            </Button>
          ) : null}
          {showViewerToggle ? (
            <div data-no-drag className="inline-flex items-center gap-1 rounded-full border border-slate-200 bg-white/80 p-0.5">
              <Button
                type="button"
                variant={sharedViewerMode === "file" ? "secondary" : "ghost"}
                size="sm"
                className="h-6 rounded-full px-2.5 text-xs"
                disabled={!workspaceConfigured}
                onClick={onOpenFileViewer}
                title={workspaceConfigured ? "查看文件预览" : "请先配置 workspace root"}
              >
                <FileText className="mr-1 size-3.5" />
                文件
              </Button>
              <Button
                type="button"
                variant={sharedViewerMode === "browser" ? "secondary" : "ghost"}
                size="sm"
                className="h-6 rounded-full px-2.5 text-xs"
                disabled={!browserViewerAvailable}
                onClick={onOpenBrowserViewer}
                title={browserViewerAvailable ? "查看浏览器会话" : "当前没有可用的 browser session"}
              >
                <Globe className="mr-1 size-3.5" />
                浏览器
              </Button>
            </div>
          ) : null}
          {showViewerToggle ? (
            <Button
              type="button"
              variant="ghost"
              size="icon"
              data-no-drag
              className="h-8 w-8 shrink-0 rounded-full text-slate-500 hover:bg-slate-100 hover:text-slate-900"
              onClick={onToggleFileViewer}
              title={fileViewerOpen ? "关闭侧边查看器" : "打开侧边查看器"}
            >
              {fileViewerOpen ? <PanelRightClose className="size-4" /> : <PanelRightOpen className="size-4" />}
            </Button>
          ) : null}
        </div>
      </div>
    </header>
  );
}
