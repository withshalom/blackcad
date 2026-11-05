# BlackCAD brand: the logo and icon

The icon is BlackCAD's model space: black, with its light grey grid, and a
large handwritten B in white chalk (blackboard and blueprint, "Blackprint").
The logo sets the icon beside the wordmark as the welcome screen does:
"Black" in chalk, "CAD" in bold outlined letters.

## Two versions of the icon

| Version | File | Use it for |
|---|---|---|
| **With centerlines**: the centerline layer's yellow long-short dash crossing behind the B | `docs/images/blackcad-icon-centerlines.svg`, and inside `docs/images/blackcad-logo.svg` and `blackcad-logo-dark.svg` | Large things: the README, advertising, landing pages, banners, slides, print |
| **Plain**: the chalk B on the grid, no centerlines | `docs/images/blackcad-icon.svg` | Small things: the app icons (Windows, macOS, Linux, Android), the web app's icon and favicon, anywhere the icon is under about 64 px |

At small sizes the centerlines shrink to a thin cross that blurs the B, so
small things use the plain version.

## The logo

| File | Background |
|---|---|
| `docs/images/blackcad-logo.svg` | Light: the chalk is slate grey, CAD is white with a dark outline |
| `docs/images/blackcad-logo-dark.svg` | Dark: the chalk is white, CAD is black with a white outline |

Where the page can follow the viewer's theme (the README does, with
`<picture>`), offer both.

## Colours

| Colour | Hex | From |
|---|---|---|
| Model space | `#0d1013` | `--space` in `apps/web/style.css` |
| Grid | `#c9ced4`, faint | the workbench's grid |
| Chalk | `#eef0e8` | `--chalk`, the Blackboard's |
| Centerlines | `#ffe14d` | the centerline layer |
| Tile edge | `#2b323b` | the workbench's panel edges |

## Making them

`tools/brand/blackcad_logo.py` draws every file above and the apps' icons
from the same code; change the artwork there, never by hand. Its docstring
says how to run it. The letters are outlines from Patrick Hand and Inter
(SIL Open Font License 1.1; see `NOTICE` and `docs/PROVENANCE.md`). After
running it, rebuild the application's icons:

```sh
cd apps/shell && npm run tauri -- icon icons/source-1024.png -o icons
```
