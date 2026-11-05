// The BlackCAD app's bridge to the workbench, put on the page before any of
// its scripts run (lib.rs, `initialization_script`). It gives the page
// `BLACKCAD_NATIVE`, the contract apps/web/host.js and core.js describe:
// engine calls go to the native engine (engine.rs), files to the system's
// own dialogs (files.rs). Bytes cross as raw bytes, not JSON.

(() => {
  const invoke = (cmd, args, opts) => window.__TAURI_INTERNALS__.invoke(cmd, args, opts);
  const bytesOf = (r) => (r instanceof Uint8Array ? r : r instanceof ArrayBuffer ? new Uint8Array(r) : Uint8Array.from(r || []));
  const withBytes = async (files) => {
    const out = [];
    for (const f of files || []) out.push({ name: f.name, path: f.path, bytes: bytesOf(await invoke("read_file", { path: f.path })) });
    return out;
  };

  globalThis.BLACKCAD_NATIVE = {
    platform: "__PLATFORM__",

    version: () => invoke("engine_version"),
    restart: () => invoke("engine_restart"),

    async call(method, args) {
      let value;
      switch (method) {
        case "open":
          value = await invoke("engine_open", bytesOf(args[0]));
          break;
        case "newDrawing":
          value = await invoke("engine_new", { imperial: !!args[0] });
          break;
        case "sample":
          value = await invoke("engine_sample");
          break;
        case "close":
          value = await invoke("engine_close", { id: args[0] });
          break;
        case "info":
          value = JSON.parse(await invoke("engine_text", { method: "info", id: args[0] }));
          break;
        case "exec":
          value = JSON.parse(await invoke("engine_text", { method: "exec", id: args[0], line: args[1] }));
          break;
        case "display":
        case "displayChanges":
          value = bytesOf(await invoke("engine_display", { id: args[0], changes: method === "displayChanges" }));
          break;
        case "save": {
          const bytes = bytesOf(await invoke("engine_save", { id: args[0], format: args[1], user: args[2] || "" }));
          value = { bytes, notes: await invoke("engine_save_notes") };
          break;
        }
        default:
          throw { message: `the engine has no call "${method}"` };
      }
      return { value, live: 0 };
    },

    async openDialog() {
      return withBytes(await invoke("open_dialog"));
    },
    saveDialog: (name, ext, description) => invoke("save_dialog", { name, ext, description }),
    writeFile: (path, bytes) => invoke("write_file", bytesOf(bytes), { headers: { "x-path": encodeURIComponent(path) } }),
    showPdf: (name, bytes, print) => invoke("show_pdf", bytesOf(bytes), { headers: { "x-name": encodeURIComponent(name), "x-print": print ? "1" : "0" } }),
    async launchFiles() {
      return withBytes(await invoke("launch_files"));
    },
    onOpenFiles(fn) {
      window.__TAURI__?.event?.listen("open-files", async (e) => fn(await withBytes(e.payload)));
    },
    setTitle: (title) => invoke("set_title", { title }),
  };

  // Closing the window asks the workbench first (unsaved drawings).
  addEventListener("DOMContentLoaded", () => {
    const w = window.__TAURI__?.window?.getCurrentWindow?.();
    w?.onCloseRequested((e) => {
      if (globalThis.blackcadCanClose && !globalThis.blackcadCanClose()) e.preventDefault();
    });
  });
})();
