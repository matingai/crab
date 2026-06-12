"use client";

import {
  useEffect,
  useRef,
  useState,
  type CSSProperties,
  type FormEvent as ReactFormEvent,
  type MouseEvent as ReactMouseEvent,
  type RefObject
} from "react";
import {
  Bot,
  Check,
  ChevronDown,
  ChevronLeft,
  ChevronRight,
  Copy,
  FileCode2,
  FileJson2,
  FileText,
  FolderClosed,
  Globe,
  ImageIcon,
  LoaderCircle,
  Music4,
  Play,
  Plus,
  RefreshCw,
  Send,
  Settings2,
  Sparkles,
  Video,
  Wrench,
  X,
} from "lucide-react";
import ReactMarkdown, { defaultUrlTransform } from "react-markdown";
import remarkGfm from "remark-gfm";

import { Badge } from "@/components/ui/badge";
import { AppTopBar } from "@/components/desktop/app-top-bar";
import { NoticeStack, type NoticeEntry } from "@/components/desktop/notice-stack";
import {
  WorkspaceFilePreviewPane,
  type WorkspaceFilePreview,
} from "@/components/desktop/workspace-file-preview-pane";
import { WorkspaceSessionTree } from "@/components/desktop/workspace-session-tree";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Separator } from "@/components/ui/separator";
import { Textarea } from "@/components/ui/textarea";
import {
  getCurrentWindow,
  isElectronDesktop,
  invoke,
  listen,
  type UnlistenFn,
} from "@/lib/desktop";
import {
  type WorkspaceListEntry,
  createWorkspaceListEntry,
  loadWorkspaceList,
  mergeWorkspaceListEntries,
  storeWorkspaceList,
  workspaceEntryKey,
  workspaceFolderName,
} from "@/lib/desktop-workspaces";
import { cn } from "@/lib/utils";

declare global {
  namespace JSX {
    interface IntrinsicElements {
      webview: any;
    }
  }
}

type Preferences = {
  workspaceRoot: string;
  dataDir: string;
  provider: string;
  model: string;
  smallModel: string;
  baseUrl: string;
  apiKey: string;
  maxIterations: number;
  enableShellTool: boolean;
  sessionId: string;
  prompt: string;
};

type DesktopInfo = {
  shell: string;
  platform: string;
  global_event_topic: string;
  global_done_topic: string;
  cleared_topic: string;
  session_event_topic_template: string;
  session_done_topic_template: string;
  last_session_id: string | null;
  current_working_dir: string;
};

type BridgeRunResult = {
  session_id: string;
  response: string;
  status: "completed" | "awaiting_approval";
};

type ToolPhase = "running" | "done" | "error" | "approval";
type ParallelBatchStatus =
  | "running"
  | "completed"
  | "completed_with_errors"
  | "awaiting_approval"
  | "canceled";

type LatestContextDebugSnapshot = {
  path: string | null;
  debugDir: string;
};

type BridgeSkillSummary = {
  category: string;
  name: string;
  description: string;
  keywords: string[];
  task_kinds: string[];
  requires_tools: string[];
  requires_shell: boolean;
  updated_at_unix: number | null;
};

type ProviderSummary = {
  id: string;
  label: string;
  kind: string;
  enabled: boolean;
  is_default: boolean;
  model: string;
  base_url: string;
  api_mode: string;
  auth_source?: string | null;
};

type ProviderRuntimeStatus = {
  id: string;
  label: string;
  kind: string;
  model: string;
  base_url: string;
  api_mode: string;
  auth_source?: string | null;
  auth_required: boolean;
  ready: boolean;
};

type SharedProviderConfig = {
  configured: boolean;
  provider?: string | null;
  model?: string | null;
  baseUrl?: string | null;
  apiKey?: string | null;
  auxModel?: string | null;
};

type BridgeSkillFile = {
  path: string;
  size_bytes: number;
  file_type: string;
};

type BridgeSkillDetail = {
  category: string;
  name: string;
  description: string;
  keywords: string[];
  task_kinds: string[];
  requires_tools: string[];
  requires_shell: boolean;
  updated_at_unix: number | null;
  file_path: string;
  file_type: string;
  content: string;
  is_binary: boolean;
  linked_files: Record<string, BridgeSkillFile[]>;
  required_environment_variables: {
    name: string;
    prompt: string;
    help: string | null;
    required_for: string | null;
  }[];
  missing_required_environment_variables: string[];
  required_commands: string[];
  missing_required_commands: string[];
  config_requirements: {
    key: string;
    description: string;
    prompt: string | null;
    default_value: string | null;
    resolved_value: string | null;
  }[];
  setup_needed: boolean;
  readiness_status: string;
};

type BridgeSessionSummary = {
  session_id: string;
  title: string | null;
  model: string;
  created_at_unix: number;
  updated_at_unix: number;
  message_count: number;
  last_user_message: string | null;
  last_assistant_message: string | null;
};

type ChatMessage = {
  role: string;
  content: unknown;
  tool_call_id?: string | null;
};

type StoredTimelineEntry =
  | {
      type: "user";
      id: string;
      turn_id: string;
      content: string;
    }
  | {
      type: "assistant";
      id: string;
      turn_id: string;
      content: string;
    }
  | {
      type: "tool";
      id: string;
      turn_id: string;
      name: string;
      detail: string;
      command?: string | null;
      phase: ToolPhase;
      execution_mode?: string | null;
      batch_id?: string | null;
      batch_index?: number | null;
      batch_total?: number | null;
    }
  | {
      type: "batch";
      id: string;
      turn_id: string;
      batch_id: string;
      iteration: number;
      total_calls: number;
      completed_calls: number;
      status: ParallelBatchStatus;
    }
  | {
      type: "approval";
      id: string;
      turn_id: string;
      approval_id: string;
      tool_name: string;
      reason: string;
      command: string;
      execution_mode?: string | null;
      batch_id?: string | null;
      batch_index?: number | null;
      batch_total?: number | null;
    };

type BridgeSessionDetail = {
  summary: BridgeSessionSummary;
  history: ChatMessage[];
  timeline?: StoredTimelineEntry[];
};

type BridgeSessionSearchResult = {
  summary: BridgeSessionSummary;
  score: number;
  match_count: number;
  matched_messages: number;
  snippet: string;
};

type BrowserStreamEndpoint = {
  wsUrl: string;
  port: number;
  sessionName: string;
};

type BrowserCurrentUrlResponse = {
  url: string;
};

type BrowserStateSyncResponse = {
  ok: boolean;
  url: string | null;
  title?: string | null;
};

type BrowserUiSyncRequest = {
  token: number;
  forceNavigate: boolean;
};

type BrowserDirectUrlRequest = {
  token: number;
  url: string;
  title?: string;
};

type SlidevPreviewResponse = {
  ok: boolean;
  path: string;
  displayPath: string;
  fileName: string;
  port: number;
  url: string;
};

type SlidevExportResponse = {
  ok: boolean;
  format: "pdf" | "pptx";
  path: string;
  displayPath: string;
  fileName: string;
  sourceDeckPath: string;
  sourceDeckDisplayPath: string;
  experimental: boolean;
};

type BrowserLiveTab = {
  id: string;
  title: string;
  url: string | null;
  active: boolean;
};

type WorkspaceFileReference = {
  path: string;
  label: string;
};

type WorkspaceTreeNode = {
  path: string;
  name: string;
  kind: "directory" | "file";
  children: WorkspaceTreeNode[];
};

type WorkspaceTreeResponse = {
  rootPath: string;
  nodes: WorkspaceTreeNode[];
  truncated: boolean;
};

type WorkspaceFileTab = WorkspaceFilePreview;

type ConversationPanelResizeState = {
  panel: "tree" | "viewer";
  startX: number;
  startWidth: number;
};

type BridgeDelegateRun = {
  id: string;
  parent_session_id: string;
  parent_delegate_run_id?: string | null;
  root_delegate_run_id: string;
  session_id: string;
  prompt: string;
  prompt_preview: string;
  status: string;
  result_preview: string;
  max_iterations: number;
  attempt: number;
  created_at_unix: number;
  updated_at_unix: number;
};

type EventLogEntry = {
  seq: number;
  type: string;
  detail: string;
};

type ToolEntry = {
  id: string;
  name: string;
  phase: ToolPhase;
  detail: string;
  executionMode?: string;
  batchId?: string | null;
  durationMs?: number | null;
};

type ParallelBatchState = {
  batchId: string;
  iteration: number;
  totalCalls: number;
  completedCalls: number;
  status: ParallelBatchStatus;
  durationMs?: number | null;
};

type TimelineEntry =
  | {
      id: string;
      type: "user";
      content: string;
      pending?: boolean;
    }
  | {
      id: string;
      type: "assistant";
      content: string;
      streaming?: boolean;
    }
  | {
      id: string;
      type: "tool";
      name: string;
      detail: string;
      commandPreview?: string;
      phase: ToolPhase;
      executionMode?: string;
      batchId?: string | null;
      batchIndex?: number | null;
      batchTotal?: number | null;
      durationMs?: number | null;
    }
  | {
      id: string;
      type: "batch";
      batchId: string;
      iteration: number;
      totalCalls: number;
      completedCalls: number;
      status: ParallelBatchState["status"];
      durationMs?: number | null;
    }
  | {
      id: string;
      type: "approval";
      approvalId: string;
      toolName: string;
      reason: string;
      command: string;
      executionMode?: string;
      batchId?: string | null;
      batchIndex?: number | null;
      batchTotal?: number | null;
    };

type AssistantGroupEntry = Exclude<TimelineEntry, { type: "user" }>;

type ConversationBlock =
  | {
      id: string;
      type: "user";
      entry: Extract<TimelineEntry, { type: "user" }>;
      index: number;
    }
  | {
      id: string;
      type: "assistant_group";
      entries: AssistantGroupEntry[];
      startIndex: number;
    };

type AgentEventEnvelope = {
  seq: number;
  event_type: string;
  event: Record<string, unknown> & { type?: string };
};

type MainView = "conversation" | "skills" | "activity" | "settings";
type SharedViewerMode = "file" | "browser";

type ExtensionsOverview = {
  plugin_dirs: string[];
  plugins: {
    name: string;
    version: string;
    description: string;
    path: string;
    enabled: boolean;
    tool_names: string[];
    hook_names: string[];
  }[];
  providers: ProviderSummary[];
  mcp_servers: {
    name: string;
    transport: string;
    target: string;
    enabled: boolean;
    cache_ttl_seconds: number;
    cache_stale: boolean;
    discovered_tools_count: number;
    discovered_tool_names: string[];
    last_inspected_at_unix?: number | null;
  }[];
  cron_jobs: {
    id: string;
    schedule: string;
    prompt: string;
    prompt_preview: string;
    enabled: boolean;
    next_run_at_unix?: number | null;
    last_run_at_unix?: number | null;
    last_status?: string | null;
    last_session_id?: string | null;
    recent_runs: {
      job_id: string;
      session_id: string;
      status: string;
      response_preview: string;
      updated_at_unix: number;
    }[];
  }[];
};

type McpServerInspection = {
  server: {
    name: string;
    transport: string;
    target: string;
    enabled: boolean;
  };
  tools: {
    name: string;
    description: string;
    input_schema: unknown;
  }[];
};

type ApprovalRequest = {
  id: string;
  session_id: string;
  command: string;
  reason: string;
  tool_name?: string | null;
  execution_mode?: string | null;
  batch_id?: string | null;
  batch_index?: number | null;
  batch_total?: number | null;
  status: "pending" | "approved" | "denied" | "consumed";
  created_at_unix: number;
  updated_at_unix: number;
};

type CronSchedulerStatus = {
  running: boolean;
  paused_reason?: string | null;
  tick_interval_seconds: number;
  last_tick_at_unix?: number | null;
  last_due_job_ids: string[];
  last_error?: string | null;
  workspace_root?: string | null;
};

type CronJobFormState = {
  previousId: string | null;
  id: string;
  schedule: string;
  prompt: string;
  enabled: boolean;
};

const storageKey = "crab.desktop.preferences";
const legacyStorageKey = "hermes-agent-rs.desktop.preferences";
const defaultCronTickIntervalSeconds = 60;
const demoLaunchPrompt =
  "Inspect README.md and docs/AGENT_LOOP.md. Summarize Crab's strongest positioning angles, risky claims to avoid, and one demo workflow.";

const demoLoopSteps = [
  {
    kind: "goal",
    label: "Goal state",
    title: "Public launch objective locked",
    detail: "Main model keeps the goal, evidence, risks, and next moves in one visible loop.",
    meta: "controller",
  },
  {
    kind: "tool",
    label: "Tools",
    title: "Repository scan completed",
    detail: "README, docs, examples, tests, and privacy surfaces are checked before claims are made.",
    meta: "local runtime",
  },
  {
    kind: "delegate",
    label: "Delegation",
    title: "Worker run reviews bounded subtasks",
    detail: "Focused worker context handles docs and launch evidence while the main loop tracks direction.",
    meta: "sub-model",
  },
  {
    kind: "answer",
    label: "Answer",
    title: "Launch-ready response assembled",
    detail: "Final output cites concrete project artifacts instead of vague agent marketing language.",
    meta: "evidence first",
  },
] as const;

const demoRuntimeSignals = [
  "Goal-state controller",
  "Worker delegation",
  "Governed local tools",
  "Inspectable timeline",
  "Rust core + desktop shell",
] as const;

function emptyCronJobFormState(): CronJobFormState {
  return {
    previousId: null,
    id: "",
    schedule: "",
    prompt: "",
    enabled: true,
  };
}

function defaultConfig(): Preferences {
  return {
    workspaceRoot: "",
    dataDir: "",
    provider: "",
    model: "gpt-4.1-mini",
    smallModel: "",
    baseUrl: "",
    apiKey: "",
    maxIterations: 12,
    enableShellTool: false,
    sessionId: "",
    prompt: "",
  };
}

function loadPreferences(): Preferences {
  if (typeof window === "undefined") {
    return defaultConfig();
  }
  try {
    const raw =
      window.localStorage.getItem(storageKey) ?? window.localStorage.getItem(legacyStorageKey);
    if (!raw) {
      return defaultConfig();
    }
    return { ...defaultConfig(), ...JSON.parse(raw), prompt: "" };
  } catch {
    return defaultConfig();
  }
}

function localPreferencePayload(config: Preferences): Preferences {
  return {
    ...config,
    provider: "",
    model: "",
    smallModel: "",
    baseUrl: "",
    apiKey: "",
    prompt: "",
  };
}

function formatError(error: unknown): string {
  if (typeof error === "string") {
    return error;
  }
  if (error && typeof error === "object" && "message" in error) {
    const value = (error as { message?: unknown }).message;
    if (typeof value === "string") {
      return value;
    }
  }
  return JSON.stringify(error);
}

function shouldOpenWorkspaceFileExternally(file: WorkspaceFilePreview): boolean {
  switch (file.kind) {
    case "binary":
      return true;
    default:
      return false;
  }
}

function emptyToNull(value: string): string | null {
  return value.trim() ? value.trim() : null;
}

function resolveDataDir(config: Preferences): string | null {
  if (config.dataDir.trim()) {
    return config.dataDir.trim();
  }
  if (!config.workspaceRoot.trim()) {
    return null;
  }
  return `${config.workspaceRoot.trim()}/.hermes-agent-rs`;
}

function applySharedProviderConfig(config: Preferences, shared: SharedProviderConfig): Preferences {
  if (!shared.configured) {
    return config;
  }
  return {
    ...config,
    provider: shared.provider || "",
    model: shared.model || "",
    smallModel: shared.auxModel || "",
    baseUrl: shared.baseUrl || "",
    apiKey: shared.apiKey || "",
  };
}

function truncate(value: string | null | undefined, maxLength: number): string {
  if (!value) {
    return "";
  }
  if (value.length <= maxLength) {
    return value;
  }
  return `${value.slice(0, maxLength)}...`;
}

function normalizeInlineText(value: string | null | undefined): string {
  return (value || "").replace(/\s+/g, " ").trim();
}

function formatToolCommandPreview(value: string | null | undefined): string {
  return truncate(normalizeInlineText(value), 160);
}

function fallbackToolCommandPreview(detail: string): string {
  const { meta, stdout, stderr } = splitToolDetail(detail);
  const firstLine = [meta, stdout, stderr]
    .flatMap((section) => section.split("\n"))
    .map((line) => normalizeInlineText(line))
    .find(Boolean);
  return truncate(firstLine, 160);
}

function resolveToolCommandPreview(
  commandPreview: string | null | undefined,
  detail: string,
  existingPreview?: string,
): string {
  return (
    formatToolCommandPreview(commandPreview) ||
    formatToolCommandPreview(existingPreview) ||
    fallbackToolCommandPreview(detail)
  );
}

function lastMeaningfulLine(value: string): string {
  const lines = value
    .split("\n")
    .map((line) => line.trim())
    .filter(Boolean);
  return lines.length ? lines[lines.length - 1] : "";
}

function formatPendingTranscriptUpdates(count: number): string {
  if (count <= 0) {
    return "";
  }
  if (count > 99) {
    return "99+ 条更新";
  }
  return `${count} 条更新`;
}

function summarizeToolDetail(value: string): string {
  const { meta, stdout, stderr } = splitToolDetail(value);
  const latestStderr = lastMeaningfulLine(stderr);
  if (latestStderr) {
    return truncate(`stderr: ${latestStderr}`, 96);
  }
  const latestStdout = lastMeaningfulLine(stdout);
  if (latestStdout) {
    return truncate(latestStdout, 96);
  }
  return truncate(meta.replace(/\s+/g, " ").trim(), 96);
}

function splitToolDetail(value: string): { meta: string; stdout: string; stderr: string } {
  const stdoutIndex = value.indexOf("stdout:\n");
  const stderrIndex = value.indexOf("stderr:\n");

  if (stdoutIndex === -1 && stderrIndex === -1) {
    return {
      meta: value.trim(),
      stdout: "",
      stderr: "",
    };
  }

  const metaEndCandidates = [stdoutIndex, stderrIndex].filter((index) => index >= 0);
  const metaEnd = metaEndCandidates.length ? Math.min(...metaEndCandidates) : value.length;
  const meta = value.slice(0, metaEnd).trim();

  const stdout =
    stdoutIndex === -1
      ? ""
      : value
          .slice(
            stdoutIndex + "stdout:\n".length,
            stderrIndex > stdoutIndex ? stderrIndex : value.length,
          )
          .trim();

  const stderr =
    stderrIndex === -1
      ? ""
      : value
          .slice(stderrIndex + "stderr:\n".length)
          .trim();

  return { meta, stdout, stderr };
}

function compactSessionId(value: string | null | undefined): string {
  const normalized = (value || "").trim();
  if (!normalized) {
    return "new";
  }
  if (normalized.length <= 12) {
    return normalized;
  }
  return normalized.slice(0, 8);
}

function workspaceLabel(value: string | null | undefined): string {
  const normalized = (value || "").trim();
  if (!normalized) {
    return "未设置";
  }
  const segments = normalized.split(/[\\/]/).filter(Boolean);
  return segments.at(-1) || normalized;
}

function contentText(content: unknown): string {
  if (typeof content === "string") {
    return content;
  }
  if (content && typeof content === "object" && "text" in content) {
    const text = (content as { text?: unknown }).text;
    if (typeof text === "string") {
      return text;
    }
  }
  if (Array.isArray(content)) {
    return content
      .map((item) => {
        if (item && typeof item === "object" && "text" in item) {
          const text = (item as { text?: unknown }).text;
          return typeof text === "string" ? text : "";
        }
        return "";
      })
      .filter(Boolean)
      .join("");
  }
  if (content == null) {
    return "";
  }
  return String(content);
}

function promptSignature(value: string): string {
  return value.replace(/\r\n/g, "\n").trim();
}

function historyToTimeline(history: ChatMessage[]): TimelineEntry[] {
  return history
    .map((message, index) => ({
      message,
      index,
    }))
    .filter(({ message }) =>
      message.role === "user" || message.role === "assistant" || message.role === "tool",
    )
    .map(({ message, index }) => {
      if (message.role === "tool") {
        return {
          id: `history-tool-${index}`,
          type: "tool" as const,
          name: message.tool_call_id || "tool",
          detail: contentText(message.content),
          commandPreview: fallbackToolCommandPreview(contentText(message.content)) || undefined,
          phase: "done" as const,
        };
      }
      return {
        id: `history-${message.role}-${index}`,
        type: message.role as "user" | "assistant",
        content: contentText(message.content),
      };
    });
}

function storedTimelineToTimeline(entries: StoredTimelineEntry[]): TimelineEntry[] {
  return entries.map((entry) => {
    switch (entry.type) {
      case "user":
        return {
          id: entry.id,
          type: "user" as const,
          content: entry.content,
        };
      case "assistant":
        return {
          id: entry.id,
          type: "assistant" as const,
          content: entry.content,
        };
      case "tool":
        return {
          id: entry.id,
          type: "tool" as const,
          name: entry.name,
          detail: entry.detail,
          commandPreview: typeof entry.command === "string" ? entry.command : undefined,
          phase: entry.phase,
          executionMode: entry.execution_mode || undefined,
          batchId: entry.batch_id || null,
          batchIndex: entry.batch_index ?? null,
          batchTotal: entry.batch_total ?? null,
        };
      case "batch":
        return {
          id: entry.id,
          type: "batch" as const,
          batchId: entry.batch_id,
          iteration: entry.iteration,
          totalCalls: entry.total_calls,
          completedCalls: entry.completed_calls,
          status: entry.status,
        };
      case "approval":
        return {
          id: entry.id,
          type: "approval" as const,
          approvalId: entry.approval_id,
          toolName: entry.tool_name,
          reason: entry.reason,
          command: entry.command,
          executionMode: entry.execution_mode || undefined,
          batchId: entry.batch_id || null,
          batchIndex: entry.batch_index ?? null,
          batchTotal: entry.batch_total ?? null,
        };
    }
  });
}

function historyToTools(history: ChatMessage[]): ToolEntry[] {
  return history
    .filter((message) => message.role === "tool")
    .slice(-40)
    .reverse()
    .map((message, index) => ({
      id: `tool-history-${index}`,
      name: message.tool_call_id || "tool",
      phase: "done" as const,
      detail: contentText(message.content),
    }));
}

function timelineToTools(entries: TimelineEntry[]): ToolEntry[] {
  return entries
    .filter((entry): entry is Extract<TimelineEntry, { type: "tool" }> => entry.type === "tool")
    .slice(-40)
    .reverse()
    .map((entry) => ({
      id: entry.id,
      name: entry.name,
      phase: entry.phase,
      detail: entry.detail,
      executionMode: entry.executionMode,
      batchId: entry.batchId,
      durationMs: entry.durationMs,
    }));
}

function buildConversationBlocks(entries: TimelineEntry[]): ConversationBlock[] {
  const blocks: ConversationBlock[] = [];
  let assistantGroupEntries: AssistantGroupEntry[] = [];
  let assistantGroupStartIndex = -1;

  function flushAssistantGroup() {
    if (!assistantGroupEntries.length) {
      return;
    }
    blocks.push({
      id: `assistant-group-${assistantGroupEntries[0].id}`,
      type: "assistant_group",
      entries: assistantGroupEntries,
      startIndex: assistantGroupStartIndex,
    });
    assistantGroupEntries = [];
    assistantGroupStartIndex = -1;
  }

  entries.forEach((entry, index) => {
    if (entry.type === "user") {
      flushAssistantGroup();
      blocks.push({
        id: `user-block-${entry.id}`,
        type: "user",
        entry,
        index,
      });
      return;
    }

    if (!assistantGroupEntries.length) {
      assistantGroupStartIndex = index;
    }
    assistantGroupEntries.push(entry);
  });

  flushAssistantGroup();
  return blocks;
}

const WORKSPACE_FILE_SCHEME = "workspace-file://";
const WORKSPACE_FILE_URL_PATTERN = /workspace-file:\/\/[^\s`<>()\[\]{}]+/g;
const WORKSPACE_FILE_PATTERN =
  /(?:\/[^\s`<>()\[\]{}]+?\.[A-Za-z0-9][^\s`<>()\[\]{}]*|(?:\.{1,2}\/|[A-Za-z0-9_.-]+\/)[^\s`<>()\[\]{}]+?\.[A-Za-z0-9][^\s`<>()\[\]{}]*)/g;
const WORKSPACE_FILE_PROTECTED_SEGMENT_PATTERN =
  /(```[\s\S]*?```|`[^`\n]+`|!?\[[^\]]*]\([^)]+\))/g;

function cleanWorkspaceFileToken(value: string): string {
  return value
    .replace(/^[([<{`'"]+/, "")
    .replace(/[)\]>}`'",;]+$/, "")
    .replace(/[?#].*$/, "")
    .replace(/:(\d+)(?::\d+)?$/, "");
}

function normalizeWorkspaceFileReference(value: string, workspaceRoot?: string): string | null {
  const raw = value.trim();
  if (!raw) {
    return null;
  }
  let cleaned = cleanWorkspaceFileToken(raw);
  if (!cleaned) {
    return null;
  }
  if (cleaned.startsWith("file://")) {
    try {
      cleaned = decodeURIComponent(new URL(cleaned).pathname);
    } catch {
      return null;
    }
  } else if (cleaned.includes("://")) {
    return null;
  }
  if (cleaned.startsWith("/")) {
    if (!workspaceRoot?.trim()) {
      return cleaned;
    }
    return cleaned.startsWith(workspaceRoot.trim()) ? cleaned : null;
  }
  if (cleaned.startsWith("./") || cleaned.startsWith("../")) {
    return cleaned;
  }
  if (cleaned.includes("/")) {
    return cleaned;
  }
  if (/^[A-Za-z0-9_. -]+\.[A-Za-z0-9_-]+$/.test(cleaned)) {
    return cleaned;
  }
  return null;
}

function resolveWorkspaceHrefForOpen(href: string, workspaceRoot?: string): string | null {
  const fromScheme = parseWorkspaceFileHref(href);
  const candidate = fromScheme ?? normalizeWorkspaceFileReference(href, workspaceRoot);
  if (!candidate) {
    return null;
  }

  const normalizedWorkspaceRoot = normalizeSlashPath(workspaceRoot?.trim() || "");
  const normalizedCandidate = normalizeSlashPath(candidate);
  if (!normalizedCandidate) {
    return null;
  }
  if (normalizedCandidate.startsWith("/")) {
    return normalizedCandidate;
  }
  if (!normalizedWorkspaceRoot) {
    return normalizedCandidate;
  }
  return normalizeSlashPath(`${normalizedWorkspaceRoot}/${normalizedCandidate}`);
}

function workspaceFileHref(value: string): string {
  return `${WORKSPACE_FILE_SCHEME}${encodeURIComponent(value)}`;
}

function markdownUrlTransform(url: string): string {
  if (url.startsWith(WORKSPACE_FILE_SCHEME)) {
    return url;
  }
  return defaultUrlTransform(url);
}

function parseWorkspaceFileHref(value?: string | null): string | null {
  if (!value?.startsWith(WORKSPACE_FILE_SCHEME)) {
    return null;
  }
  try {
    return decodeURIComponent(value.slice(WORKSPACE_FILE_SCHEME.length));
  } catch {
    return null;
  }
}

function workspaceFileLabelFromHref(value: string): string {
  return parseWorkspaceFileHref(value) || value;
}

function linkifyWorkspacePathsInMarkdown(content: string, workspaceRoot?: string): string {
  return content
    .split(WORKSPACE_FILE_PROTECTED_SEGMENT_PATTERN)
    .map((segment) => {
      if (
        segment.startsWith("```") ||
        segment.startsWith("`") ||
        /^\!?\[[^\]]*]\([^)]+\)$/.test(segment)
      ) {
        return segment;
      }
      const withWorkspaceUrls = segment.replace(WORKSPACE_FILE_URL_PATTERN, (match) => {
        return `[${workspaceFileLabelFromHref(match)}](${match})`;
      });
      return withWorkspaceUrls.replace(WORKSPACE_FILE_PATTERN, (match) => {
        const resolved = normalizeWorkspaceFileReference(match, workspaceRoot);
        if (!resolved) {
          return match;
        }
        return `[${match}](${workspaceFileHref(resolved)})`;
      });
    })
    .join("");
}

function collectWorkspaceFileReferences(text: string, workspaceRoot?: string): WorkspaceFileReference[] {
  const results: WorkspaceFileReference[] = [];
  const seen = new Set<string>();
  for (const match of text.matchAll(WORKSPACE_FILE_PATTERN)) {
    const resolved = normalizeWorkspaceFileReference(match[0], workspaceRoot);
    if (!resolved || seen.has(resolved)) {
      continue;
    }
    seen.add(resolved);
    results.push({
      path: resolved,
      label: cleanedWorkspaceFileLabel(resolved, workspaceRoot),
    });
  }
  return results;
}

function cleanedWorkspaceFileLabel(path: string, workspaceRoot?: string): string {
  if (workspaceRoot?.trim() && path.startsWith(workspaceRoot.trim())) {
    const relative = path.slice(workspaceRoot.trim().length).replace(/^[/\\]+/, "");
    return relative || path;
  }
  return path;
}

function normalizeWorkspaceViewerPath(path: string, workspaceRoot?: string): string {
  const trimmed = path.trim().replace(/\\/g, "/");
  if (!trimmed) {
    return "";
  }
  const root = workspaceRoot?.trim().replace(/\\/g, "/");
  if (root && trimmed.startsWith(root)) {
    return trimmed.slice(root.length).replace(/^\/+/, "");
  }
  return trimmed.replace(/^\.\/+/, "");
}

function normalizeSlashPath(path: string): string {
  const normalized = path.replace(/\\/g, "/");
  const hasLeadingSlash = normalized.startsWith("/");
  const parts: string[] = [];
  for (const segment of normalized.split("/")) {
    if (!segment || segment === ".") {
      continue;
    }
    if (segment === "..") {
      if (parts.length && parts[parts.length - 1] !== "..") {
        parts.pop();
      } else if (!hasLeadingSlash) {
        parts.push("..");
      }
      continue;
    }
    parts.push(segment);
  }
  if (!parts.length) {
    return hasLeadingSlash ? "/" : "";
  }
  return `${hasLeadingSlash ? "/" : ""}${parts.join("/")}`;
}

function resolveMarkdownWorkspacePath(
  href: string,
  currentFilePath: string,
  workspaceRoot?: string,
): string | null {
  const fileHrefPath = parseWorkspaceFileHref(href);
  if (fileHrefPath) {
    return fileHrefPath;
  }

  const cleaned = cleanWorkspaceFileToken(href.trim());
  if (!cleaned || cleaned.startsWith("#") || cleaned.includes("://")) {
    return null;
  }

  const normalizedWorkspaceRoot = normalizeSlashPath(workspaceRoot?.trim() || "");
  if (!normalizedWorkspaceRoot) {
    return null;
  }

  if (cleaned.startsWith("/")) {
    const normalizedAbsolute = normalizeSlashPath(cleaned);
    if (
      normalizedAbsolute === normalizedWorkspaceRoot ||
      normalizedAbsolute.startsWith(`${normalizedWorkspaceRoot}/`)
    ) {
      return normalizedAbsolute;
    }
    return null;
  }

  const normalizedCurrentFile = normalizeSlashPath(currentFilePath);
  const currentDir =
    normalizedCurrentFile.slice(0, normalizedCurrentFile.lastIndexOf("/")) || normalizedWorkspaceRoot;
  const candidate = cleaned.includes("/") || cleaned.startsWith(".")
    ? `${currentDir}/${cleaned}`
    : `${currentDir}/${cleaned}`;
  const resolved = normalizeSlashPath(candidate);
  if (resolved === normalizedWorkspaceRoot || resolved.startsWith(`${normalizedWorkspaceRoot}/`)) {
    return resolved;
  }
  return null;
}

function workspaceTabMatchesPath(
  tab: WorkspaceFileTab,
  path: string,
  workspaceRoot?: string,
): boolean {
  const normalizedPath = normalizeWorkspaceViewerPath(path, workspaceRoot);
  if (!normalizedPath) {
    return false;
  }
  return [tab.path, tab.displayPath].some(
    (value) => normalizeWorkspaceViewerPath(value, workspaceRoot) === normalizedPath,
  );
}

function WorkspaceFileIcon({ file }: { file: WorkspaceFilePreview | WorkspaceFileReference }) {
  const type = "kind" in file ? file.kind : "";
  const fileType = "fileType" in file ? file.fileType : file.path.split(".").pop() || "";
  if (type === "image" || ["png", "jpg", "jpeg", "gif", "webp", "svg"].includes(fileType)) {
    return <ImageIcon className="size-4" />;
  }
  if (type === "audio" || ["mp3", "wav", "ogg", "m4a", "flac"].includes(fileType)) {
    return <Music4 className="size-4" />;
  }
  if (type === "video" || ["mp4", "webm", "mov", "m4v"].includes(fileType)) {
    return <Video className="size-4" />;
  }
  if (["json"].includes(fileType)) {
    return <FileJson2 className="size-4" />;
  }
  if (["ts", "tsx", "js", "jsx", "rs", "py", "css", "html", "sh"].includes(fileType)) {
    return <FileCode2 className="size-4" />;
  }
  return <FileText className="size-4" />;
}

function WorkspaceFileListSection({
  title,
  items,
  currentPath,
  onSelect,
}: {
  title: string;
  items: WorkspaceFileReference[];
  currentPath?: string | null;
  onSelect: (path: string) => void;
}) {
  if (!items.length) {
    return null;
  }

  return (
    <section className="space-y-1.5">
      <div className="px-1 text-[11px] uppercase tracking-[0.14em] text-slate-400">{title}</div>
      <div className="space-y-0.5">
        {items.map((item) => {
          const selected = item.path === currentPath;
          return (
            <button
              key={item.path}
              type="button"
              onClick={() => onSelect(item.path)}
              className={`flex w-full items-center gap-2 rounded-lg px-2.5 py-2 text-left transition ${
                selected
                  ? "bg-slate-900 text-white"
                  : "text-slate-600 hover:bg-slate-100/90 hover:text-slate-900"
              }`}
            >
              <span className={selected ? "text-white/80" : "text-slate-400"}>
                <WorkspaceFileIcon file={item} />
              </span>
              <div className="min-w-0 flex-1">
                <div className="truncate text-[13px] leading-5">
                  {item.label.split(/[\\/]/).at(-1) || item.label}
                </div>
                <div className={`truncate text-[11px] ${selected ? "text-white/60" : "text-slate-400"}`}>
                  {item.label}
                </div>
              </div>
            </button>
          );
        })}
      </div>
    </section>
  );
}

function DemoTimelineShowcase({ onUsePrompt }: { onUsePrompt: () => void }) {
  return (
    <section className="overflow-hidden rounded-xl border border-slate-200 bg-white/90 shadow-[0_18px_48px_rgba(15,23,42,0.06)]">
      <div className="border-b border-slate-100 bg-[linear-gradient(180deg,rgba(248,250,252,0.92)_0%,rgba(255,255,255,0.92)_100%)] px-4 py-3">
        <div className="flex flex-wrap items-start justify-between gap-3">
          <div className="min-w-0">
            <div className="flex flex-wrap items-center gap-2">
              <span className="inline-flex size-8 items-center justify-center rounded-lg bg-slate-900 text-white">
                <Bot className="size-4" />
              </span>
              <div className="min-w-0">
                <div className="flex flex-wrap items-center gap-2">
                  <h3 className="text-[15px] font-semibold leading-6 text-slate-900">Agent Loop Demo</h3>
                  <Badge variant="outline" className="border-emerald-200 bg-emerald-50 text-emerald-700">
                    螃蟹 / Crab
                  </Badge>
                </div>
                <p className="mt-0.5 max-w-2xl text-[12px] leading-5 text-slate-500">
                  主模型负责目标追踪和思维控制，工具与子模型负责可观察的执行片段。
                </p>
              </div>
            </div>
          </div>
          <Button
            type="button"
            size="sm"
            className="h-8 shrink-0 gap-1.5 rounded-lg px-3 text-[12px]"
            onClick={onUsePrompt}
          >
            <Play className="size-3.5" />
            填入演示任务
          </Button>
        </div>
      </div>

      <div className="grid gap-0 lg:grid-cols-[minmax(0,1fr)_260px]">
        <div className="space-y-0 divide-y divide-slate-100">
          {demoLoopSteps.map((step, index) => {
            const Icon =
              step.kind === "goal"
                ? Sparkles
                : step.kind === "tool"
                  ? Wrench
                  : step.kind === "delegate"
                    ? Bot
                    : Check;
            const iconClass =
              step.kind === "goal"
                ? "bg-sky-50 text-sky-700"
                : step.kind === "tool"
                  ? "bg-amber-50 text-amber-700"
                  : step.kind === "delegate"
                    ? "bg-violet-50 text-violet-700"
                    : "bg-emerald-50 text-emerald-700";

            return (
              <div key={step.title} className="grid grid-cols-[34px_minmax(0,1fr)] gap-3 px-4 py-3">
                <div className="flex flex-col items-center">
                  <span className={cn("inline-flex size-7 items-center justify-center rounded-lg", iconClass)}>
                    <Icon className="size-3.5" />
                  </span>
                  {index < demoLoopSteps.length - 1 ? <span className="mt-2 h-full w-px bg-slate-100" /> : null}
                </div>
                <div className="min-w-0 pb-0.5">
                  <div className="flex flex-wrap items-center gap-1.5">
                    <span className="text-[11px] font-medium uppercase tracking-[0.12em] text-slate-400">
                      {step.label}
                    </span>
                    <span className="text-[11px] text-slate-300">/</span>
                    <span className="font-mono text-[11px] text-slate-500">{step.meta}</span>
                  </div>
                  <div className="mt-1 text-[13px] font-semibold leading-5 text-slate-900">{step.title}</div>
                  <p className="mt-1 max-w-2xl text-[12px] leading-5 text-slate-600">{step.detail}</p>
                </div>
              </div>
            );
          })}
        </div>

        <aside className="border-t border-slate-100 bg-slate-50/70 px-4 py-3 lg:border-l lg:border-t-0">
          <div className="text-[11px] font-medium uppercase tracking-[0.14em] text-slate-400">Runtime Signals</div>
          <div className="mt-3 space-y-2">
            {demoRuntimeSignals.map((item) => (
              <div key={item} className="flex min-w-0 items-center gap-2 rounded-lg bg-white/80 px-2.5 py-2">
                <Check className="size-3.5 shrink-0 text-emerald-600" />
                <span className="min-w-0 truncate text-[12px] text-slate-700">{item}</span>
              </div>
            ))}
          </div>
          <div className="mt-3 rounded-lg border border-slate-200 bg-white/80 px-3 py-2">
            <div className="text-[11px] font-medium text-slate-500">Demo prompt</div>
            <p className="mt-1 line-clamp-3 text-[12px] leading-5 text-slate-700">{demoLaunchPrompt}</p>
          </div>
        </aside>
      </div>
    </section>
  );
}

function mergeUniquePaths(current: string[], next: string[]): string[] {
  return Array.from(new Set([...current, ...next]));
}

function findWorkspaceTreeAncestorDirectories(
  nodes: WorkspaceTreeNode[],
  targetPath: string,
): string[] {
  for (const node of nodes) {
    if (node.kind === "directory") {
      if (node.path === targetPath || node.children.some((child) => child.path === targetPath)) {
        return [node.path];
      }
      const childAncestors = findWorkspaceTreeAncestorDirectories(node.children, targetPath);
      if (childAncestors.length) {
        return [node.path, ...childAncestors];
      }
      continue;
    }
    if (node.path === targetPath) {
      return [];
    }
  }
  return [];
}

function findFirstWorkspaceFilePath(nodes: WorkspaceTreeNode[]): string | null {
  for (const node of nodes) {
    if (node.kind === "file") {
      return node.path;
    }
    const childPath = findFirstWorkspaceFilePath(node.children);
    if (childPath) {
      return childPath;
    }
  }
  return null;
}

function WorkspaceTreeBranch({
  nodes,
  depth,
  currentPath,
  expandedDirectories,
  onToggleDirectory,
  onSelectFile,
  selectedItemRef,
}: {
  nodes: WorkspaceTreeNode[];
  depth: number;
  currentPath?: string | null;
  expandedDirectories: string[];
  onToggleDirectory: (path: string) => void;
  onSelectFile: (path: string) => void;
  selectedItemRef: RefObject<HTMLButtonElement | null>;
}) {
  return (
    <div className="space-y-0.5">
      {nodes.map((node) => {
        const selected = currentPath === node.path;
        const paddingLeft = 10 + depth * 14;

        if (node.kind === "directory") {
          const expanded = expandedDirectories.includes(node.path);
          return (
            <div key={node.path}>
              <button
                type="button"
                onClick={() => onToggleDirectory(node.path)}
                className={`flex w-full items-center gap-2 rounded-lg py-2 pr-2 text-left transition ${
                  expanded || selected
                    ? "bg-slate-100 text-slate-900"
                    : "text-slate-600 hover:bg-slate-100/90 hover:text-slate-900"
                }`}
                style={{ paddingLeft }}
              >
                {expanded ? (
                  <ChevronDown className="size-3.5 shrink-0 text-slate-400" />
                ) : (
                  <ChevronRight className="size-3.5 shrink-0 text-slate-400" />
                )}
                <FolderClosed className="size-4 shrink-0 text-slate-400" />
                <span className="min-w-0 truncate text-[13px] leading-5">{node.name}</span>
              </button>
              {expanded ? (
                <WorkspaceTreeBranch
                  nodes={node.children}
                  depth={depth + 1}
                  currentPath={currentPath}
                  expandedDirectories={expandedDirectories}
                  onToggleDirectory={onToggleDirectory}
                  onSelectFile={onSelectFile}
                  selectedItemRef={selectedItemRef}
                />
              ) : null}
            </div>
          );
        }

        return (
          <button
            key={node.path}
            type="button"
            onClick={() => onSelectFile(node.path)}
            ref={selected ? selectedItemRef : undefined}
            className={`flex w-full items-center gap-2 rounded-lg py-2 pr-2 text-left transition ${
              selected
                ? "bg-slate-900 text-white"
                : "text-slate-600 hover:bg-slate-100/90 hover:text-slate-900"
            }`}
            style={{ paddingLeft }}
          >
            <span className="w-3.5 shrink-0" />
            <span className={selected ? "text-white/80" : "text-slate-400"}>
              <WorkspaceFileIcon file={{ path: node.path, label: node.name }} />
            </span>
            <span className="min-w-0 truncate text-[13px] leading-5">{node.name}</span>
          </button>
        );
      })}
    </div>
  );
}

function WorkspaceTreeSection({
  tree,
  loading,
  error,
  currentPath,
  expandedDirectories,
  onToggleDirectory,
  onSelectFile,
  onRefresh,
}: {
  tree: WorkspaceTreeResponse | null;
  loading: boolean;
  error: string | null;
  currentPath?: string | null;
  expandedDirectories: string[];
  onToggleDirectory: (path: string) => void;
  onSelectFile: (path: string) => void;
  onRefresh: () => void;
}) {
  const selectedItemRef = useRef<HTMLButtonElement | null>(null);

  useEffect(() => {
    selectedItemRef.current?.scrollIntoView({
      block: "nearest",
      inline: "nearest",
    });
  }, [currentPath, expandedDirectories]);

  return (
    <section className="space-y-1.5">
      <div className="flex items-center justify-between gap-2 px-1">
        <div className="text-[11px] uppercase tracking-[0.14em] text-slate-400">Workspace</div>
        <Button
          type="button"
          variant="ghost"
          size="icon"
          className="h-7 w-7 rounded-full text-slate-400 hover:bg-slate-100 hover:text-slate-700"
          onClick={onRefresh}
          disabled={loading}
          title="刷新文件夹列表"
        >
          <RefreshCw className={cn("size-3.5", loading ? "animate-spin" : "")} />
        </Button>
      </div>
      {loading ? (
        <div className="rounded-lg border border-dashed border-slate-200 px-3 py-3 text-xs text-slate-500">
          正在加载文件树...
        </div>
      ) : null}
      {error ? (
        <div className="rounded-lg border border-rose-200 bg-rose-50/80 px-3 py-3 text-xs leading-6 text-rose-700">
          {error}
        </div>
      ) : null}
      {!loading && !error && tree ? (
        <div className="space-y-2">
          <div className="truncate px-1 text-[11px] text-slate-400">{tree.rootPath}</div>
          {tree.truncated ? (
            <div className="rounded-lg border border-amber-200 bg-amber-50/80 px-3 py-2 text-xs text-amber-700">
              文件树已截断，只显示前 5000 个条目。
            </div>
          ) : null}
          {tree.nodes.length ? (
            <WorkspaceTreeBranch
              nodes={tree.nodes}
              depth={0}
              currentPath={currentPath}
              expandedDirectories={expandedDirectories}
              onToggleDirectory={onToggleDirectory}
              onSelectFile={onSelectFile}
              selectedItemRef={selectedItemRef}
            />
          ) : (
            <div className="rounded-lg border border-dashed border-slate-200 px-3 py-3 text-xs text-slate-500">
              当前 workspace 没有可展示的文件。
            </div>
          )}
        </div>
      ) : null}
    </section>
  );
}

function WorkspaceFileTabsBar({
  tabs,
  activePath,
  onSelect,
  onClose,
}: {
  tabs: WorkspaceFileTab[];
  activePath: string | null;
  onSelect: (path: string) => void;
  onClose: (path: string) => void;
}) {
  if (!tabs.length) {
    return null;
  }

  return (
    <div className="flex items-center gap-1.5 overflow-x-auto border-b border-slate-200/80 bg-white/70 px-2.5 py-1.5">
      {tabs.map((tab) => {
        const active = tab.path === activePath;
        return (
          <div
            key={tab.path}
            className={`group flex max-w-[220px] shrink-0 items-center gap-1 rounded-lg border px-2 py-1 transition ${
              active
                ? "border-slate-900 bg-slate-900 text-white"
                : "border-slate-200 bg-white/80 text-slate-600 hover:border-slate-300 hover:text-slate-900"
            }`}
          >
            <button
              type="button"
              onClick={() => onSelect(tab.path)}
              className="min-w-0 flex-1 text-left"
              title={tab.displayPath}
            >
              <div className="truncate text-[12px] font-medium leading-4">{tab.fileName}</div>
            </button>
            <button
              type="button"
              onClick={() => onClose(tab.path)}
              className={`rounded-full p-0.5 transition ${
                active
                  ? "text-white/70 hover:bg-white/10 hover:text-white"
                  : "text-slate-400 hover:bg-slate-100 hover:text-slate-700"
              }`}
              title={`关闭 ${tab.fileName}`}
            >
              <X className="size-3.5" />
            </button>
          </div>
        );
      })}
    </div>
  );
}

function AgentBrowserLivePane({
  endpoint,
  endpointLoading,
  endpointError,
  retryToken,
  onRetry,
}: {
  endpoint: BrowserStreamEndpoint | null;
  endpointLoading: boolean;
  endpointError: string | null;
  retryToken: number;
  onRetry?: () => void;
}) {
  const viewportRef = useRef<HTMLDivElement | null>(null);
  const imageRef = useRef<HTMLImageElement | null>(null);
  const websocketRef = useRef<WebSocket | null>(null);
  const frameBase64Ref = useRef<string | null>(null);
  const [connectionState, setConnectionState] = useState<"idle" | "connecting" | "connected" | "error">("idle");
  const [tabs, setTabs] = useState<BrowserLiveTab[]>([]);
  const [frameBase64, setFrameBase64] = useState<string | null>(null);
  const [frameSize, setFrameSize] = useState<{ width: number; height: number } | null>(null);
  const [streamError, setStreamError] = useState<string | null>(null);
  const [viewportFocused, setViewportFocused] = useState(false);
  const [lastFrameAt, setLastFrameAt] = useState<number | null>(null);

  useEffect(() => {
    setTabs([]);
    setFrameBase64(null);
    setFrameSize(null);
    setStreamError(null);
    setViewportFocused(false);
    setLastFrameAt(null);
  }, [endpoint?.wsUrl, retryToken]);

  useEffect(() => {
    frameBase64Ref.current = frameBase64;
  }, [frameBase64]);

  useEffect(() => {
    if (!endpoint?.wsUrl) {
      setConnectionState("idle");
      return;
    }

    let disposed = false;
    setConnectionState("connecting");
    setStreamError(null);
    const socket = new WebSocket(endpoint.wsUrl);
    websocketRef.current = socket;

    socket.onopen = () => {
      if (disposed) {
        return;
      }
      setConnectionState("connected");
    };
    socket.onerror = () => {
      if (disposed) {
        return;
      }
      setConnectionState("error");
      setStreamError("WS 连接失败");
    };
    socket.onclose = () => {
      if (disposed) {
        return;
      }
      setConnectionState((current) => (current === "error" ? current : "idle"));
      setStreamError((current) => current || "浏览器 WS 已断开");
    };
    socket.onmessage = (event) => {
      if (disposed) {
        return;
      }
      try {
        const payload = JSON.parse(String(event.data || "{}")) as {
          type?: string;
          jpeg?: string;
          data?:
            | string
            | {
                jpeg?: string;
                metadata?: {
                  viewport?: { width?: number; height?: number };
                  width?: number;
                  height?: number;
                };
                size?: { width?: number; height?: number };
                tabs?: Array<{
                  tabId?: string | number;
                  id?: string | number;
                  title?: string;
                  label?: string;
                  url?: string;
                  active?: boolean;
                  selected?: boolean;
                  current?: boolean;
                }>;
              };
          metadata?: {
            deviceWidth?: number;
            deviceHeight?: number;
          };
          tabs?: Array<{
            tabId?: string | number;
            id?: string | number;
            title?: string;
            label?: string;
            url?: string;
            active?: boolean;
            selected?: boolean;
            current?: boolean;
          }>;
          activeTabId?: string | number;
          activeTab?:
            | string
            | number
            | {
                tabId?: string | number;
                id?: string | number;
              };
          connected?: boolean;
          screencasting?: boolean;
          viewportWidth?: number;
          viewportHeight?: number;
          error?: string;
        };
        const payloadObject = typeof payload.data === "object" ? payload.data : null;
        const payloadData = typeof payload.data === "string" ? payload.data : null;
        const jpeg = payload.jpeg || payloadData || payloadObject?.jpeg || null;
        const width =
          payload.metadata?.deviceWidth ||
          payloadObject?.metadata?.viewport?.width ||
          payloadObject?.metadata?.width ||
          payloadObject?.size?.width ||
          null;
        const height =
          payload.metadata?.deviceHeight ||
          payloadObject?.metadata?.viewport?.height ||
          payloadObject?.metadata?.height ||
          payloadObject?.size?.height ||
          null;
        if (payload.type === "frame" && jpeg) {
          setFrameBase64(jpeg);
          if (width && height) {
            setFrameSize({
              width,
              height,
            });
          }
          setLastFrameAt(Date.now());
          setConnectionState("connected");
          return;
        }
        if (payload.type === "tabs") {
          const activeTabId =
            typeof payload.activeTab === "object"
              ? payload.activeTab?.tabId ?? payload.activeTab?.id ?? payload.activeTabId ?? null
              : payload.activeTab ?? payload.activeTabId ?? null;
          const nextTabs = (payload.tabs || payloadObject?.tabs || [])
            .map((tab, index) => {
              const id = tab.tabId ?? tab.id ?? `tab-${index}`;
              const title = tab.label?.trim() || tab.title?.trim() || tab.url?.trim() || "New Tab";
              return {
                id: String(id),
                title,
                url: tab.url?.trim() || null,
                active:
                  Boolean(tab.active) ||
                  Boolean(tab.selected) ||
                  Boolean(tab.current) ||
                  (activeTabId !== null && String(activeTabId) === String(id)),
              } satisfies BrowserLiveTab;
            })
            .filter((tab) => tab.id);
          if (nextTabs.length) {
            setTabs(nextTabs);
          }
          return;
        }
        if (payload.type === "status") {
          if (payload.connected === false) {
            setStreamError("agent-browser stream 已连接，但浏览器还没附着到当前 session");
          } else if (payload.screencasting === false && !frameBase64Ref.current) {
            setStreamError("agent-browser stream 已连接，正在等待浏览器开始推送画面");
          } else {
            setStreamError(null);
          }
          return;
        }
        if (payload.type === "error") {
          setConnectionState("error");
          setStreamError(payload.error || "stream 返回错误");
        }
      } catch {
        setConnectionState("error");
        setStreamError("无法解析浏览器 stream 消息");
      }
    };

    return () => {
      disposed = true;
      websocketRef.current = null;
      socket.close();
    };
  }, [endpoint?.wsUrl, retryToken]);

  function sendSocketMessage(payload: unknown) {
    const socket = websocketRef.current;
    if (!socket || socket.readyState !== WebSocket.OPEN) {
      return;
    }
    socket.send(JSON.stringify(payload));
  }

  function resolveViewportPoint(clientX: number, clientY: number) {
    const image = imageRef.current;
    const size = frameSize;
    if (!image || !size) {
      return null;
    }
    const rect = image.getBoundingClientRect();
    if (!rect.width || !rect.height) {
      return null;
    }

    const scale = Math.min(rect.width / size.width, rect.height / size.height);
    const renderedWidth = size.width * scale;
    const renderedHeight = size.height * scale;
    const offsetX = rect.left + (rect.width - renderedWidth) / 2;
    const offsetY = rect.top + (rect.height - renderedHeight) / 2;
    const x = clientX - offsetX;
    const y = clientY - offsetY;
    if (x < 0 || y < 0 || x > renderedWidth || y > renderedHeight) {
      return null;
    }

    return {
      x: Math.round((x / renderedWidth) * size.width),
      y: Math.round((y / renderedHeight) * size.height),
    };
  }

  function handleViewportClick(event: ReactMouseEvent<HTMLImageElement>) {
    const point = resolveViewportPoint(event.clientX, event.clientY);
    if (!point) {
      return;
    }
    viewportRef.current?.focus();
    sendSocketMessage({
      type: "input_mouse",
      eventType: "mousePressed",
      x: point.x,
      y: point.y,
      button: "left",
      clickCount: 1,
    });
    sendSocketMessage({
      type: "input_mouse",
      eventType: "mouseReleased",
      x: point.x,
      y: point.y,
      button: "left",
      clickCount: 1,
    });
  }

  function handleViewportKeyDown(event: React.KeyboardEvent<HTMLDivElement>) {
    if (event.metaKey || event.ctrlKey || event.altKey) {
      return;
    }
    const printable = event.key.length === 1 ? event.key : "";
    sendSocketMessage({
      type: "input_keyboard",
      eventType: "keyDown",
      key: event.key,
      code: event.code,
      text: printable,
    });
    sendSocketMessage({
      type: "input_keyboard",
      eventType: "keyUp",
      key: event.key,
      code: event.code,
      text: printable,
    });
  }

  if (endpointLoading && !endpoint) {
    return (
      <div className="flex h-full min-h-[320px] items-center justify-center rounded-xl border border-dashed border-slate-200 bg-white/80 text-sm text-slate-500">
        正在连接 agent-browser stream...
      </div>
    );
  }

  if (endpointError) {
    return (
      <div className="rounded-xl border border-rose-200 bg-rose-50/80 px-4 py-4 text-sm leading-6 text-rose-700">
        <div>{endpointError}</div>
        {onRetry ? (
          <div className="mt-3">
            <Button type="button" variant="outline" size="sm" className="rounded-lg" onClick={onRetry}>
              手动重连
            </Button>
          </div>
        ) : null}
      </div>
    );
  }

  if (!endpoint) {
    return (
      <div className="flex h-full min-h-[320px] items-center justify-center rounded-xl border border-dashed border-slate-200 bg-white/80 px-6 text-center text-sm leading-7 text-slate-500">
        当前会话还没有可用的 agent-browser stream 端点。
      </div>
    );
  }

  const connectionBadgeVariant =
    connectionState === "connected"
      ? "success"
      : connectionState === "error"
        ? "danger"
        : connectionState === "connecting"
          ? "soft"
          : "outline";
  const connectionLabel =
    connectionState === "connected"
      ? "画面已连接"
      : connectionState === "error"
        ? "连接异常"
        : connectionState === "connecting"
          ? "连接中"
          : "未连接";

  return (
    <div className="flex h-full min-h-0 flex-col overflow-hidden rounded-xl border border-slate-200 bg-white/92">
      <div className="flex flex-wrap items-center justify-between gap-2 border-b border-slate-200 bg-slate-50/90 px-3 py-2">
        <div className="flex min-w-0 flex-wrap items-center gap-2">
          <Badge variant={connectionBadgeVariant}>{connectionLabel}</Badge>
          {frameSize ? <Badge variant="outline">{frameSize.width}x{frameSize.height}</Badge> : null}
          {tabs.length ? <Badge variant="outline">{tabs.length} 个标签</Badge> : null}
          <span className="truncate text-xs text-slate-500">{endpoint.sessionName}</span>
        </div>
        <div className="flex items-center gap-2 text-xs text-slate-500">
          <span>{viewportFocused ? "键盘输入已连接到画面" : "点击画面后可发送键盘输入"}</span>
          {onRetry && connectionState !== "connecting" ? (
            <Button type="button" variant="outline" size="sm" className="h-7 rounded-full px-3" onClick={onRetry}>
              重连
            </Button>
          ) : null}
        </div>
      </div>

      {tabs.length ? (
        <div className="flex gap-2 overflow-x-auto border-b border-slate-200 bg-slate-100/80 px-3 py-2">
          {tabs.map((tab) => (
            <div
              key={tab.id}
              className={cn(
                "min-w-0 max-w-[240px] shrink-0 rounded-lg border px-3 py-2",
                tab.active
                  ? "border-slate-300 bg-white text-slate-950 shadow-[0_4px_12px_rgba(15,23,42,0.06)]"
                  : "border-transparent bg-slate-200/70 text-slate-500",
              )}
              title={tab.url || tab.title}
            >
              <div className="truncate text-sm font-medium">{tab.title}</div>
            </div>
          ))}
        </div>
      ) : null}

      <div className="min-h-0 flex-1 overflow-hidden bg-[linear-gradient(180deg,#0f172a_0%,#111827_100%)]">
        {frameBase64 ? (
          <div
            ref={viewportRef}
            tabIndex={0}
            className="relative flex h-full min-h-[360px] items-center justify-center outline-none"
            onFocus={() => setViewportFocused(true)}
            onBlur={() => setViewportFocused(false)}
            onKeyDown={handleViewportKeyDown}
          >
            {!viewportFocused ? (
              <div className="pointer-events-none absolute top-3 left-1/2 -translate-x-1/2 rounded-full bg-slate-950/70 px-3 py-1 text-xs text-slate-100 shadow-[0_10px_24px_rgba(15,23,42,0.28)]">
                点击画面后可直接输入
              </div>
            ) : null}
            {streamError ? (
              <div className="pointer-events-none absolute right-3 top-3 rounded-full bg-amber-100/95 px-3 py-1 text-xs text-amber-800 shadow-sm">
                {streamError}
              </div>
            ) : null}
            {lastFrameAt ? (
              <div className="pointer-events-none absolute bottom-3 right-3 rounded-full bg-slate-950/70 px-3 py-1 text-xs text-slate-100 shadow-[0_10px_24px_rgba(15,23,42,0.28)]">
                画面已更新
              </div>
            ) : null}
            <img
              ref={imageRef}
              src={`data:image/jpeg;base64,${frameBase64}`}
              alt="agent-browser live viewport"
              className="max-h-full max-w-full cursor-pointer object-contain"
              onClick={handleViewportClick}
            />
          </div>
        ) : (
          <div className="flex h-full min-h-[360px] items-center justify-center px-6 text-center text-sm leading-7 text-slate-300">
            <div className="flex flex-col items-center gap-4">
              <div>
                {streamError ||
                  (connectionState === "connecting"
                    ? "正在连接浏览器画面..."
                    : "正在等待 agent-browser 推送第一帧 viewport...")}
              </div>
              {onRetry && connectionState !== "connecting" ? (
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  className="rounded-lg border-slate-500 bg-white/10 text-slate-100 hover:bg-white/20 hover:text-white"
                  onClick={onRetry}
                >
                  手动重连
                </Button>
              ) : null}
            </div>
          </div>
        )}
      </div>
    </div>
  );
}

type ElectronWebviewElement = HTMLElement & {
  canGoBack(): boolean;
  canGoForward(): boolean;
  getWebContentsId?(): number;
  getTitle(): string;
  getURL(): string;
  goBack(): void;
  goForward(): void;
  reload(): void;
  loadURL(url: string): void;
  openDevTools(): void;
  isLoading(): boolean;
};

const DEFAULT_ELECTRON_BROWSER_URL = "https://www.baidu.com";

type ElectronBrowserTabState = {
  id: string;
  initialUrl: string;
  addressValue: string;
  url: string;
  title: string;
  canGoBack: boolean;
  canGoForward: boolean;
  loading: boolean;
  ready: boolean;
  error: string | null;
};

function normalizeBrowserUrl(value: string): string {
  const trimmed = value.trim();
  if (!trimmed) {
    return DEFAULT_ELECTRON_BROWSER_URL;
  }
  if (/^[a-zA-Z][a-zA-Z\d+.-]*:/.test(trimmed)) {
    return trimmed;
  }
  return `https://${trimmed}`;
}

function ElectronBrowserTabView({
  tab,
  active,
  partition,
  onRefChange,
  onStateChange,
  onSyncActiveTab,
}: {
  tab: ElectronBrowserTabState;
  active: boolean;
  partition: string;
  onRefChange: (tabId: string, node: ElectronWebviewElement | null) => void;
  onStateChange: (tabId: string, patch: Partial<ElectronBrowserTabState>) => void;
  onSyncActiveTab: (tabId: string) => void;
}) {
  const webviewRef = useRef<ElectronWebviewElement | null>(null);
  const activeRef = useRef(active);
  const handlersRef = useRef({
    onStateChange,
    onSyncActiveTab,
  });

  useEffect(() => {
    activeRef.current = active;
  }, [active]);

  useEffect(() => {
    handlersRef.current = {
      onStateChange,
      onSyncActiveTab,
    };
  }, [onStateChange, onSyncActiveTab]);

  useEffect(() => {
    const webview = webviewRef.current;
    if (!webview) {
      return;
    }

    let domReady = false;

    const syncState = () => {
      if (!domReady) {
        return;
      }
      try {
        const nextUrl = webview.getURL?.() || tab.initialUrl;
        const nextTitle = webview.getTitle?.() || "Browser";
        handlersRef.current.onStateChange(tab.id, {
          addressValue: nextUrl,
          canGoBack: Boolean(webview.canGoBack?.()),
          canGoForward: Boolean(webview.canGoForward?.()),
          error: null,
          loading: Boolean(webview.isLoading?.()),
          ready: true,
          title: nextTitle || "Browser",
          url: nextUrl,
        });
      } catch {
        // Electron throws until the webview has fully attached.
      }
    };

    const syncSessionState = () => {
      if (!activeRef.current) {
        return;
      }
      handlersRef.current.onSyncActiveTab(tab.id);
    };

    const handleDomReady = () => {
      domReady = true;
      handlersRef.current.onStateChange(tab.id, {
        error: null,
        ready: true,
      });
      syncState();
      syncSessionState();
    };
    const handleStartLoading = () => {
      handlersRef.current.onStateChange(tab.id, {
        error: null,
        loading: true,
      });
      if (domReady) {
        syncState();
      }
    };
    const handleStopLoading = () => {
      handlersRef.current.onStateChange(tab.id, {
        loading: false,
      });
      syncState();
      syncSessionState();
    };
    const handleNavigate = () => {
      syncState();
      syncSessionState();
    };
    const handlePageTitle = () => {
      syncState();
    };
    const handleFailLoad = (event: any) => {
      if (event?.errorCode === -3) {
        return;
      }
      handlersRef.current.onStateChange(tab.id, {
        error: event?.errorDescription || "页面加载失败",
        loading: false,
      });
      syncState();
    };

    webview.addEventListener("dom-ready", handleDomReady);
    webview.addEventListener("did-start-loading", handleStartLoading);
    webview.addEventListener("did-stop-loading", handleStopLoading);
    webview.addEventListener("did-navigate", handleNavigate);
    webview.addEventListener("did-navigate-in-page", handleNavigate);
    webview.addEventListener("page-title-updated", handlePageTitle);
    webview.addEventListener("did-fail-load", handleFailLoad);

    return () => {
      domReady = false;
      webview.removeEventListener("dom-ready", handleDomReady);
      webview.removeEventListener("did-start-loading", handleStartLoading);
      webview.removeEventListener("did-stop-loading", handleStopLoading);
      webview.removeEventListener("did-navigate", handleNavigate);
      webview.removeEventListener("did-navigate-in-page", handleNavigate);
      webview.removeEventListener("page-title-updated", handlePageTitle);
      webview.removeEventListener("did-fail-load", handleFailLoad);
    };
  }, [tab.id, tab.initialUrl]);

  useEffect(() => {
    if (!active || !tab.ready) {
      return;
    }
    onSyncActiveTab(tab.id);
  }, [active, onSyncActiveTab, tab.id, tab.ready]);

  return (
    <webview
      key={`${partition}:${tab.id}`}
      ref={(node: ElectronWebviewElement | null) => {
        webviewRef.current = node;
        onRefChange(tab.id, node);
        node?.setAttribute("allowpopups", "true");
      }}
      src={tab.initialUrl}
      partition={partition}
      className={cn("h-full min-h-0 w-full min-w-0 bg-white", active ? "flex" : "hidden")}
    />
  );
}

function ElectronBrowserPane({
  dataDir,
  sessionId,
  syncRequest,
  directUrlRequest,
}: {
  dataDir: string | null;
  sessionId: string | null;
  syncRequest: BrowserUiSyncRequest;
  directUrlRequest?: BrowserDirectUrlRequest | null;
}) {
  const nextTabIdRef = useRef(0);
  const syncBrowserStateInFlightRef = useRef(false);
  const activeTabIdRef = useRef<string | null>(null);
  const lastAppliedSyncTokenRef = useRef(0);
  const lastAppliedDirectUrlTokenRef = useRef(0);
  const webviewRefs = useRef<Map<string, ElectronWebviewElement>>(new Map());
  const webviewDomReadyRef = useRef<Set<string>>(new Set());
  const [tabs, setTabs] = useState<ElectronBrowserTabState[]>([]);
  const [activeTabId, setActiveTabId] = useState<string | null>(null);

  const partition = sessionId
    ? `persist:hermes-browser-${sessionId}`
    : "persist:hermes-browser-default";
  const activeTab = tabs.find((tab) => tab.id === activeTabId) || tabs[0] || null;

  function createTab(url: string): ElectronBrowserTabState {
    const normalizedUrl = normalizeBrowserUrl(url);
    const tabId = `browser-tab-${nextTabIdRef.current++}`;
    return {
      addressValue: normalizedUrl,
      canGoBack: false,
      canGoForward: false,
      error: null,
      id: tabId,
      initialUrl: normalizedUrl,
      loading: false,
      ready: false,
      title: "新标签页",
      url: normalizedUrl,
    };
  }

  function updateTab(tabId: string, patch: Partial<ElectronBrowserTabState>) {
    setTabs((prev) =>
      prev.map((tab) => {
        if (tab.id !== tabId) {
          return tab;
        }
        return {
          ...tab,
          ...patch,
        };
      }),
    );
  }

  function activeWebview() {
    if (!activeTabId) {
      return null;
    }
    return webviewRefs.current.get(activeTabId) || null;
  }

  useEffect(() => {
    activeTabIdRef.current = activeTabId;
  }, [activeTabId]);

  useEffect(() => {
    if (!dataDir || !sessionId) {
      webviewRefs.current.clear();
      webviewDomReadyRef.current.clear();
      setTabs([]);
      setActiveTabId(null);
      return;
    }

    const requestedUrlToken = directUrlRequest?.token || 0;
    const requestedUrl =
      requestedUrlToken > 0 && directUrlRequest?.url
        ? normalizeBrowserUrl(directUrlRequest.url)
        : null;
    if (requestedUrl) {
      webviewRefs.current.clear();
      webviewDomReadyRef.current.clear();
      const initialTab = createTab(requestedUrl);
      if (directUrlRequest?.title) {
        initialTab.title = directUrlRequest.title;
      }
      lastAppliedDirectUrlTokenRef.current = requestedUrlToken;
      setTabs([initialTab]);
      setActiveTabId(initialTab.id);
      return;
    }

    let cancelled = false;
    void invoke<BrowserCurrentUrlResponse>("browser_current_url", {
      dataDir,
      sessionId,
    })
      .then((result) => {
        if (cancelled) {
          return;
        }
        webviewRefs.current.clear();
        webviewDomReadyRef.current.clear();
        const initialTab = createTab(result?.url || DEFAULT_ELECTRON_BROWSER_URL);
        setTabs([initialTab]);
        setActiveTabId(initialTab.id);
      })
      .catch((nextError) => {
        if (cancelled) {
          return;
        }
        webviewRefs.current.clear();
        webviewDomReadyRef.current.clear();
        const fallbackTab = createTab(DEFAULT_ELECTRON_BROWSER_URL);
        fallbackTab.error = formatError(nextError);
        setTabs([fallbackTab]);
        setActiveTabId(fallbackTab.id);
      });

    return () => {
      cancelled = true;
    };
  }, [dataDir, sessionId]);

  useEffect(() => {
    if (!dataDir || !sessionId || !directUrlRequest?.url || directUrlRequest.token <= 0) {
      return;
    }
    if (lastAppliedDirectUrlTokenRef.current === directUrlRequest.token) {
      return;
    }
    lastAppliedDirectUrlTokenRef.current = directUrlRequest.token;

    const nextUrl = normalizeBrowserUrl(directUrlRequest.url);
    const existingTab = tabs.find((tab) => tab.url === nextUrl || tab.initialUrl === nextUrl);
    if (existingTab) {
      setActiveTabId(existingTab.id);
      updateTab(existingTab.id, {
        addressValue: nextUrl,
        error: null,
        loading: false,
        ...(directUrlRequest.title ? { title: directUrlRequest.title } : {}),
        url: nextUrl,
      });
      const webview = webviewRefs.current.get(existingTab.id);
      try {
        if (webview && normalizeBrowserUrl(webview.getURL?.() || "") !== nextUrl) {
          webview.loadURL(nextUrl);
        }
      } catch {
        // Best-effort direct navigation for local preview tabs.
      }
      return;
    }

    const nextTab = createTab(nextUrl);
    if (directUrlRequest.title) {
      nextTab.title = directUrlRequest.title;
    }
    setTabs((prev) => [...prev, nextTab]);
    setActiveTabId(nextTab.id);
  }, [dataDir, directUrlRequest, sessionId, tabs]);

  async function syncActiveTab(
    tabId: string,
    options?: {
      alignVisibleTab?: boolean;
    },
  ) {
    if (!dataDir || !sessionId) {
      return;
    }
    if (!webviewDomReadyRef.current.has(tabId)) {
      return;
    }
    const webview = webviewRefs.current.get(tabId);
    let guestId: number | undefined;
    try {
      guestId = webview?.getWebContentsId?.();
    } catch {
      return;
    }
    if (!Number.isInteger(guestId)) {
      return;
    }
    try {
      await invoke<{ ok: boolean }>("set_active_browser_guest", {
        guestId,
        sessionId,
      });
    } catch {
      // Best-effort sync for Electron browser/session linkage.
      return;
    }
    if (syncBrowserStateInFlightRef.current || activeTabIdRef.current !== tabId) {
      return;
    }
    syncBrowserStateInFlightRef.current = true;
    try {
      const result = await invoke<BrowserStateSyncResponse>("sync_browser_state", {
        dataDir,
        sessionId,
      });
      const nextUrl = result?.url ? normalizeBrowserUrl(result.url) : null;
      const nextTitle =
        typeof result?.title === "string" && result.title.trim() ? result.title.trim() : null;
      if (!nextUrl) {
        return;
      }
      let currentVisibleUrl: string | null = null;
      let currentVisibleTitle: string | null = null;
      try {
        const liveUrl = webview?.getURL?.();
        if (typeof liveUrl === "string" && liveUrl.trim() && liveUrl.trim() !== "about:blank") {
          currentVisibleUrl = normalizeBrowserUrl(liveUrl);
        }
        const liveTitle = webview?.getTitle?.();
        if (typeof liveTitle === "string" && liveTitle.trim()) {
          currentVisibleTitle = liveTitle.trim();
        }
      } catch {
        currentVisibleUrl = null;
        currentVisibleTitle = null;
      }
      const currentTabState = tabs.find((tab) => tab.id === tabId) || null;
      const shouldForceNavigate =
        Boolean(options?.alignVisibleTab) &&
        syncRequest.forceNavigate &&
        webview != null &&
        (
          currentVisibleUrl !== nextUrl ||
          (nextTitle != null && currentVisibleTitle !== nextTitle) ||
          (currentTabState != null && nextTitle != null && currentTabState.title !== nextTitle)
        );
      if (shouldForceNavigate) {
        updateTab(tabId, {
          addressValue: nextUrl,
          error: null,
          loading: true,
          ...(nextTitle ? { title: nextTitle } : {}),
          url: nextUrl,
        });
        webview.loadURL(nextUrl);
        return;
      }
      setTabs((prev) =>
        prev.map((tab) => {
          if (tab.id !== tabId) {
            return tab;
          }
          return {
            ...tab,
            addressValue:
              !tab.addressValue.trim() || tab.addressValue.trim() === tab.url.trim()
                ? nextUrl
                : tab.addressValue,
            ...(nextTitle ? { title: nextTitle } : {}),
            url: nextUrl,
          };
        }),
      );
    } catch {
      // Best-effort sync for Electron browser/session linkage.
    } finally {
      syncBrowserStateInFlightRef.current = false;
    }
  }

  useEffect(() => {
    if (!activeTabId || !activeTab?.ready) {
      return;
    }
    void syncActiveTab(activeTabId);
  }, [activeTab?.ready, activeTabId, dataDir, sessionId]);

  useEffect(() => {
    if (!activeTabId || !activeTab?.ready || syncRequest.token <= 0) {
      return;
    }
    if (lastAppliedSyncTokenRef.current === syncRequest.token) {
      return;
    }
    lastAppliedSyncTokenRef.current = syncRequest.token;
    void syncActiveTab(activeTabId, { alignVisibleTab: true });
  }, [activeTab?.ready, activeTabId, dataDir, sessionId, syncRequest, tabs]);

  function navigateToAddress(event?: ReactFormEvent<HTMLFormElement>) {
    event?.preventDefault();
    const currentTab = activeTab;
    const webview = activeWebview();
    if (!currentTab || !webview) {
      return;
    }
    const nextUrl = normalizeBrowserUrl(currentTab.addressValue);
    updateTab(currentTab.id, {
      addressValue: nextUrl,
      error: null,
      loading: true,
      url: nextUrl,
    });
    webview.loadURL(nextUrl);
  }

  function createNewTab() {
    const nextTab = createTab(DEFAULT_ELECTRON_BROWSER_URL);
    setTabs((prev) => [...prev, nextTab]);
    setActiveTabId(nextTab.id);
  }

  function closeTab(tabId: string) {
    webviewRefs.current.delete(tabId);
    webviewDomReadyRef.current.delete(tabId);
    if (tabs.length <= 1) {
      const fallbackTab = createTab(DEFAULT_ELECTRON_BROWSER_URL);
      setTabs([fallbackTab]);
      setActiveTabId(fallbackTab.id);
      return;
    }

    const tabIndex = tabs.findIndex((tab) => tab.id === tabId);
    const remainingTabs = tabs.filter((tab) => tab.id !== tabId);
    setTabs(remainingTabs);
    if (activeTabId !== tabId) {
      return;
    }
    const nextActiveTab = remainingTabs[Math.max(0, tabIndex - 1)] || remainingTabs[0] || null;
    setActiveTabId(nextActiveTab?.id || null);
  }

  function openDevtools() {
    activeWebview()?.openDevTools();
  }

  if (!dataDir || !sessionId) {
    return (
      <div className="flex h-full min-h-[320px] items-center justify-center rounded-xl border border-dashed border-slate-200 bg-white/80 px-6 text-center text-sm leading-7 text-slate-500">
        当前没有可用的浏览器 session。
      </div>
    );
  }

  return (
    <div className="flex h-full min-h-0 flex-col overflow-hidden rounded-xl border border-slate-200 bg-white/92">
      <div className="flex items-center gap-2 border-b border-slate-200 bg-slate-100/90 px-2 py-2">
        <div className="flex min-w-0 flex-1 items-center gap-2 overflow-x-auto pb-1">
          {tabs.map((tab) => {
            const isActive = tab.id === activeTabId;
            return (
              <div
                key={tab.id}
                className={cn(
                  "flex min-w-[180px] max-w-[260px] items-center gap-1 rounded-full border transition",
                  isActive
                    ? "border-slate-300 bg-white text-slate-950 shadow-sm"
                    : "border-transparent bg-slate-200/70 text-slate-500 hover:border-slate-200 hover:bg-white/80",
                )}
              >
                <button
                  type="button"
                  className="flex min-w-0 flex-1 items-center gap-2 px-3 py-1.5 text-left text-sm"
                  onClick={() => setActiveTabId(tab.id)}
                >
                  {tab.loading ? (
                    <LoaderCircle className="size-3.5 shrink-0 animate-spin" />
                  ) : (
                    <Globe className="size-3.5 shrink-0" />
                  )}
                  <span className="truncate">{tab.title || tab.url}</span>
                </button>
                <Button
                  type="button"
                  variant="ghost"
                  size="icon"
                  className="mr-1 h-7 w-7 rounded-full"
                  onClick={() => closeTab(tab.id)}
                  title="关闭标签页"
                >
                  <X className="size-3.5" />
                </Button>
              </div>
            );
          })}
        </div>
        <Button
          type="button"
          variant="ghost"
          size="icon"
          className="h-8 w-8 rounded-full"
          onClick={createNewTab}
          title="新建标签页"
        >
          <Plus className="size-4" />
        </Button>
      </div>

      <div className="flex items-center gap-2 border-b border-slate-200 bg-slate-100/80 px-3 py-2">
        <Button
          type="button"
          variant="ghost"
          size="icon"
          className="h-8 w-8 rounded-full"
          disabled={!activeTab?.canGoBack}
          onClick={() => activeWebview()?.goBack()}
          title="后退"
        >
          <ChevronLeft className="size-4" />
        </Button>
        <Button
          type="button"
          variant="ghost"
          size="icon"
          className="h-8 w-8 rounded-full"
          disabled={!activeTab?.canGoForward}
          onClick={() => activeWebview()?.goForward()}
          title="前进"
        >
          <ChevronRight className="size-4" />
        </Button>
        <Button
          type="button"
          variant="ghost"
          size="icon"
          className="h-8 w-8 rounded-full"
          disabled={!activeTab}
          onClick={() => activeWebview()?.reload()}
          title="刷新"
        >
          <RefreshCw className={cn("size-4", activeTab?.loading ? "animate-spin" : "")} />
        </Button>
        <form className="min-w-0 flex-1" onSubmit={navigateToAddress}>
          <Input
            value={activeTab?.addressValue || DEFAULT_ELECTRON_BROWSER_URL}
            onChange={(event) => {
              if (!activeTabId) {
                return;
              }
              updateTab(activeTabId, {
                addressValue: event.target.value,
              });
            }}
            className="h-9 rounded-full border-slate-300 bg-white"
            placeholder="输入网址并回车"
          />
        </form>
        <Button
          type="button"
          variant="ghost"
          size="icon"
          className="h-8 w-8 rounded-full"
          onClick={openDevtools}
          title="打开 DevTools"
        >
          <Wrench className="size-4" />
        </Button>
      </div>

      <div className="flex items-center gap-2 border-b border-slate-200/80 px-3 py-2 text-xs text-slate-500">
        <Globe className="size-3.5" />
        <span className="truncate">{activeTab?.title || activeTab?.url || "Browser"}</span>
        <span className="truncate text-slate-400">{activeTab?.url || DEFAULT_ELECTRON_BROWSER_URL}</span>
      </div>

      <div className="relative min-h-0 flex-1 overflow-hidden bg-slate-950">
        {tabs.map((tab) => (
          <ElectronBrowserTabView
            key={`${partition}:${tab.id}`}
            tab={tab}
            active={tab.id === activeTabId}
            partition={partition}
            onRefChange={(tabId, node) => {
              if (!node) {
                webviewRefs.current.delete(tabId);
                webviewDomReadyRef.current.delete(tabId);
                return;
              }
              webviewRefs.current.set(tabId, node);
            }}
            onStateChange={updateTab}
            onSyncActiveTab={(tabId) => {
              webviewDomReadyRef.current.add(tabId);
              void syncActiveTab(tabId);
            }}
          />
        ))}

        {!activeTab?.ready || activeTab?.loading ? (
          <div className="pointer-events-none absolute inset-0 flex items-center justify-center bg-slate-950/12 text-sm text-slate-700 backdrop-blur-[1px]">
            {activeTab?.loading ? "正在加载页面..." : "正在初始化 Chromium..."}
          </div>
        ) : null}

        {activeTab?.error ? (
          <div className="absolute inset-x-3 bottom-3 rounded-xl border border-rose-200 bg-rose-50/95 px-3 py-2 text-xs leading-6 text-rose-700 shadow-sm">
            {activeTab.error}
          </div>
        ) : null}
      </div>
    </div>
  );
}

function MarkdownContent({
  content,
  className,
  workspaceRoot,
  onOpenWorkspaceFile,
}: {
  content: string;
  className?: string;
  workspaceRoot?: string;
  onOpenWorkspaceFile?: (path: string) => void;
}) {
  const markdown = linkifyWorkspacePathsInMarkdown(content, workspaceRoot);

  return (
    <div className={cn("min-w-0 max-w-full space-y-2 overflow-hidden break-words", className)}>
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        urlTransform={markdownUrlTransform}
        components={{
          h1: ({ ...props }) => (
            <h1 className="mt-3 first:mt-0 text-[16px] font-semibold leading-6 text-slate-800" {...props} />
          ),
          h2: ({ ...props }) => (
            <h2 className="mt-2.5 first:mt-0 text-[14px] font-semibold leading-5 text-slate-800" {...props} />
          ),
          h3: ({ ...props }) => (
            <h3 className="mt-2 first:mt-0 text-[13px] font-semibold leading-5 text-slate-700" {...props} />
          ),
          h4: ({ ...props }) => (
            <h4 className="mt-2 first:mt-0 text-[12px] font-semibold leading-5 text-slate-600" {...props} />
          ),
          p: ({ ...props }) => <p className="whitespace-pre-wrap break-words text-[13px] leading-[1.5] text-slate-700" {...props} />,
          ul: ({ ...props }) => <ul className="list-disc space-y-0.5 pl-4 text-[13px] leading-[1.5] text-slate-700 marker:text-slate-400" {...props} />,
          ol: ({ ...props }) => <ol className="list-decimal space-y-0.5 pl-4 text-[13px] leading-[1.5] text-slate-700 marker:text-slate-400" {...props} />,
          li: ({ ...props }) => <li className="break-words pl-0.5" {...props} />,
          blockquote: ({ ...props }) => (
            <blockquote
              className="rounded-r-md border-l-2 border-slate-300 bg-slate-100/70 px-2.5 py-1.5 text-[13px] leading-[1.5] text-slate-600"
              {...props}
            />
          ),
          a: ({ href, children, ...props }) => {
            const filePath = href ? resolveWorkspaceHrefForOpen(href, workspaceRoot) : null;
            if (filePath) {
              return (
                <a
                  href={href || "#"}
                  data-no-drag
                  className="cursor-pointer text-slate-700 underline decoration-slate-300 underline-offset-2 hover:text-slate-900"
                  onClick={(event) => {
                    event.preventDefault();
                    event.stopPropagation();
                    onOpenWorkspaceFile?.(filePath);
                  }}
                  {...props}
                >
                  {children}
                </a>
              );
            }
            return (
              <a
                className="text-sky-700 underline decoration-sky-300 underline-offset-2 hover:text-sky-800"
                target="_blank"
                rel="noreferrer"
                href={href}
                {...props}
              >
                {children}
              </a>
            );
          },
          table: ({ ...props }) => (
            <div className="min-w-0 max-w-full overflow-x-auto">
              <table className="w-full border-collapse text-left text-[12px] leading-5 text-slate-700" {...props} />
            </div>
          ),
          thead: ({ ...props }) => <thead className="border-b border-slate-200 bg-slate-50 text-slate-600" {...props} />,
          th: ({ ...props }) => <th className="px-2.5 py-1.5 font-medium" {...props} />,
          td: ({ ...props }) => <td className="border-t border-slate-100 px-2.5 py-1.5 align-top" {...props} />,
          hr: ({ ...props }) => <hr className="border-t border-slate-200" {...props} />,
          pre: ({ ...props }) => (
            <pre
              className="min-w-0 max-w-full overflow-x-auto rounded-md border border-slate-200 bg-slate-50 px-3 py-2.5 text-[12px] leading-5 text-slate-700"
              {...props}
            />
          ),
          code: ({ className, children, ...props }) => {
            const value = String(children);
            const isBlock = Boolean(className) || value.includes("\n");
            if (isBlock) {
              return (
                <code className={cn("block min-w-max font-mono", className)} {...props}>
                  {children}
                </code>
              );
            }
            return (
              <code
                className="rounded border border-slate-200 bg-slate-50 px-1.5 py-0.5 font-mono text-[0.9em] text-slate-700"
                {...props}
              >
                {children}
              </code>
            );
          },
        }}
      >
        {markdown}
      </ReactMarkdown>
    </div>
  );
}

function readTurnStartedPreview(event: Record<string, unknown>): string {
  const preview = event.user_input_preview ?? event.user_input ?? "";
  return typeof preview === "string" ? preview : String(preview || "");
}

function summarizeEvent(event: Record<string, unknown> & { type?: string }): string {
  switch (event.type) {
    case "turn_started":
      {
        const turnId = typeof event.turn_id === "string" ? event.turn_id : "";
        const prefix = event.resumed ? "继续执行" : "开始";
        const preview = truncate(readTurnStartedPreview(event), 80);
        return `${prefix}${turnId ? ` ${turnId}` : ""}${preview ? ` · ${preview}` : ""}`;
      }
    case "turn_finished":
      return `本轮 ${formatTurnStatus(String(event.status || ""))}，${String(event.tool_call_count || 0)} 个工具${formatEventDuration(event.duration_ms)}`;
    case "turn_interrupted":
      {
        const phase = formatTurnInterruptedPhase(String(event.phase || ""));
        const message = truncate(String(event.message || event.reason || ""), 80);
        return `本轮已中断${phase ? ` · ${phase}` : ""}${message ? ` · ${message}` : ""}`;
      }
    case "assistant_delta":
      return truncate(String(event.delta || ""), 80);
    case "goal_state_updated":
      {
        const source = formatGoalStateSource(String(event.source || ""));
        const focus = String(event.focus_goal_title || event.focus_goal_id || "");
        const status = String(event.focus_goal_status || "");
        return `目标状态${source ? ` · ${source}` : ""}${focus ? ` · ${truncate(focus, 60)}` : ""}${status ? ` · ${status}` : ""}`;
      }
    case "todo_state_updated":
      {
        const source = formatTodoStateSource(String(event.source || ""));
        return `任务列表${source ? ` · ${source}` : ""} · active ${String(event.active_count || 0)}/${String(event.total || 0)}`;
      }
    case "solve_trace_updated":
      {
        const source = formatSolveTraceSource(String(event.source || ""));
        const kind = String(event.entry_kind || "");
        const status = String(event.status || "");
        const action = String(event.action_preview || event.observation_preview || "");
        return `求解轨迹${source ? ` · ${source}` : ""}${kind ? ` · ${kind}` : ""}${status ? ` · ${status}` : ""}${action ? ` · ${truncate(action, 54)}` : ""}`;
      }
    case "model_request_started":
      return `请求模型 ${String(event.model || "")}${formatModelRequestMetadata(event)}，${String(event.message_count || 0)} 条消息`;
    case "model_request_finished":
      if (String(event.status || "ok") !== "ok") {
        return `模型请求${formatModelRequestStatus(String(event.status || ""))}${formatEventDuration(event.duration_ms)}${formatTokenUsageSummary(event)} ${truncate(String(event.content_preview || ""), 60)}`;
      }
      return Number(event.tool_call_count || 0) > 0
        ? `模型返回 ${String(event.tool_call_count || 0)} 个工具调用${formatEventDuration(event.duration_ms)}${formatTokenUsageSummary(event)}`
        : `模型返回回复 ${truncate(String(event.content_preview || ""), 60)}${formatEventDuration(event.duration_ms)}${formatTokenUsageSummary(event)}`;
    case "background_model_request_started":
      return `后台 ${String(event.purpose || "")} 请求 ${String(event.model || "")}${formatModelRequestMetadata(event)}`;
    case "background_model_request_finished":
      return `后台 ${String(event.purpose || "")} ${String(event.status || "")}${formatEventDuration(event.duration_ms)}${formatTokenUsageSummary(event)} ${truncate(String(event.content_preview || ""), 60)}`;
    case "context_prepared":
      return `上下文 ${String(event.projected_tokens || 0)}/${String(event.request_budget_tokens || 0)} tokens，块 ${String(event.kept_blocks || 0)}/${String(event.total_blocks || 0)}${formatContextTrimSummary(event)}${formatEventDuration(event.duration_ms)}`;
    case "context_sources_updated":
      {
        const labels = formatContextSourceLabels(event);
        return `上下文来源 ${String(event.kept_blocks || 0)}/${String(event.total_blocks || 0)}${labels ? ` · ${labels}` : ""}`;
      }
    case "context_compacted":
      return `上下文压缩 ${String(event.original_message_count || 0)} -> ${String(event.compressed_message_count || 0)} 条消息，tokens ${String(event.original_estimated_tokens || 0)} -> ${String(event.compressed_estimated_tokens || 0)}${Number(event.pruned_tool_messages || 0) > 0 ? `，裁剪 ${String(event.pruned_tool_messages)} 条工具输出` : ""}`;
    case "model_recovery":
      {
        const delay = Number(event.delay_ms || 0);
        const budget = Number(event.output_budget_tokens || 0);
        const limit = Number(event.context_limit_tokens || 0);
        const detail = budget
          ? `，输出上限 ${budget}`
          : limit
            ? `，上下文上限 ${limit}`
            : delay
              ? `，等待 ${delay}ms`
              : "";
        return `模型恢复 ${String(event.kind || "")}/${String(event.action || "")} 第 ${String(event.attempt || 0)} 次${detail}`;
      }
    case "delegate_run_updated":
      {
        const status = formatDelegateRunStatus(String(event.status || ""));
        const objective = truncate(String(event.objective_preview || event.result_preview || ""), 60);
        return `worker ${status}${objective ? ` · ${objective}` : ""}`;
      }
    case "tool_batch_started":
      return `并发批次 ${String(event.batch_id || "")} 启动，${String(event.total_calls || 0)} 个工具`;
    case "tool_batch_progress":
      return `并发批次 ${String(event.completed_calls || 0)}/${String(event.total_calls || 0)}`;
    case "tool_batch_finished":
      return `并发批次 ${String(event.status || "")}，${String(event.completed_calls || 0)}/${String(event.total_calls || 0)}${formatEventDuration(event.duration_ms)}`;
    case "tool_call_started":
      return `${String(event.tool_name || "")} ${truncate(String(event.arguments_preview || ""), 60)}`;
    case "tool_call_delta":
      return `${String(event.tool_name || "")} ${truncate(String(event.detail_preview || ""), 60)}`;
    case "tool_call_finished":
      return `${String(event.tool_name || "")} ${truncate(String(event.output_preview || ""), 60)}${formatEventDuration(event.duration_ms)}`;
    case "assistant_message":
      return truncate(String(event.content || ""), 80);
    case "approval_required":
      return `${String(event.tool_name || "")} ${truncate(String(event.reason || ""), 60)}`;
    case "approval_resolved":
      return `${String(event.tool_name || "tool")} 审批${event.approved ? "已批准" : "已拒绝"}`;
    case "session_saved":
      return `检查点已保存 · ${String(event.turn_id || "")} · history ${String(event.history_count || 0)} · timeline ${String(event.timeline_count || 0)}${Number(event.pending_approval_count || 0) > 0 ? ` · pending approvals ${String(event.pending_approval_count)}` : ""}`;
    case "error":
      return String(event.message || "");
    default:
      return String(event.type || "event");
  }
}

function documentTitle(timeline: TimelineEntry[], currentSessionId: string | null): string {
  const firstUser = timeline.find((entry) => entry.type === "user");
  if (firstUser) {
    return truncate(firstUser.content.replace(/\s+/g, " ").trim(), 48);
  }
  return currentSessionId || "新会话";
}

function titleFromSessionDetail(detail: BridgeSessionDetail | null): string | null {
  if (!detail) {
    return null;
  }
  if (detail.summary.title?.trim()) {
    return detail.summary.title.trim();
  }
  const timeline = detail.timeline?.length ? storedTimelineToTimeline(detail.timeline) : historyToTimeline(detail.history);
  const firstUser = timeline.find((entry) => entry.type === "user");
  const title = firstUser?.content.replace(/\s+/g, " ").trim();
  return title ? truncate(title, 48) : null;
}

function formatAuthSource(source: string | null | undefined): string {
  switch (source) {
    case "request":
      return "当前设置里的 API Key";
    case "config":
      return "provider 配置";
    case "OPENAI_API_KEY":
    case "CODEX_API_KEY":
    case "OPENAI_CODEX_API_KEY":
      return source;
    case "HERMES_HOME/auth.json":
      return "~/.hermes/auth.json";
    case "CODEX_HOME/auth.json":
      return "~/.codex/auth.json";
    default:
      return source || "未检测到";
  }
}

function skillKey(skill: { category: string; name: string }): string {
  return `${skill.category}/${skill.name}`;
}

function formatSkillTimestamp(value: number | null): string {
  if (!value) {
    return "未记录";
  }
  return new Date(value * 1000).toLocaleString("zh-CN", {
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  });
}

function formatFileSize(size: number): string {
  if (size < 1024) {
    return `${size} B`;
  }
  if (size < 1024 * 1024) {
    return `${(size / 1024).toFixed(1)} KB`;
  }
  return `${(size / (1024 * 1024)).toFixed(1)} MB`;
}

function readDurationMs(value: unknown): number | null {
  const durationMs = Number(value);
  if (!Number.isFinite(durationMs) || durationMs < 0) {
    return null;
  }
  return durationMs;
}

function formatDurationMs(durationMs: number | null | undefined): string | null {
  if (durationMs == null) {
    return null;
  }
  if (durationMs < 1000) {
    return `${Math.round(durationMs)}ms`;
  }
  if (durationMs < 10_000) {
    return `${(durationMs / 1000).toFixed(1)}s`;
  }
  return `${Math.round(durationMs / 1000)}s`;
}

function formatEventDuration(value: unknown): string {
  const duration = formatDurationMs(readDurationMs(value));
  return duration ? ` · ${duration}` : "";
}

function readPositiveNumber(value: unknown): number | null {
  const numeric = Number(value);
  if (!Number.isFinite(numeric) || numeric <= 0) {
    return null;
  }
  return numeric;
}

function formatTokenUsageSummary(event: Record<string, unknown>): string {
  const promptTokens = readPositiveNumber(event.prompt_tokens);
  const completionTokens = readPositiveNumber(event.completion_tokens);
  const totalTokens = readPositiveNumber(event.total_tokens);
  if (totalTokens != null) {
    return ` · ${totalTokens} tokens`;
  }
  if (promptTokens != null && completionTokens != null) {
    return ` · ${promptTokens}/${completionTokens} tokens`;
  }
  if (promptTokens != null) {
    return ` · 输入 ${promptTokens} tokens`;
  }
  if (completionTokens != null) {
    return ` · 输出 ${completionTokens} tokens`;
  }
  return "";
}

function formatCompactTimestamp(value: number): string {
  return new Date(value * 1000).toLocaleString("zh-CN", {
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  });
}

function formatParallelBatchStatus(status: ParallelBatchState["status"]): string {
  switch (status) {
    case "running":
      return "并发执行中";
    case "awaiting_approval":
      return "并发批次等待审批";
    case "canceled":
      return "并发批次已取消";
    case "completed_with_errors":
      return "并发批次完成，有失败工具";
    case "completed":
      return "并发批次完成";
    default:
      return status;
  }
}

function formatTurnStatus(status: string): string {
  switch (status) {
    case "completed":
      return "已完成";
    case "awaiting_approval":
      return "等待审批";
    case "canceled":
      return "已中断";
    case "error":
      return "失败";
    default:
      return status || "已结束";
  }
}

function formatGoalStateSource(source: string): string {
  switch (source) {
    case "user_input":
      return "用户目标";
    case "tool_outcome":
      return "工具证据";
    case "tool_reconcile":
      return "工具复盘";
    case "turn_reconcile":
      return "回合复盘";
    default:
      return source;
  }
}

function formatTodoStateSource(source: string): string {
  switch (source) {
    case "goal_state_sync":
      return "目标同步";
    case "delegate_worker":
      return "worker 步骤";
    case "todo_tool":
      return "todo 工具";
    default:
      return source;
  }
}

function formatSolveTraceSource(source: string): string {
  switch (source) {
    case "episode_start":
      return "开始";
    case "tool_step":
      return "工具步骤";
    case "delegate_worker":
      return "worker 推进";
    case "turn_outcome":
      return "回合结果";
    default:
      return source;
  }
}

function formatDelegateRunStatus(status: string): string {
  switch (status) {
    case "running":
      return "运行中";
    case "completed":
      return "已完成";
    case "awaiting_approval":
      return "等待审批";
    case "failed":
      return "失败";
    case "canceled":
      return "已取消";
    case "cancel_requested":
      return "取消中";
    default:
      return status || "更新";
  }
}

function formatTurnInterruptedPhase(phase: string): string {
  switch (phase) {
    case "iteration_preflight":
      return "迭代准备";
    case "parallel_batch":
      return "并发工具批次";
    case "sequential_tool":
      return "工具执行";
    case "agent_loop":
      return "主循环";
    default:
      return phase;
  }
}

function formatModelRequestStatus(status: string): string {
  switch (status) {
    case "ok":
      return "完成";
    case "error":
      return "失败";
    case "timeout":
      return "超时";
    default:
      return status ? ` ${status}` : "结束";
  }
}

function formatApiMode(value: unknown): string {
  switch (String(value || "")) {
    case "responses":
      return "Responses";
    case "chat_completions":
      return "Chat";
    default:
      return String(value || "");
  }
}

function formatModelRequestMetadata(event: Record<string, unknown>): string {
  const parts: string[] = [];
  const apiMode = formatApiMode(event.api_mode);
  if (apiMode) {
    parts.push(apiMode);
  }
  if (event.uses_response_continuation) {
    parts.push("续接");
  }
  const outputBudget = readPositiveNumber(event.output_budget_tokens);
  if (outputBudget != null) {
    parts.push(`输出 ${outputBudget}`);
  }
  return parts.length ? ` · ${parts.join(" · ")}` : "";
}

function formatContextTrimSummary(event: Record<string, unknown>): string {
  const clipped = Array.isArray(event.clipped_labels) ? event.clipped_labels.length : 0;
  const skipped = Array.isArray(event.skipped_labels) ? event.skipped_labels.length : 0;
  if (!clipped && !skipped) {
    return "";
  }
  const parts: string[] = [];
  if (clipped) {
    parts.push(`裁剪 ${clipped}`);
  }
  if (skipped) {
    parts.push(`跳过 ${skipped}`);
  }
  return `，${parts.join("，")}`;
}

function formatContextSourceLabels(event: Record<string, unknown>): string {
  if (!Array.isArray(event.sources)) {
    return "";
  }
  return event.sources
    .slice(0, 4)
    .map((source) => {
      if (!source || typeof source !== "object") {
        return "";
      }
      const record = source as Record<string, unknown>;
      const label = String(record.label || "");
      const status = formatContextSourceStatus(String(record.status || ""));
      return label ? `${label}${status ? `/${status}` : ""}` : "";
    })
    .filter(Boolean)
    .join(", ");
}

function formatContextSourceStatus(status: string): string {
  switch (status) {
    case "kept":
      return "保留";
    case "clipped":
      return "裁剪";
    case "skipped":
      return "跳过";
    default:
      return status;
  }
}

function formatInterruptedSessionSummary(
  runningToolCount: number,
  runningBatchCount: number,
  pendingApprovalCount: number,
): string {
  const parts: string[] = [];
  if (runningToolCount > 0) {
    parts.push(`${runningToolCount} 个工具停留在执行中`);
  }
  if (runningBatchCount > 0) {
    parts.push(`${runningBatchCount} 个并发批次未收尾`);
  }
  if (pendingApprovalCount > 0) {
    parts.push(`${pendingApprovalCount} 条审批仍待处理`);
  }
  return parts.join("，");
}

function isBrowserTool(toolName: string | null | undefined): boolean {
  return typeof toolName === "string" && toolName.startsWith("browser_");
}

function isNavigationBrowserTool(toolName: string | null | undefined): boolean {
  return toolName === "browser_navigate" || toolName === "browser_back" || toolName === "browser_forward";
}

type SessionLoadOptions = {
  silent?: boolean;
  configOverride?: Preferences;
  approvalsOverride?: ApprovalRequest[];
};

export default function Page() {
  const [config, setConfig] = useState<Preferences>(defaultConfig);
  const [desktopInfo, setDesktopInfo] = useState<DesktopInfo | null>(null);
  const [currentSessionId, setCurrentSessionId] = useState<string | null>(null);
  const [activeAssistantMessageId, setActiveAssistantMessageId] = useState<string | null>(null);
  const [timeline, setTimeline] = useState<TimelineEntry[]>([]);
  const [eventLog, setEventLog] = useState<EventLogEntry[]>([]);
  const [tools, setTools] = useState<ToolEntry[]>([]);
  const [notices, setNotices] = useState<NoticeEntry[]>([]);
  const [parallelBatch, setParallelBatch] = useState<ParallelBatchState | null>(null);
  const [agentActivity, setAgentActivity] = useState("正在准备");
  const [skills, setSkills] = useState<BridgeSkillSummary[]>([]);
  const [providers, setProviders] = useState<ProviderSummary[]>([]);
  const [providerRuntimeStatus, setProviderRuntimeStatus] = useState<ProviderRuntimeStatus | null>(null);
  const [providerRuntimeLoading, setProviderRuntimeLoading] = useState(false);
  const [providerRuntimeError, setProviderRuntimeError] = useState<string | null>(null);
  const [conversationTitleLock, setConversationTitleLock] = useState<{ key: string; title: string } | null>(null);
  const [skillDetail, setSkillDetail] = useState<BridgeSkillDetail | null>(null);
  const [skillDetailLoading, setSkillDetailLoading] = useState(false);
  const [selectedSkillKey, setSelectedSkillKey] = useState<string | null>(null);
  const [extensionsOverview, setExtensionsOverview] = useState<ExtensionsOverview | null>(null);
  const [cronRunningId, setCronRunningId] = useState<string | null>(null);
  const [cronSchedulerStatus, setCronSchedulerStatus] = useState<CronSchedulerStatus | null>(null);
  const [cronSchedulerBusy, setCronSchedulerBusy] = useState(false);
  const [cronJobForm, setCronJobForm] = useState<CronJobFormState>(emptyCronJobFormState);
  const [cronJobSaving, setCronJobSaving] = useState(false);
  const [cronDeletingId, setCronDeletingId] = useState<string | null>(null);
  const [mcpInspection, setMcpInspection] = useState<McpServerInspection | null>(null);
  const [mcpInspecting, setMcpInspecting] = useState<string | null>(null);
  const [approvalRequests, setApprovalRequests] = useState<ApprovalRequest[]>([]);
  const [delegateRuns, setDelegateRuns] = useState<BridgeDelegateRun[]>([]);
  const [delegateActionId, setDelegateActionId] = useState<string | null>(null);
  const [sessions, setSessions] = useState<BridgeSessionSummary[]>([]);
  const [workspaceList, setWorkspaceList] = useState<WorkspaceListEntry[]>([]);
  const [running, setRunning] = useState(false);
  const [stopping, setStopping] = useState(false);
  const [queuedRedirectPrompt, setQueuedRedirectPrompt] = useState<string | null>(null);
  const [bootError, setBootError] = useState<string | null>(null);
  const [isClient, setIsClient] = useState(false);
  const [advancedSettingsOpen, setAdvancedSettingsOpen] = useState(false);
  const [activeView, setActiveView] = useState<MainView>("conversation");
  const [sidebarCollapsed, setSidebarCollapsed] = useState(false);
  const [expandedToolEntryIds, setExpandedToolEntryIds] = useState<string[]>([]);
  const [autoExpandedToolEntryIds, setAutoExpandedToolEntryIds] = useState<string[]>([]);
  const [suppressedAutoExpandedToolEntryIds, setSuppressedAutoExpandedToolEntryIds] = useState<string[]>([]);
  const [showJumpToLatest, setShowJumpToLatest] = useState(false);
  const [pendingTranscriptUpdates, setPendingTranscriptUpdates] = useState(0);
  const [fileViewerOpen, setFileViewerOpen] = useState(false);
  const [sharedViewerMode, setSharedViewerMode] = useState<SharedViewerMode>("file");
  const [fileViewerLoading, setFileViewerLoading] = useState(false);
  const [fileViewerError, setFileViewerError] = useState<string | null>(null);
  const [fileViewerTabs, setFileViewerTabs] = useState<WorkspaceFileTab[]>([]);
  const [activeFileViewerTabPath, setActiveFileViewerTabPath] = useState<string | null>(null);
  const [browserStreamLoading, setBrowserStreamLoading] = useState(false);
  const [browserStreamError, setBrowserStreamError] = useState<string | null>(null);
  const [browserStreamEndpoint, setBrowserStreamEndpoint] = useState<BrowserStreamEndpoint | null>(null);
  const [browserStreamRetryToken, setBrowserStreamRetryToken] = useState(0);
  const [browserUiSyncRequest, setBrowserUiSyncRequest] = useState<BrowserUiSyncRequest>({
    token: 0,
    forceNavigate: false,
  });
  const [browserDirectUrlRequest, setBrowserDirectUrlRequest] = useState<BrowserDirectUrlRequest | null>(null);
  const [copiedAssistantEntryId, setCopiedAssistantEntryId] = useState<string | null>(null);
  const [fileTreeWidth, setFileTreeWidth] = useState(240);
  const [filePreviewWidth, setFilePreviewWidth] = useState(820);
  const [wideFileViewerLayout, setWideFileViewerLayout] = useState(false);
  const [narrowFileTreeVisible, setNarrowFileTreeVisible] = useState(false);
  const [conversationPanelResize, setConversationPanelResize] =
    useState<ConversationPanelResizeState | null>(null);
  const [workspaceTree, setWorkspaceTree] = useState<WorkspaceTreeResponse | null>(null);
  const [workspaceTreeLoading, setWorkspaceTreeLoading] = useState(false);
  const [workspaceTreeError, setWorkspaceTreeError] = useState<string | null>(null);
  const [workspaceTreeExpandedDirectories, setWorkspaceTreeExpandedDirectories] = useState<string[]>([]);

  const configRef = useRef(config);
  const activeAssistantMessageIdRef = useRef<string | null>(null);
  const activeAssistantPrefixRef = useRef("");
  const sessionListenerRef = useRef<UnlistenFn[]>([]);
  const globalListenerRef = useRef<UnlistenFn[]>([]);
  const transcriptRef = useRef<HTMLDivElement | null>(null);
  const promptFormRef = useRef<HTMLFormElement | null>(null);
  const promptTextareaRef = useRef<HTMLTextAreaElement | null>(null);
  const cronAutoStartedRef = useRef<string | null>(null);
  const seenEventKeysRef = useRef<string[]>([]);
  const pendingUserMessageRef = useRef<{ id: string; signature: string } | null>(null);
  const inflightPromptSignatureRef = useRef<string | null>(null);
  const runBusyRef = useRef(false);
  const currentSessionIdRef = useRef<string | null>(null);
  const pendingRunSessionIdRef = useRef<string | null>(null);
  const queuedRedirectRef = useRef<{ prompt: string; sessionId: string | null } | null>(null);
  const loadedWorkspaceSessionKeysRef = useRef<Set<string>>(new Set());
  const flushingQueuedRedirectRef = useRef(false);
  const expandedToolEntryIdsRef = useRef<string[]>([]);
  const autoExpandedToolEntryIdsRef = useRef<string[]>([]);
  const suppressedAutoExpandedToolEntryIdsRef = useRef<string[]>([]);
  const autoFollowTranscriptRef = useRef(true);
  const pendingSessionScrollToLatestRef = useRef(false);
  const conversationLayoutRef = useRef<HTMLDivElement | null>(null);
  const fileTreeWidthRef = useRef(240);
  const filePreviewWidthRef = useRef(820);
  const sharedProviderConfigReadyRef = useRef(false);
  const lastTranscriptSnapshotRef = useRef<{ sessionId: string | null; changeToken: string | null }>({
    sessionId: null,
    changeToken: null,
  });

  useEffect(() => {
    fileTreeWidthRef.current = fileTreeWidth;
  }, [fileTreeWidth]);

  useEffect(() => {
    filePreviewWidthRef.current = filePreviewWidth;
  }, [filePreviewWidth]);

  useEffect(() => {
    setWorkspaceTree(null);
    setWorkspaceTreeError(null);
    setWorkspaceTreeLoading(false);
    setWorkspaceTreeExpandedDirectories([]);
    setFileViewerTabs([]);
    setActiveFileViewerTabPath(null);
    setBrowserStreamLoading(false);
    setBrowserStreamError(null);
    setBrowserStreamEndpoint(null);
    setSharedViewerMode("file");
    setNarrowFileTreeVisible(false);
  }, [config.workspaceRoot]);

  function updateExpandedToolEntryIds(next: string[] | ((prev: string[]) => string[])) {
    setExpandedToolEntryIds((prev) => {
      const resolved = typeof next === "function" ? next(prev) : next;
      expandedToolEntryIdsRef.current = resolved;
      return resolved;
    });
  }

  function updateAutoExpandedToolEntryIds(next: string[] | ((prev: string[]) => string[])) {
    setAutoExpandedToolEntryIds((prev) => {
      const resolved = typeof next === "function" ? next(prev) : next;
      autoExpandedToolEntryIdsRef.current = resolved;
      return resolved;
    });
  }

  function updateActiveAssistantMessageId(next: string | null) {
    activeAssistantMessageIdRef.current = next;
    setActiveAssistantMessageId(next);
  }

  function resetActiveAssistantStreamState() {
    activeAssistantPrefixRef.current = "";
    updateActiveAssistantMessageId(null);
  }

  function updateSuppressedAutoExpandedToolEntryIds(next: string[] | ((prev: string[]) => string[])) {
    setSuppressedAutoExpandedToolEntryIds((prev) => {
      const resolved = typeof next === "function" ? next(prev) : next;
      suppressedAutoExpandedToolEntryIdsRef.current = resolved;
      return resolved;
    });
  }

  function toggleToolEntryExpanded(entryId: string) {
    const manuallyExpanded = expandedToolEntryIdsRef.current.includes(entryId);
    const autoExpanded = autoExpandedToolEntryIdsRef.current.includes(entryId);
    if (manuallyExpanded) {
      updateExpandedToolEntryIds((prev) => prev.filter((id) => id !== entryId));
      return;
    }
    if (autoExpanded) {
      updateSuppressedAutoExpandedToolEntryIds((prev) =>
        prev.includes(entryId) ? prev : [...prev, entryId],
      );
      updateAutoExpandedToolEntryIds((prev) => prev.filter((id) => id !== entryId));
      return;
    }
    updateExpandedToolEntryIds((prev) => [...prev, entryId]);
    updateSuppressedAutoExpandedToolEntryIds((prev) => prev.filter((id) => id !== entryId));
  }

  function ensureToolEntryAutoExpanded(entryId: string) {
    if (suppressedAutoExpandedToolEntryIdsRef.current.includes(entryId)) {
      return;
    }
    updateAutoExpandedToolEntryIds((prev) => (prev.includes(entryId) ? prev : [...prev, entryId]));
  }

  function clearToolEntryAutoExpanded(entryId: string) {
    updateAutoExpandedToolEntryIds((prev) => prev.filter((id) => id !== entryId));
    updateSuppressedAutoExpandedToolEntryIds((prev) => prev.filter((id) => id !== entryId));
  }

  function toggleWorkspaceTreeDirectory(path: string) {
    setWorkspaceTreeExpandedDirectories((prev) =>
      prev.includes(path) ? prev.filter((value) => value !== path) : [...prev, path],
    );
  }

  function selectWorkspaceFileTab(path: string) {
    setActiveFileViewerTabPath(path);
    setFileViewerError(null);
    setFileViewerOpen(true);
  }

  function closeWorkspaceFileTab(path: string) {
    setFileViewerTabs((prev) => {
      const currentIndex = prev.findIndex((tab) => tab.path === path);
      if (currentIndex === -1) {
        return prev;
      }
      const nextTabs = prev.filter((tab) => tab.path !== path);
      if (activeFileViewerTabPath === path) {
        const fallbackTab = nextTabs[currentIndex] || nextTabs[currentIndex - 1] || null;
        setActiveFileViewerTabPath(fallbackTab?.path || null);
        setFileViewerError(null);
        setFileViewerLoading(false);
      }
      return nextTabs;
    });
  }

  function beginConversationPanelResize(
    panel: ConversationPanelResizeState["panel"],
    event: ReactMouseEvent<HTMLDivElement>,
  ) {
    if (event.button !== 0) {
      return;
    }
    event.preventDefault();
    setConversationPanelResize({
      panel,
      startX: event.clientX,
      startWidth: panel === "tree" ? fileTreeWidthRef.current : filePreviewWidthRef.current,
    });
  }

  function transcriptDistanceFromBottom(element: HTMLDivElement): number {
    return element.scrollHeight - element.scrollTop - element.clientHeight;
  }

  function isTranscriptNearBottom(element: HTMLDivElement): boolean {
    return transcriptDistanceFromBottom(element) <= 96;
  }

  function scrollTranscriptToBottom(behavior: ScrollBehavior = "auto") {
    const element = transcriptRef.current;
    if (!element) {
      return;
    }
    element.scrollTo({ top: element.scrollHeight, behavior });
    autoFollowTranscriptRef.current = true;
    setShowJumpToLatest(false);
    setPendingTranscriptUpdates(0);
  }

  function scheduleTranscriptScrollToBottom(behavior: ScrollBehavior = "auto", frameCount = 2) {
    let remainingFrames = frameCount;
    const scrollAfterLayout = () => {
      scrollTranscriptToBottom(behavior);
      if (remainingFrames <= 0) {
        return;
      }
      remainingFrames -= 1;
      requestAnimationFrame(scrollAfterLayout);
    };

    requestAnimationFrame(scrollAfterLayout);
    window.setTimeout(() => scrollTranscriptToBottom("auto"), 80);
  }

  function onTranscriptScroll() {
    const element = transcriptRef.current;
    if (!element) {
      return;
    }
    const nearBottom = isTranscriptNearBottom(element);
    autoFollowTranscriptRef.current = nearBottom;
    setShowJumpToLatest(!nearBottom);
    if (nearBottom) {
      setPendingTranscriptUpdates(0);
    }
  }

  function setRunActive() {
    runBusyRef.current = true;
    pendingRunSessionIdRef.current =
      pendingRunSessionIdRef.current?.trim() || currentSessionIdRef.current?.trim() || null;
    setAgentActivity("正在准备");
    setRunning(true);
    setStopping(false);
  }

  function setRunIdle() {
    runBusyRef.current = false;
    pendingRunSessionIdRef.current = null;
    inflightPromptSignatureRef.current = null;
    setAgentActivity("正在准备");
    setRunning(false);
    setStopping(false);
  }

  function setQueuedRedirect(prompt: string, sessionId: string | null) {
    queuedRedirectRef.current = {
      prompt,
      sessionId,
    };
    setQueuedRedirectPrompt(prompt);
  }

  function clearQueuedRedirect() {
    queuedRedirectRef.current = null;
    setQueuedRedirectPrompt(null);
  }

  useEffect(() => {
    configRef.current = config;
  }, [config]);

  useEffect(() => {
    activeAssistantMessageIdRef.current = activeAssistantMessageId;
  }, [activeAssistantMessageId]);

  useEffect(() => {
    currentSessionIdRef.current = currentSessionId;
  }, [currentSessionId]);

  useEffect(() => {
    const preferences = loadPreferences();
    setConfig(preferences);
    const currentWorkspace = createWorkspaceListEntry(preferences.workspaceRoot, preferences.dataDir, true);
    setWorkspaceList(
      mergeWorkspaceListEntries([
        ...(currentWorkspace ? [currentWorkspace] : []),
        ...loadWorkspaceList(),
      ]),
    );
    setIsClient(true);
  }, []);

  useEffect(() => {
    if (!isClient) {
      return;
    }
    window.localStorage.setItem(storageKey, JSON.stringify(localPreferencePayload(config)));
    window.localStorage.removeItem(legacyStorageKey);
  }, [config, isClient]);

  useEffect(() => {
    if (!isClient || !sharedProviderConfigReadyRef.current) {
      return;
    }
    const dataDir = resolveDataDir(config);
    if (!dataDir) {
      return;
    }

    const timer = window.setTimeout(() => {
      void invoke<SharedProviderConfig>("save_shared_provider_config", {
        request: {
          dataDir,
          provider: emptyToNull(config.provider),
          model: emptyToNull(config.model),
          baseUrl: emptyToNull(config.baseUrl),
          apiKey: emptyToNull(config.apiKey),
          auxModel: emptyToNull(config.smallModel),
        },
      }).catch((error) => {
        pushNotice("error", formatError(error));
      });
    }, 250);

    return () => window.clearTimeout(timer);
  }, [
    config.workspaceRoot,
    config.dataDir,
    config.provider,
    config.model,
    config.smallModel,
    config.baseUrl,
    config.apiKey,
    isClient,
  ]);

  useEffect(() => {
    if (!isClient) {
      return;
    }
    storeWorkspaceList(workspaceList);
  }, [workspaceList, isClient]);

  useEffect(() => {
    if (!isClient) {
      return;
    }
    for (const entry of workspaceList) {
      const key = workspaceEntryKey(entry.workspaceRoot, entry.dataDir);
      if (loadedWorkspaceSessionKeysRef.current.has(key)) {
        continue;
      }
      loadedWorkspaceSessionKeysRef.current.add(key);
      void refreshWorkspaceEntrySessions(entry);
    }
  }, [workspaceList, isClient]);

  async function refreshSessions(nextConfig?: Preferences) {
    const candidate = nextConfig || configRef.current;
    const dataDir = resolveDataDir(candidate);
    if (!dataDir) {
      setSessions([]);
      return;
    }
    try {
      const result = await enrichSessionSummaries(
        dataDir,
        await invoke<BridgeSessionSummary[]>("list_sessions", { dataDir }),
      );
      setSessions(result);
      if (candidate.workspaceRoot.trim()) {
        const key = workspaceEntryKey(candidate.workspaceRoot, candidate.dataDir);
        setWorkspaceList((prev) => {
          const nextEntry = createWorkspaceListEntry(candidate.workspaceRoot, candidate.dataDir, true);
          if (!nextEntry) {
            return prev;
          }
          nextEntry.sessions = result;
          return mergeWorkspaceListEntries([nextEntry, ...prev]).map((entry) =>
            workspaceEntryKey(entry.workspaceRoot, entry.dataDir) === key
              ? { ...entry, sessions: result, loading: false, error: null }
              : entry,
          );
        });
      }
    } catch (error) {
      pushNotice("error", formatError(error));
    }
  }

  async function refreshWorkspaceEntrySessions(entry: WorkspaceListEntry) {
    const key = workspaceEntryKey(entry.workspaceRoot, entry.dataDir);
    const candidate = {
      ...configRef.current,
      workspaceRoot: entry.workspaceRoot,
      dataDir: entry.dataDir,
      sessionId: "",
    };
    const dataDir = resolveDataDir(candidate);
    if (!dataDir) {
      return;
    }
    setWorkspaceList((prev) =>
      prev.map((item) =>
        workspaceEntryKey(item.workspaceRoot, item.dataDir) === key
          ? { ...item, loading: true, error: null }
          : item,
      ),
    );
    try {
      const result = await enrichSessionSummaries(
        dataDir,
        await invoke<BridgeSessionSummary[]>("list_sessions", { dataDir }),
      );
      setWorkspaceList((prev) =>
        prev.map((item) =>
          workspaceEntryKey(item.workspaceRoot, item.dataDir) === key
            ? { ...item, sessions: result, loading: false, error: null }
            : item,
        ),
      );
      if (workspaceEntryKey(configRef.current.workspaceRoot, configRef.current.dataDir) === key) {
        setSessions(result);
      }
    } catch (error) {
      setWorkspaceList((prev) =>
        prev.map((item) =>
          workspaceEntryKey(item.workspaceRoot, item.dataDir) === key
            ? { ...item, loading: false, error: formatError(error) }
            : item,
        ),
      );
    }
  }

  async function enrichSessionSummaries(
    dataDir: string,
    summaries: BridgeSessionSummary[],
  ): Promise<BridgeSessionSummary[]> {
    const missingTitle = summaries.filter((session) => !session.title?.trim());
    if (!missingTitle.length) {
      return summaries;
    }
    const titles = new Map<string, string>();
    await Promise.all(
      missingTitle.map(async (session) => {
        try {
          const detail = await invoke<BridgeSessionDetail | null>("load_session", {
            dataDir,
            sessionId: session.session_id,
            remember: false,
          });
          const title = titleFromSessionDetail(detail);
          if (title) {
            titles.set(session.session_id, title);
          }
        } catch {
          // Leave the session id fallback when a stale or missing session cannot be loaded.
        }
      }),
    );
    if (!titles.size) {
      return summaries;
    }
    return summaries.map((session) => ({
      ...session,
      title: session.title?.trim() || titles.get(session.session_id) || session.title,
    }));
  }

  function toggleWorkspaceEntry(entry: WorkspaceListEntry) {
    const key = workspaceEntryKey(entry.workspaceRoot, entry.dataDir);
    setWorkspaceList((prev) =>
      prev.map((item) =>
        workspaceEntryKey(item.workspaceRoot, item.dataDir) === key
          ? { ...item, expanded: !item.expanded }
          : item,
      ),
    );
  }

  async function selectWorkspaceEntry(entry: WorkspaceListEntry) {
    await switchWorkspaceRoot(entry.workspaceRoot, {
      preserveActiveView: true,
      dataDir: entry.dataDir,
      notice: `已切换到 ${workspaceFolderName(entry.workspaceRoot)}`,
    });
  }

  async function loadSharedProviderConfig(nextConfig?: Preferences): Promise<Preferences> {
    const candidate = nextConfig || configRef.current;
    const dataDir = resolveDataDir(candidate);
    sharedProviderConfigReadyRef.current = false;
    if (!dataDir) {
      sharedProviderConfigReadyRef.current = true;
      return candidate;
    }
    try {
      const shared = await invoke<SharedProviderConfig>("load_shared_provider_config", { dataDir });
      const merged = applySharedProviderConfig(candidate, shared);
      sharedProviderConfigReadyRef.current = true;
      return merged;
    } catch (error) {
      sharedProviderConfigReadyRef.current = true;
      pushNotice("error", formatError(error));
      return candidate;
    }
  }

  async function loadSkills(nextConfig?: Preferences) {
    const candidate = nextConfig || configRef.current;
    const dataDir = resolveDataDir(candidate);
    if (!dataDir) {
      setSkills([]);
      setSkillDetail(null);
      setSelectedSkillKey(null);
      pushNotice("error", "请先填写 workspace root 或 data dir");
      return;
    }
    try {
      const result = await invoke<BridgeSkillSummary[]>("list_skills", { dataDir });
      setSkills(result);
      if (result.length === 0) {
        setSkillDetail(null);
        setSelectedSkillKey(null);
      }
    } catch (error) {
      pushNotice("error", formatError(error));
    }
  }

  async function loadProviders(nextConfig?: Preferences) {
    const candidate = nextConfig || configRef.current;
    const dataDir = resolveDataDir(candidate);
    if (!dataDir) {
      setProviders([]);
      return;
    }
    try {
      const result = await invoke<ProviderSummary[]>("list_providers", { dataDir });
      setProviders(result);
      if (!candidate.provider && result.some((item) => item.is_default)) {
        const fallback = result.find((item) => item.is_default);
        if (fallback) {
          setConfig((prev) => ({ ...prev, provider: fallback.id, model: prev.model || fallback.model }));
        }
      }
    } catch (error) {
      pushNotice("error", formatError(error));
    }
  }

  async function loadProviderRuntimeStatus(nextConfig?: Preferences) {
    const candidate = nextConfig || configRef.current;
    const dataDir = resolveDataDir(candidate);
    if (!dataDir) {
      setProviderRuntimeStatus(null);
      setProviderRuntimeError("请先填写 workspace root 或 data dir");
      return;
    }
    setProviderRuntimeLoading(true);
    try {
      const result = await invoke<ProviderRuntimeStatus>("resolve_provider_status", {
        request: {
          dataDir,
          provider: emptyToNull(candidate.provider),
          model: emptyToNull(candidate.model),
          baseUrl: emptyToNull(candidate.baseUrl),
          apiKey: emptyToNull(candidate.apiKey),
        },
      });
      setProviderRuntimeStatus(result);
      setProviderRuntimeError(null);
    } catch (error) {
      setProviderRuntimeStatus(null);
      setProviderRuntimeError(formatError(error));
    } finally {
      setProviderRuntimeLoading(false);
    }
  }

  async function loadSkillDetail(category: string, name: string, filePath?: string) {
    const dataDir = resolveDataDir(configRef.current);
    if (!dataDir) {
      pushNotice("error", "请先填写 workspace root 或 data dir");
      return;
    }

    setSelectedSkillKey(skillKey({ category, name }));
    setSkillDetailLoading(true);
    try {
      const result = await invoke<BridgeSkillDetail>("view_skill", {
        dataDir,
        category,
        name,
        filePath: filePath ?? null,
      });
      setSkillDetail(result);
    } catch (error) {
      pushNotice("error", formatError(error));
      if (!filePath) {
        setSkillDetail(null);
      }
    } finally {
      setSkillDetailLoading(false);
    }
  }

  async function loadExtensionsOverview(nextConfig?: Preferences) {
    const candidate = nextConfig || configRef.current;
    const dataDir = resolveDataDir(candidate);
    if (!dataDir) {
      setExtensionsOverview(null);
      return;
    }
    try {
      const result = await invoke<ExtensionsOverview>("extensions_overview", { dataDir });
      setExtensionsOverview(result);
    } catch (error) {
      pushNotice("error", formatError(error));
    }
  }

  async function inspectMcpServer(serverName: string) {
    const dataDir = resolveDataDir(configRef.current);
    if (!dataDir) {
      pushNotice("error", "请先填写 workspace root 或 data dir");
      return;
    }
    setMcpInspecting(serverName);
    try {
      const result = await invoke<McpServerInspection>("inspect_mcp_server", {
        dataDir,
        serverName,
      });
      setMcpInspection(result);
      await loadExtensionsOverview();
      pushNotice("success", `已检查 MCP server ${serverName}`);
    } catch (error) {
      pushNotice("error", formatError(error));
    } finally {
      setMcpInspecting((current) => (current === serverName ? null : current));
    }
  }

  async function loadCronSchedulerStatus() {
    try {
      const result = await invoke<CronSchedulerStatus>("cron_scheduler_status");
      setCronSchedulerStatus(result);
    } catch (error) {
      pushNotice("error", formatError(error));
    }
  }

  async function loadApprovals(nextConfig?: Preferences): Promise<ApprovalRequest[]> {
    const candidate = nextConfig || configRef.current;
    const dataDir = resolveDataDir(candidate);
    if (!dataDir) {
      setApprovalRequests([]);
      return [];
    }
    try {
      const result = await invoke<ApprovalRequest[]>("list_approvals", { dataDir });
      setApprovalRequests(result);
      return result;
    } catch (error) {
      pushNotice("error", formatError(error));
      return [];
    }
  }

  async function loadDelegateRuns(parentSessionId?: string | null, nextConfig?: Preferences) {
    const candidate = nextConfig || configRef.current;
    const dataDir = resolveDataDir(candidate);
    if (!dataDir) {
      setDelegateRuns([]);
      return;
    }
    try {
      const result = await invoke<BridgeDelegateRun[]>("list_delegate_runs", {
        dataDir,
        parentSessionId: parentSessionId ?? currentSessionId ?? null,
      });
      setDelegateRuns(result);
    } catch (error) {
      pushNotice("error", formatError(error));
    }
  }

  async function cancelDelegateRun(runId: string) {
    const dataDir = resolveDataDir(configRef.current);
    if (!dataDir) {
      pushNotice("error", "请先填写 workspace root 或 data dir");
      return;
    }
    setDelegateActionId(runId);
    try {
      await invoke<BridgeDelegateRun>("cancel_delegate_run", { dataDir, runId });
      await loadDelegateRuns(currentSessionId);
      pushNotice("nudge", `已请求取消子任务 ${runId}`);
    } catch (error) {
      pushNotice("error", formatError(error));
    } finally {
      setDelegateActionId((current) => (current === runId ? null : current));
    }
  }

  async function retryDelegateRun(runId: string) {
    const workspaceRoot = configRef.current.workspaceRoot.trim();
    if (!workspaceRoot) {
      pushNotice("error", "请先填写 workspace root");
      return;
    }
    setDelegateActionId(runId);
    try {
      const result = await invoke<BridgeDelegateRun>("retry_delegate_run", {
        request: {
          workspaceRoot,
          dataDir: emptyToNull(configRef.current.dataDir),
          runId,
          provider: emptyToNull(configRef.current.provider),
          model: emptyToNull(configRef.current.model),
          auxModel: emptyToNull(configRef.current.smallModel),
          baseUrl: emptyToNull(configRef.current.baseUrl),
          apiKey: emptyToNull(configRef.current.apiKey),
          maxIterations: Number(configRef.current.maxIterations || 12),
          systemPromptOverride: null,
          enableShellTool: configRef.current.enableShellTool,
        },
      });
      setCurrentSessionId(result.session_id);
      setConfig((prev) => ({ ...prev, sessionId: result.session_id }));
      await attachSessionListeners(result.session_id);
      await Promise.all([
        refreshSessions(),
        loadDelegateRuns(result.parent_session_id),
        loadWorkspaceTree(true),
      ]);
      pushNotice("success", `已重试子任务 ${runId}`);
    } catch (error) {
      pushNotice("error", formatError(error));
    } finally {
      setDelegateActionId((current) => (current === runId ? null : current));
    }
  }

  async function resolveApproval(approvalId: string, approved: boolean) {
    const dataDir = resolveDataDir(configRef.current);
    if (!dataDir) {
      pushNotice("error", "请先填写 workspace root 或 data dir");
      return;
    }
    try {
      const result = await invoke<ApprovalRequest>("resolve_approval", {
        dataDir,
        approvalId,
        approved,
      });
      const workspaceRoot = configRef.current.workspaceRoot.trim();
      if (!workspaceRoot) {
        pushNotice("error", "请先填写 workspace root");
        await loadApprovals();
        return;
      }
      setRunActive();
      const resumed = await invoke<BridgeRunResult>("resume_approval", {
        request: {
          workspaceRoot,
          dataDir: emptyToNull(configRef.current.dataDir),
          sessionId: result.session_id,
          provider: emptyToNull(configRef.current.provider),
          model: emptyToNull(configRef.current.model),
          auxModel: emptyToNull(configRef.current.smallModel),
          baseUrl: emptyToNull(configRef.current.baseUrl),
          apiKey: emptyToNull(configRef.current.apiKey),
          maxIterations: Number(configRef.current.maxIterations || 12),
          systemPromptOverride: null,
          enableShellTool: configRef.current.enableShellTool,
        },
        approvalId,
      });
      setCurrentSessionId(resumed.session_id);
      await attachSessionListeners(resumed.session_id);
      await Promise.all([loadApprovals(), loadWorkspaceTree(true)]);
      if (resumed.status === "awaiting_approval") {
        setRunIdle();
        pushNotice("approval", `审批 ${result.id} 已处理，但当前会话还在等待新的审批。`);
      } else {
        pushNotice(approved ? "success" : "nudge", `${approved ? "已批准" : "已拒绝"}审批 ${result.id}`);
      }
    } catch (error) {
      setRunIdle();
      pushNotice("error", formatError(error));
    }
  }

  async function runCronJob(jobId: string) {
    const workspaceRoot = configRef.current.workspaceRoot.trim();
    if (!workspaceRoot) {
      pushNotice("error", "请先填写 workspace root");
      return;
    }

    setRunActive();
    setCronRunningId(jobId);
    try {
      const result = await invoke<BridgeRunResult>("run_cron_job", {
        request: {
          workspaceRoot,
          dataDir: emptyToNull(configRef.current.dataDir),
          jobId,
          provider: emptyToNull(configRef.current.provider),
          model: emptyToNull(configRef.current.model),
          auxModel: emptyToNull(configRef.current.smallModel),
          baseUrl: emptyToNull(configRef.current.baseUrl),
          apiKey: emptyToNull(configRef.current.apiKey),
          maxIterations: Number(configRef.current.maxIterations || 12),
          systemPromptOverride: null,
          enableShellTool: configRef.current.enableShellTool,
        },
      });
      setCurrentSessionId(result.session_id);
      await attachSessionListeners(result.session_id);
      await Promise.all([loadExtensionsOverview(), refreshSessions(), loadWorkspaceTree(true)]);
      if (result.status === "awaiting_approval") {
        setRunIdle();
        pushNotice("approval", `Cron 任务 ${jobId} 已启动，但当前会话在等待审批。`);
      } else {
        pushNotice("success", `Cron 任务 ${jobId} 已执行`);
      }
    } catch (error) {
      setRunIdle();
      pushNotice("error", formatError(error));
    } finally {
      setCronRunningId((current) => (current === jobId ? null : current));
    }
  }

  async function startCronScheduler(options?: { silent?: boolean }) {
    const workspaceRoot = configRef.current.workspaceRoot.trim();
    if (!workspaceRoot) {
      if (!options?.silent) {
        pushNotice("error", "请先填写 workspace root");
      }
      return;
    }

    setCronSchedulerBusy(true);
    try {
      const result = await invoke<CronSchedulerStatus>("start_cron_scheduler", {
        request: {
          workspaceRoot,
          dataDir: emptyToNull(configRef.current.dataDir),
          provider: emptyToNull(configRef.current.provider),
          model: emptyToNull(configRef.current.model),
          auxModel: emptyToNull(configRef.current.smallModel),
          baseUrl: emptyToNull(configRef.current.baseUrl),
          apiKey: emptyToNull(configRef.current.apiKey),
          maxIterations: Number(configRef.current.maxIterations || 12),
          systemPromptOverride: null,
          enableShellTool: configRef.current.enableShellTool,
          tickIntervalSeconds: defaultCronTickIntervalSeconds,
        },
      });
      setCronSchedulerStatus(result);
      if (!options?.silent) {
        pushNotice("success", "Cron scheduler 已启动");
      }
    } catch (error) {
      if (!options?.silent) {
        pushNotice("error", formatError(error));
      }
    } finally {
      setCronSchedulerBusy(false);
    }
  }

  async function stopCronScheduler() {
    setCronSchedulerBusy(true);
    try {
      const result = await invoke<CronSchedulerStatus>("stop_cron_scheduler");
      setCronSchedulerStatus(result);
      pushNotice("nudge", "Cron scheduler 已停止");
    } catch (error) {
      pushNotice("error", formatError(error));
    } finally {
      setCronSchedulerBusy(false);
    }
  }

  function editCronJob(item: ExtensionsOverview["cron_jobs"][number]) {
    setCronJobForm({
      previousId: item.id,
      id: item.id,
      schedule: item.schedule,
      prompt: item.prompt,
      enabled: item.enabled,
    });
  }

  function resetCronJobForm() {
    setCronJobForm(emptyCronJobFormState());
  }

  async function saveCronJob() {
    const dataDir = resolveDataDir(configRef.current);
    if (!dataDir) {
      pushNotice("error", "请先填写 workspace root 或 data dir");
      return;
    }
    if (!cronJobForm.id.trim() || !cronJobForm.schedule.trim() || !cronJobForm.prompt.trim()) {
      pushNotice("error", "请填写任务 ID、执行计划和提示词");
      return;
    }

    setCronJobSaving(true);
    try {
      await invoke("save_cron_job", {
        dataDir,
        request: {
          previousId: cronJobForm.previousId,
          id: cronJobForm.id.trim(),
          schedule: cronJobForm.schedule.trim(),
          prompt: cronJobForm.prompt.trim(),
          enabled: cronJobForm.enabled,
        },
      });
      await loadExtensionsOverview();
      resetCronJobForm();
      pushNotice("success", `已保存定时任务 ${cronJobForm.id.trim()}`);
    } catch (error) {
      pushNotice("error", formatError(error));
    } finally {
      setCronJobSaving(false);
    }
  }

  async function deleteCronJob(jobId: string) {
    const dataDir = resolveDataDir(configRef.current);
    if (!dataDir) {
      pushNotice("error", "请先填写 workspace root 或 data dir");
      return;
    }
    if (typeof window !== "undefined" && !window.confirm(`确认删除定时任务 ${jobId} 吗？`)) {
      return;
    }

    setCronDeletingId(jobId);
    try {
      await invoke("delete_cron_job", { dataDir, jobId });
      await loadExtensionsOverview();
      if (cronJobForm.previousId === jobId || cronJobForm.id === jobId) {
        resetCronJobForm();
      }
      pushNotice("nudge", `已删除定时任务 ${jobId}`);
    } catch (error) {
      pushNotice("error", formatError(error));
    } finally {
      setCronDeletingId((current) => (current === jobId ? null : current));
    }
  }

  function pushNotice(kind: string, message: string) {
    const id = `${Date.now()}-${Math.random()}`;
    setNotices((prev) => [
      { id, kind, message },
      ...prev,
    ].slice(0, 24));
    window.setTimeout(() => {
      setNotices((prev) => prev.filter((notice) => notice.id !== id));
    }, 1000);
  }

  function dismissNotice(id: string) {
    setNotices((prev) => prev.filter((notice) => notice.id !== id));
  }

  function appendTimelineEntry(entry: TimelineEntry) {
    setTimeline((prev) => [...prev, entry]);
  }

  function appendOptimisticUserMessage(content: string) {
    const id = `user-pending-${Date.now()}`;
    pendingUserMessageRef.current = { id, signature: promptSignature(content) };
    appendTimelineEntry({
      id,
      type: "user",
      content,
      pending: true,
    });
  }

  function confirmUserMessage(
    content: string,
    seq: number,
    options?: { preservePendingContent?: boolean },
  ) {
    const targetId = `user-${seq}`;
    const signature = promptSignature(content);
    const pending = pendingUserMessageRef.current;
    const preservePendingContent = Boolean(options?.preservePendingContent);
    setTimeline((prev) => {
      if (prev.some((entry) => entry.id === targetId)) {
        return prev
          .filter((entry) => entry.id !== pending?.id)
          .map((entry) =>
            entry.id === targetId && entry.type === "user"
              ? { ...entry, content: preservePendingContent ? entry.content : content, pending: false }
              : entry,
          );
      }
      if (pending && (pending.signature === signature || preservePendingContent)) {
        let replaced = false;
        const next = prev.map((entry) => {
          if (entry.id === pending.id && entry.type === "user") {
            replaced = true;
            return {
              ...entry,
              id: targetId,
              content: preservePendingContent ? entry.content : content,
              pending: false,
            };
          }
          return entry;
        });
        if (replaced) {
          return next;
        }
        return [...prev, { id: targetId, type: "user" as const, content }];
      }
      return [...prev, { id: targetId, type: "user" as const, content }];
    });
    if (pending && (pending.signature === signature || preservePendingContent)) {
      pendingUserMessageRef.current = null;
    }
  }

  function discardPendingUserMessage() {
    const pending = pendingUserMessageRef.current;
    if (!pending) {
      return;
    }
    pendingUserMessageRef.current = null;
    setTimeline((prev) => prev.filter((entry) => entry.id !== pending.id));
  }

  function sealActiveAssistantSegment() {
    const targetId = activeAssistantMessageIdRef.current;
    if (!targetId) {
      return;
    }
    setTimeline((prev) => {
      let segmentContent = "";
      const next = prev.map((entry) => {
        if (entry.id === targetId && entry.type === "assistant") {
          segmentContent = entry.content;
          return entry.streaming ? { ...entry, streaming: false } : entry;
        }
        return entry;
      });
      if (segmentContent) {
        activeAssistantPrefixRef.current += segmentContent;
      }
      return next;
    });
    updateActiveAssistantMessageId(null);
  }

  function appendAssistantDelta(delta: string) {
    setTimeline((prev) => {
      let targetId = activeAssistantMessageIdRef.current;
      if (!targetId) {
        targetId = `assistant-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
        updateActiveAssistantMessageId(targetId);
        return [
          ...prev,
          { id: targetId, type: "assistant" as const, content: delta, streaming: true },
        ];
      }
      let matched = false;
      const next = prev.map((entry) => {
        if (entry.id === targetId && entry.type === "assistant") {
          matched = true;
          return { ...entry, content: `${entry.content}${delta}`, streaming: true };
        }
        return entry;
      });
      if (matched) {
        return next;
      }
      return [
        ...prev,
        { id: targetId, type: "assistant" as const, content: delta, streaming: true },
      ];
    });
  }

  function finalizeAssistantMessage(content: string) {
    const committedPrefix = activeAssistantPrefixRef.current;
    const normalizedContent =
      committedPrefix && content.startsWith(committedPrefix)
        ? content.slice(committedPrefix.length)
        : content;
    const targetId = activeAssistantMessageIdRef.current;
    setTimeline((prev) => {
      if (!targetId) {
        if (!normalizedContent) {
          return prev;
        }
        return [
          ...prev,
          {
            id: `assistant-final-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`,
            type: "assistant" as const,
            content: normalizedContent,
          },
        ];
      }
      let matched = false;
      const next = prev.map((entry) => {
        if (entry.id === targetId && entry.type === "assistant") {
          matched = true;
          return { ...entry, content: normalizedContent || entry.content, streaming: false };
        }
        return entry;
      });
      if (matched) {
        return next;
      }
      return [
        ...prev,
        { id: targetId, type: "assistant" as const, content: normalizedContent, streaming: false },
      ];
    });
    resetActiveAssistantStreamState();
  }

  function upsertBatchTimeline(batch: ParallelBatchState) {
    const id = `batch-${batch.batchId}`;
    setTimeline((prev) => {
      const index = prev.findIndex((entry) => entry.id === id && entry.type === "batch");
      const batchEntry: TimelineEntry = { id, type: "batch", ...batch };
      if (index === -1) {
        return [...prev, batchEntry];
      }
      const next = [...prev];
      next[index] = batchEntry;
      return next;
    });
  }

  function markToolApprovalPending(
    id: string,
    name: string,
    reason: string,
    command: string,
    executionMode?: string,
    batchId?: string | null,
    batchIndex?: number | null,
    batchTotal?: number | null,
    approvalId?: string,
  ) {
    clearToolEntryAutoExpanded(id);
    setTools((prev) => {
      const index = prev.findIndex((entry) => entry.id === id);
      if (index === -1) {
        return [
          { id, name, phase: "approval" as const, detail: reason, executionMode, batchId },
          ...prev,
        ].slice(0, 40);
      }
      const next = [...prev];
      next[index] = { ...next[index], phase: "approval" as const, detail: reason, executionMode, batchId };
      return next;
    });
    setTimeline((prev) => {
      const toolIndex = prev.findIndex((entry) => entry.id === id && entry.type === "tool");
      const next = [...prev];
      if (toolIndex === -1) {
        const toolEntry: TimelineEntry = {
          id,
          type: "tool",
          name,
          detail: reason,
          commandPreview: resolveToolCommandPreview(command, reason),
          phase: "approval",
          executionMode,
          batchId,
          batchIndex,
          batchTotal,
        };
        next.push(toolEntry);
      } else {
        const toolEntry: TimelineEntry = {
          ...next[toolIndex],
          type: "tool",
          name,
          detail: reason,
          commandPreview:
            next[toolIndex].type === "tool"
              ? resolveToolCommandPreview(command, reason, next[toolIndex].commandPreview)
              : resolveToolCommandPreview(command, reason),
          phase: "approval",
          executionMode,
          batchId,
          batchIndex,
          batchTotal,
        };
        next[toolIndex] = toolEntry;
      }
      if (approvalId) {
        const approvalEntryId = `approval-${approvalId}`;
        const approvalIndex = next.findIndex((entry) => entry.id === approvalEntryId && entry.type === "approval");
        const approvalEntry: TimelineEntry = {
          id: approvalEntryId,
          type: "approval",
          approvalId,
          toolName: name,
          reason,
          command,
          executionMode,
          batchId,
          batchIndex,
          batchTotal,
        };
        if (approvalIndex === -1) {
          next.push(approvalEntry);
        } else {
          next[approvalIndex] = approvalEntry;
        }
      }
      return next;
    });
  }

  function markToolFinished(
    id: string,
    name: string,
    detail: string,
    executionMode?: string,
    batchId?: string | null,
    batchIndex?: number | null,
    batchTotal?: number | null,
    commandPreview?: string,
    phase: Extract<ToolPhase, "done" | "error"> = "done",
    durationMs?: number | null,
  ) {
    clearToolEntryAutoExpanded(id);
    setTools((prev) => {
      const index = prev.findIndex((entry) => entry.id === id);
      if (index === -1) {
        return [
          { id, name, phase, detail, executionMode, batchId, durationMs },
          ...prev,
        ].slice(0, 40);
      }
      const next = [...prev];
      next[index] = { ...next[index], phase, detail, executionMode, batchId, durationMs };
      return next;
    });
    setTimeline((prev) => {
      const index = prev.findIndex((entry) => entry.id === id && entry.type === "tool");
      if (index === -1) {
        const toolEntry: TimelineEntry = {
          id,
          type: "tool",
          name,
          detail,
          commandPreview: resolveToolCommandPreview(commandPreview, detail),
          phase,
          executionMode,
          batchId,
          batchIndex,
          batchTotal,
          durationMs,
        };
        return [
          ...prev,
          toolEntry,
        ];
      }
      const next = [...prev];
      const toolEntry: TimelineEntry = {
        ...next[index],
        type: "tool",
        name,
        detail,
        commandPreview:
          next[index].type === "tool"
            ? resolveToolCommandPreview(commandPreview, detail, next[index].commandPreview)
            : resolveToolCommandPreview(commandPreview, detail),
        phase,
        executionMode,
        batchId,
        batchIndex,
        batchTotal,
        durationMs,
      };
      next[index] = toolEntry;
      return next;
    });
  }

  function markToolStreaming(
    id: string,
    name: string,
    detail: string,
    executionMode?: string,
    batchId?: string | null,
    batchIndex?: number | null,
    batchTotal?: number | null,
    commandPreview?: string,
  ) {
    setTools((prev) => {
      const index = prev.findIndex((entry) => entry.id === id);
      if (index === -1) {
        return [
          { id, name, phase: "running" as const, detail, executionMode, batchId },
          ...prev,
        ].slice(0, 40);
      }
      const next = [...prev];
      next[index] = { ...next[index], name, phase: "running" as const, detail, executionMode, batchId };
      return next;
    });
    setTimeline((prev) => {
      const index = prev.findIndex((entry) => entry.id === id && entry.type === "tool");
      const existingEntry = index !== -1 && prev[index].type === "tool" ? prev[index] : null;
      const toolEntry: TimelineEntry = {
        id,
        type: "tool",
        name,
        detail,
        commandPreview:
          existingEntry != null
            ? resolveToolCommandPreview(commandPreview, detail, existingEntry.commandPreview)
            : resolveToolCommandPreview(commandPreview, detail),
        phase: "running",
        executionMode,
        batchId,
        batchIndex,
        batchTotal,
      };
      if (index === -1) {
        return [...prev, toolEntry];
      }
      const next = [...prev];
      next[index] = toolEntry;
      return next;
    });
  }

  function clearSessionListeners() {
    for (const unlisten of sessionListenerRef.current) {
      unlisten();
    }
    sessionListenerRef.current = [];
  }

  function clearGlobalListeners() {
    for (const unlisten of globalListenerRef.current) {
      unlisten();
    }
    globalListenerRef.current = [];
  }

  async function attachSessionListeners(sessionId: string) {
    clearSessionListeners();
    sessionListenerRef.current = [
      await listen(`hermes://agent/event/${sessionId}`, (event) => {
        handleEnvelope(event.payload as AgentEventEnvelope);
      }),
      await listen(`hermes://agent/done/${sessionId}`, async (event) => {
        const payload = event.payload as { session_id: string };
        setRunIdle();
        pushNotice("success", `当前会话 ${payload.session_id} 已返回结果`);
        await Promise.all([refreshSessions(), loadDelegateRuns(payload.session_id), loadWorkspaceTree(true)]);
      }),
    ];
  }

  function sessionIdFromEvent(event: AgentEventEnvelope["event"]): string {
    const raw = event.session_id;
    return typeof raw === "string" ? raw : "";
  }

  function hasSeenEvent(envelope: AgentEventEnvelope): boolean {
    const sessionId = sessionIdFromEvent(envelope.event);
    const key = `${sessionId}:${envelope.seq}:${envelope.event_type}`;
    const seen = seenEventKeysRef.current;
    if (seen.includes(key)) {
      return true;
    }
    seen.push(key);
    if (seen.length > 500) {
      seen.splice(0, seen.length - 500);
    }
    return false;
  }

  function shouldHandleGlobalSession(sessionId: string): boolean {
    const normalizedSessionId = sessionId.trim();
    const activeSessionId = currentSessionIdRef.current?.trim() || "";
    const pendingSessionId = pendingRunSessionIdRef.current?.trim() || "";
    if (!normalizedSessionId) {
      return true;
    }
    if (!activeSessionId) {
      return true;
    }
    if (normalizedSessionId === activeSessionId) {
      return true;
    }
    if (pendingSessionId && normalizedSessionId === pendingSessionId) {
      return true;
    }
    if (runBusyRef.current) {
      return true;
    }
    return false;
  }

  function handleEnvelope(envelope: AgentEventEnvelope) {
    if (!envelope || !envelope.event) {
      return;
    }
    if (hasSeenEvent(envelope)) {
      return;
    }
    const event = envelope.event;
    if (!shouldHandleGlobalSession(sessionIdFromEvent(event))) {
      return;
    }
    if (envelope.event_type !== "tool_call_delta") {
      setEventLog((prev) => [
        { seq: envelope.seq, type: envelope.event_type, detail: summarizeEvent(event) },
        ...prev,
      ].slice(0, 120));
    }

    switch (event.type) {
      case "session_ready":
        setCurrentSessionId(String(event.session_id || ""));
        setConfig((prev) => ({ ...prev, sessionId: String(event.session_id || "") }));
        break;
      case "turn_started":
        {
          resetActiveAssistantStreamState();
          setParallelBatch(null);
          const resumed = Boolean(event.resumed);
          const turnId = typeof event.turn_id === "string" ? event.turn_id : "";
          setAgentActivity(resumed ? `正在继续${turnId ? ` ${turnId}` : "本轮"}` : "正在准备上下文");
          if (!resumed) {
            confirmUserMessage(readTurnStartedPreview(event), envelope.seq, {
              preservePendingContent: true,
            });
          }
        }
        break;
      case "turn_finished":
        {
          const status = String(event.status || "");
          const duration = formatDurationMs(readDurationMs(event.duration_ms));
          const toolCount = Number(event.tool_call_count || 0);
          const suffix = duration ? ` · ${duration}` : "";
          setAgentActivity(
            status === "awaiting_approval"
              ? `等待审批 · ${toolCount} 个工具${suffix}`
              : status === "canceled"
                ? `已中断 · ${toolCount} 个工具${suffix}`
                : status === "error"
                  ? `运行失败 · ${toolCount} 个工具${suffix}`
                  : `本轮完成 · ${toolCount} 个工具${suffix}`,
          );
        }
        break;
      case "turn_interrupted":
        {
          const phase = formatTurnInterruptedPhase(String(event.phase || ""));
          setAgentActivity(`已中断${phase ? ` · ${phase}` : ""}`);
          setRunIdle();
          discardPendingUserMessage();
          pushNotice(
            "nudge",
            queuedRedirectRef.current ? "当前步骤已中断，准备执行最新指令。" : "当前步骤已中断。",
          );
        }
        break;
      case "iteration_started":
        setAgentActivity(`正在准备第 ${String(event.iteration || 1)} 轮`);
        break;
      case "model_request_started":
        setAgentActivity(`正在请求 ${String(event.model || "模型")}${formatModelRequestMetadata(event)}`);
        break;
      case "model_request_finished":
        {
          const duration = formatDurationMs(readDurationMs(event.duration_ms));
          const usage = formatTokenUsageSummary(event);
          const suffix = `${duration ? ` · ${duration}` : ""}${usage}`;
          setAgentActivity(
            String(event.status || "ok") !== "ok"
              ? `模型请求${formatModelRequestStatus(String(event.status || ""))}${suffix}`
              : Number(event.tool_call_count || 0) > 0
                ? `正在准备工具调用${suffix}`
                : `正在整理回复${suffix}`,
          );
        }
        break;
      case "background_model_request_started":
        setAgentActivity(
          `后台 ${String(event.purpose || "任务")} 正在请求 ${String(event.model || "模型")}${formatModelRequestMetadata(event)}`,
        );
        break;
      case "background_model_request_finished":
        {
          const duration = formatDurationMs(readDurationMs(event.duration_ms));
          const usage = formatTokenUsageSummary(event);
          const suffix = `${duration ? ` · ${duration}` : ""}${usage}`;
          setAgentActivity(
            String(event.status || "") === "ok"
              ? `后台模型请求完成${suffix}`
              : `后台模型请求未完成${suffix}`,
          );
        }
        break;
      case "context_prepared":
        {
          const projected = Number(event.projected_tokens || 0);
          const budget = Number(event.request_budget_tokens || 0);
          const kept = Number(event.kept_blocks || 0);
          const total = Number(event.total_blocks || 0);
          const duration = formatDurationMs(readDurationMs(event.duration_ms));
          const suffix = duration ? ` · ${duration}` : "";
          setAgentActivity(`上下文已准备 ${projected}/${budget} tokens · ${kept}/${total} 块${suffix}`);
        }
        break;
      case "context_sources_updated":
        {
          const kept = Number(event.kept_blocks || 0);
          const total = Number(event.total_blocks || 0);
          const labels = formatContextSourceLabels(event);
          setAgentActivity(`上下文来源已更新 · ${kept}/${total} 块${labels ? ` · ${labels}` : ""}`);
        }
        break;
      case "context_compacted":
        {
          const before = Number(event.original_estimated_tokens || 0);
          const after = Number(event.compressed_estimated_tokens || 0);
          const messagesBefore = Number(event.original_message_count || 0);
          const messagesAfter = Number(event.compressed_message_count || 0);
          const pruned = Number(event.pruned_tool_messages || 0);
          const suffix = pruned > 0 ? ` · 裁剪 ${pruned} 条工具输出` : "";
          setAgentActivity(
            `上下文已压缩 ${before}/${after} tokens · ${messagesBefore}/${messagesAfter} 条消息${suffix}`,
          );
        }
        break;
      case "model_recovery":
        {
          const action = String(event.action || "");
          const kind = String(event.kind || "");
          const delay = Number(event.delay_ms || 0);
          const budget = Number(event.output_budget_tokens || 0);
          const limit = Number(event.context_limit_tokens || 0);
          if (action === "sleep_then_retry") {
            setAgentActivity(`模型请求将重试 · ${kind || "transient"} · ${delay}ms`);
          } else if (action === "reduce_output_budget") {
            setAgentActivity(`已下调输出上限 · ${budget || "auto"} tokens`);
          } else if (action === "force_context_compression") {
            setAgentActivity(`请求超限，正在压缩上下文${limit ? ` · ${limit} tokens` : ""}`);
          } else {
            setAgentActivity(`正在恢复模型请求 · ${kind || action || "recovery"}`);
          }
        }
        break;
      case "delegate_run_updated":
        {
          const status = formatDelegateRunStatus(String(event.status || ""));
          const objective = truncate(String(event.objective_preview || event.result_preview || ""), 46);
          setAgentActivity(`worker ${status}${objective ? ` · ${objective}` : ""}`);
          void loadDelegateRuns(String(event.session_id || currentSessionIdRef.current || ""));
        }
        break;
      case "assistant_delta":
        appendAssistantDelta(String(event.delta || ""));
        break;
      case "goal_state_updated":
        {
          const source = formatGoalStateSource(String(event.source || ""));
          const focus = String(event.focus_goal_title || event.focus_goal_id || "");
          const status = String(event.focus_goal_status || "");
          const counts = `${String(event.active_goal_count || 0)}/${String(event.goal_count || 0)} goals`;
          setAgentActivity(
            `目标状态已更新${source ? ` · ${source}` : ""}${focus ? ` · ${truncate(focus, 36)}` : ""}${status ? ` · ${status}` : ""} · ${counts}`,
          );
        }
        break;
      case "todo_state_updated":
        {
          const source = formatTodoStateSource(String(event.source || ""));
          const active = Number(event.active_count || 0);
          const total = Number(event.total || 0);
          const preview = Array.isArray(event.active_preview)
            ? event.active_preview.map(String).filter(Boolean)[0]
            : "";
          setAgentActivity(
            `任务列表已更新${source ? ` · ${source}` : ""} · active ${active}/${total}${preview ? ` · ${truncate(preview, 44)}` : ""}`,
          );
        }
        break;
      case "solve_trace_updated":
        {
          const source = formatSolveTraceSource(String(event.source || ""));
          const kind = String(event.entry_kind || "");
          const status = String(event.status || "");
          const steps = Number(event.step_count || 0);
          const decisions = Number(event.decision_count || 0);
          const preview = String(event.action_preview || event.observation_preview || "");
          setAgentActivity(
            `求解轨迹已更新${source ? ` · ${source}` : ""}${kind ? ` · ${kind}` : ""}${status ? ` · ${status}` : ""} · steps ${steps} · decisions ${decisions}${preview ? ` · ${truncate(preview, 40)}` : ""}`,
          );
        }
        break;
      case "assistant_message":
        setAgentActivity("正在准备");
        finalizeAssistantMessage(String(event.content || ""));
        break;
      case "tool_batch_started":
        {
          sealActiveAssistantSegment();
          setAgentActivity(`正在并发执行 ${String(event.total_calls || 0)} 个工具`);
          const batch = {
          batchId: String(event.batch_id || `parallel-${envelope.seq}`),
          iteration: Number(event.iteration || 0),
          totalCalls: Number(event.total_calls || 0),
          completedCalls: 0,
          status: "running",
          } as const;
          setParallelBatch(batch);
          upsertBatchTimeline(batch);
        }
        break;
      case "tool_batch_progress":
        sealActiveAssistantSegment();
        setAgentActivity(
          `工具批次 ${String(event.completed_calls || 0)}/${String(event.total_calls || 0)}`,
        );
        setParallelBatch((prev) => {
          const batch = {
            batchId: String(event.batch_id || prev?.batchId || `parallel-${envelope.seq}`),
            iteration: Number(event.iteration || prev?.iteration || 0),
            totalCalls: Number(event.total_calls || prev?.totalCalls || 0),
            completedCalls: Number(event.completed_calls || 0),
            status: "running" as const,
          };
          upsertBatchTimeline(batch);
          return batch;
        });
        break;
      case "tool_batch_finished":
        {
          sealActiveAssistantSegment();
          const status = String(event.status || "");
          const durationMs = readDurationMs(event.duration_ms);
          setAgentActivity(
            status === "awaiting_approval"
              ? "等待审批"
              : status === "completed_with_errors"
                ? "工具批次完成，有失败"
                : "工具批次已完成",
          );
          const batch = {
            batchId: String(event.batch_id || `parallel-${envelope.seq}`),
            iteration: Number(event.iteration || 0),
            totalCalls: Number(event.total_calls || 0),
            completedCalls: Number(event.completed_calls || 0),
            durationMs,
            status:
              status === "awaiting_approval"
                ? "awaiting_approval"
                : status === "canceled"
                  ? "canceled"
                  : status === "completed_with_errors"
                    ? "completed_with_errors"
                    : "completed",
          } as const;
          setParallelBatch(batch);
          upsertBatchTimeline(batch);
        }
        break;
      case "tool_call_started":
        {
          sealActiveAssistantSegment();
          const id = String(event.tool_call_id || `${envelope.seq}-${String(event.tool_name || "tool")}`);
          const name = String(event.tool_name || "tool");
          setAgentActivity(`正在执行 ${name}`);
          if (isBrowserTool(name)) {
            revealBrowserViewer();
          }
          const detail = String(event.arguments_preview || "");
          const executionMode = String(event.execution_mode || "");
          const batchId = typeof event.batch_id === "string" ? event.batch_id : null;
          const batchIndex = typeof event.batch_index === "number" ? event.batch_index : null;
          const batchTotal = typeof event.batch_total === "number" ? event.batch_total : null;
          setTools((prev) => [
            {
              id,
              name,
              phase: "running" as const,
              detail,
              executionMode,
              batchId,
            },
            ...prev,
          ].slice(0, 40));
          setTimeline((prev) => {
            const index = prev.findIndex((entry) => entry.id === id && entry.type === "tool");
            const toolEntry: TimelineEntry = {
              id,
              type: "tool",
              name,
              detail,
              commandPreview: resolveToolCommandPreview(detail, detail),
              phase: "running",
              executionMode,
              batchId,
              batchIndex,
              batchTotal,
            };
            if (index === -1) {
              return [...prev, toolEntry];
            }
            const next = [...prev];
            next[index] = toolEntry;
            return next;
          });
        }
        break;
      case "tool_call_delta":
        sealActiveAssistantSegment();
        if (isBrowserTool(String(event.tool_name || ""))) {
          revealBrowserViewer({ loadStream: false });
        }
        markToolStreaming(
          String(event.tool_call_id || `${envelope.seq}-${String(event.tool_name || "tool")}`),
          String(event.tool_name || "tool"),
          String(event.detail_preview || ""),
          String(event.execution_mode || ""),
          typeof event.batch_id === "string" ? event.batch_id : null,
          typeof event.batch_index === "number" ? event.batch_index : null,
          typeof event.batch_total === "number" ? event.batch_total : null,
        );
        break;
      case "tool_call_finished":
        sealActiveAssistantSegment();
        if (isBrowserTool(String(event.tool_name || ""))) {
          revealBrowserViewer({ loadStream: false });
          requestBrowserUiSync(isNavigationBrowserTool(String(event.tool_name || "")));
        }
        const toolPhase = String(event.status || "") === "error" ? "error" : "done";
        const durationMs = readDurationMs(event.duration_ms);
        setAgentActivity(toolPhase === "error" ? "工具执行失败" : "工具执行完成");
        markToolFinished(
          String(event.tool_call_id || `${envelope.seq}-${String(event.tool_name || "tool")}`),
          String(event.tool_name || "tool"),
          String(event.output_preview || ""),
          String(event.execution_mode || ""),
          typeof event.batch_id === "string" ? event.batch_id : null,
          typeof event.batch_index === "number" ? event.batch_index : null,
          typeof event.batch_total === "number" ? event.batch_total : null,
          undefined,
          toolPhase,
          durationMs,
        );
        void loadApprovals();
        if (String(event.tool_name || "") === "delegate_task") {
          void loadDelegateRuns(String(event.session_id || currentSessionIdRef.current || ""));
        }
        if (String(event.tool_name || "") === "cron_manage") {
          void loadExtensionsOverview();
          void loadCronSchedulerStatus();
        }
        break;
      case "approval_required":
        sealActiveAssistantSegment();
        setAgentActivity("等待审批");
        markToolApprovalPending(
          String(event.tool_call_id || `${envelope.seq}-${String(event.tool_name || "tool")}`),
          String(event.tool_name || "tool"),
          String(event.reason || ""),
          String(event.command || ""),
          String(event.execution_mode || ""),
          typeof event.batch_id === "string" ? event.batch_id : null,
          typeof event.batch_index === "number" ? event.batch_index : null,
          typeof event.batch_total === "number" ? event.batch_total : null,
          typeof event.approval_id === "string" ? event.approval_id : undefined,
        );
        pushNotice(
          "approval",
          `${String(event.tool_name || "tool")} 需要审批: ${String(event.reason || "")}`,
        );
        void loadApprovals();
        break;
      case "approval_resolved":
        {
          const approved = Boolean(event.approved);
          const toolName = String(event.tool_name || "tool");
          sealActiveAssistantSegment();
          setAgentActivity(approved ? "审批已批准" : "审批已拒绝");
          pushNotice(
            approved ? "success" : "nudge",
            `${toolName} 审批${approved ? "已批准" : "已拒绝"}`,
          );
          void loadApprovals();
        }
        break;
      case "session_saved":
        {
          const turnId = String(event.turn_id || "");
          const historyCount = Number(event.history_count || 0);
          const timelineCount = Number(event.timeline_count || 0);
          const pendingApprovals = Number(event.pending_approval_count || 0);
          const continuation = event.has_response_continuation ? " · response continuation" : "";
          const pending = pendingApprovals > 0 ? ` · ${pendingApprovals} 个审批待处理` : "";
          setAgentActivity(
            `检查点已保存${turnId ? ` · ${turnId}` : ""} · history ${historyCount} · timeline ${timelineCount}${pending}${continuation}`,
          );
        }
        break;
      case "skill_lifecycle_suggested":
        pushNotice(
          "skill",
          `${String(event.action || "")} ${String(event.category || "")}/${String(event.name || "")}: ${String(event.reason || "")}`,
        );
        break;
      case "skill_matched":
        pushNotice("skill", `命中 skills: ${Array.isArray(event.skills) ? event.skills.join(", ") : ""}`);
        break;
      case "nudge":
        pushNotice("nudge", String(event.message || ""));
        break;
      case "error":
        setAgentActivity("正在准备");
        setRunIdle();
        discardPendingUserMessage();
        if (!String(event.message || "").includes("stop requested for current session")) {
          pushNotice("error", String(event.message || ""));
        }
        break;
      default:
        break;
    }
  }

  useEffect(() => {
    if (!isClient) {
      return;
    }

    let cancelled = false;

    async function bootstrap() {
      try {
        const info = await invoke<DesktopInfo>("desktop_info");
        if (cancelled) {
          return;
        }
        setDesktopInfo(info);
        const initialConfig: Preferences = {
          ...configRef.current,
          workspaceRoot: configRef.current.workspaceRoot.trim() || info.current_working_dir || "",
          sessionId: configRef.current.sessionId || info.last_session_id || "",
        };
        const hydratedConfig = await loadSharedProviderConfig(initialConfig);
        setConfig(hydratedConfig);

        clearGlobalListeners();
        const listeners = [
          await listen("hermes://agent/event", (event) => {
            handleEnvelope(event.payload as AgentEventEnvelope);
          }),
          await listen("hermes://agent/done", async (event) => {
            const payload = event.payload as { session_id: string };
            await Promise.all([refreshSessions(), loadWorkspaceTree(true)]);
            if (!shouldHandleGlobalSession(payload.session_id)) {
              return;
            }
            setRunIdle();
            setCurrentSessionId(payload.session_id);
            setConfig((prev) => ({ ...prev, sessionId: payload.session_id }));
            pushNotice("success", `会话 ${payload.session_id} 已完成`);
            await loadDelegateRuns(payload.session_id);
          }),
          await listen("hermes://agent/cleared", async (event) => {
            const payload = event.payload as { ok?: boolean; session_id?: string };
            if (payload && payload.ok) {
              setTimeline([]);
              setTools([]);
              setNotices([]);
              pushNotice("success", `已清空会话 ${payload.session_id || ""}`);
              await refreshSessions();
            }
          }),
        ];
        if (cancelled) {
          for (const unlisten of listeners) {
            unlisten();
          }
          return;
        }
        globalListenerRef.current = listeners;

        await refreshSessions(hydratedConfig);
        await loadProviders(hydratedConfig);
        const approvals = await loadApprovals(hydratedConfig);
        if (hydratedConfig.sessionId) {
          await loadSessionIntoView(hydratedConfig.sessionId, {
            silent: true,
            configOverride: hydratedConfig,
            approvalsOverride: approvals,
          });
        } else {
          await loadDelegateRuns(null, hydratedConfig);
        }
        await loadCronSchedulerStatus();
      } catch (error) {
        setBootError(formatError(error));
      }
    }

    void bootstrap();

    return () => {
      cancelled = true;
      clearGlobalListeners();
      clearSessionListeners();
    };
  }, [isClient]);

  useEffect(() => {
    if (activeView === "skills" && skills.length === 0) {
      void loadSkills();
    }
  }, [activeView, skills.length]);

  useEffect(() => {
    if (activeView !== "skills" || skills.length === 0) {
      return;
    }

    const currentKey = skillDetail ? skillKey(skillDetail) : null;
    const selectedExists = selectedSkillKey
      ? skills.some((skill) => skillKey(skill) === selectedSkillKey)
      : false;
    if (selectedExists && currentKey === selectedSkillKey) {
      return;
    }

    const target =
      skills.find((skill) => selectedSkillKey && skillKey(skill) === selectedSkillKey) || skills[0];
    if (!target) {
      return;
    }
    void loadSkillDetail(target.category, target.name);
  }, [activeView, selectedSkillKey, skillDetail, skills]);

  useEffect(() => {
    if (activeView !== "settings") {
      return;
    }
    void loadProviders();
  }, [activeView]);

  useEffect(() => {
    if (activeView !== "settings") {
      return;
    }
    const timer = window.setTimeout(() => {
      void loadProviderRuntimeStatus();
    }, 150);
    return () => window.clearTimeout(timer);
  }, [
    activeView,
    config.workspaceRoot,
    config.dataDir,
    config.provider,
    config.model,
    config.baseUrl,
    config.apiKey,
  ]);

  useEffect(() => {
    if (activeView === "activity") {
      void loadExtensionsOverview();
      void loadCronSchedulerStatus();
    }
  }, [activeView, currentSessionId]);

  useEffect(() => {
    if (!cronSchedulerStatus?.running) {
      return;
    }
    const timer = window.setInterval(() => {
      void loadCronSchedulerStatus();
      void loadExtensionsOverview();
      void refreshSessions();
    }, 5000);
    return () => window.clearInterval(timer);
  }, [cronSchedulerStatus?.running]);

  useEffect(() => {
    if (!isClient || cronSchedulerStatus?.running || cronSchedulerBusy) {
      return;
    }
    const workspaceRoot = config.workspaceRoot.trim();
    if (!workspaceRoot) {
      return;
    }
    const autostartKey = `${workspaceRoot}::${resolveDataDir(config) || ""}`;
    if (cronAutoStartedRef.current === autostartKey) {
      return;
    }
    cronAutoStartedRef.current = autostartKey;
    void startCronScheduler({ silent: true });
  }, [
    config.workspaceRoot,
    config.dataDir,
    cronSchedulerBusy,
    cronSchedulerStatus?.running,
    isClient,
  ]);

  useEffect(() => {
    if (!queuedRedirectRef.current || !running || stopping) {
      return;
    }
    const sessionId = currentSessionId || config.sessionId;
    if (!sessionId) {
      return;
    }
    void onStopSession();
  }, [config.sessionId, currentSessionId, running, stopping]);

  async function maybeFlushQueuedRedirect(trigger: "completed" | "stopped" | "failed" | "awaiting_approval") {
    if (trigger === "awaiting_approval") {
      if (queuedRedirectRef.current) {
        pushNotice("approval", "下一条指令已排队，需先处理当前审批。");
      }
      return;
    }
    if (flushingQueuedRedirectRef.current) {
      return;
    }
    const queued = queuedRedirectRef.current;
    if (!queued) {
      return;
    }
    flushingQueuedRedirectRef.current = true;
    clearQueuedRedirect();
    try {
      await executePromptRun(queued.prompt, queued.sessionId);
    } finally {
      flushingQueuedRedirectRef.current = false;
    }
  }

  async function executePromptRun(prompt: string, sessionIdOverride?: string | null) {
    const workspaceRoot = configRef.current.workspaceRoot.trim();
    if (!workspaceRoot) {
      pushNotice("error", "请先填写 workspace root");
      return;
    }

    try {
      inflightPromptSignatureRef.current = promptSignature(prompt);
      pendingRunSessionIdRef.current =
        (sessionIdOverride ?? currentSessionIdRef.current ?? configRef.current.sessionId).trim() || null;
      setRunActive();
      setConfig((prev) => ({ ...prev, prompt: "" }));
      appendOptimisticUserMessage(prompt);
      const result = await invoke<BridgeRunResult>("run_agent", {
        request: {
          prompt,
          provider: emptyToNull(configRef.current.provider),
          model: emptyToNull(configRef.current.model),
          auxModel: emptyToNull(configRef.current.smallModel),
          baseUrl: emptyToNull(configRef.current.baseUrl),
          apiKey: emptyToNull(configRef.current.apiKey),
          workspaceRoot,
          dataDir: emptyToNull(configRef.current.dataDir),
          sessionId: emptyToNull(sessionIdOverride ?? configRef.current.sessionId),
          maxIterations: Number(configRef.current.maxIterations || 12),
          systemPromptOverride: null,
          enableShellTool: configRef.current.enableShellTool,
        },
      });
      setCurrentSessionId(result.session_id);
      setConfig((prev) => ({ ...prev, sessionId: result.session_id }));
      await attachSessionListeners(result.session_id);
      await Promise.all([refreshSessions(), loadWorkspaceTree(true)]);
      if (result.status === "awaiting_approval") {
        setRunIdle();
        await loadApprovals();
        pushNotice("approval", "当前会话正在等待审批，处理后可以继续执行。");
        await maybeFlushQueuedRedirect("awaiting_approval");
        return;
      }
      setRunIdle();
      await maybeFlushQueuedRedirect("completed");
    } catch (error) {
      setRunIdle();
      const promptStillPending = pendingUserMessageRef.current?.signature === promptSignature(prompt);
      discardPendingUserMessage();
      const message = formatError(error);
      const stopRequested = message.includes("stop requested for current session");
      if (!stopRequested) {
        if (promptStillPending) {
          setConfig((prev) => (prev.prompt.trim() ? prev : { ...prev, prompt }));
        }
        pushNotice("error", message);
        await maybeFlushQueuedRedirect("failed");
        return;
      }
      pushNotice("nudge", "已中断当前步骤，准备执行最新指令。");
      await maybeFlushQueuedRedirect("stopped");
    }
  }

  async function queuePromptRedirect(prompt: string) {
    const sessionId = currentSessionId || configRef.current.sessionId || null;
    const replaced = Boolean(queuedRedirectRef.current);
    setQueuedRedirect(prompt, sessionId);
    setConfig((prev) => ({ ...prev, prompt: "" }));
    pushNotice(
      "nudge",
      replaced
        ? "已更新排队中的下一条指令，当前步骤结束后会执行最新内容。"
        : "已记录下一条指令，当前步骤收尾后会自动继续。",
    );
    if (sessionId && !stopping) {
      await onStopSession();
    }
  }

  async function onSubmitPrompt(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (desktopShellUnavailable) {
      pushNotice("error", "当前不是 Electron/Tauri 桌面壳，无法提交需要 Rust bridge 的会话请求。");
      return;
    }
    const prompt = (promptTextareaRef.current?.value ?? configRef.current.prompt).trim();
    const signature = promptSignature(prompt);
    const workspaceRoot = configRef.current.workspaceRoot.trim();
    if (!prompt) {
      pushNotice("error", "请输入 prompt");
      return;
    }
    if (!workspaceRoot) {
      pushNotice("error", "请先填写 workspace root");
      return;
    }

    const staleBusy = runBusyRef.current && !running && !stopping;
    if (staleBusy) {
      if (inflightPromptSignatureRef.current === signature) {
        return;
      }
      setRunIdle();
    }

    if (!staleBusy && (runBusyRef.current || running || stopping)) {
      await queuePromptRedirect(prompt);
      return;
    }

    await executePromptRun(prompt);
  }

  function onPromptKeyDown(event: React.KeyboardEvent<HTMLTextAreaElement>) {
    if (event.key !== "Enter" || event.shiftKey) {
      return;
    }
    if (event.nativeEvent.isComposing || event.nativeEvent.keyCode === 229) {
      return;
    }
    event.preventDefault();
    promptFormRef.current?.requestSubmit();
  }

  function onPromptImageClick() {
    pushNotice("nudge", "当前桌面端还未接通图片输入，暂不支持随 prompt 一起发送图片。");
  }

  async function onClearSession() {
    if (!config.sessionId) {
      pushNotice("error", "当前没有 session_id");
      return;
    }
    try {
      await invoke("clear_session", {
        request: {
          workspaceRoot: config.workspaceRoot.trim(),
          dataDir: emptyToNull(config.dataDir),
          sessionId: config.sessionId.trim(),
          provider: emptyToNull(config.provider),
          model: emptyToNull(config.model),
          auxModel: emptyToNull(config.smallModel),
          baseUrl: emptyToNull(config.baseUrl),
          apiKey: emptyToNull(config.apiKey),
          maxIterations: Number(config.maxIterations || 12),
          systemPromptOverride: null,
          enableShellTool: config.enableShellTool,
        },
      });
    } catch (error) {
      pushNotice("error", formatError(error));
    }
  }

  async function onStopSession() {
    const dataDir = resolveDataDir(configRef.current);
    const sessionId = currentSessionId || configRef.current.sessionId;
    if (!dataDir || !sessionId) {
      pushNotice("error", "当前没有可停止的会话");
      return;
    }
    if (stopping) {
      return;
    }
    try {
      setStopping(true);
      await invoke("stop_session", { dataDir, sessionId });
      pushNotice("nudge", `已请求停止会话 ${sessionId}，等待当前步骤收尾。`);
    } catch (error) {
      setStopping(false);
      pushNotice("error", formatError(error));
    }
  }

  async function switchWorkspaceRoot(
    workspaceRoot: string,
    options?: {
      preserveActiveView?: boolean;
      resetDataDir?: boolean;
      dataDir?: string;
      notice?: string;
    },
  ) {
    const nextConfig = {
      ...configRef.current,
      workspaceRoot,
      dataDir: options?.dataDir ?? (options?.resetDataDir ? "" : configRef.current.dataDir),
      sessionId: "",
      prompt: "",
    };

    const hydratedConfig = await loadSharedProviderConfig(nextConfig);

    if (!options?.preserveActiveView) {
      setActiveView("conversation");
    }
    onClearViewState();
    setConfig(hydratedConfig);
    await Promise.all([
      refreshSessions(hydratedConfig),
      loadProviders(hydratedConfig),
      loadDelegateRuns(null, hydratedConfig),
    ]);
    pushNotice("nudge", options?.notice || `已切换到 ${workspaceRoot}`);
  }

  async function promptForWorkspaceRoot(options?: {
    preserveActiveView?: boolean;
    resetDataDir?: boolean;
    dataDir?: string;
    notice?: string;
  }) {
    if (desktopShellUnavailable) {
      pushNotice("error", "当前不是 Electron/Tauri 桌面壳，无法调用原生目录选择器。");
      return;
    }
    try {
      const selectedFolder = await invoke<string | null>("pick_workspace_folder", {
        currentDir: emptyToNull(configRef.current.workspaceRoot),
      });
      if (!selectedFolder) {
        return;
      }
      await switchWorkspaceRoot(selectedFolder, {
        preserveActiveView: options?.preserveActiveView,
        resetDataDir: options?.resetDataDir ?? true,
        dataDir: options?.dataDir,
        notice: options?.notice || `已切换 workspace 到 ${selectedFolder}`,
      });
    } catch (error) {
      pushNotice("error", formatError(error));
    }
  }

  async function prepareFreshThread() {
    const workspaceRoot = configRef.current.workspaceRoot.trim();
    if (!workspaceRoot) {
      await promptForWorkspaceRoot({
        resetDataDir: true,
        notice: "已选择 workspace，下一次发送会创建新的会话。",
      });
      return;
    }

    const nextConfig = {
      ...configRef.current,
      sessionId: "",
      prompt: "",
    };

    setActiveView("conversation");
    onClearViewState();
    setConfig(nextConfig);
    await Promise.all([refreshSessions(nextConfig), loadDelegateRuns(null, nextConfig)]);
    pushNotice("nudge", "已准备新会话，下一次发送会在当前 workspace 创建新的 session。");
  }

  async function onNewThread() {
    try {
      await prepareFreshThread();
    } catch (error) {
      pushNotice("error", formatError(error));
    }
  }

  function onClearViewState() {
    clearSessionListeners();
    pendingUserMessageRef.current = null;
    clearQueuedRedirect();
    autoFollowTranscriptRef.current = true;
    pendingSessionScrollToLatestRef.current = false;
    lastTranscriptSnapshotRef.current = { sessionId: null, changeToken: null };
    setRunIdle();
    setCurrentSessionId(null);
    resetActiveAssistantStreamState();
    setParallelBatch(null);
    setTimeline([]);
    updateExpandedToolEntryIds([]);
    updateAutoExpandedToolEntryIds([]);
    updateSuppressedAutoExpandedToolEntryIds([]);
    setShowJumpToLatest(false);
    setPendingTranscriptUpdates(0);
    setFileViewerOpen(false);
    setFileViewerLoading(false);
    setFileViewerError(null);
    setFileViewerTabs([]);
    setActiveFileViewerTabPath(null);
    setEventLog([]);
    setTools([]);
    setNotices([]);
    setSkills([]);
    setSkillDetail(null);
    setSelectedSkillKey(null);
    setMcpInspection(null);
    setApprovalRequests([]);
    setDelegateRuns([]);
  }

  async function loadSessionIntoView(sessionId: string, options?: SessionLoadOptions) {
    const candidate = options?.configOverride || configRef.current;
    const dataDir = resolveDataDir(candidate);
    if (!dataDir) {
      pushNotice("error", "请先填写 workspace root 或 data dir");
      return;
    }

    try {
      const keepBrowserViewerOpen = fileViewerOpen && sharedViewerMode === "browser";
      const detail = await invoke<BridgeSessionDetail | null>("load_session", {
        dataDir,
        sessionId,
      });
      if (!detail) {
        pushNotice("error", `会话 ${sessionId} 不存在`);
        return;
      }
      setCurrentSessionId(detail.summary.session_id);
      pendingUserMessageRef.current = null;
      clearQueuedRedirect();
      autoFollowTranscriptRef.current = true;
      pendingSessionScrollToLatestRef.current = true;
      lastTranscriptSnapshotRef.current = { sessionId: detail.summary.session_id, changeToken: null };
      setRunIdle();
      resetActiveAssistantStreamState();
      setParallelBatch(null);
      updateExpandedToolEntryIds([]);
      updateAutoExpandedToolEntryIds([]);
      updateSuppressedAutoExpandedToolEntryIds([]);
      setShowJumpToLatest(false);
      setPendingTranscriptUpdates(0);
      setFileViewerOpen(keepBrowserViewerOpen);
      setFileViewerLoading(false);
      setFileViewerError(null);
      setFileViewerTabs([]);
      setActiveFileViewerTabPath(null);
      const restoredTimeline =
        detail.timeline && detail.timeline.length
          ? storedTimelineToTimeline(detail.timeline)
          : historyToTimeline(detail.history);
      const restoredRunningToolCount = restoredTimeline.filter(
        (entry) => entry.type === "tool" && entry.phase === "running",
      ).length;
      const restoredRunningBatchCount = restoredTimeline.filter(
        (entry) => entry.type === "batch" && entry.status === "running",
      ).length;
      const approvals = options?.approvalsOverride || (await loadApprovals(candidate));
      const pendingApprovalCount = approvals.filter(
        (item) => item.status === "pending" && item.session_id === detail.summary.session_id,
      ).length;
      setTimeline(restoredTimeline);
      setTools(
        detail.timeline && detail.timeline.length
          ? timelineToTools(restoredTimeline)
          : historyToTools(detail.history),
      );
      setEventLog([]);
      setConfig((prev) => ({
        ...prev,
        workspaceRoot: candidate.workspaceRoot || prev.workspaceRoot,
        dataDir: candidate.dataDir,
        sessionId: detail.summary.session_id,
        model: detail.summary.model || prev.model,
      }));
      await attachSessionListeners(detail.summary.session_id);
      await loadDelegateRuns(detail.summary.session_id, candidate);
      if (!options?.silent) {
        pushNotice("success", `已加载会话 ${detail.summary.session_id}`);
      }
      const recoveredSummary = formatInterruptedSessionSummary(
        restoredRunningToolCount,
        restoredRunningBatchCount,
        pendingApprovalCount,
      );
      if (recoveredSummary) {
        pushNotice(
          pendingApprovalCount ? "approval" : "nudge",
          `已恢复会话 ${detail.summary.session_id}，${recoveredSummary}。`,
        );
      }
    } catch (error) {
      pushNotice("error", formatError(error));
    }
  }

  const dataDir = resolveDataDir(config);
  const isElectronShell = isClient && (isElectronDesktop() || desktopInfo?.shell === "electron");
  const isTauriShell = isClient && desktopInfo?.shell === "tauri";
  const windowDragRegionProps = isElectronShell
    ? ({ "data-electron-drag-region": true } as const)
    : isTauriShell
      ? ({ "data-tauri-drag-region": true } as const)
      : {};
  const activeWorkspaceKey = workspaceEntryKey(config.workspaceRoot, config.dataDir);
  const desktopShellUnavailable = Boolean(
    bootError?.includes("Desktop APIs are unavailable outside the Electron/Tauri shell."),
  );
  const desktopShellStatusMessage = desktopShellUnavailable
    ? "当前页面运行在浏览器预览里，不是 Electron/Tauri 桌面壳；所有 workspace、session、provider、审批和 Rust bridge 调用都会失败。请通过桌面壳启动后再测试。"
    : bootError;
  const workspaceStatusLabel = !config.workspaceRoot.trim()
    ? "尚未选择 workspace"
    : currentSessionId
      ? ""
      : "准备创建新会话";
  const pendingApprovals = approvalRequests.filter((item) => item.status === "pending");
  const currentSessionPendingApprovals = pendingApprovals.filter(
    (item) => item.session_id === currentSessionId,
  );
  const currentSessionPendingApprovalCount = currentSessionPendingApprovals.length;
  const pendingApprovalCount = pendingApprovals.length;
  const otherPendingApprovalCount = Math.max(pendingApprovalCount - currentSessionPendingApprovalCount, 0);
  const pendingParallelApproval =
    parallelBatch
      ? null
      : currentSessionPendingApprovals.find(
          (item) =>
            typeof item.batch_id === "string" &&
            item.batch_id.length > 0 &&
            typeof item.batch_total === "number" &&
            item.batch_total > 0,
        ) || null;
  const visibleParallelBatch =
    parallelBatch ||
    (pendingParallelApproval
      ? {
          batchId: String(pendingParallelApproval.batch_id),
          iteration: 0,
          totalCalls: Number(pendingParallelApproval.batch_total || 0),
          completedCalls: Math.max(Number(pendingParallelApproval.batch_index || 1) - 1, 0),
          status: "awaiting_approval" as const,
        }
      : null);
  const derivedPendingApprovalEntries = pendingApprovals
    .filter((item) => item.session_id === currentSessionId)
    .filter((item) => !timeline.some((entry) => entry.id === `approval-${item.id}`))
    .map(
      (item) =>
        ({
          id: `approval-${item.id}`,
          type: "approval",
          approvalId: item.id,
          toolName: item.tool_name || "tool",
          reason: item.reason,
          command: item.command,
          executionMode: item.execution_mode || undefined,
          batchId: item.batch_id || null,
          batchIndex: item.batch_index ?? null,
          batchTotal: item.batch_total ?? null,
        }) satisfies TimelineEntry,
    );
  const conversationTimeline = [...timeline, ...derivedPendingApprovalEntries];
  const runningTerminalEntries = conversationTimeline.filter(
    (entry): entry is Extract<TimelineEntry, { type: "tool" }> =>
      entry.type === "tool" && entry.phase === "running" && entry.name === "terminal",
  );
  const transcriptTimeline = conversationTimeline.filter(
    (entry) =>
      entry.type !== "tool" || entry.phase !== "running" || entry.name !== "terminal",
  );
  const conversationBlocks = buildConversationBlocks(transcriptTimeline);
  const restoredRunningToolCount = conversationTimeline.filter(
    (entry) => entry.type === "tool" && entry.phase === "running",
  ).length;
  const restoredRunningBatchCount = conversationTimeline.filter(
    (entry) => entry.type === "batch" && entry.status === "running",
  ).length;
  const interruptedSessionSummary =
    !running && !stopping
      ? formatInterruptedSessionSummary(
          restoredRunningToolCount,
          restoredRunningBatchCount,
          currentSessionPendingApprovalCount,
        )
      : "";
  const title = documentTitle(conversationTimeline, currentSessionId);
  const currentSessionSummary = sessions.find((session) => session.session_id === currentSessionId) || null;
  const currentTitle = currentSessionSummary?.title || title;
  const conversationTitleKey = currentSessionId || "__new__";
  const lockedConversationTitle =
    conversationTitleLock?.key === conversationTitleKey ? conversationTitleLock.title : currentTitle;
  const defaultProvider = providers.find((item) => item.is_default) || null;
  const selectedProvider = providers.find((item) => item.id === config.provider) || defaultProvider;
  const effectiveProvider = providerRuntimeStatus || null;
  const explicitApiKey = config.apiKey.trim();
  const resolvedAuthSource = explicitApiKey
    ? "request"
    : effectiveProvider?.auth_source || selectedProvider?.auth_source || null;
  const providerReady = effectiveProvider ? effectiveProvider.ready : Boolean(explicitApiKey || resolvedAuthSource);
  const resolvedModel = effectiveProvider?.model || config.model.trim() || selectedProvider?.model || "未设置";
  const resolvedSmallModel = config.smallModel.trim() || "跟随主模型";
  const resolvedBaseUrl =
    effectiveProvider?.base_url || config.baseUrl.trim() || selectedProvider?.base_url || "未设置";
  const providerStatusLabel = providerRuntimeLoading
    ? "解析中"
    : providerReady
      ? "已就绪"
      : effectiveProvider
        ? "待授权"
        : selectedProvider
          ? "待配置"
          : "未解析";
  const showCodexAuthHint = (effectiveProvider?.kind || selectedProvider?.kind) === "openai-codex" && !providerReady;
  const showGenericAuthHint = Boolean((effectiveProvider || selectedProvider) && !providerReady && !showCodexAuthHint);
  const runningToolCount = tools.filter((entry) => entry.phase === "running").length;
  const conversationState = stopping
    ? "正在停止"
    : running
      ? "正在执行"
      : currentSessionPendingApprovalCount
        ? "等待审批"
        : interruptedSessionSummary
          ? "上次中断"
        : "空闲";
  const conversationStateVariant = stopping
    ? "secondary"
    : running
      ? "soft"
      : currentSessionPendingApprovalCount || interruptedSessionSummary
        ? "secondary"
        : "outline";
  const showConversationState = conversationState !== "空闲";
  const showAssistantLoading = (running || stopping) && !activeAssistantMessageId && runningToolCount === 0;
  const jumpToLatestLabel = pendingTranscriptUpdates
    ? `跳到最新 · ${formatPendingTranscriptUpdates(pendingTranscriptUpdates)}`
    : "跳到最新";
  const browserSessionId = currentSessionId || config.sessionId.trim() || "slidev-preview";
  const promptModelLabel = config.smallModel.trim()
    ? `${config.model} · bg ${config.smallModel.trim()}`
    : config.model;
  const browserViewerAvailable = Boolean(
    dataDir && (isElectronShell ? browserSessionId : currentSessionId || config.sessionId.trim()),
  );
  const contextDebugAvailable = Boolean(
    isElectronShell && dataDir && (currentSessionId || config.sessionId.trim()),
  );
  const activeFileViewerTab =
    fileViewerTabs.find((tab) => tab.path === activeFileViewerTabPath) || null;
  const viewerPanelVisible = activeView === "conversation" && fileViewerOpen;
  const fileViewerVisible = viewerPanelVisible && sharedViewerMode === "file";
  const browserViewerVisible = viewerPanelVisible && sharedViewerMode === "browser";
  const settingsViewVisible = activeView === "settings";
  const topBarTitle =
    activeView === "conversation"
      ? lockedConversationTitle
      : activeView === "skills"
        ? "技能和应用"
        : activeView === "activity"
          ? "定时任务"
          : "设置";
  const conversationColumnWidthClass = "max-w-[1040px]";
  const showFileViewerToggle =
    activeView === "conversation" &&
    (
      fileViewerOpen ||
      Boolean(activeFileViewerTab) ||
      Boolean(config.workspaceRoot.trim()) ||
      browserViewerAvailable
    );

  useEffect(() => {
    const candidate = (currentTitle || "新会话").trim() || "新会话";
    setConversationTitleLock((prev) => {
      if (!prev || prev.key !== conversationTitleKey) {
        return { key: conversationTitleKey, title: candidate };
      }
      const previous = prev.title.trim();
      const previousIsPlaceholder =
        !previous || previous === "新会话" || Boolean(currentSessionId && previous === currentSessionId);
      const candidateIsConcrete =
        candidate !== "新会话" && (!currentSessionId || candidate !== currentSessionId);
      if (previousIsPlaceholder && candidateIsConcrete) {
        return { key: conversationTitleKey, title: candidate };
      }
      return prev;
    });
  }, [conversationTitleKey, currentSessionId, currentTitle]);

  function revealBrowserViewer(options?: { loadStream?: boolean }) {
    setActiveView("conversation");
    setFileViewerOpen(true);
    setSharedViewerMode("browser");
    shrinkConversationPanelForFilePreview();
    if (!wideFileViewerLayout) {
      setNarrowFileTreeVisible(false);
    }
    if (!isElectronShell && options?.loadStream !== false) {
      void loadBrowserStreamEndpoint(false);
    }
  }

  function requestBrowserUiSync(forceNavigate = false) {
    setBrowserUiSyncRequest((prev) => ({
      token: prev.token + 1,
      forceNavigate,
    }));
  }

  async function loadWorkspaceTree(force = false): Promise<WorkspaceTreeResponse | null> {
    const workspaceRoot = configRef.current.workspaceRoot.trim();
    if (!workspaceRoot) {
      setWorkspaceTree(null);
      setWorkspaceTreeError(null);
      return null;
    }
    if (!force && workspaceTree && workspaceTree.rootPath === workspaceRoot) {
      return workspaceTree;
    }

    setWorkspaceTreeLoading(true);
    setWorkspaceTreeError(null);
    try {
      const tree = await invoke<WorkspaceTreeResponse>("list_workspace_tree", {
        workspaceRoot,
      });
      setWorkspaceTree(tree);
      if (activeFileViewerTabPath) {
        const ancestors = findWorkspaceTreeAncestorDirectories(tree.nodes, activeFileViewerTabPath);
        if (ancestors.length) {
          setWorkspaceTreeExpandedDirectories((prev) => mergeUniquePaths(prev, ancestors));
        }
      }
      return tree;
    } catch (error) {
      setWorkspaceTreeError(formatError(error));
      return null;
    } finally {
      setWorkspaceTreeLoading(false);
    }
  }

  async function refreshWorkspaceTree() {
    await loadWorkspaceTree(true);
  }

  function shrinkConversationPanelForFilePreview() {
    const layout = conversationLayoutRef.current;
    if (!layout) {
      return;
    }

    const totalWidth = layout.getBoundingClientRect().width;
    if (!totalWidth) {
      return;
    }

    const nextWideLayout = totalWidth >= 1500;
    const centerMinWidth = 420;
    const outerHandleWidth = 20;
    const minOuterPreviewWidth = nextWideLayout ? 720 : 480;
    const maxPreviewWidth = Math.max(
      minOuterPreviewWidth,
      totalWidth - centerMinWidth - outerHandleWidth,
    );

    setWideFileViewerLayout(nextWideLayout);
    setFilePreviewWidth(maxPreviewWidth);

    if (nextWideLayout) {
      const minTreeWidth = 200;
      const maxTreeWidth = Math.max(minTreeWidth, Math.min(320, maxPreviewWidth - 432));
      setFileTreeWidth((prev) => Math.min(prev, maxTreeWidth));
    }
  }

  async function openWorkspaceFilePreview(path: string) {
    const workspaceRoot = configRef.current.workspaceRoot.trim();
    const dataDir = resolveDataDir(configRef.current);
    if (!workspaceRoot) {
      pushNotice("error", "请先填写 workspace root");
      return;
    }
    const existingTab =
      fileViewerTabs.find((tab) => workspaceTabMatchesPath(tab, path, workspaceRoot)) || null;
    if (existingTab) {
      setFileViewerOpen(true);
      setSharedViewerMode("file");
      setActiveFileViewerTabPath(existingTab.path);
      if (!wideFileViewerLayout) {
        setNarrowFileTreeVisible(false);
      }
      shrinkConversationPanelForFilePreview();
      setFileViewerError(null);
      setFileViewerLoading(false);
      if (workspaceTree) {
        const ancestors = findWorkspaceTreeAncestorDirectories(workspaceTree.nodes, existingTab.path);
        if (ancestors.length) {
          setWorkspaceTreeExpandedDirectories((prev) => mergeUniquePaths(prev, ancestors));
        }
      }
      return;
    }
    setFileViewerLoading(true);
    setFileViewerError(null);
    try {
      const preview = await invoke<WorkspaceFilePreview>("view_workspace_file", {
        workspaceRoot,
        dataDir,
        filePath: path,
      });
      if (shouldOpenWorkspaceFileExternally(preview)) {
        await invoke("open_workspace_file", {
          workspaceRoot,
          filePath: path,
        });
        pushNotice("success", `已使用系统软件打开 ${preview.fileName}`);
        setFileViewerError(null);
        return;
      }
      setFileViewerOpen(true);
      setSharedViewerMode("file");
      shrinkConversationPanelForFilePreview();
      setFileViewerTabs((prev) => {
        const nextTabs = prev.filter((tab) => tab.path !== preview.path);
        return [...nextTabs, preview];
      });
      setActiveFileViewerTabPath(preview.path);
      if (!wideFileViewerLayout) {
        setNarrowFileTreeVisible(false);
      }
      if (workspaceTree) {
        const ancestors = findWorkspaceTreeAncestorDirectories(workspaceTree.nodes, preview.path);
        if (ancestors.length) {
          setWorkspaceTreeExpandedDirectories((prev) => mergeUniquePaths(prev, ancestors));
        }
      }
    } catch (error) {
      setFileViewerError(formatError(error));
    } finally {
      setFileViewerLoading(false);
    }
  }

  async function openLatestContextDebugSnapshot() {
    const dataDir = resolveDataDir(configRef.current);
    const sessionId = currentSessionIdRef.current?.trim() || configRef.current.sessionId.trim();
    if (!dataDir || !sessionId) {
      pushNotice("error", "当前没有可查看上下文快照的会话。");
      return;
    }
    try {
      const result = await invoke<LatestContextDebugSnapshot>("latest_context_debug_snapshot", {
        dataDir,
        sessionId,
      });
      if (!result.path) {
        pushNotice(
          "nudge",
          `当前会话还没有上下文快照。请先用 HERMES_RS_DEBUG_CONTEXT=1 跑一轮对话，再查看 ${result.debugDir}。`,
        );
        return;
      }
      await openWorkspaceFilePreview(result.path);
      pushNotice("success", "已打开当前会话最新的上下文快照。");
    } catch (error) {
      console.error(error);
      pushNotice("error", error instanceof Error ? error.message : "打开上下文快照失败");
    }
  }

  async function copyAssistantMessage(entryId: string, content: string) {
    try {
      await navigator.clipboard.writeText(content);
      setCopiedAssistantEntryId(entryId);
      pushNotice("success", "已复制助手消息");
      window.setTimeout(() => {
        setCopiedAssistantEntryId((current) => (current === entryId ? null : current));
      }, 1800);
    } catch (error) {
      pushNotice("error", `复制失败：${formatError(error)}`);
    }
  }

  function openMarkdownPreviewLink(file: WorkspaceFilePreview, href: string) {
    const trimmed = href.trim();
    if (!trimmed || trimmed.startsWith("#")) {
      return;
    }

    if (/^https?:\/\//i.test(trimmed)) {
      setBrowserDirectUrlRequest((prev) => ({
        token: (prev?.token || 0) + 1,
        title: trimmed,
        url: trimmed,
      }));
      revealBrowserViewer({ loadStream: false });
      return;
    }

    const resolved = resolveMarkdownWorkspacePath(
      trimmed,
      file.path,
      configRef.current.workspaceRoot.trim(),
    );
    if (resolved) {
      void openWorkspaceFilePreview(resolved);
      return;
    }

    if (typeof window !== "undefined") {
      window.open(trimmed, "_blank", "noopener,noreferrer");
    }
  }

  async function openSlidevPreview(file: WorkspaceFilePreview) {
    if (!isElectronShell) {
      pushNotice("error", "Slidev 预览需要 Electron 浏览器内核");
      return;
    }
    const workspaceRoot = configRef.current.workspaceRoot.trim();
    if (!workspaceRoot) {
      pushNotice("error", "请先填写 workspace root");
      return;
    }

    setFileViewerLoading(true);
    setFileViewerError(null);
    try {
      const preview = await invoke<SlidevPreviewResponse>("preview_slidev_deck", {
        workspaceRoot,
        filePath: file.path,
      });
      setBrowserDirectUrlRequest((prev) => ({
        token: (prev?.token || 0) + 1,
        title: `Slidev · ${preview.fileName}`,
        url: preview.url,
      }));
      revealBrowserViewer({ loadStream: false });
      pushNotice("success", `已打开 Slidev 预览：${preview.displayPath}`);
    } catch (error) {
      setFileViewerError(formatError(error));
    } finally {
      setFileViewerLoading(false);
    }
  }

  async function exportSlidevDeck(file: WorkspaceFilePreview, format: "pdf" | "pptx") {
    if (!isElectronShell) {
      pushNotice("error", "Slidev 导出需要 Electron 浏览器内核");
      return;
    }
    const workspaceRoot = configRef.current.workspaceRoot.trim();
    if (!workspaceRoot) {
      pushNotice("error", "请先填写 workspace root");
      return;
    }

    setFileViewerLoading(true);
    setFileViewerError(null);
    try {
      const result = await invoke<SlidevExportResponse>("export_slidev_deck", {
        workspaceRoot,
        filePath: file.path,
        format,
      });
      pushNotice(
        "success",
        result.format === "pptx" && result.experimental
          ? `已导出实验性截图型 PPTX：${result.displayPath}`
          : `已导出 ${result.format.toUpperCase()}：${result.displayPath}`,
      );
      await openWorkspaceFilePreview(result.displayPath);
    } catch (error) {
      setFileViewerError(formatError(error));
    } finally {
      setFileViewerLoading(false);
    }
  }

  async function loadBrowserStreamEndpoint(showLoading = true) {
    if (isElectronDesktop()) {
      setBrowserStreamLoading(false);
      setBrowserStreamError(null);
      setBrowserStreamEndpoint(null);
      return;
    }

    const dataDir = resolveDataDir(configRef.current);
    const workspaceRoot = configRef.current.workspaceRoot.trim();
    const sessionId = currentSessionIdRef.current?.trim() || configRef.current.sessionId.trim();
    if (!dataDir || !workspaceRoot || !sessionId) {
      setBrowserStreamLoading(false);
      setBrowserStreamError(null);
      setBrowserStreamEndpoint(null);
      return;
    }

    if (showLoading) {
      setBrowserStreamLoading(true);
    }
    setBrowserStreamError(null);
    try {
      const endpoint = await invoke<BrowserStreamEndpoint>("browser_stream_endpoint", {
        workspaceRoot,
        dataDir,
        sessionId,
      });
      setBrowserStreamEndpoint(endpoint);
    } catch (error) {
      setBrowserStreamError(formatError(error));
    } finally {
      setBrowserStreamLoading(false);
    }
  }

  function retryBrowserStream() {
    setBrowserStreamRetryToken((prev) => prev + 1);
    void loadBrowserStreamEndpoint();
  }

  async function toggleFileViewer(nextOpen?: boolean) {
    const shouldOpen = nextOpen ?? !fileViewerOpen;
    if (!shouldOpen) {
      setFileViewerOpen(false);
      return;
    }

    setFileViewerOpen(true);
    if (sharedViewerMode === "file" && !wideFileViewerLayout) {
      setNarrowFileTreeVisible(true);
    }
    if (sharedViewerMode === "browser") {
      if (!isElectronShell) {
        await loadBrowserStreamEndpoint();
      }
      return;
    }
    if (!workspaceTreeLoading) {
      await loadWorkspaceTree(true);
    }
  }

  async function openSharedViewer(mode: SharedViewerMode) {
    setSharedViewerMode(mode);
    setFileViewerOpen(true);
    shrinkConversationPanelForFilePreview();

    if (mode === "browser") {
      if (!wideFileViewerLayout) {
        setNarrowFileTreeVisible(false);
      }
      if (!isElectronShell) {
        await loadBrowserStreamEndpoint();
      }
      return;
    }

    if (!wideFileViewerLayout) {
      setNarrowFileTreeVisible(true);
    }
    if (!workspaceTreeLoading) {
      await loadWorkspaceTree(true);
    }
  }

  async function onTopBarMouseDown(event: ReactMouseEvent<HTMLElement>) {
    if (event.button !== 0) {
      return;
    }
    const target = event.target as HTMLElement | null;
    if (
      target?.closest(
        "button, a, input, textarea, select, option, summary, details, [role='button'], [data-no-drag]",
      )
    ) {
      return;
    }
    try {
      await getCurrentWindow().startDragging();
    } catch (error) {
      console.warn("failed to start window drag", error);
    }
  }

  useEffect(() => {
    if (
      !fileViewerOpen ||
      sharedViewerMode !== "file" ||
      !config.workspaceRoot.trim() ||
      workspaceTree ||
      workspaceTreeLoading
    ) {
      return;
    }
    void loadWorkspaceTree();
  }, [fileViewerOpen, sharedViewerMode, config.workspaceRoot, workspaceTree, workspaceTreeLoading]);

  useEffect(() => {
    if (!workspaceTree || !activeFileViewerTabPath) {
      return;
    }
    const ancestors = findWorkspaceTreeAncestorDirectories(workspaceTree.nodes, activeFileViewerTabPath);
    if (!ancestors.length) {
      return;
    }
    setWorkspaceTreeExpandedDirectories((prev) => mergeUniquePaths(prev, ancestors));
  }, [workspaceTree, activeFileViewerTabPath]);

  useEffect(() => {
    if (!browserViewerVisible || isElectronShell) {
      return;
    }
    void loadBrowserStreamEndpoint();
  }, [
    browserViewerVisible,
    currentSessionId,
    config.sessionId,
    config.dataDir,
    config.workspaceRoot,
    isElectronShell,
  ]);

  useEffect(() => {
    if (!viewerPanelVisible) {
      return;
    }
    const layout = conversationLayoutRef.current;
    if (!layout || typeof ResizeObserver === "undefined") {
      return;
    }

    const observer = new ResizeObserver((entries) => {
      const width = entries[0]?.contentRect.width || 0;
      const nextWideLayout = width >= 1500;
      setWideFileViewerLayout(nextWideLayout);

      const centerMinWidth = 420;
      const outerHandleWidth = 20;
      const minOuterPreviewWidth = nextWideLayout ? 720 : 480;
      const maxPreviewWidth = Math.max(
        minOuterPreviewWidth,
        width - centerMinWidth - outerHandleWidth,
      );
      setFilePreviewWidth((prev) => Math.min(prev, maxPreviewWidth));

      if (nextWideLayout) {
        const minTreeWidth = 200;
        const maxTreeWidth = Math.max(minTreeWidth, Math.min(320, maxPreviewWidth - 432));
        setFileTreeWidth((prev) => Math.min(prev, maxTreeWidth));
      }
    });

    observer.observe(layout);
    return () => observer.disconnect();
  }, [viewerPanelVisible]);

  useEffect(() => {
    if (!conversationPanelResize) {
      return;
    }
    const drag = conversationPanelResize;

    function onMouseMove(event: MouseEvent) {
      const layout = conversationLayoutRef.current;
      if (!layout) {
        return;
      }

      const totalWidth = layout.getBoundingClientRect().width;
      const centerMinWidth = 420;
      const outerHandleWidth = 20;
      const innerHandleWidth = 20;
      const minTreeWidth = 200;
      const minPreviewWidth = wideFileViewerLayout ? 720 : 480;
      const maxTreeWidth = Math.max(
        minTreeWidth,
        filePreviewWidthRef.current - minPreviewWidth - innerHandleWidth,
      );
      const maxPreviewWidth = Math.max(
        minPreviewWidth,
        totalWidth - centerMinWidth - outerHandleWidth,
      );
      const delta = event.clientX - drag.startX;

      if (drag.panel === "tree") {
        const nextWidth = Math.min(maxTreeWidth, Math.max(minTreeWidth, drag.startWidth + delta));
        setFileTreeWidth(nextWidth);
        return;
      }

      const nextWidth = Math.min(maxPreviewWidth, Math.max(minPreviewWidth, drag.startWidth - delta));
      setFilePreviewWidth(nextWidth);
    }

    function onMouseUp() {
      setConversationPanelResize(null);
    }

    const previousCursor = document.body.style.cursor;
    const previousUserSelect = document.body.style.userSelect;
    document.body.style.cursor = "col-resize";
    document.body.style.userSelect = "none";
    window.addEventListener("mousemove", onMouseMove);
    window.addEventListener("mouseup", onMouseUp);

    return () => {
      document.body.style.cursor = previousCursor;
      document.body.style.userSelect = previousUserSelect;
      window.removeEventListener("mousemove", onMouseMove);
      window.removeEventListener("mouseup", onMouseUp);
    };
  }, [conversationPanelResize, wideFileViewerLayout]);

  useEffect(() => {
    if (activeView !== "conversation" || !transcriptRef.current) {
      return;
    }

    const latestEntry = transcriptTimeline[transcriptTimeline.length - 1];
    const latestSignature =
      latestEntry?.type === "tool"
        ? `${latestEntry.id}:${latestEntry.phase}:${latestEntry.detail.length}`
        : latestEntry?.type === "assistant"
          ? `${latestEntry.id}:${latestEntry.streaming ? "streaming" : "stable"}:${latestEntry.content.length}`
          : latestEntry?.type === "approval"
            ? `${latestEntry.id}:${latestEntry.reason.length}:${latestEntry.command.length}`
            : latestEntry?.type === "batch"
              ? `${latestEntry.id}:${latestEntry.status}:${latestEntry.completedCalls}/${latestEntry.totalCalls}`
              : latestEntry
                ? `${latestEntry.id}:${latestEntry.pending ? "pending" : "stable"}:${latestEntry.content.length}`
                : "empty";
    const changeToken = `${transcriptTimeline.length}:${latestSignature}`;
    const previous = lastTranscriptSnapshotRef.current;
    const sessionChanged = previous.sessionId !== currentSessionId;
    const changed = previous.changeToken !== changeToken;

    lastTranscriptSnapshotRef.current = {
      sessionId: currentSessionId,
      changeToken,
    };

    if (!changed && !sessionChanged) {
      return;
    }

    const shouldForceLatest = sessionChanged || pendingSessionScrollToLatestRef.current;
    if (shouldForceLatest || autoFollowTranscriptRef.current) {
      if (shouldForceLatest) {
        pendingSessionScrollToLatestRef.current = false;
      }
      scheduleTranscriptScrollToBottom(shouldForceLatest ? "auto" : "smooth", shouldForceLatest ? 4 : 1);
      return;
    }

    setShowJumpToLatest(true);
    setPendingTranscriptUpdates((prev) => Math.min(prev + 1, 999));
  }, [activeView, currentSessionId, transcriptTimeline]);

  return (
    <main className="flex h-screen min-w-[1280px] flex-col overflow-hidden bg-[linear-gradient(180deg,#f5f6f8_0%,#eef1f4_100%)] text-slate-900">
      {desktopShellStatusMessage ? (
        <div className="pointer-events-none absolute inset-x-0 top-2 z-50 flex justify-center px-4">
          <div className="pointer-events-auto max-w-[1040px] rounded-xl border border-amber-200 bg-amber-50/95 px-3 py-2 text-[13px] leading-[1.5] text-amber-900 shadow-[0_12px_32px_rgba(15,23,42,0.08)]">
            {desktopShellStatusMessage}
          </div>
        </div>
      ) : null}
      <div
        className={`grid min-h-0 w-full flex-1 grid-cols-1 ${
          sidebarCollapsed ? "xl:grid-cols-[88px_minmax(0,1fr)]" : "xl:grid-cols-[248px_minmax(0,1fr)]"
        }`}
      >
        <aside className="flex min-h-0 flex-col border-b border-slate-200/80 bg-[rgba(248,249,252,0.96)] px-2.5 pb-3 pt-11 xl:border-r xl:border-b-0">
          <nav className="grid gap-0.5">
            <Button
              variant="ghost"
              className={`h-8 w-full justify-start gap-2 rounded-md px-2 text-[14px] font-medium ${
                activeView === "conversation"
                  ? "bg-slate-200/70 text-slate-950 hover:bg-slate-200 hover:text-slate-950"
                  : "text-slate-600 hover:bg-slate-100/70 hover:text-slate-900"
              } ${sidebarCollapsed ? "xl:justify-center xl:px-0" : ""}`}
              onClick={() => setActiveView("conversation")}
              title="对话"
            >
              <Bot className="size-4" />
              <span className={sidebarCollapsed ? "xl:hidden" : ""}>对话</span>
            </Button>
            <Button
              variant="ghost"
              className={`h-8 w-full justify-start gap-2 rounded-md px-2 text-[14px] font-medium ${
                activeView === "skills"
                  ? "bg-slate-200/70 text-slate-950 hover:bg-slate-200 hover:text-slate-950"
                  : "text-slate-600 hover:bg-slate-100/70 hover:text-slate-900"
              } ${
                sidebarCollapsed ? "xl:justify-center xl:px-0" : ""
              }`}
              onClick={() => {
                setActiveView("skills");
                void loadSkills();
              }}
              title="技能和应用"
            >
              <Sparkles className="size-4" />
              <span className={sidebarCollapsed ? "xl:hidden" : ""}>技能和应用</span>
            </Button>
            <Button
              variant="ghost"
              className={`h-8 w-full justify-start gap-2 rounded-md px-2 text-[14px] font-medium ${
                activeView === "activity"
                  ? "bg-slate-200/70 text-slate-950 hover:bg-slate-200 hover:text-slate-950"
                  : "text-slate-600 hover:bg-slate-100/70 hover:text-slate-900"
              } ${
                sidebarCollapsed ? "xl:justify-center xl:px-0" : ""
              }`}
              onClick={() => {
                setActiveView("activity");
                void loadExtensionsOverview();
                void loadCronSchedulerStatus();
              }}
              title="定时任务"
            >
              <Wrench className="size-4" />
              <span className={sidebarCollapsed ? "xl:hidden" : ""}>定时任务</span>
            </Button>
          </nav>

          <Separator className={`my-3 ${sidebarCollapsed ? "xl:hidden" : ""}`} />

          <div className={`min-h-0 flex-1 ${sidebarCollapsed ? "xl:hidden" : ""}`}>
            <WorkspaceSessionTree
              workspaces={workspaceList}
              activeWorkspaceKey={activeWorkspaceKey}
              currentSessionId={currentSessionId}
              workspaceStatusLabel={workspaceStatusLabel}
              onAddWorkspace={() => {
                void promptForWorkspaceRoot({
                  notice: "已切换 workspace，左侧列表已刷新。",
                });
              }}
              onToggleWorkspace={toggleWorkspaceEntry}
              onSelectWorkspace={(workspace) => {
                void selectWorkspaceEntry(workspace);
              }}
              onSelectSession={(workspace, session) => {
                setActiveView("conversation");
                void loadSessionIntoView(session.session_id, {
                  configOverride: {
                    ...configRef.current,
                    workspaceRoot: workspace.workspaceRoot,
                    dataDir: workspace.dataDir,
                  },
                });
              }}
            />
          </div>

          <div className="mt-auto border-t border-slate-200/80 pt-4">
            <div className="flex items-center gap-2">
              <Button
                variant="ghost"
                className={`h-9 min-w-0 flex-1 justify-start gap-2 rounded-md px-2 ${
                  activeView === "settings"
                    ? "bg-slate-200/70 text-slate-950 hover:bg-slate-200 hover:text-slate-950"
                    : "text-slate-600 hover:bg-slate-100/70 hover:text-slate-900"
                } ${sidebarCollapsed ? "xl:justify-center xl:px-0" : ""}`}
                onClick={() => setActiveView("settings")}
                title="设置"
              >
                <Settings2 className="size-4" />
                <span className={sidebarCollapsed ? "xl:hidden" : ""}>设置</span>
              </Button>
              <Button
                type="button"
                variant="ghost"
                size="icon"
                className="hidden h-9 w-9 shrink-0 rounded-md text-slate-500 hover:bg-slate-100/70 hover:text-slate-900 xl:inline-flex"
                onClick={() => setSidebarCollapsed((prev) => !prev)}
                title={sidebarCollapsed ? "展开侧栏" : "折叠侧栏"}
              >
                {sidebarCollapsed ? <ChevronRight className="size-4" /> : <ChevronLeft className="size-4" />}
              </Button>
            </div>
          </div>
        </aside>

        <section className="relative flex min-h-0 min-w-0 flex-col">
            <AppTopBar
              dragRegionProps={windowDragRegionProps}
              isMacElectron={isElectronShell && desktopInfo?.platform === "darwin"}
            viewerPanelVisible={viewerPanelVisible}
            title={topBarTitle}
            showConversationMeta={activeView === "conversation"}
            showConversationState={showConversationState}
            conversationState={conversationState}
            conversationStateVariant={conversationStateVariant}
            runningToolCount={runningToolCount}
            pendingApprovalCount={currentSessionPendingApprovalCount}
            queuedRedirectPrompt={queuedRedirectPrompt}
            showViewerToggle={showFileViewerToggle}
            sharedViewerMode={sharedViewerMode}
            workspaceConfigured={Boolean(config.workspaceRoot.trim())}
            browserViewerAvailable={browserViewerAvailable}
            fileViewerOpen={fileViewerOpen}
            showContextDebugButton={contextDebugAvailable}
            onOpenContextDebug={() => {
              void openLatestContextDebugSnapshot();
            }}
            onMouseDown={(event) => {
              void onTopBarMouseDown(event);
            }}
            onOpenFileViewer={() => {
              void openSharedViewer("file");
            }}
            onOpenBrowserViewer={() => {
              void openSharedViewer("browser");
            }}
            onToggleFileViewer={() => {
              void toggleFileViewer();
            }}
          />
          <NoticeStack notices={notices.slice(0, 3)} onDismiss={dismissNotice} />
          {activeView === "conversation" ? (
            <div
              ref={conversationLayoutRef}
              className={`grid min-h-0 flex-1 gap-3 px-3 pb-3 pt-0 xl:px-4 ${
                viewerPanelVisible
                  ? "xl:grid-cols-[minmax(420px,1fr)_12px_var(--file-preview-width)]"
                  : "grid-cols-1"
              }`}
              style={
                viewerPanelVisible
                  ? ({
                      ["--file-preview-width" as string]: `${filePreviewWidth}px`,
                    } as CSSProperties)
                  : undefined
              }
            >
              <div className="grid min-h-0 min-w-0 grid-rows-[minmax(0,1fr)_auto] gap-0 overflow-hidden">
                <article className="min-h-0 min-w-0 overflow-hidden">
                  <div
                    className={`mx-auto flex h-full min-h-0 min-w-0 w-full flex-col ${conversationColumnWidthClass}`}
                  >
                    <div className="relative min-h-0 flex-1">
                      <div
                        ref={transcriptRef}
                        data-no-drag
                        onScroll={onTranscriptScroll}
                        className="scrollbar-none flex h-full min-h-0 flex-col gap-3 overflow-auto px-1 pb-3 pr-2 pt-1"
                      >
                        {interruptedSessionSummary ? (
                          <div className="rounded-2xl border border-amber-200/90 bg-[linear-gradient(180deg,rgba(255,250,239,0.96)_0%,rgba(255,244,221,0.92)_100%)] px-4 py-3 shadow-[0_10px_24px_rgba(15,23,42,0.05)]">
                            <div className="flex flex-wrap items-center gap-1.5 text-[11px] font-medium uppercase tracking-[0.14em] text-amber-700">
                              <LoaderCircle className="size-3.5" />
                              恢复提示
                              {currentSessionSummary?.updated_at_unix ? (
                                <span className="text-amber-700/70">
                                  最后更新 {formatCompactTimestamp(currentSessionSummary.updated_at_unix)}
                                </span>
                              ) : null}
                            </div>
                            <p className="mt-2 text-[13px] leading-[1.5] text-slate-700">
                              这个会话上次退出时还没有完整收尾。已恢复历史时间线；这些运行中标记是上次留下的快照，你可以继续发送新指令，或先处理当前审批。
                            </p>
                            <p className="mt-1.5 text-[13px] leading-[1.5] text-slate-700">{interruptedSessionSummary}。</p>
                          </div>
                        ) : null}
                        {conversationBlocks.length ? (
                          conversationBlocks.map((block) => {
                            if (block.type === "user") {
                              const entry = block.entry;
                              return (
                                <article key={block.id} className="flex w-full min-w-0 justify-end">
                                  <div className="min-w-0 w-fit max-w-full">
                                    <div
                                      className={`min-w-0 overflow-hidden rounded-2xl border px-3.5 py-2.5 ${
                                        entry.pending
                                          ? "border-slate-200/90 bg-slate-100/80 opacity-80"
                                          : "border-slate-200/90 bg-slate-100/95"
                                      }`}
                                    >
                                      <MarkdownContent
                                        content={entry.content}
                                        className="text-[13px] leading-[1.5] text-slate-700"
                                        workspaceRoot={config.workspaceRoot}
                                        onOpenWorkspaceFile={(path) => {
                                          void openWorkspaceFilePreview(path);
                                        }}
                                      />
                                    </div>
                                  </div>
                                </article>
                              );
                            }

                            return (
                              <article key={block.id} className="min-w-0 w-full max-w-full self-start">
                                <div className="max-w-full space-y-1.5 py-0.5 pr-2">
                                  {block.entries.map((entry) => {
                                    if (entry.type === "assistant") {
                                      return (
                                        <div key={entry.id} className="group relative rounded-xl px-2 py-1.5 hover:bg-slate-50/80">
                                          <div className="absolute right-1 top-1 opacity-0 transition-opacity group-hover:opacity-100">
                                            <Button
                                              type="button"
                                              variant="ghost"
                                              size="sm"
                                              className="h-7 gap-1.5 rounded-md px-2 text-[11px] text-slate-500 hover:text-slate-800"
                                              onClick={() => {
                                                void copyAssistantMessage(entry.id, entry.content);
                                              }}
                                              title="复制这条助手消息"
                                            >
                                              {copiedAssistantEntryId === entry.id ? (
                                                <>
                                                  <Check className="size-3.5" />
                                                  已复制
                                                </>
                                              ) : (
                                                <>
                                                  <Copy className="size-3.5" />
                                                  复制
                                                </>
                                              )}
                                            </Button>
                                          </div>
                                          <MarkdownContent
                                            content={entry.content}
                                            className="pr-14 text-[13px] leading-[1.5] text-slate-700"
                                            workspaceRoot={config.workspaceRoot}
                                            onOpenWorkspaceFile={(path) => {
                                              void openWorkspaceFilePreview(path);
                                            }}
                                          />
                                        </div>
                                      );
                                    }

                                    if (entry.type === "tool") {
                                      const toolEntryExpanded =
                                        expandedToolEntryIds.includes(entry.id) ||
                                        autoExpandedToolEntryIds.includes(entry.id);
                                      const toolDetailSections = splitToolDetail(entry.detail);
                                      const toolCommandLine = resolveToolCommandPreview(
                                        entry.commandPreview,
                                        entry.detail,
                                      );
                                      const toolSummary = summarizeToolDetail(entry.detail);

                                      return (
                                        <div key={entry.id}>
                                          <button
                                            type="button"
                                            onClick={() => toggleToolEntryExpanded(entry.id)}
                                            aria-expanded={toolEntryExpanded}
                                            className="flex w-full items-center gap-1.5 px-0 py-0.5 text-left transition-colors hover:text-slate-900 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-slate-300"
                                          >
                                            <span className="inline-flex shrink-0 items-center gap-1 text-[11px] font-medium text-slate-500">
                                              <Wrench className="size-3.5" />
                                              工具
                                            </span>
                                            <span className="min-w-0 flex-1 truncate text-[12px] text-slate-600">
                                              <span className="font-medium text-slate-900">{entry.name}</span>
                                              <span className="mx-2 text-slate-300">·</span>
                                              <span className="text-slate-500">
                                                {entry.phase === "running"
                                                  ? "运行中"
                                                  : entry.phase === "approval"
                                                    ? "待审批"
                                                    : entry.phase === "error"
                                                      ? "失败"
                                                      : "已完成"}
                                              </span>
                                              {entry.executionMode ? (
                                                <>
                                                  <span className="mx-1.5 text-slate-300">·</span>
                                                  <span className="uppercase tracking-[0.08em] text-slate-400">
                                                    {entry.executionMode}
                                                  </span>
                                                </>
                                              ) : null}
                                              {typeof entry.batchIndex === "number" &&
                                              typeof entry.batchTotal === "number" ? (
                                                <>
                                                  <span className="mx-1.5 text-slate-300">·</span>
                                                  <span className="text-slate-400">
                                                    {entry.batchIndex}/{entry.batchTotal}
                                                  </span>
                                                </>
                                              ) : null}
                                              {formatDurationMs(entry.durationMs) ? (
                                                <>
                                                  <span className="mx-1.5 text-slate-300">·</span>
                                                  <span className="text-slate-400">
                                                    {formatDurationMs(entry.durationMs)}
                                                  </span>
                                                </>
                                              ) : null}
                                              <span className="mx-1.5 text-slate-300">·</span>
                                              <span className="font-mono text-slate-600">
                                                {toolCommandLine || toolSummary || "查看详情"}
                                              </span>
                                            </span>
                                            <span className="inline-flex shrink-0 items-center text-slate-400">
                                              <ChevronDown
                                                className={`size-4 transition-transform ${
                                                  toolEntryExpanded ? "rotate-180" : ""
                                                }`}
                                              />
                                            </span>
                                          </button>
                                          {toolEntryExpanded ? (
                                            <div className="mt-1 space-y-1.5 pl-4">
                                              {toolDetailSections.meta ? (
                                                <p className="whitespace-pre-wrap break-words font-mono text-[11px] leading-5 text-slate-600">
                                                  {toolDetailSections.meta}
                                                </p>
                                              ) : null}
                                              {toolDetailSections.stdout ? (
                                                <div className="overflow-hidden">
                                                  <div className="py-0.5 text-[10px] font-medium uppercase tracking-[0.08em] text-slate-500">
                                                    stdout
                                                  </div>
                                                  <p className="whitespace-pre-wrap break-words font-mono text-[11px] leading-5 text-slate-700">
                                                    {toolDetailSections.stdout}
                                                  </p>
                                                </div>
                                              ) : null}
                                              {toolDetailSections.stderr ? (
                                                <div className="overflow-hidden">
                                                  <div className="py-0.5 text-[10px] font-medium uppercase tracking-[0.08em] text-slate-500">
                                                    stderr
                                                  </div>
                                                  <p className="whitespace-pre-wrap break-words font-mono text-[11px] leading-5 text-slate-700">
                                                    {toolDetailSections.stderr}
                                                  </p>
                                                </div>
                                              ) : null}
                                            </div>
                                          ) : null}
                                        </div>
                                      );
                                    }

                                    if (entry.type === "approval") {
                                      return (
                                        <div key={entry.id}>
                                          <div className="flex flex-wrap items-center gap-1.5 text-[13px] font-medium text-slate-800">
                                            <span>{entry.toolName}</span>
                                            <Badge variant="secondary">待审批</Badge>
                                          </div>
                                          <div className="mt-1.5 flex flex-wrap items-center gap-1.5 text-[11px] uppercase tracking-[0.08em] text-slate-400">
                                            {entry.executionMode ? <span>{entry.executionMode}</span> : null}
                                            {entry.batchId ? <span>{truncate(entry.batchId, 28)}</span> : null}
                                            {typeof entry.batchIndex === "number" &&
                                            typeof entry.batchTotal === "number" ? (
                                              <span>
                                                {entry.batchIndex}/{entry.batchTotal}
                                              </span>
                                            ) : null}
                                          </div>
                                          <p className="mt-2 text-[13px] leading-[1.5] text-slate-700">{entry.reason}</p>
                                          <p className="mt-2 whitespace-pre-wrap break-words rounded-lg bg-white/75 px-3 py-2 font-mono text-xs leading-5 text-slate-600">
                                            {entry.command}
                                          </p>
                                        </div>
                                      );
                                    }

                                    return (
                                      <div key={entry.id}>
                                        <div className="flex flex-wrap items-center gap-1.5">
                                          <div className="text-[13px] font-medium text-slate-800">{entry.batchId}</div>
                                          <Badge variant={entry.status === "running" ? "soft" : "outline"}>
                                            {formatParallelBatchStatus(entry.status)}
                                          </Badge>
                                        </div>
                                        <div className="mt-1.5 flex flex-wrap items-center gap-1.5 text-[11px] uppercase tracking-[0.08em] text-slate-400">
                                          <span>轮次 {entry.iteration}</span>
                                          <span>
                                            {entry.completedCalls}/{entry.totalCalls}
                                          </span>
                                          {formatDurationMs(entry.durationMs) ? (
                                            <span>{formatDurationMs(entry.durationMs)}</span>
                                          ) : null}
                                        </div>
                                        <div className="mt-2 h-1.5 overflow-hidden rounded-full bg-white/80">
                                          <div
                                            className="h-full rounded-full bg-slate-900 transition-[width] duration-300"
                                            style={{
                                              width: `${entry.totalCalls ? (entry.completedCalls / entry.totalCalls) * 100 : 0}%`,
                                            }}
                                          />
                                        </div>
                                      </div>
                                    );
                                  })}
                                </div>
                              </article>
                            );
                          })
                        ) : (
                          <DemoTimelineShowcase
                            onUsePrompt={() => {
                              setConfig((prev) => ({ ...prev, prompt: demoLaunchPrompt }));
                              window.setTimeout(() => {
                                promptTextareaRef.current?.focus();
                              }, 0);
                            }}
                          />
                        )}
                        {showAssistantLoading ? (
                          <article className="min-w-0 w-full max-w-full self-start">
                            <div className="max-w-full border-l border-slate-200/80 py-0.5 pl-3 pr-2">
                              <div className="inline-flex items-center gap-1.5 rounded-full bg-white/80 px-2.5 py-1 text-[13px] text-slate-500 shadow-[0_8px_24px_rgba(15,23,42,0.04)]">
                                <LoaderCircle className="size-3.5 animate-spin" />
                                <span>{stopping ? "正在停止" : agentActivity}</span>
                              </div>
                            </div>
                          </article>
                        ) : null}
                      </div>
                      {showJumpToLatest ? (
                        <div className="pointer-events-none absolute inset-x-0 bottom-4 flex justify-center px-4">
                          <Button
                            type="button"
                            size="sm"
                            className="pointer-events-auto h-9 rounded-full px-3.5 shadow-[0_16px_32px_rgba(15,23,42,0.16)]"
                            onClick={() => scrollTranscriptToBottom("smooth")}
                          >
                            <ChevronDown className="mr-1.5 size-4" />
                            {jumpToLatestLabel}
                          </Button>
                        </div>
                      ) : null}
                    </div>
                  </div>
                </article>

                <form
                  ref={promptFormRef}
                  onSubmit={onSubmitPrompt}
                  className={`mx-auto w-full overflow-hidden rounded-2xl border border-slate-200/80 bg-white/94 p-3 shadow-[0_12px_32px_rgba(15,23,42,0.06)] ${conversationColumnWidthClass}`}
                >
                {runningTerminalEntries.length ? (
                  <div className="mb-2 rounded-xl border border-slate-200/80 bg-slate-50/85 px-2.5 py-2">
                    <div className="space-y-1.5">
                      {runningTerminalEntries.map((entry) => {
                        const commandPreview = resolveToolCommandPreview(undefined, entry.detail);
                        return (
                          <div
                            key={entry.id}
                            className="flex items-center gap-2 rounded-lg border border-slate-200/80 bg-white/90 px-2.5 py-2"
                          >
                            <div className="inline-flex shrink-0 items-center gap-1 rounded-full bg-slate-100 px-2 py-1 text-[10px] font-medium uppercase tracking-[0.08em] text-slate-500">
                              <span className="size-1.5 rounded-full bg-emerald-500" />
                              terminal
                            </div>
                            <p
                              className="min-w-0 flex-1 truncate font-mono text-[12px] leading-5 text-slate-700"
                              title={commandPreview || "等待命令输出"}
                            >
                              {commandPreview || "等待命令输出"}
                            </p>
                            <Button
                              type="button"
                              size="icon"
                              variant="ghost"
                              className="h-7 w-7 shrink-0 rounded-full text-slate-500 hover:bg-slate-100 hover:text-slate-900"
                              disabled={stopping}
                              title={stopping ? "终止中" : "终止当前执行"}
                              onClick={() => {
                                void onStopSession();
                              }}
                            >
                              {stopping ? <LoaderCircle className="size-3.5 animate-spin" /> : <X className="size-3.5" />}
                            </Button>
                          </div>
                        );
                      })}
                    </div>
                  </div>
                ) : null}
                {pendingApprovals.length ? (
                  <div className="mb-2.5 space-y-1.5">
                    {pendingApprovals.map((item) => (
                      <div
                        key={item.id}
                        className="rounded-xl border border-orange-200 bg-orange-50/80 px-3 py-2.5"
                      >
                        <div className="mb-1 flex items-center justify-between gap-1.5">
                          <div className="text-xs font-medium uppercase tracking-[0.1em] text-orange-700">
                            待审批 · {item.session_id === currentSessionId ? "当前会话" : item.session_id}
                          </div>
                          <Badge variant="secondary">{item.status}</Badge>
                        </div>
                        {(item.tool_name || item.batch_id || item.execution_mode) ? (
                          <div className="mb-1.5 flex flex-wrap items-center gap-1.5 text-[11px] uppercase tracking-[0.08em] text-orange-700/80">
                            {item.tool_name ? <span>{item.tool_name}</span> : null}
                            {item.execution_mode ? <span>{item.execution_mode}</span> : null}
                            {item.batch_id ? (
                              <span>
                                {truncate(item.batch_id, 28)}
                                {typeof item.batch_index === "number" && typeof item.batch_total === "number"
                                  ? ` ${item.batch_index}/${item.batch_total}`
                                  : ""}
                              </span>
                            ) : null}
                          </div>
                        ) : null}
                        <div className="text-[13px] font-medium text-slate-800">{item.reason}</div>
                        <p className="mt-1.5 rounded-lg bg-white px-2.5 py-1.5 font-mono text-xs leading-5 text-slate-600">
                          {item.command}
                        </p>
                        <div className="mt-2 flex items-center gap-1.5">
                          {item.session_id !== currentSessionId ? (
                            <Button
                              type="button"
                              size="sm"
                              variant="outline"
                              className="rounded-lg"
                              onClick={() => void loadSessionIntoView(item.session_id)}
                            >
                              打开会话
                            </Button>
                          ) : null}
                          <Button
                            type="button"
                            size="sm"
                            className="rounded-lg"
                            onClick={() => void resolveApproval(item.id, true)}
                          >
                            批准
                          </Button>
                          <Button
                            type="button"
                            size="sm"
                            variant="outline"
                            className="rounded-lg"
                            onClick={() => void resolveApproval(item.id, false)}
                          >
                            拒绝
                          </Button>
                        </div>
                      </div>
                    ))}
                  </div>
                ) : null}
                {queuedRedirectPrompt ? (
                  <div className="mb-2.5 rounded-xl border border-sky-200 bg-sky-50/80 px-3 py-2.5">
                    <div className="flex items-start justify-between gap-2">
                      <div className="min-w-0">
                        <div className="text-xs font-medium uppercase tracking-[0.1em] text-sky-700">
                          下一条已排队
                        </div>
                        <p className="mt-1.5 whitespace-pre-wrap break-words text-[13px] leading-[1.5] text-slate-700">
                          {queuedRedirectPrompt}
                        </p>
                        <p className="mt-1.5 text-xs leading-5 text-sky-700/80">
                          当前步骤结束后会自动继续；再次发送会替换成最新指令。
                        </p>
                      </div>
                      <Button
                        type="button"
                        size="sm"
                        variant="ghost"
                        className="rounded-full px-2.5 text-slate-600 hover:bg-slate-100 hover:text-slate-900"
                        onClick={clearQueuedRedirect}
                      >
                        取消排队
                      </Button>
                    </div>
                  </div>
                ) : null}
                <Textarea
                  ref={promptTextareaRef}
                  value={config.prompt}
                  placeholder={running ? "继续输入，新消息会中断当前步骤并自动续跑" : "要求后续变更"}
                  className="min-h-[56px] resize-none rounded-none border-0 bg-transparent px-1 py-0 text-[14px] leading-[1.5] text-slate-800 shadow-none placeholder:text-slate-400 focus-visible:border-0 focus-visible:ring-0"
                  onChange={(event) => setConfig((prev) => ({ ...prev, prompt: event.target.value }))}
                  onKeyDown={onPromptKeyDown}
                />
                <div className="mt-2 flex items-center justify-between gap-1.5 border-t border-slate-200/80 pt-1.5">
                  <div className="flex items-center gap-1.5">
                    <Button
                      type="button"
                      variant="ghost"
                      size="icon"
                      className="h-8 w-8 rounded-full text-slate-500 hover:bg-slate-100 hover:text-slate-900"
                      onClick={onPromptImageClick}
                      title="图片输入暂未接通"
                    >
                      <ImageIcon className="size-4.5" />
                    </Button>
                    <button
                      type="button"
                      onClick={() => setActiveView("settings")}
                      className="inline-flex h-8 items-center gap-1.5 rounded-full px-2 text-[14px] text-slate-600 transition hover:bg-slate-100 hover:text-slate-900"
                    >
                      <span>{promptModelLabel}</span>
                      <ChevronDown className="size-4" />
                    </button>
                  </div>
                  <div className="flex items-center gap-1.5">
                    {running || stopping ? (
                      <Button
                        type="button"
                        size="sm"
                        variant="outline"
                        className="h-10 rounded-full px-4"
                        disabled={stopping}
                        onClick={() => {
                          void onStopSession();
                        }}
                      >
                        {stopping ? <LoaderCircle className="mr-1.5 size-3.5 animate-spin" /> : null}
                        {stopping ? "停止中" : "停止"}
                      </Button>
                    ) : null}
                    <Button
                      type="submit"
                      size="icon"
                      className={`h-11 w-11 rounded-full text-white shadow-[0_10px_24px_rgba(15,23,42,0.18)] ${
                        running || stopping
                          ? "bg-[rgb(74,108,247)] hover:bg-[rgb(63,92,216)]"
                          : "bg-slate-900 hover:bg-slate-800"
                      }`}
                      title={running || stopping ? "发送并接管当前步骤" : "发送"}
                    >
                      {running || stopping ? (
                        <LoaderCircle className="size-4 animate-spin" />
                      ) : (
                        <Send className="size-4" />
                      )}
                    </Button>
                  </div>
                </div>
              </form>
              </div>

              {viewerPanelVisible ? (
                <div
                  data-no-drag
                  className="relative hidden xl:flex cursor-col-resize items-stretch justify-center"
                  onMouseDown={(event) => beginConversationPanelResize("viewer", event)}
                >
                  <div className="h-full w-px bg-slate-200" />
                  <div className="absolute inset-y-0 left-1/2 w-3 -translate-x-1/2 rounded-full bg-transparent transition hover:bg-slate-300/40" />
                </div>
              ) : null}

              {viewerPanelVisible ? (
                <aside className="hidden min-h-0 xl:flex xl:flex-col">
                  <div className="flex h-full min-h-0 flex-col overflow-hidden rounded-[24px] border border-slate-200/80 bg-[rgba(249,250,252,0.92)] shadow-[0_12px_32px_rgba(15,23,42,0.05)]">
                    {sharedViewerMode === "file" ? (
                      <div
                        className={`min-h-0 flex-1 ${wideFileViewerLayout ? "grid" : "flex flex-col"}`}
                        style={
                          wideFileViewerLayout
                            ? ({
                                gridTemplateColumns: `${fileTreeWidth}px 12px minmax(0,1fr)`,
                              } as CSSProperties)
                            : undefined
                        }
                      >
                        {wideFileViewerLayout || narrowFileTreeVisible ? (
                          <div
                            className={`bg-[rgba(255,255,255,0.45)] ${
                              wideFileViewerLayout
                                ? "min-h-0 border-r border-slate-200/70"
                                : "max-h-[240px] shrink-0 border-b border-slate-200/70"
                            }`}
                          >
                            <div className={`scrollbar-none overflow-auto px-3 py-3 ${wideFileViewerLayout ? "h-full" : "h-[240px]"}`}>
                              <WorkspaceTreeSection
                                tree={workspaceTree}
                                loading={workspaceTreeLoading}
                                error={workspaceTreeError}
                                currentPath={activeFileViewerTabPath}
                                expandedDirectories={workspaceTreeExpandedDirectories}
                                onToggleDirectory={toggleWorkspaceTreeDirectory}
                                onSelectFile={(path) => {
                                  void openWorkspaceFilePreview(path);
                                }}
                                onRefresh={() => {
                                  void refreshWorkspaceTree();
                                }}
                              />
                            </div>
                          </div>
                        ) : null}

                        {wideFileViewerLayout ? (
                          <div
                            data-no-drag
                            className="relative flex cursor-col-resize items-stretch justify-center"
                            onMouseDown={(event) => beginConversationPanelResize("tree", event)}
                          >
                            <div className="h-full w-px bg-slate-200" />
                            <div className="absolute inset-y-0 left-1/2 w-3 -translate-x-1/2 rounded-full bg-transparent transition hover:bg-slate-300/40" />
                          </div>
                        ) : null}

                        <div className="flex min-h-0 flex-col">
                          <WorkspaceFileTabsBar
                            tabs={fileViewerTabs}
                            activePath={activeFileViewerTabPath}
                            onSelect={selectWorkspaceFileTab}
                            onClose={closeWorkspaceFileTab}
                          />

                          <div className="min-h-[320px] flex-1 p-3">
                            <WorkspaceFilePreviewPane
                              file={activeFileViewerTab}
                              loading={fileViewerLoading}
                              error={fileViewerError}
                              workspaceRoot={config.workspaceRoot}
                              onOpenMarkdownLink={openMarkdownPreviewLink}
                              onOpenSlidevPreview={isElectronShell ? openSlidevPreview : undefined}
                              onExportSlidevDeck={isElectronShell ? exportSlidevDeck : undefined}
                            />
                          </div>
                        </div>
                      </div>
                    ) : (
                      <div className="min-h-0 flex-1 p-3">
                        {isElectronShell ? (
                          <ElectronBrowserPane
                            dataDir={dataDir}
                            sessionId={browserSessionId || null}
                            syncRequest={browserUiSyncRequest}
                            directUrlRequest={browserDirectUrlRequest}
                          />
                        ) : (
                          <AgentBrowserLivePane
                            endpoint={browserStreamEndpoint}
                            endpointLoading={browserStreamLoading}
                            endpointError={browserStreamError}
                            retryToken={browserStreamRetryToken}
                            onRetry={retryBrowserStream}
                          />
                        )}
                      </div>
                    )}
                  </div>
                </aside>
              ) : null}
            </div>
          ) : null}

          {activeView === "skills" ? (
            <section className="min-h-0 overflow-auto p-4">
              <div className="grid gap-4 xl:grid-cols-[320px_minmax(0,1fr)]">
                <div className="min-h-0 rounded-2xl border border-slate-200/70 bg-white/78 p-4 shadow-[0_12px_36px_rgba(15,23,42,0.05)]">
                  <header className="mb-4">
                    <div className="mb-3 flex items-center gap-2">
                      <Badge variant="soft">Skills</Badge>
                      <Badge variant="outline">{skills.length}</Badge>
                    </div>
                    <h2 className="text-[28px] font-semibold tracking-[-0.04em] text-slate-950">技能和应用</h2>
                    <p className="mt-2 text-sm leading-7 text-slate-500">左侧选 skill，右侧查看 `SKILL.md` 和关联文件。</p>
                  </header>

                  <div className="space-y-3">
                    {skills.length ? (
                      skills.map((skill) => {
                        const currentKey = skillKey(skill);
                        const selected = currentKey === selectedSkillKey;
                        return (
                          <button
                            key={currentKey}
                            type="button"
                            onClick={() => void loadSkillDetail(skill.category, skill.name)}
                            className={`block w-full rounded-xl border px-4 py-3 text-left transition ${
                              selected
                                ? "border-slate-900 bg-slate-900 text-white shadow-[0_16px_30px_rgba(15,23,42,0.12)]"
                                : "border-slate-200 bg-slate-50/70 text-slate-900 hover:border-slate-300 hover:bg-white"
                            }`}
                          >
                            <div className="flex items-center justify-between gap-3">
                              <div className="min-w-0">
                                <div className="truncate text-sm font-semibold">{skill.category}/{skill.name}</div>
                                <div className={`mt-1 text-xs ${selected ? "text-slate-300" : "text-slate-500"}`}>
                                  {skill.task_kinds.join(", ") || "general"}
                                </div>
                              </div>
                              {skill.requires_shell ? (
                                <Badge variant={selected ? "secondary" : "outline"}>shell</Badge>
                              ) : null}
                            </div>
                            <p className={`mt-3 text-sm leading-6 ${selected ? "text-slate-200" : "text-slate-600"}`}>
                              {skill.description || "No description"}
                            </p>
                          </button>
                        );
                      })
                    ) : (
                      <div className="rounded-xl border border-dashed border-slate-200 bg-white/80 px-6 py-8 text-sm text-slate-500">
                        还没有加载到任何 skills。
                      </div>
                    )}
                  </div>
                </div>

                <div className="min-h-0 rounded-2xl border border-slate-200/70 bg-white/84 p-5 shadow-[0_12px_36px_rgba(15,23,42,0.05)]">
                  {skillDetailLoading ? (
                    <div className="flex h-full min-h-[420px] items-center justify-center rounded-xl border border-dashed border-slate-200 bg-slate-50/70 text-sm text-slate-500">
                      正在加载 skill 内容...
                    </div>
                  ) : skillDetail ? (
                    <div className="flex h-full min-h-[420px] flex-col">
                      <header className="border-b border-slate-200 pb-4">
                        <div className="flex flex-wrap items-center gap-2">
                          <Badge variant="soft">{skillDetail.category}</Badge>
                          <Badge variant="outline">{skillDetail.name}</Badge>
                          <Badge variant="outline">{skillDetail.file_path}</Badge>
                          {skillDetail.is_binary ? <Badge variant="secondary">binary</Badge> : null}
                          <Badge variant={skillDetail.setup_needed ? "secondary" : "outline"}>
                            {skillDetail.readiness_status}
                          </Badge>
                        </div>
                        <h3 className="mt-3 text-[28px] font-semibold tracking-[-0.04em] text-slate-950">
                          {skillDetail.category}/{skillDetail.name}
                        </h3>
                        <p className="mt-2 max-w-[760px] text-sm leading-7 text-slate-500">
                          {skillDetail.description || "No description"}
                        </p>
                        <div className="mt-4 flex flex-wrap items-center gap-2 text-xs text-slate-500">
                          <span>更新 {formatSkillTimestamp(skillDetail.updated_at_unix)}</span>
                          <span>{skillDetail.task_kinds.join(", ") || "general"}</span>
                          {skillDetail.requires_tools.length ? <span>tools: {skillDetail.requires_tools.join(", ")}</span> : null}
                          {skillDetail.keywords.length ? <span>tags: {skillDetail.keywords.join(", ")}</span> : null}
                        </div>
                      </header>

                      <div className="mt-4 grid min-h-0 flex-1 gap-4 xl:grid-cols-[280px_minmax(0,1fr)]">
                        <div className="rounded-xl border border-slate-200 bg-slate-50/70 p-3">
                          <div className="mb-3 flex items-center justify-between gap-3">
                            <div className="text-sm font-semibold text-slate-900">关联文件</div>
                            <Badge variant="outline">
                              {Object.values(skillDetail.linked_files).reduce((total, files) => total + files.length, 0)}
                            </Badge>
                          </div>
                          <div className="space-y-3">
                            <Button
                              type="button"
                              variant={skillDetail.file_path === "SKILL.md" ? "secondary" : "ghost"}
                              className="w-full justify-start rounded-lg px-3"
                              onClick={() => void loadSkillDetail(skillDetail.category, skillDetail.name)}
                            >
                              SKILL.md
                            </Button>
                            {Object.entries(skillDetail.linked_files).length ? (
                              Object.entries(skillDetail.linked_files).map(([group, files]) => (
                                <div key={group}>
                                  <div className="mb-2 text-[11px] font-semibold uppercase tracking-[0.12em] text-slate-400">
                                    {group}
                                  </div>
                                  <div className="space-y-1">
                                    {files.map((file) => (
                                      <Button
                                        key={file.path}
                                        type="button"
                                        variant={skillDetail.file_path === file.path ? "secondary" : "ghost"}
                                        className="w-full justify-between rounded-lg px-3"
                                        onClick={() => void loadSkillDetail(skillDetail.category, skillDetail.name, file.path)}
                                      >
                                        <span className="truncate text-left">{file.path}</span>
                                        <span className="ml-3 shrink-0 text-[11px] text-slate-400">
                                          {formatFileSize(file.size_bytes)}
                                        </span>
                                      </Button>
                                    ))}
                                  </div>
                                </div>
                              ))
                            ) : (
                              <div className="rounded-lg border border-dashed border-slate-200 bg-white/80 p-4 text-sm text-slate-500">
                                当前 skill 没有 references、templates、scripts 或 assets。
                              </div>
                            )}
                          </div>
                        </div>

                        <div className="grid min-h-0 gap-4 xl:grid-rows-[auto_minmax(0,1fr)]">
                          <div className="rounded-xl border border-slate-200 bg-slate-50/70 p-3">
                            <div className="mb-3 flex items-center justify-between gap-3">
                              <div className="text-sm font-semibold text-slate-900">Readiness</div>
                              <Badge variant={skillDetail.setup_needed ? "secondary" : "outline"}>
                                {skillDetail.setup_needed ? "需要准备" : "可直接使用"}
                              </Badge>
                            </div>
                            <div className="space-y-3 text-sm text-slate-600">
                              {skillDetail.required_environment_variables.length ? (
                                <div>
                                  <div className="mb-1 font-medium text-slate-800">环境变量</div>
                                  <div className="flex flex-wrap gap-2">
                                    {skillDetail.required_environment_variables.map((item) => (
                                      <Badge
                                        key={item.name}
                                        variant={
                                          skillDetail.missing_required_environment_variables.includes(item.name)
                                            ? "secondary"
                                            : "outline"
                                        }
                                      >
                                        {item.name}
                                      </Badge>
                                    ))}
                                  </div>
                                </div>
                              ) : null}
                              {skillDetail.required_commands.length ? (
                                <div>
                                  <div className="mb-1 font-medium text-slate-800">命令依赖</div>
                                  <div className="flex flex-wrap gap-2">
                                    {skillDetail.required_commands.map((item) => (
                                      <Badge
                                        key={item}
                                        variant={
                                          skillDetail.missing_required_commands.includes(item) ? "secondary" : "outline"
                                        }
                                      >
                                        {item}
                                      </Badge>
                                    ))}
                                  </div>
                                </div>
                              ) : null}
                              {skillDetail.config_requirements.length ? (
                                <div>
                                  <div className="mb-1 font-medium text-slate-800">配置项</div>
                                  <div className="space-y-2">
                                    {skillDetail.config_requirements.map((item) => (
                                      <div key={item.key} className="rounded-lg border border-slate-200 bg-white px-3 py-2">
                                        <div className="text-xs font-semibold uppercase tracking-[0.08em] text-slate-400">
                                          {item.key}
                                        </div>
                                        <div className="mt-1 text-sm text-slate-700">{item.resolved_value || item.default_value || "未配置"}</div>
                                      </div>
                                    ))}
                                  </div>
                                </div>
                              ) : null}
                              {!skillDetail.required_environment_variables.length &&
                              !skillDetail.required_commands.length &&
                              !skillDetail.config_requirements.length ? (
                                <div className="rounded-lg border border-dashed border-slate-200 bg-white/80 p-3 text-slate-500">
                                  当前 skill 没有声明额外准备项。
                                </div>
                              ) : null}
                            </div>
                          </div>

                          <div className="min-h-0 rounded-xl border border-slate-200 bg-slate-50/70 p-3">
                          <div className="mb-3 flex items-center justify-between gap-3">
                            <div className="text-sm font-semibold text-slate-900">文件内容</div>
                            <Badge variant="outline">{skillDetail.file_type || "text"}</Badge>
                          </div>
                          <pre className="h-[420px] overflow-auto rounded-lg bg-white p-4 text-[13px] leading-6 text-slate-700 whitespace-pre-wrap break-words">
                            {skillDetail.content}
                          </pre>
                        </div>
                        </div>
                      </div>
                    </div>
                  ) : (
                    <div className="flex h-full min-h-[420px] items-center justify-center rounded-xl border border-dashed border-slate-200 bg-slate-50/70 text-sm text-slate-500">
                      选择一个 skill 查看详情。
                    </div>
                  )}
                </div>
              </div>
              <footer className="mt-4 flex flex-wrap items-center justify-center gap-4 pb-2 text-xs text-slate-500">
                <span>{dataDir || "未解析 data dir"}</span>
                <span>完全访问权限</span>
                <span>{currentSessionId || "main"}</span>
                {bootError ? <span className="text-orange-600">{bootError}</span> : null}
              </footer>
            </section>
          ) : null}

          {activeView === "activity" ? (
            <section className="flex min-h-0 flex-1 flex-col overflow-hidden p-4">
              <div className="min-h-0 flex-1 overflow-auto pr-1">
                <div className="mx-auto flex max-w-5xl flex-col gap-4">
                <div className="rounded-2xl border border-slate-200/80 bg-white/84 px-5 py-5 shadow-[0_12px_36px_rgba(15,23,42,0.05)]">
                  <div className="mb-3 flex flex-wrap items-center gap-2">
                    <Badge variant="soft">定时任务</Badge>
                    <Badge variant="outline">{extensionsOverview?.cron_jobs.length || 0} 个任务</Badge>
                    <Badge variant={cronSchedulerStatus?.running ? "soft" : "secondary"}>
                      {cronSchedulerStatus?.running ? "scheduler 运行中" : "scheduler 已停止"}
                    </Badge>
                    {cronSchedulerStatus?.paused_reason ? <Badge variant="secondary">已暂停</Badge> : null}
                  </div>
                  <h2 className="text-[30px] font-semibold tracking-[-0.04em] text-slate-950">定时任务</h2>
                  <p className="mt-2 text-sm leading-7 text-slate-500">
                    这里只保留后台调度器和任务列表，不展示审批、事件、子任务这些过程信息。
                  </p>
                </div>

                <Card className="rounded-xl">
                  <CardHeader className="pb-3">
                    <CardTitle>调度器</CardTitle>
                    <CardDescription>
                      {cronSchedulerStatus?.running ? "后台正在轮询可执行任务" : "当前未启动后台轮询"}
                    </CardDescription>
                  </CardHeader>
                  <CardContent className="space-y-2 text-sm text-slate-600">
                    <div className="rounded-lg border border-slate-200 bg-white px-3 py-3">
                      <div className="flex items-start justify-between gap-3">
                        <div>
                          <div className="font-medium text-slate-800">Background Scheduler</div>
                          <div className="mt-1 text-xs leading-5 text-slate-500">
                            {cronSchedulerStatus?.workspace_root || config.workspaceRoot || "未设置 workspace"}
                          </div>
                          <div className="mt-2 flex flex-wrap items-center gap-2 text-xs text-slate-500">
                            <Badge variant={cronSchedulerStatus?.running ? "soft" : "secondary"}>
                              {cronSchedulerStatus?.running ? "running" : "stopped"}
                            </Badge>
                            <span>tick {cronSchedulerStatus?.tick_interval_seconds || defaultCronTickIntervalSeconds}s</span>
                            {cronSchedulerStatus?.last_tick_at_unix ? (
                              <span>最近检查 {formatCompactTimestamp(cronSchedulerStatus.last_tick_at_unix)}</span>
                            ) : null}
                            {cronSchedulerStatus?.last_due_job_ids.length ? (
                              <span>命中 {cronSchedulerStatus.last_due_job_ids.join(", ")}</span>
                            ) : null}
                          </div>
                          {cronSchedulerStatus?.paused_reason ? (
                            <div className="mt-2 rounded-md border border-amber-200 bg-amber-50 px-3 py-2 text-xs leading-5 text-amber-700">
                              已暂停: {cronSchedulerStatus.paused_reason}
                            </div>
                          ) : null}
                          {cronSchedulerStatus?.last_error ? (
                            <div className="mt-2 rounded-md border border-orange-200 bg-orange-50 px-3 py-2 text-xs leading-5 text-orange-700">
                              {cronSchedulerStatus.last_error}
                            </div>
                          ) : null}
                        </div>
                        {cronSchedulerStatus?.running ? (
                          <Button
                            type="button"
                            size="sm"
                            variant="outline"
                            className="h-8 rounded-lg"
                            disabled={cronSchedulerBusy}
                            onClick={() => void stopCronScheduler()}
                          >
                            {cronSchedulerBusy ? <LoaderCircle className="mr-1.5 size-3.5 animate-spin" /> : null}
                            停止
                          </Button>
                        ) : (
                          <Button
                            type="button"
                            size="sm"
                            variant="outline"
                            className="h-8 rounded-lg"
                            disabled={cronSchedulerBusy}
                            onClick={() => void startCronScheduler()}
                          >
                            {cronSchedulerBusy ? (
                              <LoaderCircle className="mr-1.5 size-3.5 animate-spin" />
                            ) : (
                              <Play className="mr-1.5 size-3.5" />
                            )}
                            启动
                          </Button>
                        )}
                      </div>
                    </div>
                  </CardContent>
                </Card>

                <Card className="rounded-xl">
                  <CardHeader className="pb-3">
                    <div className="flex items-center justify-between gap-3">
                      <div>
                        <CardTitle>任务列表</CardTitle>
                        <CardDescription>{extensionsOverview?.cron_jobs.length || 0} 个已配置任务</CardDescription>
                      </div>
                      <Button type="button" size="sm" variant="outline" className="rounded-lg" onClick={resetCronJobForm}>
                        新增任务
                      </Button>
                    </div>
                  </CardHeader>
                  <CardContent className="space-y-2 text-sm text-slate-600">
                    <div className="rounded-lg border border-slate-200 bg-white px-4 py-4">
                      <div className="mb-3 flex items-center justify-between gap-3">
                        <div className="font-medium text-slate-800">
                          {cronJobForm.previousId ? `编辑任务 ${cronJobForm.previousId}` : "新增定时任务"}
                        </div>
                        {cronJobForm.previousId ? (
                          <Button type="button" size="sm" variant="ghost" className="rounded-lg" onClick={resetCronJobForm}>
                            取消编辑
                          </Button>
                        ) : null}
                      </div>
                      <div className="grid gap-3 md:grid-cols-2">
                        <label className="grid gap-2">
                          <span className="text-[11px] uppercase tracking-[0.1em] text-slate-400">Task ID</span>
                          <Input
                            value={cronJobForm.id}
                            placeholder="nightly-audit"
                            onChange={(event) =>
                              setCronJobForm((prev) => ({ ...prev, id: event.target.value }))
                            }
                          />
                        </label>
                        <label className="grid gap-2">
                          <span className="text-[11px] uppercase tracking-[0.1em] text-slate-400">Schedule</span>
                          <Input
                            value={cronJobForm.schedule}
                            placeholder="0 2 * * * / every 30m / 2026-04-12T10:00:00+08:00"
                            onChange={(event) =>
                              setCronJobForm((prev) => ({ ...prev, schedule: event.target.value }))
                            }
                          />
                        </label>
                        <label className="grid gap-2 md:col-span-2">
                          <span className="text-[11px] uppercase tracking-[0.1em] text-slate-400">Prompt</span>
                          <Textarea
                            value={cronJobForm.prompt}
                            className="min-h-[120px] rounded-lg border-slate-200 px-4 py-3 text-[14px] leading-7"
                            placeholder="描述这个定时任务要执行什么"
                            onChange={(event) =>
                              setCronJobForm((prev) => ({ ...prev, prompt: event.target.value }))
                            }
                          />
                        </label>
                        <label className="flex items-center gap-3 rounded-lg border border-slate-200 bg-slate-50/80 px-4 py-3 text-sm text-slate-600 md:col-span-2">
                          <input
                            type="checkbox"
                            checked={cronJobForm.enabled}
                            onChange={(event) =>
                              setCronJobForm((prev) => ({ ...prev, enabled: event.target.checked }))
                            }
                          />
                          启用这个任务
                        </label>
                      </div>
                      <div className="mt-3 flex items-center gap-2">
                        <Button
                          type="button"
                          className="rounded-lg"
                          disabled={cronJobSaving}
                          onClick={() => void saveCronJob()}
                        >
                          {cronJobSaving ? <LoaderCircle className="mr-1.5 size-4 animate-spin" /> : null}
                          保存任务
                        </Button>
                      </div>
                    </div>
                    {extensionsOverview?.cron_jobs.length ? (
                      extensionsOverview.cron_jobs.map((item) => (
                        <div key={item.id} className="rounded-lg border border-slate-200 bg-slate-50/80 px-3 py-3">
                          <div className="flex items-start justify-between gap-3">
                            <div className="min-w-0 flex-1">
                              <div className="font-medium text-slate-800">{item.id}</div>
                              <div className="mt-1 text-xs text-slate-500">{item.schedule}</div>
                              <div className="mt-3 rounded-md bg-white px-3 py-2 text-sm leading-6 text-slate-700">
                                {item.prompt_preview}
                              </div>
                              <div className="mt-3 flex flex-wrap items-center gap-2 text-xs text-slate-500">
                                <Badge variant={item.enabled ? "outline" : "secondary"}>
                                  {item.enabled ? "已启用" : "已禁用"}
                                </Badge>
                                {item.last_status ? (
                                  <Badge variant={item.last_status === "completed" ? "soft" : "secondary"}>
                                    {item.last_status}
                                  </Badge>
                                ) : null}
                                {item.next_run_at_unix ? <span>下次运行 {formatCompactTimestamp(item.next_run_at_unix)}</span> : null}
                                {item.last_run_at_unix ? <span>最近运行 {formatCompactTimestamp(item.last_run_at_unix)}</span> : null}
                              </div>
                              {item.recent_runs.length ? (
                                <div className="mt-3 rounded-md border border-slate-200 bg-white px-3 py-3">
                                  <div className="mb-2 text-xs font-medium uppercase tracking-[0.1em] text-slate-400">
                                    最近执行
                                  </div>
                                  <div className="space-y-2">
                                    {item.recent_runs.map((run) => (
                                      <div
                                        key={`${run.session_id}-${run.updated_at_unix}`}
                                        className="rounded-md border border-slate-200 bg-slate-50 px-3 py-2"
                                      >
                                        <div className="flex flex-wrap items-center gap-2 text-xs text-slate-500">
                                          <Badge
                                            variant={
                                              run.status === "completed"
                                                ? "soft"
                                                : run.status === "awaiting_approval"
                                                  ? "secondary"
                                                  : "outline"
                                            }
                                          >
                                            {run.status}
                                          </Badge>
                                          <span>{formatCompactTimestamp(run.updated_at_unix)}</span>
                                          <span>{run.session_id}</span>
                                        </div>
                                        {run.response_preview ? (
                                          <div className="mt-2 text-xs leading-5 text-slate-600">
                                            {truncate(run.response_preview, 160)}
                                          </div>
                                        ) : null}
                                      </div>
                                    ))}
                                  </div>
                                </div>
                              ) : null}
                            </div>
                            <div className="flex shrink-0 items-center gap-2">
                              <Button
                                type="button"
                                size="sm"
                                variant="outline"
                                className="rounded-lg"
                                onClick={() => editCronJob(item)}
                              >
                                编辑
                              </Button>
                              <Button
                                type="button"
                                size="sm"
                                variant="outline"
                                className="rounded-lg"
                                disabled={cronDeletingId === item.id}
                                onClick={() => void deleteCronJob(item.id)}
                              >
                                {cronDeletingId === item.id ? "删除中..." : "删除"}
                              </Button>
                              {item.last_session_id ? (
                                <Button
                                  type="button"
                                  size="sm"
                                  variant="outline"
                                  className="rounded-lg"
                                  onClick={() => void loadSessionIntoView(item.last_session_id!)}
                                >
                                  最近会话
                                </Button>
                              ) : null}
                              <Button
                                type="button"
                                size="sm"
                                variant="outline"
                                className="rounded-lg"
                                disabled={!item.enabled || cronRunningId === item.id}
                                onClick={() => void runCronJob(item.id)}
                              >
                                {cronRunningId === item.id ? (
                                  <LoaderCircle className="mr-1.5 size-3.5 animate-spin" />
                                ) : (
                                  <Play className="mr-1.5 size-3.5" />
                                )}
                                立即运行
                              </Button>
                            </div>
                          </div>
                        </div>
                      ))
                    ) : (
                      <div className="rounded-lg border border-dashed border-slate-200 bg-slate-50/80 p-4 text-sm text-slate-500">
                        未配置定时任务
                      </div>
                    )}
                  </CardContent>
                </Card>
                </div>
              </div>
              <footer className="mt-4 flex flex-wrap items-center justify-center gap-4 pb-2 text-xs text-slate-500">
                <span>{dataDir || "未解析 data dir"}</span>
                <span>完全访问权限</span>
                <span>{currentSessionId || "main"}</span>
                {bootError ? <span className="text-orange-600">{bootError}</span> : null}
              </footer>
            </section>
          ) : null}

          {settingsViewVisible ? (
            <section className="flex min-h-0 flex-1 flex-col overflow-hidden p-4">
              <div className="min-h-0 flex-1 overflow-auto pr-1">
                <div className="mx-auto flex max-w-3xl flex-col gap-4">
                  <div className="rounded-2xl border border-slate-200/80 bg-white/84 px-5 py-5 shadow-[0_12px_36px_rgba(15,23,42,0.05)]">
                    <div className="flex flex-wrap items-center justify-between gap-3">
                      <div className="flex flex-wrap items-center gap-2">
                        <Badge variant="soft">Settings</Badge>
                        <Badge variant={providerReady ? "soft" : "secondary"}>{providerStatusLabel}</Badge>
                        {providerRuntimeLoading ? <Badge variant="outline">重新解析中</Badge> : null}
                      </div>
                      <Button
                        type="button"
                        variant="outline"
                        className="rounded-lg"
                        onClick={() => void loadProviders()}
                      >
                        刷新
                      </Button>
                    </div>
                    <h2 className="mt-4 text-[28px] font-semibold text-slate-950">运行设置</h2>
                    <div className="mt-3 flex flex-wrap gap-3 text-xs text-slate-500">
                      <span>当前: {effectiveProvider?.id || selectedProvider?.id || "default"}</span>
                      <span>Model: {resolvedModel}</span>
                      <span>凭证: {formatAuthSource(resolvedAuthSource)}</span>
                    </div>
                    {providerRuntimeError ? (
                      <div className="mt-3 rounded-lg border border-rose-200 bg-rose-50 px-3 py-2 text-xs leading-5 text-rose-700">
                        {providerRuntimeError}
                      </div>
                    ) : null}
                  </div>

                  <div className="rounded-2xl border border-slate-200/80 bg-white/92 p-5 shadow-[0_12px_32px_rgba(15,23,42,0.05)]">
                    <div className="grid gap-4">
                      <label className="grid gap-2 text-sm">
                        <span className="text-[11px] uppercase tracking-[0.1em] text-slate-400">Provider</span>
                        <select
                          value={config.provider}
                          className="h-10 rounded-lg border border-slate-200 bg-white px-3 text-sm text-slate-700"
                          onChange={(event) => {
                            const next = event.target.value;
                            const matched = providers.find((item) => item.id === next);
                            setConfig((prev) => ({
                              ...prev,
                              provider: next,
                              model: matched ? matched.model : prev.model,
                              baseUrl: matched && !prev.baseUrl.trim() ? matched.base_url : prev.baseUrl,
                            }));
                          }}
                        >
                          <option value="">自动 / 默认</option>
                          {providers.map((item) => (
                            <option key={item.id} value={item.id}>
                              {item.label} ({item.id})
                            </option>
                          ))}
                        </select>
                      </label>

                      <label className="grid gap-2 text-sm">
                        <span className="text-[11px] uppercase tracking-[0.1em] text-slate-400">Model</span>
                        <Input
                          value={config.model}
                          placeholder="gpt-5.4"
                          onChange={(event) => setConfig((prev) => ({ ...prev, model: event.target.value }))}
                        />
                      </label>

                      <label className="grid gap-2 text-sm">
                        <span className="text-[11px] uppercase tracking-[0.1em] text-slate-400">Small Model</span>
                        <Input
                          value={config.smallModel}
                          placeholder="gpt-5.4-mini"
                          onChange={(event) => setConfig((prev) => ({ ...prev, smallModel: event.target.value }))}
                        />
                      </label>

                      <label className="grid gap-2 text-sm">
                        <span className="text-[11px] uppercase tracking-[0.1em] text-slate-400">Base URL</span>
                        <Input
                          value={config.baseUrl}
                          placeholder="https://api.openai.com/v1"
                          onChange={(event) => setConfig((prev) => ({ ...prev, baseUrl: event.target.value }))}
                        />
                      </label>

                      <label className="grid gap-2 text-sm">
                        <span className="text-[11px] uppercase tracking-[0.1em] text-slate-400">API Key</span>
                        <Input
                          value={config.apiKey}
                          placeholder="sk-..."
                          onChange={(event) => setConfig((prev) => ({ ...prev, apiKey: event.target.value }))}
                        />
                      </label>

                    </div>
                  </div>
                </div>
              </div>
              <footer className="mt-4 flex flex-wrap items-center justify-center gap-4 pb-2 text-xs text-slate-500">
                <span>{dataDir || "未解析 data dir"}</span>
                <span>完全访问权限</span>
                <span>{currentSessionId || "main"}</span>
                {bootError ? <span className="text-orange-600">{bootError}</span> : null}
              </footer>
            </section>
          ) : null}
        </section>
      </div>
    </main>
  );
}
