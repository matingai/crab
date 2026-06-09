import { spawn } from "node:child_process";
import net from "node:net";
import process from "node:process";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const rootDir = path.resolve(__dirname, "..");
const devServerUrl = "http://127.0.0.1:1420/";
const defaultAutomationPort = Number(process.env.HERMES_ELECTRON_DEVTOOLS_PORT || 47712);

let devServer = null;
const startedDevServer = !(await isUrlReady(devServerUrl));
if (startedDevServer) {
  devServer = spawn("npm", ["run", "dev"], {
    cwd: rootDir,
    stdio: "inherit",
    shell: true,
  });
}
const automationPort = await findAvailablePort(defaultAutomationPort);

let exiting = false;

function shutdown(code = 0) {
  if (exiting) {
    return;
  }
  exiting = true;
  if (startedDevServer && devServer && !devServer.killed) {
    devServer.kill("SIGTERM");
  }
  process.exit(code);
}

process.on("SIGINT", () => shutdown(130));
process.on("SIGTERM", () => shutdown(143));

await waitForUrl(devServerUrl);

const electron = spawn("npx", ["electron", "./electron/main.mjs"], {
  cwd: rootDir,
  env: {
    ...process.env,
    HERMES_ELECTRON_DEVTOOLS_PORT: String(automationPort),
    HERMES_RS_ELECTRON_DEVTOOLS_BASE_URL: `http://127.0.0.1:${automationPort}`,
  },
  stdio: "inherit",
  shell: true,
});

electron.on("exit", (code) => shutdown(code ?? 0));
if (devServer) {
  devServer.on("exit", (code) => shutdown(code ?? 1));
}

async function waitForUrl(url) {
  for (let attempt = 0; attempt < 60; attempt += 1) {
    if (await isUrlReady(url)) {
      return;
    }
    await sleep(1000);
  }
  throw new Error(`dev server did not become ready: ${url}`);
}

async function isUrlReady(url) {
  try {
    const response = await fetch(url);
    return response.ok;
  } catch {
    return false;
  }
}

async function findAvailablePort(startPort) {
  for (let offset = 0; offset < 20; offset += 1) {
    const candidate = startPort + offset;
    // eslint-disable-next-line no-await-in-loop
    if (await canListen(candidate)) {
      return candidate;
    }
  }
  throw new Error(`failed to find available Electron automation port near ${startPort}`);
}

function canListen(port) {
  return new Promise((resolve) => {
    const server = net.createServer();
    server.once("error", () => resolve(false));
    server.listen(port, "127.0.0.1", () => {
      server.close(() => resolve(true));
    });
  });
}

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}
