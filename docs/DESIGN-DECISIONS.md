# BlackCAD and DraftCraft: design decisions and plan

Written 2026-11-03. Status: **decided direction; removal done in the PR that adds this file.** Decisions marked
"Decided" come from the owner; "Open" ones need an answer before the work that
depends on them.

## 1. Names

| Name | What it is |
| --- | --- |
| BlackCAD | WithShalom's Rust engine with a JavaScript UI. Web build, wrapped for native. |
| DraftCraft | Proposed new name for the DWGCraft direction: BlackCAD's Rust engine with a modified CADCraft Rust (egui) interface. |
| DWGCraft | The owner's fork of ArtCraft's CADCraft. Develops toward DraftCraft. |
| CADCraft | ArtCraft's Rust app. |
| CadCraft | Data Design's JS suite. |
| SolidCraft | A separate project. |

File extensions (Decided): BlackCAD `.cad` (lock `.bcl`); the CADCraft fork `.cadcraft`.
Licence (Decided): MIT OR Apache-2.0 for the whole BlackCAD repository.

## 2. The decision

**Decided.** The two products take different UI routes, each using the engine.

- **BlackCAD: Rust engine, JavaScript UI.** The existing JS workbench (`apps/web`)
  is the one UI on every platform. Native builds (desktop, Android, iOS) are thin
  shells that load the same page. A feature is written once.
- **DraftCraft: Rust engine, Rust UI.** BlackCAD's engine under a modified
  CADCraft egui interface, developed in the DWGCraft repository.

### Why

- The JS UI is well developed, and building a UI in JS is faster and easier.
- A Rust UI for BlackCAD would be a second, independent UI to build, track and
  keep consistent with the web one. The desktop egui app that was built
  (PR #21, #23) already differs from the web UI in look, layout and features.
- With one UI, the web build is simpler and every native build inherits it,
  including iOS and APK.
- The Rust-UI approach is not wasted: it moves to DraftCraft, where it is the
  product.

## 3. What is erased from BlackCAD

The Rust UI is isolated, so removal is clean. Nothing else imports it (checked by
searching the repository).

| Remove | Where |
| --- | --- |
| The egui desktop app | `apps/viewport` |
| Its toolkit-free logic crate | `crates/cad-view`, its entry in the root `Cargo.toml` members, and the `exclude = ["apps/viewport"]` line |
| CI | the `viewport` job in `.github/workflows/build.yml`, and `viewport` in the `needs:` of `release` and `pages`, and the `blackcad-viewport-*` downloads in the release job |
| Packaging | the viewport lines in `.github/package-release.sh` and `.github/release-notes.md` |
| Docs | README download buttons and table rows for the viewport, the NOTICE entries for the egui viewport and its crates, the architecture sections describing it, `.gitignore` line |

History is kept in the branch `archive/egui-viewport` (the last commit with the
egui viewport; a tag could not be created from the working environment). One PR does the removal.

The parked branch `cad-view-triangulation-pick` (earcut fills, GPU mesh builder,
picking, corpus tests; 33 tests passing) is for DraftCraft. Keep it; do not merge
it into BlackCAD.

## 4. The shells

Every shell loads the same web build, with `core.wasm` as the engine.
(Open decision 7.1 covers using a native core instead.)

**Common contract.** A small native bridge: open file, save file, "open with",
share. `app.js` already uses `showOpenFilePicker` and `launchQueue`. Add a thin
adapter that uses the bridge when present and the browser APIs otherwise.

**Loading the WASM.** A page loaded from `file://` cannot fetch `core.wasm`, so
each shell serves the bundled files through a local request handler or custom
scheme.

| Platform | Shell | Notes |
| --- | --- | --- |
| Windows, macOS, Linux | A native window around the web build. Start with the existing std-only launcher (`desktop-webview-launcher` branch: serves the web build, opens an app window). Move to a wrapper (wry or Tauri, both MIT/Apache) for native dialogs, file association, offline use and installers. | Wrapper adds third-party crates, under the "borrow from unrestricted licences" policy. |
| Android | Replace the Java viewer (`apps/android`) with a WebView that loads the bundled build through `shouldInterceptRequest`, plus a small Java bridge for open and save through the system picker. | Keeps the Java-only, no-library property that CI checks. Signed sideload APK pipeline: PR #24 (needs the four `ANDROID_*` secrets). |
| iOS | A small Swift app with WKWebView, a custom scheme handler for the bundled build, and a document-picker bridge. | Needs an Apple developer account and a macOS CI runner. Cannot be tested in the Claude sandbox. |
| Web | Unchanged: published at cad.mosaiciron.com, installable as a PWA. | iPhone can use this today with no Apple account, within web file-access limits. |

## 5. Order of work

1. Archive branch `archive/egui-viewport` (done). Removal PR (section 3), including the
   README and architecture edits. This is the clean-slate PR.
2. Settle the desktop shell (open decision 7.2). Ship the launcher first if
   speed matters.
3. Build the file bridge adapter in `app.js`, then the Android WebView shell.
   Sideload first.
4. Merge PR #24 and set the four signing secrets so the release publishes a signed APK.
5. iOS shell once the Apple account exists.
6. Packaging per platform: MSI, DMG with notarisation, AppImage/deb/rpm for desktop;
   signed APK; TestFlight for iOS. Needs publisher name, app ID, WiX UpgradeCode,
   signing credentials.
7. Separately: DraftCraft in the DWGCraft repo (section 6).

## 6. DraftCraft (DWGCraft direction)

- Engine: BlackCAD's crates (`cad-core`, `cad-dwg`, `cad-native`, `cad-dxf`) as a
  dependency, replacing acadrust as the DWG reader and writer. This is the standing
  CADCraft project goal.
- UI: ArtCraft's CADCraft egui interface, modified. The GPU canvas (wgpu paint
  callback, shaders, fault handling, frame cap, Windows DX12 workaround) is the
  part worth keeping from CADCraft.
- Bring over from the parked BlackCAD branch: earcut triangulation, mesh builder,
  picking, corpus tests.
- Native extension for that fork's files: `.cadcraft`; fix DWGCraft's `cad-native`
  branch (still `.cad`) and update its pin on the BlackCAD crates.
- Branding: DraftCraft is a proposal. Confirm the name and check it is free before
  it appears in installers, bundle IDs or store listings.

## 7. Open decisions

1. **Native or WASM core in the shells.** Recommendation: WASM everywhere for now
   (one code path, identical behaviour). A native core over a bridge would be
   faster on very large drawings; the Android JNI code already exists. This reverses
   the earlier "BlackCAD should not depend on WASM" preference for the app, so it
   needs confirming.
2. **Desktop shell.** Launcher first, or go straight to wry/Tauri.
3. **App identity for the BlackCAD apps.** Name, icon, and application ID
   (`com.withshalom.cad` is a placeholder and cannot change after a Google Play
   publish); bundle ID for iOS.
4. **Signing.** Keystore secrets in repository secrets (Android); Apple developer
   account (iOS, notarisation); Windows signing method.
5. **DraftCraft naming and repository plan.**
6. **Storage on mobile.** The file picker gives a content link to one file, not a
   folder; saving needs a decision (see `docs/ARCHITECTURE.md`).

## 8. State on 2026-11-03

- `main` includes PR #23 (the egui app with menus, toolbar, command line, layers).
  The latest `main` build passed all jobs, and the `main-latest` release is published.
- PR #24 (signed release APK for sideloading): open, all jobs passed except two
  Windows jobs still running when last checked. Signing needs the four secrets.
- PR #19 (`release/0.18.0`): open.
- Branches kept: `cad-view-triangulation-pick` (for DraftCraft),
  `desktop-webview-launcher` (candidate desktop shell).
- Android today is a native Java test viewer (summary plus SVG of model space, view
  only); it does not look like or behave like the web UI. This plan replaces it.
