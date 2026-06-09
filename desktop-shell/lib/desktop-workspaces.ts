export type WorkspaceSessionSummary = {
  session_id: string;
  title: string | null;
  updated_at_unix: number;
  last_user_message: string | null;
};

export type WorkspaceListEntry = {
  workspaceRoot: string;
  dataDir: string;
  expanded: boolean;
  sessions: WorkspaceSessionSummary[];
  loading: boolean;
  error: string | null;
};

type StoredWorkspaceListEntry = {
  workspaceRoot: string;
  dataDir?: string;
  expanded?: boolean;
};

const workspaceListStorageKey = "crab.desktop.workspaces";
const legacyWorkspaceListStorageKey = "hermes-agent-rs.desktop.workspaces";

export function workspaceEntryKey(workspaceRoot: string, dataDir = ""): string {
  return `${workspaceRoot.trim()}::${dataDir.trim()}`;
}

export function workspaceFolderName(workspaceRoot: string): string {
  const trimmed = workspaceRoot.trim();
  if (!trimmed) {
    return "未选择 workspace";
  }
  return trimmed.split(/[\\/]/).pop() || trimmed;
}

export function createWorkspaceListEntry(
  workspaceRoot: string,
  dataDir = "",
  expanded = true,
): WorkspaceListEntry | null {
  const trimmedRoot = workspaceRoot.trim();
  if (!trimmedRoot) {
    return null;
  }
  return {
    workspaceRoot: trimmedRoot,
    dataDir: dataDir.trim(),
    expanded,
    sessions: [],
    loading: false,
    error: null,
  };
}

export function mergeWorkspaceListEntries(entries: WorkspaceListEntry[]): WorkspaceListEntry[] {
  const seen = new Set<string>();
  const merged: WorkspaceListEntry[] = [];
  for (const entry of entries) {
    const key = workspaceEntryKey(entry.workspaceRoot, entry.dataDir);
    if (seen.has(key)) {
      continue;
    }
    seen.add(key);
    merged.push(entry);
  }
  return merged;
}

export function loadWorkspaceList(): WorkspaceListEntry[] {
  if (typeof window === "undefined") {
    return [];
  }
  try {
    const raw =
      window.localStorage.getItem(workspaceListStorageKey) ??
      window.localStorage.getItem(legacyWorkspaceListStorageKey);
    if (!raw) {
      return [];
    }
    const parsed = JSON.parse(raw);
    if (!Array.isArray(parsed)) {
      return [];
    }
    return mergeWorkspaceListEntries(
      parsed
        .map((entry: StoredWorkspaceListEntry) =>
          createWorkspaceListEntry(entry.workspaceRoot, entry.dataDir || "", entry.expanded !== false),
        )
        .filter(Boolean) as WorkspaceListEntry[],
    );
  } catch {
    return [];
  }
}

export function storeWorkspaceList(entries: WorkspaceListEntry[]) {
  if (typeof window === "undefined") {
    return;
  }
  const payload: StoredWorkspaceListEntry[] = entries.map((entry) => ({
    workspaceRoot: entry.workspaceRoot,
    dataDir: entry.dataDir,
    expanded: entry.expanded,
  }));
  window.localStorage.setItem(workspaceListStorageKey, JSON.stringify(payload));
  window.localStorage.removeItem(legacyWorkspaceListStorageKey);
}
