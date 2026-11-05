# BlackCAD native apps: advice and reasoning

Written 2026-11-04. Status: **advice, nothing changed.** Builds on
`NATIVE-APP-BUILD-PLAN.md` (2026-11-03) and `docs/DESIGN-DECISIONS.md`. Where this
document differs from the build plan, this document is the newer recommendation.

## 1. Summary

- Keep the JS UI (`apps/web`). Run the Rust engine natively, compiled into the app,
  holding the document and model space. The UI draws and sends commands.
- Use **Tauri 2 as the shell on every platform**: Windows, macOS, Linux, Android, and
  later iOS. One shell crate, one bridge.
- **Skip the sidecar stopgap.** Prove the native backend directly in a Tauri dev build.
- Do the **async `Core` refactor first**, on the web, before any shell work.
- Make the display list **viewport-aware** so large drawings stay fast in every webview.
- Keep the web build at cad.mosaiciron.com on `core.wasm`.

File extensions (already Decided): `.cad` for BlackCAD (lock `.bcl`), `.cadcraft` for
the CADCraft fork.

## 2. The goal, stated precisely

| Requirement | Meaning |
| --- | --- |
| Native engine | Kernel, document, model space, DWG/DXF I/O run as native Rust: 64-bit memory, threads, native file access. No WASM32 ceiling. |
| JS UI kept | The existing workbench, its look and its features. Written once, used everywhere. |
| Real apps | Installable, own identity and icon, file association, no dependency on an installed browser. |
| Platforms | Windows, Linux, macOS, Android now; iOS later. |
| Web stays | The PWA keeps working on WASM. |

## 3. Why the current exe is a web wrapper

`apps/desktop` serves `dist/web` on 127.0.0.1 and opens a Chromium `--app` window.
Everything, including `core.wasm`, runs inside the browser sandbox:

- 32-bit WASM memory (4 GB hard ceiling, often less in practice).
- One thread unless you build WASM threads, which need special headers and still share
  the browser's limits.
- The browser's file rules (File System Access API, which WebKit does not have).
- A hard dependency on Chrome, Edge or Brave being installed.
- No app identity, no file association, no installer.

So it looks like an app but behaves like a website. Adding more launcher features does
not change that, because the engine is still in the browser.

**The fix is to move the engine, not the UI.** The engine must be linked into the
application process, and the UI must talk to it through a bridge.

## 4. What "native" honestly means here

With a webview shell, the UI still renders in a webview. That is a normal and respected
way to ship desktop software (VS Code, Slack, Figma's desktop app, Obsidian, many
engineering tools). What makes it native:

- The process is your executable, signed, installed and associated with `.cad`/`.dwg`/`.dxf`.
- The engine runs as native machine code with the full machine.
- File access goes through native dialogs and the OS, not browser APIs.

The only route with **no webview at all** is a UI written in a native toolkit (egui,
Qt, SwiftUI, and so on). You tried that and found it means rebuilding the whole JS UI
and keeping two UIs in step. That cost is why the Rust-UI approach moved to DraftCraft,
where it is the product. For BlackCAD, engine-native plus webview UI is the correct
trade.

## 5. Architecture

```
          apps/web  (JS UI, unchanged look and features)
                     |
                core.js  ->  Core interface (async)
                     |
       +-------------+----------------+
       |                              |
 WasmCore (web/PWA)            NativeCore (apps)
 core.wasm in a Worker         bridge: Tauri IPC + blackcad:// protocol
                                      |
                             crates/cad-ffi (Session)
                             cad-core, cad-dwg, cad-dxf, cad-native
```

### 5.1 One `Core` interface, two backends

Mirror the `Session` API in `crates/cad-ffi/src/session.rs`: `open`, `newDrawing`,
`sample`, `display`, `info`, `exec`, `save`. `core.js` picks the backend at startup:
native when the Tauri bridge is present, WASM otherwise.

**Reasoning:** the UI never needs to know which engine it is talking to. Every UI
feature is written once and works on the web and in every app. This is the property
that justifies keeping the JS UI in the first place.

### 5.2 Everything async

WASM calls are synchronous; any native bridge (Tauri IPC, a custom protocol, JNI) is
asynchronous. Make `Core` async for **both** backends so there is one code path.

- Every `core.*` call site in `app.js` gets `await`. Count them first; that number
  sizes this job.
- Hover snap and pick run on every mouse move. Use **latest-wins**: tag each request with
  a sequence number and drop replies older than the newest, so a slow reply never lands
  on the wrong cursor position.
- Keep a small JS-side snap cache built from the display list already on screen, for
  instant hover feedback. The engine stays authoritative for committed points.
- Queue edits (`exec`) per drawing, in order, so undo and redo stay deterministic.

**Reasoning:** this is the largest single cost and everything else depends on it. Doing
it first, on the web, against the WASM backend, means it ships and gets tested before any
native code relies on it. Moving WASM into a Worker comes almost free with this change and
frees the UI thread on the web too.

### 5.3 Viewport-aware display lists

Change `display(id)` (whole drawing) to `display(id, view)` where `view` is the visible
rectangle plus pixel scale. The engine returns only what the viewport needs:

- **Culling:** entities outside the view are not sent.
- **Level of detail:** sub-pixel entities become a dot or are dropped; dense hatches and
  long polylines are simplified for the current zoom.
- **Tiling/chunking** for very large drawings, so JS draws a bounded amount at any size.
- A **revision number** in the reply so JS can skip redraws when nothing changed.

**Reasoning:** a native engine removes the memory ceiling but not the drawing ceiling.
The webview still has to draw whatever it is sent. If the engine sends a 2-million-entity
drawing in full, the UI will crawl in any shell. Culling and LOD keep the UI's work
proportional to the screen, not the file. This matters most on Linux (section 8.1) and on
phones.

Before settling the display-list shape, read how `app.js` renders (Canvas 2D or WebGL).
The renderer sets the real ceiling.

### 5.4 The bridge

Two channels:

| Channel | Used for | How |
| --- | --- | --- |
| Commands and queries | `exec`, `info`, small results | Tauri commands (IPC), JSON or text lines |
| Bulk bytes | display lists, file open, file save | Custom URI scheme `blackcad://engine/...`, read with `fetch` |

**Reasoning:** IPC serialises through JSON, which is fine for small messages but wasteful
for megabytes of geometry. A custom protocol returns raw binary to `fetch` with no base64
or JSON overhead, and `fetch` is already async. No TCP port is opened in the shipped apps,
which removes a class of local security problems the loopback launcher had to guard against.

## 6. Shell choice: Tauri 2 everywhere

### 6.1 Options considered

| Option | For | Against |
| --- | --- | --- |
| **Tauri 2** | Window, menus, dialogs, file association, updater, installers (MSI/NSIS, DMG, AppImage, deb, rpm), **and Android and iOS targets**, all from one Rust crate. Uses the OS webview, so installs are small. | Third-party crates. Its own config and conventions. Mobile plugins less mature than desktop. |
| wry (Tauri's webview layer) | Leaner, fewer opinions. | You write menus, dialogs, packaging and mobile shells yourself. |
| Hand-written shells per OS (build plan's Android Java + iOS Swift) | Minimal dependencies; Android stays Java-only. | Three or more separate shells and bridges to build and keep consistent. Repeats the "two UIs" problem at the shell layer. |
| Chromium `--app` launcher (current) | Already built, zero dependencies. | Not native (section 3). |
| Electron | Same Chromium everywhere. | Large installs, ships a whole browser, engine would run as a Node addon. No mobile. |

### 6.2 Recommendation

**Tauri 2 for desktop and mobile.** This is the main change from the 2026-11-03 build
plan, which used Tauri/wry on desktop but separate hand-written Java and Swift shells for
Android and iOS.

**Reasoning:**

- **One bridge, not three.** With hand-written shells, the bridge exists three times:
  Tauri IPC on desktop, `addJavascriptInterface`/JNI on Android, `WKScriptMessageHandler`
  on iOS. Each must implement open, save, share, display and exec identically. Every
  bridge feature is written and tested per platform. Tauri gives one Rust-side bridge for
  all targets.
- **Same principle you already chose for the UI.** You kept the JS UI to avoid maintaining
  parallel implementations. The same logic applies to shells.
- **Packaging and file association come with it.** Writing MSI, DMG, AppImage, intent
  filters and `Info.plist` document types by hand is real, unglamorous work.
- **The engine stays clean.** Tauri crates live only in `apps/shell`, in its own Cargo
  workspace (as `apps/viewport` was). Engine crates (`cad-model`, `cad-dwg`, `cad-native`,
  `cad-dxf`, `cad-core`, `cad-ffi`) stay std-only. Tauri and wry are MIT or Apache-2.0,
  compatible with the repository licence. Record them in `docs/PROVENANCE.md` and `NOTICE`.

**The trade-off you are accepting:** the Android app stops being "Java-only, no
libraries," which CI currently checks. That property was useful for a test viewer; it is
less valuable than one shared shell for a product. If you want to keep it, the
hand-written Android shell in the build plan still works with the same `Core` interface;
you just maintain that bridge separately.

**Fallback:** if Tauri's mobile support turns out to be missing something you need (a
file-picker behaviour, a share target), the `Core` interface means you can replace only
the Android or iOS shell with a hand-written one without touching the UI or engine.

### 6.3 Webview per OS (set by the OS)

| OS | Webview | Engine |
| --- | --- | --- |
| Windows | WebView2 | Chromium |
| macOS, iOS | WKWebView | WebKit |
| Linux | WebKitGTK | WebKit |
| Android | Android System WebView | Chromium |

The JS must work on both Chromium and WebKit. The File System Access API does not exist
in WebKit; the native bridge replaces it in every app, so the apps should never call it.

## 7. Why skip the sidecar stopgap

The build plan (section 5) proposed linking the engine into the existing launcher and
exposing it over the loopback server first.

**Reasoning against it:**

- Standing up a Tauri dev window with one command and one custom protocol handler is
  about as much work as adding a sidecar API to the launcher.
- The sidecar's transport (loopback HTTP with a token) is thrown away later. Tauri's IPC
  and protocol are the shipping transport.
- The sidecar still needs Chrome/Edge/Brave, so you cannot test the real WebKit and
  WebView2 behaviour with it.

Prove `NativeCore` in a Tauri dev build on your own machine instead. The proof is the
same: open a large DWG that WASM cannot open.

## 8. Risks

### 8.1 Linux webview performance

WebKitGTK is noticeably slower for heavy canvas and WebGL work than WebView2 or
WKWebView, and quality varies by distro. **Mitigation:** viewport-aware display lists
(5.3); test on Linux early, not last; build on the oldest supported distro for glibc
compatibility.

### 8.2 Size of the async refactor

The largest single cost. **Mitigation:** do it first, on the web, with tests for
latest-wins and edit ordering.

### 8.3 Engine round trip on every mouse move

**Mitigation:** latest-wins plus the JS snap cache.

### 8.4 Backend drift

Two backends (WASM and native) can diverge. **Mitigation:** one conformance test suite
run against both `WasmCore` and `NativeCore` in CI.

### 8.5 Chromium versus WebKit differences

**Mitigation:** CI smoke tests on both engines; no browser-only APIs in the app code path.

### 8.6 Tauri mobile maturity

Desktop Tauri is mature; mobile plugins (dialogs, file access, sharing) are younger.
**Mitigation:** check current Tauri 2 docs for the exact plugins you need before
committing; keep the hand-written shell as a fallback (6.2).

### 8.7 Signing lead time

Windows certificates and Apple developer accounts take days to weeks. Unsigned builds
trigger SmartScreen, Gatekeeper and Play Protect. **Mitigation:** start the paperwork
now, in parallel with the refactor.

## 9. Order of work

1. **Measure.** Count `core.` call sites in `apps/web/app.js`. Read how it renders
   (Canvas 2D or WebGL).
2. **`Core` interface and async `app.js`** against the WASM backend; move WASM into a
   Worker. Ships on the web. Tests for latest-wins and edit ordering.
3. **`apps/shell` with Tauri 2, desktop dev build.** One custom protocol, `NativeCore`
   in JS, native open/save dialogs. Prove it with a large DWG that WASM cannot open.
4. **Viewport-aware `display(id, view)`** in `cad-ffi` with culling and LOD, plus the JS
   change. Corpus tests with large DWGs. Test on Linux here.
5. **Desktop packaging,** unsigned first: MSI/NSIS, DMG (universal2), AppImage/deb/rpm.
   File association for `.cad`, `.dwg`, `.dxf`.
6. **Android** through Tauri 2's Android target: `cad-ffi` built for arm64-v8a and
   x86_64, Storage Access Framework for files. Sideload APK first.
7. **Signing** per OS; set the secrets; publish signed builds from CI.
8. **iOS** once an Apple developer account exists. Needs a macOS CI runner.
9. **Retire** `apps/desktop` (the Chromium launcher) and the Java test viewer in
   `apps/android` once their replacements ship.

## 10. Decisions needed

1. **Confirm the flip:** native engine is the default in the apps; WASM only on the web.
   (Replaces open decision 7.1 in `DESIGN-DECISIONS.md`.)
2. **Tauri 2 for mobile too,** accepting that Android is no longer Java-only.
3. **Skip the sidecar** and go straight to `apps/shell`.
4. **App identity:** name, icon, application ID (`com.withshalom.cad` is a placeholder
   and permanent once published on Google Play), iOS bundle ID, Windows publisher,
   WiX UpgradeCode.
5. **Signing:** Windows method (EV certificate or Azure Trusted Signing), Apple developer
   account, Android keystore (PR #24 secrets).
6. **Mobile storage model:** the picker gives a link to one file, not a folder (open
   decision 7.6).
7. **DWG corpus** for native-versus-WASM and large-drawing tests.

## 11. Not verified

- I have not read `app.js`, so the size of the async refactor and the renderer type are
  unknown.
- Tauri 2's current mobile plugin coverage, version numbers, platform minimums and store
  policies change. Check the vendor documentation before committing to versions or
  submission steps.
- DWGCraft's `cad-native` branch still writes `.cad`; it needs changing to `.cadcraft`
  and its pin on the BlackCAD crates updated.
