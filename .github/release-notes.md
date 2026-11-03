Builds of `main` at [`{short}`](https://github.com/{repo}/commit/{sha}), made by CI on {date} after every check passed. This pre-release is replaced by each push to `main`; the numbered releases are the `vX.Y.Z` tags.

| File | What it is |
|---|---|
| `blackcad-viewport-windows-x86_64.zip`<br>`blackcad-viewport-macos-arm64.tar.gz`<br>`blackcad-viewport-linux-x86_64.tar.gz` | The desktop app: unpack it and run `blackcad`, or `blackcad drawing.dwg`. Menu bar, toolbar, command line (type `help`) and layers panel. Opens DWG, DXF and `.cad` (File menu, Ctrl+O, or drop a file), draws lines, circles, rectangles, polylines and arcs by clicking or typing points, selects and erases, undo and redo, saves as DXF, DWG or `.cad`. Wheel zooms, drag pans. |
| `BlackCAD.html` | The whole workbench in one file (about 4 MB). Open it in a browser by double-clicking; it needs no server. It cannot be installed. |
| `blackcad-web.zip` | The installable web app (PWA). Unzip it and serve the folder over HTTPS, or `localhost`, to install it. |
| `blackcad-viewer-android-debug.apk` | CAD Viewer, the Android test app. |
| `blackcad-cli-linux-x86_64.tar.gz`<br>`blackcad-cli-macos-arm64.tar.gz`<br>`blackcad-cli-windows-x86_64.zip` | `cad`, the command-line test build: `cad info part.dwg`, `cad svg part.dwg part.svg`, and the rest of the README's "Use the test build". Linux needs glibc 2.35 or later; macOS is Apple silicon. |
| `SHA256SUMS.txt` | Checksums of the files above. |

- The APK is signed with a debug key that is new in every build. To install one over an earlier build, uninstall that first: `adb uninstall com.withshalom.cad`.
- Nothing here is code-signed. If your system warns about a downloaded file, allow it in the system's security settings.
- The desktop app draws and edits the basics; the web app has the full command set.
