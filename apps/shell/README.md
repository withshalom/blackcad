# BlackCAD, the application (`apps/shell`)

BlackCAD for Windows, macOS, Linux and Android. One program on each
system:

- the **workbench** (`apps/web`, built into `dist/web`) runs in the system's
  own webview: WebView2 on Windows, WKWebView on macOS, WebKitGTK on Linux,
  the Android System WebView on Android;
- the **engine** (`crates/cad-ffi`'s `Session` over `cad-core`, `cad-dwg`,
  `cad-dxf`, `cad-native`) is compiled natively into the same process, with
  the computer's memory and threads. `core.wasm` is not used here.

The page reaches the engine and the system's file dialogs through
`BLACKCAD_NATIVE`, a bridge the app puts on the page before its scripts run
(`src/bridge.js`). `apps/web/core.js` and `apps/web/host.js` describe the
contract; on the web the same page uses `core.wasm` instead.

## The files

| File | What it is |
|---|---|
| `src/lib.rs` | Starts the app: the window, the bridge, the plugins, files opened with the app. |
| `src/engine.rs` | The engine's calls (open, new, info, exec, display list, save), one at a time. A panic is caught and reported as a crash; the workbench restarts the engine and reopens its drawings. |
| `src/files.rs` | Open and save dialogs, reading and writing in place (Android content links too), PDFs to the system viewer, launch files. |
| `src/bridge.js` | `BLACKCAD_NATIVE` for the page. Bytes cross as raw bytes, not JSON. |
| `tauri.conf.json` | Name, identifier (`withshalom.blackcad`), icons, file associations (`.cad`, `.dwg`, `.dxf`), installers. |
| `capabilities/default.json` | What the page may ask of Tauri itself: its own window, nothing more. |
| `icons/` | Rendered from `apps/web/icons/icon-512.png` with `tauri icon`. |

## Dependencies

This folder is a Cargo workspace of its own, excluded from the root one,
and the only place in the repository with third-party crates: Tauri 2 and
its dialog, fs, opener and single-instance plugins (MIT OR Apache-2.0).
The engine crates stay std-only. See `docs/PROVENANCE.md` and `NOTICE`.

## Build

Needs Rust, Node (for the Tauri command line, run through `npx`) and, on
Linux, WebKitGTK 4.1 (`libwebkit2gtk-4.1-dev` and the packages in
`.github/workflows/build.yml`, job `app`).

```sh
cargo xtask web                       # the workbench, into dist/web
cd apps/shell
cargo run                             # a development build, in a window
npx @tauri-apps/cli@2 build           # the release build and this system's installers
```

Installers land in `apps/shell/target/release/bundle/`: `.msi` and
setup `.exe` on Windows, `.dmg` and `.app` on macOS
(`--target universal-apple-darwin` for both processors), `.AppImage`,
`.deb` and `.rpm` on Linux.

Android, with the Android SDK and NDK installed (`NDK_HOME` set):

```sh
npx @tauri-apps/cli@2 android init    # generates gen/android from tauri.conf.json
cp -R icons/android/. gen/android/app/src/main/res/
npx @tauri-apps/cli@2 android build --apk --target aarch64 --target armv7 --target x86_64
```

CI (`app` and `app-android` jobs) does all of this on every push and the
`main-latest` release offers the results.

## Signing

Nothing is code-signed yet, so SmartScreen and Gatekeeper warn the first
time. What each system needs:

- **Android**: the upload keystore as the repository secrets
  `ANDROID_KEYSTORE_BASE64`, `ANDROID_KEYSTORE_PASSWORD`, `ANDROID_KEY_ALIAS`,
  `ANDROID_KEY_PASSWORD`. CI signs with it when they are set; until then
  each build gets a key of its own, so a new APK installs only after the
  old one is uninstalled.
- **Windows**: an Authenticode certificate or Azure Trusted Signing, then
  `bundle.windows` signing settings in `tauri.conf.json`.
- **macOS**: an Apple Developer account, a Developer ID certificate and
  notarisation (Tauri's `APPLE_*` environment variables in CI).

## Identity

These are permanent once published; change them only before the first
store release:

- identifier `withshalom.blackcad` (Android application ID, macOS bundle ID);
- Windows MSI upgrade code `77e3cb85-a937-4e35-877e-ea6992de2aef`;
- publisher `WithShalom`.

## Testing

`cargo test` here checks the shell's own code. The whole app was checked end
to end on Linux with `tauri-driver` (WebDriver) under a virtual display:
the bridge present and `core.wasm` not loaded, a DWG given on the command
line opened, lines typed at the command line, Ctrl+S writing the DWG back in
place and reopening with the new lines, one Undo per command, no
"browser" anywhere in the interface.
