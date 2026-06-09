#!/usr/bin/env node

import { spawn } from "node:child_process";
import { mkdir, rm, writeFile } from "node:fs/promises";
import { createServer } from "node:net";
import path from "node:path";
import { setTimeout as sleep } from "node:timers/promises";
import { fileURLToPath } from "node:url";

const __filename = fileURLToPath(import.meta.url);
const repoRoot = path.resolve(path.dirname(__filename), "..");
const outputDir = path.join(repoRoot, "docs", "assets", "screenshots");
const appUrl = process.env.CRAB_SCREENSHOT_URL || "http://127.0.0.1:1420";
const chromePath =
  process.env.CHROME_BIN || "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";

const now = Math.floor(Date.now() / 1000);
const workspaceRoot = "/workspace/crab";
const dataDir = `${workspaceRoot}/.hermes-agent-rs`;

const sessions = [
  {
    session_id: "demo-agent-loop",
    title: "Agent loop traces a release goal",
    model: "gpt-4.1-mini",
    created_at_unix: now - 8200,
    updated_at_unix: now - 160,
    message_count: 9,
    last_user_message: "Audit the repo for open-source readiness and update the docs.",
    last_assistant_message: "Updated README, docs, license, and privacy review notes.",
  },
  {
    session_id: "demo-docs",
    title: "Documentation polish",
    model: "gpt-4.1-mini",
    created_at_unix: now - 16400,
    updated_at_unix: now - 5400,
    message_count: 6,
    last_user_message: "Explain the delegation model clearly.",
    last_assistant_message: "Added the agent loop and future vision docs.",
  },
  {
    session_id: "demo-runtime",
    title: "Runtime profile check",
    model: "gpt-4.1-mini",
    created_at_unix: now - 32000,
    updated_at_unix: now - 9200,
    message_count: 5,
    last_user_message: "Check browser and Office runtime readiness.",
    last_assistant_message: "Runtime profile is ready for local workflows.",
  },
];

const timeline = [
  {
    type: "user",
    id: "u1",
    turn_id: "t1",
    content: "Prepare Crab for a public open-source launch. Make the README professional and check for private information.",
  },
  {
    type: "tool",
    id: "tool-privacy",
    turn_id: "t1",
    name: "search_files",
    detail:
      "command: rg --hidden -n \"api_key|secret|token|password|private key\"\nstdout:\nNo tracked secrets found in source, docs, or commit-visible files.",
    command: "rg secret/token patterns",
    phase: "done",
    execution_mode: "read-only",
  },
  {
    type: "assistant",
    id: "a1",
    turn_id: "t1",
    content:
      "The tracked tree is clean for the common secret patterns I checked. I also rebuilt the public-facing docs around Crab, the agent loop, privacy review, and future vision.",
  },
  {
    type: "user",
    id: "u2",
    turn_id: "t2",
    content: "Make sure the agent loop story is obvious: the main model tracks goals and delegates bounded work to submodels.",
  },
  {
    type: "batch",
    id: "batch-1",
    turn_id: "t2",
    batch_id: "release-readiness",
    iteration: 2,
    total_calls: 4,
    completed_calls: 4,
    status: "completed",
  },
  {
    type: "tool",
    id: "tool-docs",
    turn_id: "t2",
    name: "delegate_worker",
    detail:
      "summary: reviewed README and docs for the goal-state controller narrative\nkey_evidence: docs/AGENT_LOOP.md, docs/ARCHITECTURE.md, README.md",
    command: "review agent loop docs",
    phase: "done",
    execution_mode: "delegated",
  },
  {
    type: "assistant",
    id: "a2",
    turn_id: "t2",
    content:
      "Crab now presents the main agent as a goal-solving controller: it maintains goal state, routes evidence into memory and solve traces, delegates bounded subtasks, and streams each step to the desktop shell.",
  },
];

const skills = [
  {
    category: "runtime",
    name: "goal-state-review",
    description: "Inspect an active goal, risks, evidence, blockers, and next actions before a long-running task continues.",
    keywords: ["goal-state", "evidence", "risk"],
    task_kinds: ["planning", "review"],
    requires_tools: ["read_file", "search_files"],
    requires_shell: false,
    updated_at_unix: now - 5000,
  },
  {
    category: "documents",
    name: "render-verified-docs",
    description: "Generate or edit documents, render previews, and iterate until layout and export quality are acceptable.",
    keywords: ["docx", "pdf", "render"],
    task_kinds: ["document", "qa"],
    requires_tools: ["view_workspace_file"],
    requires_shell: false,
    updated_at_unix: now - 8200,
  },
  {
    category: "browser",
    name: "browser-workflow-check",
    description: "Drive a browser workflow, capture evidence, and summarize observed page state.",
    keywords: ["browser", "automation", "screenshot"],
    task_kinds: ["research", "verification"],
    requires_tools: ["browser_snapshot", "browser_screenshot"],
    requires_shell: false,
    updated_at_unix: now - 9200,
  },
];

const providers = [
  {
    id: "openai",
    label: "OpenAI Compatible",
    kind: "openai",
    enabled: true,
    is_default: true,
    model: "gpt-4.1-mini",
    base_url: "https://api.openai.com/v1",
    api_mode: "responses",
    auth_source: "environment",
  },
  {
    id: "local",
    label: "Local Gateway",
    kind: "openai",
    enabled: true,
    is_default: false,
    model: "qwen3-coder",
    base_url: "http://127.0.0.1:11434/v1",
    api_mode: "chat",
    auth_source: null,
  },
];

const extensionsOverview = {
  plugin_dirs: ["bundled-skills"],
  plugins: [
    {
      name: "office-workflows",
      version: "0.1.0",
      description: "Office, PDF, and Slidev oriented local workflows.",
      path: "bundled-skills/office",
      enabled: true,
      tool_names: ["preview_pdf", "preview_docx", "create_slidev_deck"],
      hook_names: ["context"],
    },
  ],
  providers,
  mcp_servers: [
    {
      name: "filesystem",
      transport: "stdio",
      target: "local",
      enabled: true,
      cache_ttl_seconds: 300,
      cache_stale: false,
      discovered_tools_count: 8,
      discovered_tool_names: ["read_file", "search_files", "patch_file"],
      last_inspected_at_unix: now - 900,
    },
  ],
  cron_jobs: [
    {
      id: "nightly-runtime-audit",
      schedule: "0 2 * * *",
      prompt: "Review the active workspace for failing checks, stale docs, and privacy-sensitive files.",
      prompt_preview: "Review the active workspace for failing checks, stale docs, and privacy-sensitive files.",
      enabled: true,
      next_run_at_unix: now + 3600 * 7,
      last_run_at_unix: now - 3600 * 17,
      last_status: "completed",
      last_session_id: "demo-runtime",
      recent_runs: [
        {
          job_id: "nightly-runtime-audit",
          session_id: "demo-runtime",
          status: "completed",
          response_preview: "No tracked secrets found. Runtime profile and docs are ready.",
          updated_at_unix: now - 3600 * 17,
        },
      ],
    },
  ],
};

const workspaceTree = {
  rootPath: workspaceRoot,
  truncated: false,
  nodes: [
    {
      path: `${workspaceRoot}/README.md`,
      name: "README.md",
      kind: "file",
      children: [],
    },
    {
      path: `${workspaceRoot}/docs`,
      name: "docs",
      kind: "directory",
      children: [
        {
          path: `${workspaceRoot}/docs/AGENT_LOOP.md`,
          name: "AGENT_LOOP.md",
          kind: "file",
          children: [],
        },
        {
          path: `${workspaceRoot}/docs/ARCHITECTURE.md`,
          name: "ARCHITECTURE.md",
          kind: "file",
          children: [],
        },
      ],
    },
    {
      path: `${workspaceRoot}/src`,
      name: "src",
      kind: "directory",
      children: [
        {
          path: `${workspaceRoot}/src/agent.rs`,
          name: "agent.rs",
          kind: "file",
          children: [],
        },
      ],
    },
  ],
};

const initScript = `
(() => {
  const now = ${now};
  const workspaceRoot = ${JSON.stringify(workspaceRoot)};
  const dataDir = ${JSON.stringify(dataDir)};
  const sessions = ${JSON.stringify(sessions)};
  const timeline = ${JSON.stringify(timeline)};
  const skills = ${JSON.stringify(skills)};
  const providers = ${JSON.stringify(providers)};
  const extensionsOverview = ${JSON.stringify(extensionsOverview)};
  const workspaceTree = ${JSON.stringify(workspaceTree)};

  const preferences = {
    workspaceRoot,
    dataDir: "",
    provider: "openai",
    model: "gpt-4.1-mini",
    smallModel: "gpt-4.1-mini",
    baseUrl: "https://api.openai.com/v1",
    apiKey: "",
    maxIterations: 12,
    enableShellTool: false,
    sessionId: "demo-agent-loop",
    prompt: "Trace the release goal, verify open-source hygiene, and update the docs."
  };
  window.localStorage.setItem("crab.desktop.preferences", JSON.stringify(preferences));
  window.localStorage.setItem("crab.desktop.workspaces", JSON.stringify([
    { workspaceRoot, dataDir: "", expanded: true }
  ]));
  window.__HERMES_DESKTOP_SHELL__ = "electron";
  window.hermesDesktop = {
    async invoke(command, args) {
      switch (command) {
        case "desktop_info":
          return {
            shell: "electron",
            platform: "darwin",
            global_event_topic: "hermes://agent/event",
            global_done_topic: "hermes://agent/done",
            cleared_topic: "hermes://agent/cleared",
            session_event_topic_template: "hermes://agent/event/<session_id>",
            session_done_topic_template: "hermes://agent/done/<session_id>",
            last_session_id: "demo-agent-loop",
            current_working_dir: workspaceRoot
          };
        case "load_shared_provider_config":
          return {
            configured: true,
            provider: "openai",
            model: "gpt-4.1-mini",
            baseUrl: "https://api.openai.com/v1",
            apiKey: "",
            auxModel: "gpt-4.1-mini"
          };
        case "save_shared_provider_config":
          return { ok: true };
        case "list_sessions":
          return sessions;
        case "load_session":
          return {
            summary: sessions.find((item) => item.session_id === (args?.sessionId || "demo-agent-loop")) || sessions[0],
            history: [],
            timeline
          };
        case "list_providers":
          return providers;
        case "resolve_provider_status":
          return {
            id: "openai",
            label: "OpenAI Compatible",
            kind: "openai",
            model: "gpt-4.1-mini",
            base_url: "https://api.openai.com/v1",
            api_mode: "responses",
            auth_source: "environment",
            auth_required: true,
            ready: true
          };
        case "list_approvals":
          return [];
        case "list_delegate_runs":
          return [
            {
              id: "delegate-docs",
              parent_session_id: "demo-agent-loop",
              parent_delegate_run_id: null,
              root_delegate_run_id: "delegate-docs",
              session_id: "demo-agent-loop.delegate.docs",
              prompt: "Review the README and docs for the agent-loop positioning.",
              prompt_preview: "Review the README and docs for the agent-loop positioning.",
              status: "completed",
              result_preview: "Agent loop, delegation, and goal tracking are now explicit in the docs.",
              max_iterations: 4,
              attempt: 1,
              created_at_unix: now - 4200,
              updated_at_unix: now - 3900
            }
          ];
        case "list_skills":
          return skills;
        case "view_skill":
          return {
            ...skills.find((skill) => skill.category === args?.category && skill.name === args?.name) || skills[0],
            file_path: "bundled-skills/runtime/goal-state-review/SKILL.md",
            file_type: "markdown",
            content: "# Goal State Review\\n\\nUse this skill when a task spans multiple turns and needs explicit evidence, blockers, risks, and next actions.\\n\\n1. Inspect the active goal.\\n2. Gather evidence from tools.\\n3. Update risks and recommended actions.",
            is_binary: false,
            linked_files: {},
            required_environment_variables: [],
            missing_required_environment_variables: [],
            required_commands: ["rg"],
            missing_required_commands: [],
            config_requirements: [],
            setup_needed: false,
            readiness_status: "ready"
          };
        case "extensions_overview":
          return extensionsOverview;
        case "cron_scheduler_status":
        case "start_cron_scheduler":
          return {
            running: true,
            paused_reason: null,
            tick_interval_seconds: 60,
            last_tick_at_unix: now - 90,
            last_due_job_ids: [],
            last_error: null,
            workspace_root: workspaceRoot
          };
        case "list_workspace_tree":
          return workspaceTree;
        case "resolve_runtime_profile":
        case "resolve_runtime_status":
          return { ok: true, ready: true };
        case "browser_stream_endpoint":
          return { wsUrl: "", port: 0, sessionName: "demo-agent-loop" };
        case "view_workspace_file":
          return {
            path: args?.filePath || workspaceRoot + "/README.md",
            displayPath: "README.md",
            fileName: "README.md",
            fileType: "markdown",
            kind: "text",
            content: "# Crab\\n\\nCrab is a Rust-native local agent runtime with goal tracking, tool governance, worker delegation, and desktop event streaming.",
            sizeBytes: 186,
            renderedPdfPath: null,
            preview: null
          };
        default:
          return { ok: true };
      }
    },
    async listen() {
      return () => {};
    },
    async startDragging() {}
  };
})();
`;

async function main() {
  await waitForApp(appUrl);
  await mkdir(outputDir, { recursive: true });

  const chrome = await launchChrome();
  const client = await connectToPage(chrome.port);

  try {
    await client.send("Page.enable");
    await client.send("Runtime.enable");
    await client.send("Emulation.setDeviceMetricsOverride", {
      width: 1440,
      height: 960,
      deviceScaleFactor: 1,
      mobile: false,
    });
    await client.send("Page.addScriptToEvaluateOnNewDocument", { source: initScript });
    await navigate(client, appUrl);
    await waitFor(client, () => document.body?.innerText.includes("Agent loop traces a release goal"));
    await sleep(650);
    await screenshot(client, "crab-conversation.png");

    await clickByText(client, "技能和应用");
    await waitFor(client, () => document.body?.innerText.includes("goal-state-review"));
    await sleep(450);
    await screenshot(client, "crab-skills.png");

    await clickByText(client, "定时任务");
    await waitFor(client, () => document.body?.innerText.includes("nightly-runtime-audit"));
    await sleep(450);
    await screenshot(client, "crab-activity.png");

    await clickByText(client, "设置");
    await waitFor(client, () => document.body?.innerText.includes("运行设置"));
    await sleep(450);
    await screenshot(client, "crab-settings.png");
  } finally {
    client.close();
    chrome.process.kill("SIGTERM");
    await rm(chrome.userDataDir, { recursive: true, force: true });
  }
}

async function waitForApp(url) {
  for (let attempt = 0; attempt < 90; attempt += 1) {
    try {
      const response = await fetch(url);
      if (response.ok) {
        return;
      }
    } catch {}
    await sleep(1000);
  }
  throw new Error(`app did not become ready: ${url}`);
}

async function launchChrome() {
  const port = await findAvailablePort();
  const userDataDir = path.join("/tmp", `crab-screenshots-${Date.now()}`);
  const chrome = spawn(chromePath, [
    "--headless=new",
    `--remote-debugging-port=${port}`,
    `--user-data-dir=${userDataDir}`,
    "--hide-scrollbars",
    "--disable-gpu",
    "--no-first-run",
    "--no-default-browser-check",
    "about:blank",
  ], {
    stdio: ["ignore", "ignore", "pipe"],
  });
  chrome.stderr.on("data", () => {});
  for (let attempt = 0; attempt < 60; attempt += 1) {
    try {
      const response = await fetch(`http://127.0.0.1:${port}/json/version`);
      if (response.ok) {
        return { process: chrome, port, userDataDir };
      }
    } catch {}
    await sleep(250);
  }
  chrome.kill("SIGTERM");
  throw new Error("Chrome remote debugging endpoint did not become ready");
}

async function findAvailablePort() {
  for (let port = 48080; port < 48150; port += 1) {
    if (await canListen(port)) {
      return port;
    }
  }
  throw new Error("no available debugging port");
}

function canListen(port) {
  return new Promise((resolve) => {
    const server = createServer();
    server.once("error", () => resolve(false));
    server.listen(port, "127.0.0.1", () => {
      server.close(() => resolve(true));
    });
  });
}

async function connectToPage(port) {
  const response = await fetch(`http://127.0.0.1:${port}/json/new?about:blank`, { method: "PUT" });
  if (!response.ok) {
    throw new Error(`failed to create Chrome target: ${response.status}`);
  }
  const target = await response.json();
  const ws = new WebSocket(target.webSocketDebuggerUrl);
  const callbacks = new Map();
  let nextId = 1;
  await new Promise((resolve, reject) => {
    ws.addEventListener("open", resolve, { once: true });
    ws.addEventListener("error", reject, { once: true });
  });
  ws.addEventListener("message", (event) => {
    const message = JSON.parse(event.data);
    if (!message.id || !callbacks.has(message.id)) {
      return;
    }
    const { resolve, reject } = callbacks.get(message.id);
    callbacks.delete(message.id);
    if (message.error) {
      reject(new Error(message.error.message || JSON.stringify(message.error)));
      return;
    }
    resolve(message.result || {});
  });
  return {
    send(method, params = {}) {
      const id = nextId;
      nextId += 1;
      ws.send(JSON.stringify({ id, method, params }));
      return new Promise((resolve, reject) => {
        callbacks.set(id, { resolve, reject });
      });
    },
    close() {
      ws.close();
    },
  };
}

async function navigate(client, url) {
  await client.send("Page.navigate", { url });
  await waitFor(client, () => document.readyState === "complete");
}

async function waitFor(client, predicate, timeoutMs = 15000) {
  const source = `(${predicate.toString()})()`;
  const started = Date.now();
  while (Date.now() - started < timeoutMs) {
    const result = await client.send("Runtime.evaluate", {
      expression: source,
      returnByValue: true,
    });
    if (result.result?.value) {
      return;
    }
    await sleep(250);
  }
  throw new Error(`timed out waiting for predicate: ${predicate.toString()}`);
}

async function clickByText(client, text) {
  const expression = `
    (() => {
      const needle = ${JSON.stringify(text)};
      const elements = Array.from(document.querySelectorAll("button, [role='button'], a"));
      const target = elements.find((element) => (element.innerText || element.textContent || "").trim().includes(needle));
      if (!target) return false;
      target.click();
      return true;
    })()
  `;
  const result = await client.send("Runtime.evaluate", {
    expression,
    returnByValue: true,
  });
  if (!result.result?.value) {
    throw new Error(`could not find clickable text: ${text}`);
  }
}

async function screenshot(client, filename) {
  const result = await client.send("Page.captureScreenshot", {
    format: "png",
    captureBeyondViewport: false,
    fromSurface: true,
  });
  await writeFile(path.join(outputDir, filename), Buffer.from(result.data, "base64"));
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
