import { spawn } from "node:child_process";
import { promises as fs } from "node:fs";
import { createServer } from "node:http";
import net from "node:net";
import { app, BrowserWindow, dialog, ipcMain, shell, webContents } from "electron";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const appRoot = path.resolve(__dirname, "..");
const rustRoot = path.resolve(appRoot, "..");
const rustManifestPath = path.join(rustRoot, "Cargo.toml");
const rustDebugBinary = path.join(
  rustRoot,
  "target",
  "debug",
  process.platform === "win32" ? "hermes-agent-rs.exe" : "hermes-agent-rs",
);

const DEV_URL = process.env.HERMES_ELECTRON_DEV_URL || "http://127.0.0.1:1420";
const PROD_ENTRY = path.join(appRoot, "out", "index.html");
const ELECTRON_DEVTOOLS_PORT = Number(process.env.HERMES_ELECTRON_DEVTOOLS_PORT || 47712);
const DEFAULT_BROWSER_URL = "https://www.baidu.com";
const MAX_TEXT_PREVIEW_BYTES = 120_000;
const MAX_BINARY_PREVIEW_BYTES = 8_000_000;
const MAX_TREE_NODES = 2_000;
const BRIDGE_COMMANDS = new Set([
  "list_sessions",
  "load_session",
  "search_sessions",
  "list_skills",
  "view_skill",
  "extensions_overview",
  "list_providers",
  "resolve_provider_status",
  "load_shared_provider_config",
  "save_shared_provider_config",
  "list_approvals",
  "resolve_approval",
  "list_delegate_runs",
  "cancel_delegate_run",
  "save_cron_job",
  "delete_cron_job",
  "clear_session",
  "stop_session",
  "inspect_mcp_server",
  "run_agent",
  "resume_approval",
  "run_cron_job",
  "retry_delegate_run",
  "resolve_runtime_profile",
  "resolve_runtime_status",
  "start_runtime",
  "repair_runtime",
  "reset_runtime",
  "browser_stream_endpoint",
  "browser_current_url",
  "view_workspace_file",
]);
const EVENTFUL_BRIDGE_COMMANDS = new Set([
  "run_agent",
  "resume_approval",
  "run_cron_job",
  "retry_delegate_run",
]);
const INTERACTIVE_BRIDGE_COMMANDS = new Set(["run_agent", "resume_approval"]);

const cronSchedulerStatus = {
  running: false,
  paused_reason: null,
  tick_interval_seconds: 60,
  last_tick_at_unix: null,
  last_due_job_ids: [],
  last_error: null,
  workspace_root: null,
};

let mainWindow = null;
let lastSessionId = null;
let activeInteractiveRuns = 0;
const cronSchedulerRuntime = {
  generation: 0,
  timer: null,
  request: null,
};
let browserAutomationServer = null;
const browserGuests = new Map();
const activeBrowserGuestIds = new Map();
const slidevPreviews = new Map();

function isDev() {
  return !app.isPackaged && process.env.HERMES_ELECTRON_MODE !== "production";
}

async function createMainWindow() {
  const preloadPath = path.join(__dirname, "preload.mjs");
  mainWindow = new BrowserWindow({
    width: 1480,
    height: 960,
    minWidth: 1100,
    minHeight: 760,
    backgroundColor: "#f8fafc",
    autoHideMenuBar: true,
    titleBarStyle: process.platform === "darwin" ? "hiddenInset" : undefined,
    webPreferences: {
      preload: preloadPath,
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: false,
      webviewTag: true,
    },
  });
  console.log("[electron-shell] preload=", preloadPath);
  mainWindow.webContents.on("did-finish-load", async () => {
    try {
      const diagnostics = await mainWindow.webContents.executeJavaScript(
        `({
          href: window.location.href,
          hasHermesDesktop: Boolean(window.hermesDesktop),
          hasDesktopInvoke: Boolean(window.hermesDesktop && typeof window.hermesDesktop.invoke === "function"),
          shellMarker: window.__HERMES_DESKTOP_SHELL__ || null,
          hasTauri: Boolean(window.__TAURI__ || window.__TAURI_INTERNALS__),
        })`,
        true,
      );
      console.log("[electron-shell] renderer-diagnostics=", JSON.stringify(diagnostics));
    } catch (error) {
      console.error("[electron-shell] failed to inspect renderer", error);
    }
  });
  mainWindow.webContents.on("preload-error", (_event, preloadPathValue, error) => {
    console.error("[electron-shell] preload-error", preloadPathValue, error);
  });
  mainWindow.webContents.on("did-attach-webview", (_event, guestContents, params) => {
    const sessionId = sessionIdFromPartition(params?.partition);
    if (!sessionId) {
      return;
    }
    let guests = browserGuests.get(sessionId);
    if (!guests) {
      guests = new Map();
      browserGuests.set(sessionId, guests);
    }
    guests.set(guestContents.id, guestContents);
    if (!activeBrowserGuestIds.has(sessionId)) {
      activeBrowserGuestIds.set(sessionId, guestContents.id);
    }
    guestContents.once("destroyed", () => {
      const currentGuests = browserGuests.get(sessionId);
      if (currentGuests) {
        currentGuests.delete(guestContents.id);
        if (currentGuests.size === 0) {
          browserGuests.delete(sessionId);
          activeBrowserGuestIds.delete(sessionId);
        } else if (activeBrowserGuestIds.get(sessionId) === guestContents.id) {
          activeBrowserGuestIds.set(sessionId, currentGuests.keys().next().value);
        }
      }
    });
  });

  if (isDev()) {
    await mainWindow.loadURL(DEV_URL);
    mainWindow.webContents.openDevTools({ mode: "detach" });
    return;
  }

  await mainWindow.loadFile(PROD_ENTRY);
}

ipcMain.handle("hermes:invoke", async (_event, payload) => {
  const command = payload?.command;
  const args = payload?.args || {};

  switch (command) {
    case "desktop_info":
      return {
        shell: "electron",
        platform: process.platform,
        global_event_topic: "hermes://agent/event",
        global_done_topic: "hermes://agent/done",
        cleared_topic: "hermes://agent/cleared",
        session_event_topic_template: "hermes://agent/event/<session_id>",
        session_done_topic_template: "hermes://agent/done/<session_id>",
        last_session_id: lastSessionId,
        current_working_dir: process.cwd(),
      };
    case "cron_scheduler_status":
      return cronSchedulerStatus;
    case "start_cron_scheduler":
      return startCronScheduler(args?.request);
    case "stop_cron_scheduler":
      return stopCronScheduler();
    case "pick_workspace_folder":
      return pickWorkspaceFolder(args?.currentDir);
    case "list_workspace_tree":
      return listWorkspaceTree(args?.workspaceRoot);
    case "open_workspace_file":
      return openWorkspaceFile(args?.workspaceRoot, args?.filePath);
    case "preview_slidev_deck":
      return previewSlidevDeck(args?.workspaceRoot, args?.filePath);
    case "export_slidev_deck":
      return exportSlidevDeck(
        args?.workspaceRoot,
        args?.filePath,
        args?.format,
        args?.outputPath,
      );
    case "latest_context_debug_snapshot":
      return latestContextDebugSnapshot(args?.dataDir, args?.sessionId);
    case "browser_current_url":
      return resolveBrowserCurrentUrl(args?.dataDir, args?.sessionId);
    case "set_active_browser_guest":
      return setActiveBrowserGuest(args?.sessionId, args?.guestId);
    case "sync_browser_state":
      return syncBrowserState(args?.dataDir, args?.sessionId);
    case "view_workspace_file":
      return handleRustBridgeCommand(command, args);
    default:
      if (BRIDGE_COMMANDS.has(command)) {
        return handleRustBridgeCommand(command, args);
      }
      throw new Error(`Electron bridge has not implemented command "${command}" yet.`);
  }
});

app.whenReady().then(async () => {
  await startBrowserAutomationServer();
  await createMainWindow();

  app.on("activate", async () => {
    if (BrowserWindow.getAllWindows().length === 0) {
      await createMainWindow();
    }
  });
});

app.on("window-all-closed", () => {
  if (process.platform !== "darwin") {
    app.quit();
  }
});

app.on("before-quit", () => {
  if (browserAutomationServer) {
    browserAutomationServer.close();
    browserAutomationServer = null;
  }
  stopSlidevPreviews();
});

async function pickWorkspaceFolder(currentDir) {
  if (!mainWindow || mainWindow.isDestroyed()) {
    return null;
  }

  if (mainWindow.isMinimized()) {
    mainWindow.restore();
  }
  if (!mainWindow.isVisible()) {
    mainWindow.show();
  }
  app.focus({ steal: true });
  mainWindow.focus();

  const filePaths =
    dialog.showOpenDialogSync(mainWindow, {
      title: "选择工作目录",
      buttonLabel: "选择目录",
      message: "选择当前会话要使用的 workspace 目录",
      defaultPath: currentDir || undefined,
      properties: ["openDirectory", "createDirectory", "dontAddToRecent"],
    }) || [];

  if (filePaths.length === 0) {
    return null;
  }
  return filePaths[0];
}

function sessionPartition(sessionId) {
  return `persist:hermes-browser-${sessionId}`;
}

function sessionIdFromPartition(partition) {
  const prefix = "persist:hermes-browser-";
  if (typeof partition !== "string" || !partition.startsWith(prefix)) {
    return null;
  }
  return partition.slice(prefix.length) || null;
}

function currentAttachedBrowserGuest(sessionId) {
  const guests = browserGuests.get(sessionId);
  if (!guests || guests.size === 0) {
    return currentGlobalAttachedBrowserGuest();
  }
  const activeGuestId = activeBrowserGuestIds.get(sessionId);
  if (typeof activeGuestId === "number") {
    const activeGuest = guests.get(activeGuestId);
    if (activeGuest && !activeGuest.isDestroyed()) {
      return activeGuest;
    }
  }
  for (const [guestId, guest] of guests.entries()) {
    if (!guest.isDestroyed()) {
      activeBrowserGuestIds.set(sessionId, guestId);
      return guest;
    }
  }
  return currentGlobalAttachedBrowserGuest();
}

function currentGlobalAttachedBrowserGuest() {
  const liveGuests = [];
  for (const guests of browserGuests.values()) {
    for (const guest of guests.values()) {
      if (guest && !guest.isDestroyed()) {
        liveGuests.push(guest);
      }
    }
  }
  if (liveGuests.length !== 1) {
    return null;
  }
  return liveGuests[0];
}

function setActiveBrowserGuest(sessionId, guestId) {
  if (typeof sessionId !== "string" || !sessionId.trim()) {
    return { ok: false };
  }
  const normalizedGuestId = Number(guestId);
  if (!Number.isInteger(normalizedGuestId)) {
    return { ok: false };
  }
  let guests = browserGuests.get(sessionId);
  if (!guests) {
    guests = new Map();
    browserGuests.set(sessionId, guests);
  }
  let guest = guests.get(normalizedGuestId);
  if (!guest || guest.isDestroyed()) {
    const resolved = webContents.fromId(normalizedGuestId);
    if (!resolved || resolved.isDestroyed()) {
      return { ok: false };
    }
    guest = resolved;
    guests.set(normalizedGuestId, guest);
  }
  activeBrowserGuestIds.set(sessionId, normalizedGuestId);
  return { ok: true };
}

function currentBrowserContents(sessionId) {
  return currentAttachedBrowserGuest(sessionId);
}

function safeBrowserUrl(contents) {
  try {
    const url = contents?.getURL?.();
    if (typeof url !== "string") {
      return null;
    }
    const trimmed = url.trim();
    if (!trimmed || trimmed === "about:blank") {
      return null;
    }
    return trimmed;
  } catch {
    return null;
  }
}

function browserStatePath(dataDir, sessionId) {
  return path.join(String(dataDir), "browser", `${sessionId}.json`);
}

async function readStoredBrowserUrl(dataDir, sessionId) {
  if (typeof dataDir !== "string" || !dataDir.trim() || typeof sessionId !== "string" || !sessionId.trim()) {
    return null;
  }
  try {
    const raw = await fs.readFile(browserStatePath(dataDir, sessionId), "utf8");
    const payload = JSON.parse(raw);
    const current = payload?.current ?? payload;
    const candidates = [current?.final_url, current?.url];
    for (const candidate of candidates) {
      if (typeof candidate === "string" && candidate.trim() && candidate.trim() !== "about:blank") {
        return candidate.trim();
      }
    }
  } catch {
    return null;
  }
  return null;
}

function serializeCapturedBrowserState(state) {
  return {
    url: typeof state?.url === "string" ? state.url : "",
    final_url: typeof state?.finalUrl === "string" ? state.finalUrl : typeof state?.url === "string" ? state.url : "",
    content_type: typeof state?.contentType === "string" ? state.contentType : "text/html",
    title: typeof state?.title === "string" ? state.title : null,
    content: typeof state?.content === "string" ? state.content : "",
    elements: Array.isArray(state?.elements)
      ? state.elements.map((element) => ({
          ref_id: typeof element?.refId === "string" ? element.refId : "",
          kind: typeof element?.kind === "string" ? element.kind : "element",
          label: typeof element?.label === "string" ? element.label : "",
          target: typeof element?.target === "string" ? element.target : null,
          role: typeof element?.role === "string" ? element.role : null,
          selector: typeof element?.selector === "string" ? element.selector : null,
          bbox: element?.bbox && Number.isFinite(Number(element.bbox.x))
            ? {
                x: Math.round(Number(element.bbox.x)),
                y: Math.round(Number(element.bbox.y)),
                width: Math.round(Number(element.bbox.width)),
                height: Math.round(Number(element.bbox.height)),
              }
            : null,
          disabled: typeof element?.disabled === "boolean" ? element.disabled : null,
          checked: typeof element?.checked === "boolean" ? element.checked : null,
          selected: typeof element?.selected === "boolean" ? element.selected : null,
          required: typeof element?.required === "boolean" ? element.required : null,
          field_name: typeof element?.fieldName === "string" ? element.fieldName : null,
          value: typeof element?.value === "string" ? element.value : null,
          form_id: typeof element?.formId === "string" ? element.formId : null,
          form_action: typeof element?.formAction === "string" ? element.formAction : null,
          form_method: typeof element?.formMethod === "string" ? element.formMethod : null,
        }))
      : [],
    images: Array.isArray(state?.images)
      ? state.images.map((image) => ({
          src: typeof image?.src === "string" ? image.src : "",
          alt: typeof image?.alt === "string" ? image.alt : "",
        }))
      : [],
    truncated_body: Boolean(state?.truncatedBody),
    fetched_at_unix: Math.floor(Date.now() / 1000),
  };
}

async function persistBrowserState(dataDir, sessionId, capturedState) {
  const browserDir = path.join(String(dataDir), "browser");
  await fs.mkdir(browserDir, { recursive: true });
  const targetPath = browserStatePath(dataDir, sessionId);
  const tempPath = `${targetPath}.${process.pid}.${Date.now()}.${Math.random().toString(16).slice(2)}.tmp`;

  let preserved = null;
  try {
    preserved = JSON.parse(await fs.readFile(targetPath, "utf8"));
  } catch {
    preserved = null;
  }

  const nextState = {
    current: serializeCapturedBrowserState(capturedState),
    back_stack: Array.isArray(preserved?.back_stack) ? preserved.back_stack : [],
    forward_stack: Array.isArray(preserved?.forward_stack) ? preserved.forward_stack : [],
    focused_ref: typeof preserved?.focused_ref === "string" ? preserved.focused_ref : null,
    scroll_offset: Number.isFinite(Number(preserved?.scroll_offset)) ? Number(preserved.scroll_offset) : 0,
    console_messages: Array.isArray(preserved?.console_messages) ? preserved.console_messages : [],
  };

  await fs.writeFile(tempPath, JSON.stringify(nextState, null, 2), "utf8");
  await fs.rename(tempPath, targetPath);
}

async function resolveBrowserCurrentUrl(dataDir, sessionId) {
  const liveUrl = typeof sessionId === "string" ? safeBrowserUrl(currentBrowserContents(sessionId)) : null;
  if (liveUrl) {
    return { url: liveUrl };
  }
  const storedUrl = await readStoredBrowserUrl(dataDir, sessionId);
  return { url: storedUrl || DEFAULT_BROWSER_URL };
}

async function syncBrowserState(dataDir, sessionId) {
  if (typeof dataDir !== "string" || !dataDir.trim() || typeof sessionId !== "string" || !sessionId.trim()) {
    return { ok: false, url: null };
  }
  const contents = currentBrowserContents(sessionId);
  if (!contents) {
    return {
      ok: false,
      title: null,
      url: (await readStoredBrowserUrl(dataDir, sessionId)) || null,
    };
  }
  const capturedState = await captureGuestState(contents);
  await persistBrowserState(dataDir, sessionId, capturedState);
  return {
    ok: true,
    title: typeof capturedState?.title === "string" ? capturedState.title : null,
    url: capturedState?.finalUrl || capturedState?.url || null,
  };
}

async function startBrowserAutomationServer() {
  if (browserAutomationServer) {
    return;
  }

  browserAutomationServer = createServer((request, response) => {
    void handleBrowserAutomationRequest(request, response);
  });

  await new Promise((resolve, reject) => {
    browserAutomationServer.once("error", reject);
    browserAutomationServer.listen(ELECTRON_DEVTOOLS_PORT, "127.0.0.1", () => {
      browserAutomationServer.off("error", reject);
      resolve();
    });
  });
}

async function handleBrowserAutomationRequest(request, response) {
  try {
    if (request.method !== "POST" || !request.url) {
      sendJson(response, 404, { error: "not found" });
      return;
    }

    const url = new URL(request.url, `http://127.0.0.1:${ELECTRON_DEVTOOLS_PORT}`);
    const segments = url.pathname.split("/").filter(Boolean);
    if (segments.length !== 3 || segments[0] !== "session") {
      sendJson(response, 404, { error: "not found" });
      return;
    }

    const sessionId = decodeURIComponent(segments[1]);
    const action = segments[2];
    const body = await readJsonBody(request);
    const guest = await getBrowserAutomationContents(sessionId);

    let result;
    switch (action) {
      case "snapshot":
        result = await captureGuestState(guest);
        break;
      case "screenshot":
        result = await devtoolsScreenshot(
          guest,
          Boolean(body?.fullPage ?? true),
          Boolean(body?.annotate ?? false),
          Number(body?.timeoutSeconds) || 20,
        );
        break;
      case "navigate":
        result = await devtoolsNavigate(guest, body?.url, Number(body?.timeoutSeconds) || 20);
        break;
      case "back":
        result = await devtoolsBack(guest, Number(body?.timeoutSeconds) || 20);
        break;
      case "forward":
        result = await devtoolsForward(guest, Number(body?.timeoutSeconds) || 20);
        break;
      case "click":
        result = await devtoolsClick(guest, body?.reference, Number(body?.timeoutSeconds) || 20);
        break;
      case "hover":
        result = await devtoolsHover(guest, body?.reference, Number(body?.timeoutSeconds) || 20);
        break;
      case "fill":
        result = await devtoolsFill(
          guest,
          body?.reference,
          typeof body?.text === "string" ? body.text : "",
          Number(body?.timeoutSeconds) || 20,
        );
        break;
      case "select":
        result = await devtoolsSelect(
          guest,
          body?.reference,
          typeof body?.value === "string" ? body.value : null,
          typeof body?.label === "string" ? body.label : null,
          Number.isFinite(Number(body?.index)) ? Number(body.index) : null,
          Number(body?.timeoutSeconds) || 20,
        );
        break;
      case "upload":
        result = await devtoolsUpload(
          guest,
          body?.reference,
          Array.isArray(body?.files) ? body.files : [],
          Number(body?.timeoutSeconds) || 20,
        );
        break;
      case "press":
        result = await devtoolsPress(guest, body?.key, Number(body?.timeoutSeconds) || 20);
        break;
      case "scroll":
        result = await devtoolsScroll(
          guest,
          body?.direction,
          Number(body?.amount) || 1200,
          Number(body?.timeoutSeconds) || 20,
        );
        break;
      case "wait":
        result = await devtoolsWait(
          guest,
          typeof body?.selector === "string" ? body.selector : null,
          typeof body?.text === "string" ? body.text : null,
          Number(body?.timeoutSeconds) || 20,
          Number(body?.pollIntervalMs) || 200,
        );
        break;
      case "eval":
        result = await devtoolsEval(guest, body?.expression, Number(body?.timeoutSeconds) || 20);
        break;
      default:
        sendJson(response, 404, { error: `unknown browser action: ${action}` });
        return;
    }

    sendJson(response, 200, result);
  } catch (error) {
    sendJson(response, 500, {
      error: error instanceof Error ? error.message : String(error),
    });
  }
}

function sendJson(response, statusCode, payload) {
  response.statusCode = statusCode;
  response.setHeader("Content-Type", "application/json; charset=utf-8");
  response.end(JSON.stringify(payload));
}

async function readJsonBody(request) {
  const chunks = [];
  for await (const chunk of request) {
    chunks.push(chunk);
  }
  if (!chunks.length) {
    return {};
  }
  return JSON.parse(Buffer.concat(chunks).toString("utf8"));
}

function getBrowserGuestForSession(sessionId) {
  const guest = currentAttachedBrowserGuest(sessionId);
  if (!guest || guest.isDestroyed()) {
    throw new Error(
      `no embedded Chromium guest is attached for session ${sessionId}; open the browser panel for this session first`,
    );
  }
  return guest;
}

async function getBrowserAutomationContents(sessionId) {
  const timeoutMs = 5000;
  const startedAt = Date.now();

  while (Date.now() - startedAt < timeoutMs) {
    const guest = currentAttachedBrowserGuest(sessionId);
    if (guest && !guest.isDestroyed()) {
      return guest;
    }
    await sleep(100);
  }

  return getBrowserGuestForSession(sessionId);
}

function ensureGuestDebugger(guest) {
  const debuggerHandle = guest.debugger;
  if (!debuggerHandle.isAttached()) {
    debuggerHandle.attach("1.3");
  }
  return debuggerHandle;
}

async function sendCdpCommand(guest, method, params = {}) {
  const debuggerHandle = ensureGuestDebugger(guest);
  return debuggerHandle.sendCommand(method, params);
}

async function evaluateGuestJson(guest, expression) {
  const result = await sendCdpCommand(guest, "Runtime.evaluate", {
    expression,
    awaitPromise: true,
    returnByValue: true,
  });
  if (result?.exceptionDetails) {
    throw new Error(result.exceptionDetails.text || "Runtime.evaluate failed");
  }
  return result?.result?.value ?? null;
}

async function waitForGuestToSettle(guest, timeoutSeconds = 20) {
  const timeoutMs = Math.max(1000, timeoutSeconds * 1000);
  if (!guest.isLoadingMainFrame?.() && !guest.isLoading?.()) {
    await sleep(150);
    return;
  }

  await new Promise((resolve, reject) => {
    let finished = false;
    const cleanup = () => {
      guest.removeListener("did-stop-loading", handleDone);
      guest.removeListener("did-finish-load", handleDone);
      guest.removeListener("did-fail-load", handleFail);
      clearTimeout(timer);
    };
    const done = (callback) => {
      if (finished) {
        return;
      }
      finished = true;
      cleanup();
      callback();
    };
    const handleDone = () => {
      setTimeout(() => done(resolve), 150);
    };
    const handleFail = (_event, errorCode, errorDescription, validatedUrl, isMainFrame) => {
      if (isMainFrame === false || errorCode === -3) {
        return;
      }
      done(() => reject(new Error(errorDescription || `failed to load ${validatedUrl || ""}`.trim())));
    };
    const timer = setTimeout(() => {
      done(() => reject(new Error("browser action timed out while waiting for load to settle")));
    }, timeoutMs);

    guest.once("did-stop-loading", handleDone);
    guest.once("did-finish-load", handleDone);
    guest.on("did-fail-load", handleFail);
  });
}

async function captureGuestState(guest) {
  const payload = await evaluateGuestJson(
    guest,
    `(() => {
      const visible = (element) => {
        if (!element || !(element instanceof Element)) return false;
        const rect = element.getBoundingClientRect();
        const style = window.getComputedStyle(element);
        return rect.width > 0 && rect.height > 0 && style.visibility !== "hidden" && style.display !== "none";
      };
      const textFor = (element) => {
        const aria = element.getAttribute("aria-label") || "";
        const placeholder = "placeholder" in element ? (element.placeholder || "") : "";
        const title = element.getAttribute("title") || "";
        const text = (element.innerText || element.textContent || "").trim();
        return [aria, placeholder, title, text].map((value) => String(value || "").trim()).find(Boolean) || element.tagName.toLowerCase();
      };
      const kindFor = (element) => {
        const tag = element.tagName.toLowerCase();
        if (tag === "a") return "link";
        if (tag === "button") return "button";
        if (tag === "textarea") return "input:textarea";
        if (tag === "select") return "input:select";
        if (tag === "input") return "input:" + ((element.getAttribute("type") || "text").toLowerCase());
        if (element.getAttribute("contenteditable") === "true") return "input:contenteditable";
        return (element.getAttribute("role") || "element").toLowerCase();
      };
      const cssEscape = (value) => {
        if (window.CSS?.escape) return window.CSS.escape(value);
        return String(value).replace(/["\\\\#.:\\[\\](),>+~*]/g, "\\\\$&");
      };
      const selectorFor = (element) => {
        if (element.id) return "#" + cssEscape(element.id);
        const parts = [];
        let current = element;
        while (current && current.nodeType === Node.ELEMENT_NODE && parts.length < 4) {
          let part = current.tagName.toLowerCase();
          const name = current.getAttribute("name");
          if (name) part += '[name="' + cssEscape(name) + '"]';
          else {
            const siblings = Array.from(current.parentElement?.children || []).filter((item) => item.tagName === current.tagName);
            if (siblings.length > 1) part += ":nth-of-type(" + (siblings.indexOf(current) + 1) + ")";
          }
          parts.unshift(part);
          current = current.parentElement;
        }
        return parts.join(" > ");
      };
      const interactive = Array.from(document.querySelectorAll('a,button,input,textarea,select,[role="button"],[contenteditable="true"]'));
      const elements = [];
      let index = 1;
      for (const element of interactive) {
        if (!visible(element)) continue;
        const refToken = "e" + index++;
        element.setAttribute("data-hermes-ref", refToken);
        const rect = element.getBoundingClientRect();
        const href = element.tagName.toLowerCase() === "a" ? (element.href || null) : null;
        const fieldName = "name" in element ? (element.name || null) : null;
        const value = "value" in element ? (element.value || null) : null;
        elements.push({
          refId: "@" + refToken,
          kind: kindFor(element),
          label: textFor(element),
          target: href,
          role: element.getAttribute("role") || element.tagName.toLowerCase(),
          selector: selectorFor(element),
          bbox: {
            x: Math.round(rect.left),
            y: Math.round(rect.top),
            width: Math.round(rect.width),
            height: Math.round(rect.height),
          },
          disabled: "disabled" in element ? Boolean(element.disabled) : null,
          checked: "checked" in element ? Boolean(element.checked) : null,
          selected: "selected" in element ? Boolean(element.selected) : null,
          required: "required" in element ? Boolean(element.required) : null,
          fieldName,
          value,
          formId: element.form?.id || null,
          formAction: element.form?.action || null,
          formMethod: element.form?.method || null,
        });
      }

      const bodyText = (document.body?.innerText || document.documentElement?.innerText || "")
        .replace(/\\n{3,}/g, "\\n\\n")
        .trim();
      const images = Array.from(document.images || [])
        .slice(0, 64)
        .map((image) => ({
          src: image.currentSrc || image.src || "",
          alt: image.alt || "",
        }))
        .filter((image) => image.src);

      return {
        url: location.href,
        finalUrl: location.href,
        contentType: document.contentType || "text/html",
        title: document.title || null,
        content: bodyText,
        elements,
        images,
        truncatedBody: false,
      };
    })()`,
  );
  return payload;
}

async function devtoolsNavigate(guest, url, timeoutSeconds) {
  if (typeof url !== "string" || !url.trim()) {
    throw new Error("navigate requires a non-empty url");
  }
  await sendCdpCommand(guest, "Page.enable");
  await sendCdpCommand(guest, "Page.navigate", { url: url.trim() });
  await waitForGuestToSettle(guest, timeoutSeconds);
  return captureGuestState(guest);
}

async function devtoolsBack(guest, timeoutSeconds) {
  const history = await sendCdpCommand(guest, "Page.getNavigationHistory");
  const currentIndex = Number(history?.currentIndex ?? 0);
  const previousEntry = Array.isArray(history?.entries) ? history.entries[currentIndex - 1] : null;
  if (!previousEntry?.id) {
    throw new Error("browser back has no previous entry");
  }
  await sendCdpCommand(guest, "Page.navigateToHistoryEntry", { entryId: previousEntry.id });
  await waitForGuestToSettle(guest, timeoutSeconds);
  return captureGuestState(guest);
}

async function devtoolsForward(guest, timeoutSeconds) {
  const history = await sendCdpCommand(guest, "Page.getNavigationHistory");
  const currentIndex = Number(history?.currentIndex ?? 0);
  const nextEntry = Array.isArray(history?.entries) ? history.entries[currentIndex + 1] : null;
  if (!nextEntry?.id) {
    throw new Error("browser forward has no next entry");
  }
  await sendCdpCommand(guest, "Page.navigateToHistoryEntry", { entryId: nextEntry.id });
  await waitForGuestToSettle(guest, timeoutSeconds);
  return captureGuestState(guest);
}

function normalizeReferenceToken(reference) {
  if (typeof reference !== "string" || !reference.trim()) {
    throw new Error("browser action requires a reference");
  }
  return reference.trim().replace(/^@/, "");
}

async function resolveElementCenter(guest, reference) {
  const refToken = normalizeReferenceToken(reference);
  const point = await evaluateGuestJson(
    guest,
    `(() => {
      const refToken = ${JSON.stringify(refToken)};
      const element = document.querySelector('[data-hermes-ref="' + refToken + '"]');
      if (!element) return null;
      element.scrollIntoView({ block: "center", inline: "center" });
      const rect = element.getBoundingClientRect();
      return {
        x: Math.round(rect.left + rect.width / 2),
        y: Math.round(rect.top + rect.height / 2),
      };
    })()`,
  );
  if (!point || typeof point.x !== "number" || typeof point.y !== "number") {
    throw new Error(`browser element ${reference} was not found in the current Chromium snapshot`);
  }
  return point;
}

async function devtoolsClick(guest, reference, timeoutSeconds) {
  const point = await resolveElementCenter(guest, reference);
  await sendCdpCommand(guest, "Input.dispatchMouseEvent", {
    type: "mouseMoved",
    x: point.x,
    y: point.y,
    button: "left",
  });
  await sendCdpCommand(guest, "Input.dispatchMouseEvent", {
    type: "mousePressed",
    x: point.x,
    y: point.y,
    button: "left",
    clickCount: 1,
  });
  await sendCdpCommand(guest, "Input.dispatchMouseEvent", {
    type: "mouseReleased",
    x: point.x,
    y: point.y,
    button: "left",
    clickCount: 1,
  });
  await waitForGuestToSettle(guest, timeoutSeconds);
  return captureGuestState(guest);
}

async function devtoolsScreenshot(guest, fullPage, annotate, timeoutSeconds) {
  await waitForGuestToSettle(guest, timeoutSeconds);
  await sendCdpCommand(guest, "Page.enable");
  if (annotate) {
    await captureGuestState(guest);
    await evaluateGuestJson(
      guest,
      `(() => {
        const existing = document.getElementById("hermes-screenshot-annotations");
        if (existing) existing.remove();
        const root = document.createElement("div");
        root.id = "hermes-screenshot-annotations";
        root.style.position = "absolute";
        root.style.left = "0";
        root.style.top = "0";
        root.style.zIndex = "2147483647";
        root.style.pointerEvents = "none";
        for (const element of Array.from(document.querySelectorAll("[data-hermes-ref]"))) {
          const rect = element.getBoundingClientRect();
          if (rect.width <= 0 || rect.height <= 0) continue;
          const label = document.createElement("div");
          label.textContent = "@" + element.getAttribute("data-hermes-ref");
          label.style.position = "absolute";
          label.style.left = Math.round(rect.left + window.scrollX) + "px";
          label.style.top = Math.round(rect.top + window.scrollY) + "px";
          label.style.padding = "2px 5px";
          label.style.borderRadius = "4px";
          label.style.background = "#ffcc00";
          label.style.color = "#111111";
          label.style.font = "bold 12px system-ui, sans-serif";
          label.style.boxShadow = "0 1px 4px rgba(0,0,0,.35)";
          root.appendChild(label);
        }
        document.documentElement.appendChild(root);
        return true;
      })()`,
    );
  }

  let captureParams = {
    format: "png",
    fromSurface: true,
    captureBeyondViewport: Boolean(fullPage),
  };
  if (fullPage) {
    const metrics = await sendCdpCommand(guest, "Page.getLayoutMetrics");
    const size = metrics?.cssContentSize || metrics?.contentSize;
    if (size && Number.isFinite(Number(size.width)) && Number.isFinite(Number(size.height))) {
      captureParams = {
        ...captureParams,
        clip: {
          x: 0,
          y: 0,
          width: Math.ceil(Number(size.width)),
          height: Math.ceil(Number(size.height)),
          scale: 1,
        },
      };
    }
  }
  try {
    const captured = await sendCdpCommand(guest, "Page.captureScreenshot", captureParams);
    return { data: captured?.data || "" };
  } finally {
    if (annotate) {
      try {
        await evaluateGuestJson(guest, `document.getElementById("hermes-screenshot-annotations")?.remove(); true`);
      } catch {
        // Best-effort cleanup only.
      }
    }
  }
}

async function devtoolsHover(guest, reference, timeoutSeconds) {
  const point = await resolveElementCenter(guest, reference);
  await sendCdpCommand(guest, "Input.dispatchMouseEvent", {
    type: "mouseMoved",
    x: point.x,
    y: point.y,
    button: "none",
  });
  await sleep(Math.min(Math.max(timeoutSeconds * 50, 100), 400));
  return captureGuestState(guest);
}

async function devtoolsFill(guest, reference, text, timeoutSeconds) {
  const refToken = normalizeReferenceToken(reference);
  await evaluateGuestJson(
    guest,
    `(() => {
      const refToken = ${JSON.stringify(refToken)};
      const nextValue = ${JSON.stringify(text)};
      const element = document.querySelector('[data-hermes-ref="' + refToken + '"]');
      if (!element) return false;
      element.scrollIntoView({ block: "center", inline: "center" });
      element.focus();
      if ("value" in element) {
        element.value = nextValue;
      } else {
        element.textContent = nextValue;
      }
      element.dispatchEvent(new Event("input", { bubbles: true }));
      element.dispatchEvent(new Event("change", { bubbles: true }));
      return true;
    })()`,
  );
  await waitForGuestToSettle(guest, timeoutSeconds);
  return captureGuestState(guest);
}

async function devtoolsSelect(guest, reference, value, label, index, timeoutSeconds) {
  const refToken = normalizeReferenceToken(reference);
  const selected = await evaluateGuestJson(
    guest,
    `(() => {
      const refToken = ${JSON.stringify(refToken)};
      const nextValue = ${JSON.stringify(value)};
      const nextLabel = ${JSON.stringify(label)};
      const nextIndex = ${index === null ? "null" : Number(index)};
      const element = document.querySelector('[data-hermes-ref="' + refToken + '"]');
      if (!(element instanceof HTMLSelectElement)) {
        throw new Error("target element is not a <select>");
      }
      element.scrollIntoView({ block: "center", inline: "center" });
      let matched = false;
      if (typeof nextValue === "string" && nextValue.length) {
        matched = Array.from(element.options).some((option) => {
          if (option.value !== nextValue) return false;
          element.value = option.value;
          return true;
        });
      }
      if (!matched && typeof nextLabel === "string" && nextLabel.length) {
        matched = Array.from(element.options).some((option) => {
          if ((option.label || option.textContent || "").trim() !== nextLabel) return false;
          element.value = option.value;
          return true;
        });
      }
      if (!matched && Number.isInteger(nextIndex) && nextIndex >= 0 && nextIndex < element.options.length) {
        element.selectedIndex = nextIndex;
        matched = true;
      }
      if (!matched) {
        return false;
      }
      element.dispatchEvent(new Event("input", { bubbles: true }));
      element.dispatchEvent(new Event("change", { bubbles: true }));
      return true;
    })()`,
  );
  if (!selected) {
    throw new Error("browser select could not match any option");
  }
  await waitForGuestToSettle(guest, timeoutSeconds);
  return captureGuestState(guest);
}

async function resolveElementNodeId(guest, reference) {
  const refToken = normalizeReferenceToken(reference);
  const runtime = await sendCdpCommand(guest, "Runtime.evaluate", {
    expression: `document.querySelector('[data-hermes-ref="${refToken}"]')`,
    awaitPromise: true,
  });
  const objectId = runtime?.result?.objectId;
  if (!objectId) {
    throw new Error(`browser element ${reference} was not found in the current Chromium snapshot`);
  }
  const node = await sendCdpCommand(guest, "DOM.requestNode", { objectId });
  const nodeId = node?.nodeId;
  await sendCdpCommand(guest, "Runtime.releaseObject", { objectId }).catch(() => {});
  if (!nodeId) {
    throw new Error(`failed to resolve browser element ${reference} as DOM node`);
  }
  return nodeId;
}

async function devtoolsUpload(guest, reference, files, timeoutSeconds) {
  if (!Array.isArray(files) || files.length === 0) {
    throw new Error("upload requires at least one file path");
  }
  await sendCdpCommand(guest, "DOM.enable");
  const nodeId = await resolveElementNodeId(guest, reference);
  await sendCdpCommand(guest, "DOM.setFileInputFiles", {
    nodeId,
    files: files.map((item) => String(item)),
  });
  await waitForGuestToSettle(guest, timeoutSeconds);
  return captureGuestState(guest);
}

async function devtoolsPress(guest, key, timeoutSeconds) {
  if (typeof key !== "string" || !key.trim()) {
    throw new Error("press requires a non-empty key");
  }
  const normalized = key.trim();
  const text = normalized.length === 1 ? normalized : "";
  await sendCdpCommand(guest, "Input.dispatchKeyEvent", {
    type: "keyDown",
    key: normalized,
    text,
    unmodifiedText: text,
  });
  if (text) {
    await sendCdpCommand(guest, "Input.dispatchKeyEvent", {
      type: "char",
      key: normalized,
      text,
      unmodifiedText: text,
    });
  }
  await sendCdpCommand(guest, "Input.dispatchKeyEvent", {
    type: "keyUp",
    key: normalized,
  });
  await waitForGuestToSettle(guest, timeoutSeconds);
  return captureGuestState(guest);
}

async function devtoolsScroll(guest, direction, amount, timeoutSeconds) {
  const delta = String(direction) === "up" ? -Math.abs(amount) : Math.abs(amount);
  await evaluateGuestJson(
    guest,
    `(() => {
      window.scrollBy({ top: ${Number(delta)}, left: 0, behavior: "instant" });
      return true;
    })()`,
  );
  await waitForGuestToSettle(guest, timeoutSeconds);
  return captureGuestState(guest);
}

async function devtoolsWait(guest, selector, text, timeoutSeconds, pollIntervalMs) {
  const deadline = Date.now() + Math.max(timeoutSeconds, 1) * 1000;
  const pollMs = Math.max(50, Math.min(Number(pollIntervalMs) || 200, 1000));
  const selectorValue = typeof selector === "string" && selector.trim() ? selector.trim() : null;
  const textValue = typeof text === "string" && text.trim() ? text.trim() : null;
  if (!selectorValue && !textValue) {
    await sleep(pollMs);
    return captureGuestState(guest);
  }

  while (Date.now() <= deadline) {
    const matched = await evaluateGuestJson(
      guest,
      `(() => {
        const selector = ${JSON.stringify(selectorValue)};
        const text = ${JSON.stringify(textValue)};
        if (selector && document.querySelector(selector)) {
          return true;
        }
        if (text) {
          const bodyText = (document.body?.innerText || document.documentElement?.innerText || "");
          if (bodyText.includes(text)) {
            return true;
          }
        }
        return false;
      })()`,
    );
    if (matched) {
      return captureGuestState(guest);
    }
    await sleep(pollMs);
  }

  throw new Error("browser wait timed out before the requested condition was met");
}

async function devtoolsEval(guest, expression, timeoutSeconds) {
  if (typeof expression !== "string" || !expression.trim()) {
    throw new Error("eval requires a non-empty expression");
  }
  const result = await sendCdpCommand(guest, "Runtime.evaluate", {
    expression,
    awaitPromise: true,
    returnByValue: true,
    timeout: Math.max(timeoutSeconds, 1) * 1000,
  });
  if (result?.exceptionDetails) {
    throw new Error(result.exceptionDetails.text || "browser eval failed");
  }
  return {
    result: result?.result?.value ?? null,
  };
}

async function startCronScheduler(request) {
  if (!request?.workspaceRoot) {
    throw new Error("workspaceRoot is required");
  }

  stopCronSchedulerTimer();
  cronSchedulerRuntime.generation += 1;
  cronSchedulerRuntime.request = { ...request };
  cronSchedulerStatus.running = true;
  cronSchedulerStatus.paused_reason = null;
  cronSchedulerStatus.last_error = null;
  cronSchedulerStatus.last_due_job_ids = [];
  cronSchedulerStatus.workspace_root = request.workspaceRoot;
  cronSchedulerStatus.tick_interval_seconds = Math.max(
    15,
    Number(request.tickIntervalSeconds) || cronSchedulerStatus.tick_interval_seconds || 60,
  );

  scheduleCronSchedulerTick(cronSchedulerRuntime.generation, 0);
  return { ...cronSchedulerStatus };
}

async function stopCronScheduler() {
  stopCronSchedulerTimer();
  cronSchedulerRuntime.generation += 1;
  cronSchedulerRuntime.request = null;
  cronSchedulerStatus.running = false;
  cronSchedulerStatus.paused_reason = null;
  return { ...cronSchedulerStatus };
}

function stopCronSchedulerTimer() {
  if (cronSchedulerRuntime.timer) {
    clearTimeout(cronSchedulerRuntime.timer);
    cronSchedulerRuntime.timer = null;
  }
}

function scheduleCronSchedulerTick(generation, delayMs) {
  stopCronSchedulerTimer();
  cronSchedulerRuntime.timer = setTimeout(() => {
    void runCronSchedulerTick(generation);
  }, delayMs);
}

async function runCronSchedulerTick(generation) {
  if (generation !== cronSchedulerRuntime.generation || !cronSchedulerRuntime.request) {
    return;
  }

  const request = cronSchedulerRuntime.request;
  const dataDir = resolveSchedulerDataDir(request);
  const now = unixNow();

  try {
    const pauseReason = await currentSchedulerPauseReason(dataDir);
    if (pauseReason) {
      cronSchedulerStatus.last_tick_at_unix = now;
      cronSchedulerStatus.last_due_job_ids = [];
      cronSchedulerStatus.last_error = null;
      cronSchedulerStatus.paused_reason = pauseReason;
    } else {
      const dueJobIds = await invokeRustBridge("list_due_cron_jobs", {
        dataDir,
        nowUnix: now,
      });
      const normalizedDueJobIds = Array.isArray(dueJobIds) ? dueJobIds.filter((item) => typeof item === "string") : [];
      cronSchedulerStatus.last_tick_at_unix = now;
      cronSchedulerStatus.last_due_job_ids = normalizedDueJobIds;
      cronSchedulerStatus.last_error = null;
      cronSchedulerStatus.paused_reason = null;

      for (const jobId of normalizedDueJobIds) {
        if (generation !== cronSchedulerRuntime.generation || !cronSchedulerRuntime.request) {
          return;
        }
        try {
          await handleRustBridgeCommand("run_cron_job", {
            request: buildCronRunRequest(request, jobId),
          });
        } catch (error) {
          cronSchedulerStatus.last_error = formatSchedulerError(error);
        }
      }
    }
  } catch (error) {
    cronSchedulerStatus.last_tick_at_unix = now;
    cronSchedulerStatus.last_error = formatSchedulerError(error);
    cronSchedulerStatus.paused_reason = null;
  }

  if (generation !== cronSchedulerRuntime.generation || !cronSchedulerRuntime.request) {
    return;
  }
  scheduleCronSchedulerTick(generation, cronSchedulerStatus.tick_interval_seconds * 1000);
}

async function currentSchedulerPauseReason(dataDir) {
  if (activeInteractiveRuns > 0) {
    return "对话运行中";
  }
  try {
    const approvals = await invokeRustBridge("list_approvals", { dataDir });
    if (Array.isArray(approvals) && approvals.some((item) => item?.status === "pending")) {
      return "存在待处理审批";
    }
    return null;
  } catch (error) {
    return `审批状态检查失败: ${formatSchedulerError(error)}`;
  }
}

function resolveSchedulerDataDir(request) {
  if (request?.dataDir) {
    return request.dataDir;
  }
  return path.join(request.workspaceRoot, ".hermes-agent-rs");
}

function buildCronRunRequest(request, jobId) {
  return {
    workspaceRoot: request.workspaceRoot,
    dataDir: request.dataDir || null,
    jobId,
    provider: request.provider || null,
    model: request.model || null,
    baseUrl: request.baseUrl || null,
    apiKey: request.apiKey || null,
    auxProvider: request.auxProvider || null,
    auxModel: request.auxModel || null,
    auxBaseUrl: request.auxBaseUrl || null,
    auxApiKey: request.auxApiKey || null,
    maxIterations: request.maxIterations ?? null,
    systemPromptOverride: request.systemPromptOverride || null,
    enableShellTool: Boolean(request.enableShellTool),
  };
}

function formatSchedulerError(error) {
  if (error instanceof Error) {
    return error.message;
  }
  return String(error);
}

function unixNow() {
  return Math.floor(Date.now() / 1000);
}

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

async function handleRustBridgeCommand(command, args = {}) {
  const trackInteractive = INTERACTIVE_BRIDGE_COMMANDS.has(command);
  if (trackInteractive) {
    activeInteractiveRuns += 1;
  }

  let payload;
  try {
    payload = await invokeRustBridge(command, args);
  } finally {
    if (trackInteractive) {
      activeInteractiveRuns = Math.max(0, activeInteractiveRuns - 1);
    }
  }

  if (EVENTFUL_BRIDGE_COMMANDS.has(command)) {
    const events = Array.isArray(payload?.events) ? payload.events : [];
    const result = payload?.result ?? null;
    replayAgentEvents(events);
    updateLastSessionId(result);
    if (command !== "retry_delegate_run" && result?.status === "completed") {
      emitRunCompleted(result);
    }
    return result;
  }

  if (command === "clear_session") {
    updateLastSessionId(payload);
    emitRendererEvent("hermes://agent/cleared", payload);
    return payload;
  }

  if (command === "load_session") {
    if (args?.remember !== false && payload?.summary?.session_id) {
      lastSessionId = payload.summary.session_id;
    }
    return payload;
  }

  updateLastSessionId(payload);
  return payload;
}

async function invokeRustBridge(command, args = {}) {
  const requestBody = JSON.stringify({ command, args });
  const processConfig = await resolveRustBridgeProcess();
  const expectsStreamingFrames = EVENTFUL_BRIDGE_COMMANDS.has(command);

  return new Promise((resolve, reject) => {
    const child = spawn(processConfig.command, processConfig.args, {
      cwd: processConfig.cwd,
      env: {
        ...process.env,
        RUST_LOG: process.env.RUST_LOG || "warn",
        NO_COLOR: process.env.NO_COLOR || "1",
        CLICOLOR: "0",
        CLICOLOR_FORCE: "0",
      },
      stdio: ["pipe", "pipe", "pipe"],
    });

    let stdout = "";
    let stderr = "";
    let stdoutBuffer = "";
    let streamedResult = undefined;
    let sawStreamingFrame = false;

    child.stdout.on("data", (chunk) => {
      const text = chunk.toString("utf8");
      stdout += text;
      if (!expectsStreamingFrames) {
        return;
      }
      stdoutBuffer += text;
      try {
        const parsed = consumeRustBridgeStreamFrames(stdoutBuffer, {
          onEvent(envelope) {
            sawStreamingFrame = true;
            replayAgentEvents([envelope]);
          },
          onResult(payload) {
            sawStreamingFrame = true;
            streamedResult = payload;
          },
        });
        stdoutBuffer = parsed.remainder;
      } catch (error) {
        stderr += `\n[rust-bridge-stdout-parse] ${error instanceof Error ? error.message : String(error)}`;
      }
    });
    child.stderr.on("data", (chunk) => {
      stderr += chunk.toString("utf8");
    });
    child.on("error", (error) => {
      reject(new Error(`failed to start Rust bridge for ${command}: ${error.message}`));
    });
    child.on("close", (code) => {
      if (code !== 0) {
        reject(new Error(formatRustBridgeFailure(command, code, stdout, stderr)));
        return;
      }
      try {
        if (expectsStreamingFrames) {
          const parsed = consumeRustBridgeStreamFrames(stdoutBuffer, {
            onEvent(envelope) {
              sawStreamingFrame = true;
              replayAgentEvents([envelope]);
            },
            onResult(payload) {
              sawStreamingFrame = true;
              streamedResult = payload;
            },
          }, { flush: true });
          stdoutBuffer = parsed.remainder;
          if (streamedResult !== undefined) {
            resolve(streamedResult);
            return;
          }
          if (sawStreamingFrame) {
            reject(
              new Error(
                `Rust bridge command "${command}" finished without a result frame.\nstdout:\n${stdout}\nstderr:\n${stderr}`,
              ),
            );
            return;
          }
        }
        resolve(parseRustBridgeResponse(stdout));
      } catch (error) {
        const message = error instanceof Error ? error.message : String(error);
        reject(
          new Error(
            `failed to parse Rust bridge response for ${command}: ${message}\nstdout:\n${stdout}\nstderr:\n${stderr}`,
          ),
        );
      }
    });

    child.stdin.end(requestBody);
  });
}

async function resolveRustBridgeProcess() {
  const configuredBinary = process.env.HERMES_RUST_BRIDGE_BIN;
  if (configuredBinary) {
    return {
      command: configuredBinary,
      args: ["desktop-bridge"],
      cwd: rustRoot,
    };
  }

  try {
    await fs.access(rustDebugBinary);
    return {
      command: rustDebugBinary,
      args: ["desktop-bridge"],
      cwd: rustRoot,
    };
  } catch {}

  return {
    command: "cargo",
    args: ["run", "--quiet", "--manifest-path", rustManifestPath, "--", "desktop-bridge"],
    cwd: rustRoot,
  };
}

function parseRustBridgeResponse(stdout) {
  const sanitized = stripAnsi(stdout || "");
  const trimmed = sanitized.trim();
  if (!trimmed) {
    return null;
  }
  const lines = trimmed.split(/\r?\n/).map((line) => line.trim()).filter(Boolean);
  for (let index = lines.length - 1; index >= 0; index -= 1) {
    const parsed = tryParseJsonLine(lines[index]);
    if (parsed !== undefined) {
      return parsed;
    }
  }
  throw new Error("no JSON payload found in Rust bridge stdout");
}

function consumeRustBridgeStreamFrames(buffer, handlers, options = {}) {
  const flush = Boolean(options.flush);
  const lines = buffer.split(/\r?\n/);
  const remainder = flush ? "" : lines.pop() ?? "";

  for (const rawLine of lines) {
    const line = stripAnsi(rawLine).trim();
    if (!line) {
      continue;
    }
    const frame = tryParseJsonLine(line);
    if (frame === undefined) {
      continue;
    }
    if (!frame || typeof frame !== "object") {
      continue;
    }
    if (frame.type === "event") {
      handlers.onEvent?.(frame.payload);
      continue;
    }
    if (frame.type === "result") {
      handlers.onResult?.(frame.payload);
    }
  }

  return { remainder };
}

function stripAnsi(value) {
  return String(value || "").replace(/\u001b\[[0-9;?]*[ -/]*[@-~]/g, "");
}

function tryParseJsonLine(line) {
  const trimmed = String(line || "").trim();
  if (!trimmed || (!trimmed.startsWith("{") && !trimmed.startsWith("["))) {
    return undefined;
  }
  try {
    return JSON.parse(trimmed);
  } catch {
    return undefined;
  }
}

function formatRustBridgeFailure(command, code, stdout, stderr) {
  const output = [stderr.trim(), stdout.trim()].filter(Boolean).join("\n");
  if (!output) {
    return `Rust bridge command "${command}" failed with exit code ${code}`;
  }
  return `Rust bridge command "${command}" failed with exit code ${code}\n${output}`;
}

function replayAgentEvents(events) {
  for (const envelope of events) {
    emitRendererEvent("hermes://agent/event", envelope);
    const sessionId = eventSessionId(envelope);
    if (sessionId) {
      emitRendererEvent(`hermes://agent/event/${sessionId}`, envelope);
    }
  }
}

function emitRunCompleted(result) {
  if (!result?.session_id) {
    return;
  }
  emitRendererEvent("hermes://agent/done", {
    session_id: result.session_id,
    response: result.response,
  });
  emitRendererEvent(`hermes://agent/done/${result.session_id}`, result);
}

function emitRendererEvent(eventName, payload) {
  if (!mainWindow || mainWindow.isDestroyed()) {
    return;
  }
  mainWindow.webContents.send(eventName, payload);
}

function eventSessionId(envelope) {
  const sessionId = envelope?.event?.session_id;
  if (typeof sessionId !== "string") {
    return null;
  }
  const trimmed = sessionId.trim();
  return trimmed || null;
}

function updateLastSessionId(payload) {
  const sessionId = payload?.session_id;
  if (typeof sessionId === "string" && sessionId.trim()) {
    lastSessionId = sessionId;
  }
}

async function listWorkspaceTree(workspaceRoot) {
  const rootPath = workspaceRoot || "";
  if (!rootPath) {
    return {
      rootPath,
      nodes: [],
      truncated: false,
    };
  }

  const counter = { count: 0, truncated: false };
  const nodes = await readTreeNodes(rootPath, counter);
  return {
    rootPath,
    nodes,
    truncated: counter.truncated,
  };
}

async function openWorkspaceFile(workspaceRoot, filePath) {
  const rootPath = workspaceRoot || "";
  const targetPath = resolveWorkspaceFile(rootPath, filePath || "");
  const errorMessage = await shell.openPath(targetPath);
  if (errorMessage) {
    throw new Error(errorMessage);
  }
  return {
    ok: true,
    path: targetPath,
  };
}

async function latestContextDebugSnapshot(dataDir, sessionId) {
  if (!dataDir || !sessionId) {
    throw new Error("latest_context_debug_snapshot requires dataDir and sessionId");
  }
  const debugDir = path.join(dataDir, "runtime", "context-debug", sessionId);
  let entries = [];
  try {
    entries = await fs.readdir(debugDir, { withFileTypes: true });
  } catch (error) {
    if (error && typeof error === "object" && error.code === "ENOENT") {
      return { path: null, debugDir };
    }
    throw error;
  }
  const files = entries
    .filter((entry) => entry.isFile() && entry.name.endsWith(".json"))
    .map((entry) => entry.name)
    .sort((a, b) => b.localeCompare(a));
  return {
    path: files.length ? path.join(debugDir, files[0]) : null,
    debugDir,
  };
}

async function previewSlidevDeck(workspaceRoot, filePath) {
  const rootPath = path.resolve(workspaceRoot || "");
  const targetPath = resolveWorkspaceFile(rootPath, filePath || "");
  const extension = path.extname(targetPath).slice(1).toLowerCase();
  if (extension !== "md" && extension !== "mdx") {
    throw new Error("Slidev preview only supports .md and .mdx files");
  }

  const realRoot = await fs.realpath(rootPath);
  const realTarget = await fs.realpath(targetPath);
  if (!isPathInside(realRoot, realTarget)) {
    throw new Error(`path escapes workspace root: ${filePath}`);
  }

  const key = realTarget;
  const existing = slidevPreviews.get(key);
  if (existing && existing.child.exitCode == null && !existing.child.killed) {
    return slidevPreviewPayload(realRoot, realTarget, existing);
  }
  if (existing) {
    slidevPreviews.delete(key);
  }

  const port = await allocateLocalPort();
  const commandConfig = await resolveSlidevCommand(realRoot);
  const args = [
    ...commandConfig.args,
    realTarget,
    "--port",
    String(port),
    "--remote",
    "--bind",
    "127.0.0.1",
  ];
  const child = spawn(commandConfig.command, args, {
    cwd: path.dirname(realTarget),
    env: {
      ...process.env,
      BROWSER: "none",
      NO_COLOR: process.env.NO_COLOR || "1",
    },
    stdio: ["ignore", "pipe", "pipe"],
  });
  const preview = {
    child,
    command: commandConfig.command,
    args,
    port,
    spawnError: null,
    stderr: "",
    stdout: "",
    url: `http://127.0.0.1:${port}/`,
  };
  slidevPreviews.set(key, preview);

  child.stdout.on("data", (chunk) => {
    preview.stdout = trimProcessLog(`${preview.stdout}${chunk.toString("utf8")}`);
  });
  child.stderr.on("data", (chunk) => {
    preview.stderr = trimProcessLog(`${preview.stderr}${chunk.toString("utf8")}`);
  });
  child.on("exit", () => {
    const current = slidevPreviews.get(key);
    if (current === preview) {
      slidevPreviews.delete(key);
    }
  });
  child.on("error", (error) => {
    preview.spawnError = error;
  });

  try {
    await waitForHttpOk(preview.url, 20_000, () => preview.spawnError || child.exitCode != null || child.killed);
  } catch (error) {
    child.kill();
    slidevPreviews.delete(key);
    const detail = [preview.stderr, preview.stdout]
      .map((value) => value.trim())
      .filter(Boolean)
      .join("\n")
      .trim();
    const suffix = detail ? `\n${detail}` : "";
    const message = preview.spawnError
      ? preview.spawnError.message
      : error instanceof Error
        ? error.message
        : String(error);
    throw new Error(`failed to start Slidev preview: ${message}${suffix}`);
  }

  return slidevPreviewPayload(realRoot, realTarget, preview);
}

async function exportSlidevDeck(workspaceRoot, filePath, format, outputPath) {
  const rootPath = path.resolve(workspaceRoot || "");
  const targetPath = resolveWorkspaceFile(rootPath, filePath || "");
  const extension = path.extname(targetPath).slice(1).toLowerCase();
  if (extension !== "md" && extension !== "mdx") {
    throw new Error("Slidev export only supports .md and .mdx files");
  }

  const normalizedFormat = format === "pptx" ? "pptx" : "pdf";
  const realRoot = await fs.realpath(rootPath);
  const realTarget = await fs.realpath(targetPath);
  if (!isPathInside(realRoot, realTarget)) {
    throw new Error(`path escapes workspace root: ${filePath}`);
  }

  const preview = await previewSlidevDeck(realRoot, path.relative(realRoot, realTarget));
  const exportPath = resolveWorkspaceFile(
    realRoot,
    outputPath || path.relative(realRoot, defaultSlidevExportPath(realTarget, normalizedFormat)),
  );
  await fs.mkdir(path.dirname(exportPath), { recursive: true });

  const exportUrl = buildSlidevExportUrl(preview.url);
  if (normalizedFormat === "pptx") {
    await exportSlidevPptxViaHiddenWindow(exportUrl, exportPath, realTarget);
  } else {
    await exportSlidevPdfViaHiddenWindow(exportUrl, exportPath);
  }

  return {
    ok: true,
    format: normalizedFormat,
    path: exportPath,
    displayPath: path.relative(realRoot, exportPath) || path.basename(exportPath),
    fileName: path.basename(exportPath),
    sourceDeckPath: realTarget,
    sourceDeckDisplayPath: path.relative(realRoot, realTarget) || path.basename(realTarget),
    experimental: normalizedFormat === "pptx",
  };
}

function slidevPreviewPayload(rootPath, targetPath, preview) {
  return {
    ok: true,
    path: targetPath,
    displayPath: path.relative(rootPath, targetPath) || path.basename(targetPath),
    fileName: path.basename(targetPath),
    port: preview.port,
    url: preview.url,
  };
}

function buildSlidevExportUrl(previewUrl) {
  return new URL("export", previewUrl).toString();
}

function defaultSlidevExportPath(deckPath, format) {
  const parsed = path.parse(deckPath);
  return path.join(parsed.dir, `${parsed.name}-export.${format}`);
}

async function exportSlidevPdfViaHiddenWindow(exportUrl, outputPath) {
  const window = createHiddenSlidevWindow();
  try {
    await window.loadURL(exportUrl);
    await waitForSlidevExportReady(window, { requireSlides: true });
    await delay(500);
    const buffer = await window.webContents.printToPDF({
      printBackground: true,
      preferCSSPageSize: true,
    });
    await fs.writeFile(outputPath, buffer);
  } finally {
    await destroyWindow(window);
  }
}

async function exportSlidevPptxViaHiddenWindow(exportUrl, outputPath, deckPath) {
  const window = createHiddenSlidevWindow();
  try {
    await window.loadURL(exportUrl);
    await waitForSlidevExportReady(window, { requireSlides: true });
    await delay(500);

    const slideCount = await window.webContents.executeJavaScript(
      `(() => document.querySelectorAll('.print-slide-container').length)()`,
      true,
    );
    if (!slideCount) {
      throw new Error("no slides were rendered for PPTX export");
    }

    const captures = [];
    for (let index = 0; index < slideCount; index += 1) {
      const rect = await window.webContents.executeJavaScript(
        `(() => {
          const slide = document.querySelectorAll('.print-slide-container')[${index}];
          if (!slide) return null;
          slide.scrollIntoView({ block: 'start', inline: 'nearest' });
          const bounds = slide.getBoundingClientRect();
          return {
            x: Math.max(0, Math.floor(bounds.left)),
            y: Math.max(0, Math.floor(bounds.top)),
            width: Math.max(1, Math.ceil(bounds.width)),
            height: Math.max(1, Math.ceil(bounds.height)),
          };
        })()`,
        true,
      );
      if (!rect) {
        throw new Error(`failed to resolve rendered bounds for slide ${index + 1}`);
      }
      await delay(250);
      const image = await window.capturePage(rect);
      captures.push({
        png: image.toPNG(),
        size: image.getSize(),
      });
    }

    const { default: PptxGenJS } = await import("pptxgenjs");
    const pptx = new PptxGenJS();
    const firstImage = captures[0];
    const fallbackSize = await imageSizeFromPng(firstImage.png);
    const widthPx = firstImage.size?.width || fallbackSize.width;
    const heightPx = firstImage.size?.height || fallbackSize.height;
    const layoutName = `${widthPx}x${heightPx}`;
    pptx.defineLayout({
      name: layoutName,
      width: widthPx / 96,
      height: heightPx / 96,
    });
    pptx.layout = layoutName;
    pptx.company = "Created using Slidev (Electron hidden-window export)";
    pptx.title = path.parse(deckPath).name;

    for (const capture of captures) {
      const slide = pptx.addSlide();
      slide.background = {
        data: `data:image/png;base64,${capture.png.toString("base64")}`,
      };
    }

    const buffer = await pptx.write({
      outputType: "nodebuffer",
      compression: true,
    });
    await fs.writeFile(outputPath, buffer);
  } finally {
    await destroyWindow(window);
  }
}

function createHiddenSlidevWindow() {
  return new BrowserWindow({
    show: false,
    width: 1800,
    height: 1200,
    backgroundColor: "#111827",
    autoHideMenuBar: true,
    webPreferences: {
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: false,
    },
  });
}

async function waitForSlidevExportReady(window, { requireSlides = false } = {}) {
  await window.webContents.executeJavaScript(
    `new Promise((resolve, reject) => {
      const startedAt = Date.now();
      const tick = () => {
        const exportRoot = document.querySelector('#export-container');
        const slideCount = document.querySelectorAll('.print-slide-container').length;
        const pendingImages = Array.from(document.images || []).some((image) => !image.complete);
        if (exportRoot && (!${requireSlides} || slideCount > 0) && !pendingImages) {
          resolve(true);
          return;
        }
        if (Date.now() - startedAt > 30000) {
          reject(new Error('timed out waiting for Slidev export page'));
          return;
        }
        setTimeout(tick, 100);
      };
      tick();
    })`,
    true,
  );
}

async function destroyWindow(window) {
  if (window.isDestroyed()) {
    return;
  }
  window.destroy();
  await Promise.resolve();
}

async function imageSizeFromPng(buffer) {
  const signature = buffer.subarray(0, 8).toString("hex");
  if (signature !== "89504e470d0a1a0a" || buffer.length < 24) {
    throw new Error("captured image is not a valid PNG");
  }
  return {
    width: buffer.readUInt32BE(16),
    height: buffer.readUInt32BE(20),
  };
}

function delay(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

async function resolveSlidevCommand(workspaceRoot) {
  const binaryName = process.platform === "win32" ? "slidev.cmd" : "slidev";
  const candidates = [
    path.join(appRoot, "node_modules", ".bin", binaryName),
    path.join(process.resourcesPath || "", "app", "node_modules", ".bin", binaryName),
    path.join(process.resourcesPath || "", "app.asar.unpacked", "node_modules", ".bin", binaryName),
    path.join(workspaceRoot, "node_modules", ".bin", binaryName),
  ];
  for (const candidate of candidates) {
    if (!candidate) {
      continue;
    }
    try {
      await fs.access(candidate);
      return { command: candidate, args: [] };
    } catch {}
  }
  throw new Error(
    "Slidev CLI is not available. Run `npm install` in desktop-shell or bundle @slidev/cli with the desktop shell.",
  );
}

function allocateLocalPort() {
  return new Promise((resolve, reject) => {
    const server = net.createServer();
    server.unref();
    server.on("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const address = server.address();
      server.close(() => {
        if (!address || typeof address === "string") {
          reject(new Error("failed to allocate local port"));
          return;
        }
        resolve(address.port);
      });
    });
  });
}

async function waitForHttpOk(url, timeoutMs, shouldAbort) {
  const startedAt = Date.now();
  let lastError = null;
  while (Date.now() - startedAt < timeoutMs) {
    if (shouldAbort()) {
      throw new Error("Slidev process exited before the preview became ready");
    }
    try {
      const response = await fetch(url, { redirect: "manual" });
      if (response.status >= 200 && response.status < 500) {
        return;
      }
    } catch (error) {
      lastError = error;
    }
    await new Promise((resolve) => setTimeout(resolve, 300));
  }
  const detail = lastError instanceof Error ? `: ${lastError.message}` : "";
  throw new Error(`timed out waiting for ${url}${detail}`);
}

function isPathInside(rootPath, targetPath) {
  const relative = path.relative(rootPath, targetPath);
  return relative === "" || (!relative.startsWith("..") && !path.isAbsolute(relative));
}

function trimProcessLog(value) {
  return value.slice(-8_000);
}

function stopSlidevPreviews() {
  for (const preview of slidevPreviews.values()) {
    if (preview.child.exitCode == null && !preview.child.killed) {
      preview.child.kill();
    }
  }
  slidevPreviews.clear();
}

async function readTreeNodes(rootPath, counter, relativePath = "") {
  if (counter.truncated) {
    return [];
  }

  const currentPath = relativePath ? path.join(rootPath, relativePath) : rootPath;
  let entries = [];
  try {
    entries = await fs.readdir(currentPath, { withFileTypes: true });
  } catch {
    return [];
  }

  entries.sort((left, right) => {
    if (left.isDirectory() !== right.isDirectory()) {
      return left.isDirectory() ? -1 : 1;
    }
    return left.name.localeCompare(right.name);
  });

  const nodes = [];
  for (const entry of entries) {
    if (entry.name.startsWith(".git") || entry.name === "node_modules") {
      continue;
    }
    if (counter.count >= MAX_TREE_NODES) {
      counter.truncated = true;
      break;
    }

    const entryRelativePath = relativePath ? path.join(relativePath, entry.name) : entry.name;
    counter.count += 1;
    nodes.push({
      path: entryRelativePath,
      name: entry.name,
      kind: entry.isDirectory() ? "directory" : "file",
      children: entry.isDirectory()
        ? await readTreeNodes(rootPath, counter, entryRelativePath)
        : [],
    });
  }
  return nodes;
}

async function viewWorkspaceFile(workspaceRoot, filePath) {
  const rootPath = path.resolve(workspaceRoot || "");
  const targetPath = resolveWorkspaceFile(rootPath, filePath);
  const stats = await fs.stat(targetPath);
  const extension = path.extname(targetPath).slice(1).toLowerCase();
  const fileName = path.basename(targetPath);
  const displayPath = path.relative(rootPath, targetPath) || fileName;
  const { kind, mimeType } = workspaceFileKindAndMime(extension);
  const bytes = await fs.readFile(targetPath);

  let content = null;
  let sourceUrl = null;
  let isBinary = false;
  let truncated = false;

  if (kind === "image" || kind === "pdf" || kind === "audio" || kind === "video") {
    isBinary = true;
    if (bytes.length > MAX_BINARY_PREVIEW_BYTES) {
      truncated = true;
      content = `文件过大，暂不内嵌预览（${bytes.length} bytes）`;
    } else {
      sourceUrl = `data:${mimeType};base64,${bytes.toString("base64")}`;
    }
  } else {
    try {
      const text = bytes.toString("utf8");
      if (Buffer.from(text, "utf8").equals(bytes)) {
        if (text.length > MAX_TEXT_PREVIEW_BYTES) {
          truncated = true;
          content = text.slice(0, MAX_TEXT_PREVIEW_BYTES);
        } else {
          content = text;
        }
      } else {
        throw new Error("binary");
      }
    } catch {
      isBinary = true;
      content = `[Binary file: ${fileName}, size: ${bytes.length} bytes]`;
    }
  }

  return {
    path: targetPath,
    displayPath,
    fileName,
    fileType: extension,
    mimeType,
    kind,
    sizeBytes: stats.size,
    content,
    sourceUrl,
    isBinary,
    truncated,
  };
}

function resolveWorkspaceFile(rootPath, filePath) {
  if (!rootPath) {
    throw new Error("workspaceRoot is required");
  }
  if (!filePath) {
    throw new Error("filePath is required");
  }
  const resolved = path.resolve(rootPath, filePath);
  const relative = path.relative(rootPath, resolved);
  if (relative.startsWith("..") || path.isAbsolute(relative)) {
    throw new Error(`path escapes workspace root: ${filePath}`);
  }
  return resolved;
}

function workspaceFileKindAndMime(extension) {
  switch (extension) {
    case "md":
    case "mdx":
      return { kind: "markdown", mimeType: "text/markdown" };
    case "txt":
    case "rs":
    case "ts":
    case "tsx":
    case "js":
    case "jsx":
    case "css":
    case "scss":
    case "html":
    case "xml":
    case "json":
    case "yaml":
    case "yml":
    case "toml":
    case "csv":
    case "log":
    case "sh":
    case "py":
    case "java":
    case "go":
    case "c":
    case "cc":
    case "cpp":
    case "h":
    case "hpp":
    case "swift":
    case "kt":
    case "sql":
      return { kind: "text", mimeType: "text/plain" };
    case "png":
      return { kind: "image", mimeType: "image/png" };
    case "jpg":
    case "jpeg":
      return { kind: "image", mimeType: "image/jpeg" };
    case "gif":
      return { kind: "image", mimeType: "image/gif" };
    case "webp":
      return { kind: "image", mimeType: "image/webp" };
    case "bmp":
      return { kind: "image", mimeType: "image/bmp" };
    case "ico":
      return { kind: "image", mimeType: "image/x-icon" };
    case "svg":
      return { kind: "image", mimeType: "image/svg+xml" };
    case "pdf":
      return { kind: "pdf", mimeType: "application/pdf" };
    case "mp3":
      return { kind: "audio", mimeType: "audio/mpeg" };
    case "wav":
      return { kind: "audio", mimeType: "audio/wav" };
    case "ogg":
      return { kind: "audio", mimeType: "audio/ogg" };
    case "m4a":
      return { kind: "audio", mimeType: "audio/mp4" };
    case "flac":
      return { kind: "audio", mimeType: "audio/flac" };
    case "mp4":
      return { kind: "video", mimeType: "video/mp4" };
    case "webm":
      return { kind: "video", mimeType: "video/webm" };
    case "mov":
      return { kind: "video", mimeType: "video/quicktime" };
    case "m4v":
      return { kind: "video", mimeType: "video/x-m4v" };
    default:
      return { kind: "binary", mimeType: "application/octet-stream" };
  }
}
