Builds of `main` at [`{short}`](https://github.com/{repo}/commit/{sha}), made by CI on {date} after every check passed. This pre-release is replaced by each push to `main`; the numbered releases are the `vX.Y.Z` tags.

| File | What it is |
|---|---|
| `BlackCAD-windows-x64-setup.exe`<br>`BlackCAD-windows-x64.msi` | BlackCAD for Windows 10 and 11: the setup program, or the same app as an MSI for managed installs. It opens `.cad`, `.dwg` and `.dxf` files from Explorer. |
| `BlackCAD-macos-universal.dmg` | BlackCAD for macOS 10.15 and later, Apple silicon and Intel. Drag it to Applications. |
| `BlackCAD-linux-x86_64.AppImage`<br>`BlackCAD-linux-amd64.deb`<br>`BlackCAD-linux-x86_64.rpm` | BlackCAD for Linux: the AppImage runs on most distributions (`chmod +x`, then run it); the .deb is for Debian and Ubuntu, the .rpm for Fedora and openSUSE. Needs glibc 2.35 or later and WebKitGTK 4.1. |
| `BlackCAD-android.apk` | BlackCAD for Android 7 and later: the same app as on a computer. |
| `BlackCAD.html` | The whole workbench in one file (about 4 MB). Open it in a browser by double-clicking; it needs no server. It cannot be installed. |
| `blackcad-web.zip` | The installable web app (PWA). Unzip it and serve the folder over HTTPS, or `localhost`, to install it. |
| `blackcad-viewer-android-debug.apk` | CAD Viewer, the Android test app. |
| `blackcad-cli-linux-x86_64.tar.gz`<br>`blackcad-cli-macos-arm64.tar.gz`<br>`blackcad-cli-windows-x86_64.zip` | `cad`, the command-line test build: `cad info part.dwg`, `cad svg part.dwg part.svg`, and the rest of the README's "Use the test build". Linux needs glibc 2.35 or later; macOS is Apple silicon. |
| `SHA256SUMS.txt` | Checksums of the files above. |

- BlackCAD is one application on every system: the workbench runs in the system's own webview and the drafting engine runs natively inside it, with the computer's full memory. Drawings stay on the device.
- The Windows, macOS and Linux installers are not code-signed yet. Windows SmartScreen warns once (More info, then Run anyway). On macOS, open it the first time with right-click, Open; if macOS says it is damaged, run `xattr -cr /Applications/BlackCAD.app`.
- Unless the repository's signing secrets are set, each build's APKs are signed with a key made for that build, so a new one installs only after uninstalling the earlier one (`adb uninstall withshalom.blackcad`; the test viewer is `com.withshalom.cad`).
