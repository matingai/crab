import { contextBridge, ipcRenderer } from "electron";

contextBridge.exposeInMainWorld("__HERMES_DESKTOP_SHELL__", "electron");

contextBridge.exposeInMainWorld("hermesDesktop", {
  async invoke(command, args = {}) {
    return ipcRenderer.invoke("hermes:invoke", { command, args });
  },
  async listen(eventName, handler) {
    const listener = (_event, payload) => {
      handler({ payload });
    };
    ipcRenderer.on(eventName, listener);
    return () => {
      ipcRenderer.removeListener(eventName, listener);
    };
  },
  async startDragging() {
    return;
  },
});
