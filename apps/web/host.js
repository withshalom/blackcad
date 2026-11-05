// Where the workbench is running, and what that place offers.
//
// The same app.js runs on the web (cad.mosaiciron.com, the installable web
// app) and inside the BlackCAD application for Windows, macOS, Linux and
// Android (apps/shell). Everything that differs between the two lives here:
// the words the interface uses about itself, file dialogs, saving in place,
// printing, and the window's title. app.js asks `host` and never checks
// which one it is in.
//
// Inside the app, the shell puts a bridge on `globalThis.BLACKCAD_NATIVE`
// before this page's scripts run (apps/shell/src/bridge.js). Its contract:
//
//   platform             "windows" | "macos" | "linux" | "android" | "ios"
//   version()            the native engine's name and version
//   call(method, args)   one engine call (see core.js NativeBackend)
//   restart()            a fresh engine after a crash
//   openDialog()         [{name, path, bytes}] the person chose, or []
//   saveDialog(name, ext, description)   a path, or null when cancelled
//   writeFile(path, bytes)
//   showPdf(name, bytes, print)          opens the PDF in the system viewer
//   launchFiles()        [{name, path, bytes}] the app was opened with
//   onOpenFiles(fn)      files opened with the app while it runs
//   setTitle(text)
//
// This file imports nothing.

const bridge = globalThis.BLACKCAD_NATIVE || null;
const platform = bridge?.platform || "web";
const phone = platform === "android" || platform === "ios";

/** Bytes as the bridge takes them. */
const asBytes = (b) => (b instanceof Uint8Array ? b : new Uint8Array(b));

export const host = {
  /** True inside the BlackCAD application. */
  native: !!bridge,
  platform,

  /** How the interface names the place a drawing is being edited:
   *  "Editing on this computer", "Shalom · this computer". */
  place: bridge ? (phone ? "this device" : "this computer") : "this browser",
  editingOn: bridge ? (phone ? "Editing on this device" : "Editing on this computer") : "Editing in this browser",
  /** What to do when the engine cannot be restarted. */
  restartHint: bridge ? "Close BlackCAD and open it again." : "Reload the page.",
  /** Where per-person settings (name on saves, Blackboard progress) live. */
  settingsPlace: bridge ? (phone ? "on this device" : "on this computer") : "in this browser",

  /** The web's own install button, offline cache and file handlers apply
   *  only on the web. */
  webOnly: !bridge,

  setTitle(text) {
    document.title = text;
    if (bridge) bridge.setTitle?.(text)?.catch?.(() => {});
  },

  /** Drawings chosen in the app's open dialog: [{name, path, bytes}]. */
  async openDialog() {
    const files = (await bridge.openDialog()) || [];
    return files.map((f) => ({ ...f, bytes: asBytes(f.bytes) }));
  },

  /** A path to save to, from the app's save dialog, or null. */
  saveDialog(name, ext, description) {
    return bridge.saveDialog(name, ext, description);
  },

  writeFile(path, bytes) {
    return bridge.writeFile(path, asBytes(bytes));
  },

  /** A plotted sheet in the system's PDF viewer, or straight to its print dialog. */
  showPdf(name, bytes, print) {
    return bridge.showPdf(name, asBytes(bytes), !!print);
  },

  /** Files the app was started with, and files opened with it later. */
  async launchFiles(fn) {
    if (!bridge) return;
    bridge.onOpenFiles?.((files) => files.forEach((f) => fn({ ...f, bytes: asBytes(f.bytes) })));
    for (const f of (await bridge.launchFiles?.()) || []) await fn({ ...f, bytes: asBytes(f.bytes) });
  },
};

/**
 * Inside the app, the page behaves as a program rather than a web page:
 * no browser context menu (Back, Reload, Inspect), no reload or find keys,
 * no text selection across the interface, no dragging of icons, no rubber-
 * band scrolling. Text fields and the command line keep their own menus
 * and selection. On the web nothing changes.
 */
export function settleIntoApp() {
  const root = document.documentElement;
  root.classList.add(bridge ? "in-app" : "on-web", `platform-${platform}`);
  if (!bridge) return;
  const editable = (t) => t instanceof Element && !!t.closest("input, textarea, [contenteditable=''], [contenteditable='true'], pre");
  addEventListener("contextmenu", (e) => {
    if (!editable(e.target)) e.preventDefault();
  });
  addEventListener(
    "keydown",
    (e) => {
      const k = e.key.toLowerCase();
      const mod = e.ctrlKey || e.metaKey;
      const browserKey =
        k === "f5" ||
        (mod && (k === "r" || k === "f" || k === "g" || k === "u" || k === "j" || k === "h" || k === "+" || k === "=" || k === "-" || k === "0")) ||
        (mod && e.shiftKey && (k === "i" || k === "c")) ||
        (e.altKey && (k === "arrowleft" || k === "arrowright")) ||
        k === "browserback" ||
        k === "browserforward" ||
        k === "browserrefresh";
      if (browserKey) e.preventDefault();
    },
    true
  );
  // A file dropped outside the drop zone must never replace the page.
  addEventListener("dragover", (e) => e.preventDefault());
}

/** The last part of a path, on any system. */
export function baseName(path) {
  return String(path).split(/[\\/]/).pop();
}
