"use client";

export type UnlistenFn = () => void;

type DesktopEvent<T = unknown> = {
  payload: T;
};

type DesktopBridge = {
  invoke<T = unknown>(command: string, args?: Record<string, unknown>): Promise<T>;
  listen<T = unknown>(
    eventName: string,
    handler: (event: DesktopEvent<T>) => void,
  ): Promise<UnlistenFn>;
  startDragging(): Promise<void>;
};

declare global {
  interface Window {
    hermesDesktop?: DesktopBridge;
    __TAURI__?: unknown;
    __TAURI_INTERNALS__?: unknown;
  }
}

function electronBridge(): DesktopBridge | null {
  if (typeof window === "undefined") {
    return null;
  }
  return window.hermesDesktop ?? null;
}

function hasTauriRuntime(): boolean {
  if (typeof window === "undefined") {
    return false;
  }
  return Boolean(window.__TAURI_INTERNALS__ || window.__TAURI__);
}

export function isElectronDesktop(): boolean {
  return electronBridge() != null;
}

export function getWindowDragRegionProps(): Record<string, true> {
  if (electronBridge()) {
    return { "data-electron-drag-region": true };
  }
  if (hasTauriRuntime()) {
    return { "data-tauri-drag-region": true };
  }
  return {};
}

export async function invoke<T = unknown>(
  command: string,
  args?: Record<string, unknown>,
): Promise<T> {
  const bridge = electronBridge();
  if (bridge) {
    return bridge.invoke<T>(command, args);
  }

  if (!hasTauriRuntime()) {
    throw new Error("Desktop APIs are unavailable outside the Electron/Tauri shell.");
  }
  const tauri = await import("@tauri-apps/api/core");
  if (typeof tauri.invoke !== "function") {
    throw new Error("Tauri runtime is not available in the current environment.");
  }
  return tauri.invoke<T>(command, args);
}

export async function listen<T = unknown>(
  eventName: string,
  handler: (event: DesktopEvent<T>) => void,
): Promise<UnlistenFn> {
  const bridge = electronBridge();
  if (bridge) {
    return bridge.listen<T>(eventName, handler);
  }

  if (!hasTauriRuntime()) {
    throw new Error("Desktop event APIs are unavailable outside the Electron/Tauri shell.");
  }
  const tauri = await import("@tauri-apps/api/event");
  if (typeof tauri.listen !== "function") {
    throw new Error("Tauri event runtime is not available in the current environment.");
  }
  const unlisten = await tauri.listen<T>(eventName, handler);
  return () => {
    unlisten();
  };
}

export function getCurrentWindow() {
  return {
    async startDragging() {
      const bridge = electronBridge();
      if (bridge) {
        await bridge.startDragging();
        return;
      }

      if (!hasTauriRuntime()) {
        return;
      }
      const tauri = await import("@tauri-apps/api/window");
      if (typeof tauri.getCurrentWindow !== "function") {
        return;
      }
      await tauri.getCurrentWindow().startDragging();
    },
  };
}
