Builds of `main` at [`{short}`](https://github.com/{repo}/commit/{sha}), made by CI on {date} after every check passed. This pre-release is replaced by each push to `main`; the numbered releases are the `vX.Y.Z` tags.

| File | What it is |
|---|---|
| `blackcad-desktop-windows-x86_64.zip`<br>`blackcad-desktop-macos-arm64.tar.gz`<br>`blackcad-desktop-linux-x86_64.tar.gz` | The desktop app: the web workbench in its own window. Unpack it and run `blackcad`, or `blackcad drawing.dwg`. It opens in an app window of Chrome, Edge, Brave or Chromium, so one of them must be installed. Drawings stay on the computer. |
| `BlackCAD.html` | The whole workbench in one file (about 4 MB). Open it in a browser by double-clicking; it needs no server. It cannot be installed. |
| `blackcad-web.zip` | The installable web app (PWA). Unzip it and serve the folder over HTTPS, or `localhost`, to install it. |
| `blackcad-viewer-android-debug.apk` | CAD Viewer, the Android test app. |
| `blackcad-cli-linux-x86_64.tar.gz`<br>`blackcad-cli-macos-arm64.tar.gz`<br>`blackcad-cli-windows-x86_64.zip` | `cad`, the command-line test build: `cad info part.dwg`, `cad svg part.dwg part.svg`, and the rest of the README's "Use the test build". Linux needs glibc 2.35 or later; macOS is Apple silicon. |
| `SHA256SUMS.txt` | Checksums of the files above. |

- The APK is signed with a debug key that is new in every build. To install one over an earlier build, uninstall that first: `adb uninstall com.withshalom.cad`.
- Nothing here is code-signed. If your system warns about a downloaded file, allow it in the system's security settings.
