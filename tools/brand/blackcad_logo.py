#!/usr/bin/env python3
"""Draw BlackCAD's icon and logo.

Original artwork made for BlackCAD (MIT OR Apache-2.0), in the workbench's
own look: a blackboard-black tile with a faint chalk grid (blackboard and
blueprint, "Blackprint"), and on it a shop part, a lifting lug, drafted in
the default layers' colours: VISIBLE white, centerlines yellow (long-short
dash), dimensions green. Beside it the wordmark as the welcome screen sets
it: "Black" in chalk handwriting, "CAD" in bold outlined drafting letters.

The letters are set in Patrick Hand (Black) and Inter ExtraBold (CAD), both
SIL Open Font License 1.1, and converted to outlines, so the logo needs no
font to show and no font file lives in this repository.

    python3 tools/brand/blackcad_logo.py FONTS_DIR

FONTS_DIR holds patrick-hand-latin-400-normal.woff2 and
inter-latin-800-normal.woff2 (npm: @fontsource/patrick-hand,
@fontsource/inter; their files/ folders). Needs fontTools and brotli
(pip install fonttools brotli). Writes:

    docs/images/blackcad-icon.svg       the icon, 1024 x 1024
    docs/images/blackcad-logo.svg       icon and wordmark, for light backgrounds
    docs/images/blackcad-logo-dark.svg  the same, for dark backgrounds
    apps/web/icons/icon.svg             the icon, for the web app and its PNGs

then, when rsvg-convert is installed, the web app's PNGs
(apps/web/icons/icon-192.png, icon-512.png, icon-maskable-512.png) and
apps/shell/icons/source-1024.png, from which `npm run tauri icon` in
apps/shell makes the application's icons.
"""

import math
import os
import shutil
import subprocess
import sys

from fontTools.pens.svgPathPen import SVGPathPen
from fontTools.pens.transformPen import TransformPen
from fontTools.ttLib import TTFont

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

# The workbench's colours (apps/web/style.css).
BOARD_TOP = "#151a1f"
BOARD_BOTTOM = "#07090b"
WHITE = "#ffffff"
CHALK = "#eef0e8"  # --chalk, the Blackboard's
YELLOW = "#ffe14d"  # centerlines
GREEN = "#3ddc84"  # dimensions (--ok)
INK = "#0d1013"  # --space, model space
SLATE = "#1d2329"  # chalk on a light page
GRID_GREY = "#c9ced4"  # the grid, light grey
EDGE = "#2b323b"  # the tile's edge, as the workbench's panels (--edge)
HAND = "patrick-hand-latin-400-normal.woff2"
BOLD = "inter-latin-800-normal.woff2"

S = 1024  # the icon's side
R = 228  # its corner radius (the system's rounded square at this size)


def num(v):
    s = f"{v:.1f}"
    return s[:-2] if s.endswith(".0") else s


GRID = 64  # the grid's spacing in the icon


def chalk_filter(fid, k):
    """Chalk: edges roughened, the stroke thinned in soft streaks where the
    chalk skipped, a few specks lifted out. `k` scales the texture to the
    size the letters are drawn at."""
    return f"""<filter id="{fid}" x="-5%" y="-5%" width="110%" height="110%">
      <feTurbulence type="fractalNoise" baseFrequency="{0.5 / k:.4f}" numOctaves="2" seed="7" result="n"/>
      <feDisplacementMap in="SourceGraphic" in2="n" scale="{2.4 * k:.2f}" xChannelSelector="R" yChannelSelector="G" result="d"/>
      <feTurbulence type="fractalNoise" baseFrequency="{0.9 / k:.4f} {0.16 / k:.4f}" numOctaves="3" seed="3" result="streak"/>
      <feColorMatrix in="streak" type="matrix" values="0 0 0 0 0  0 0 0 0 0  0 0 0 0 0  0 0 0 -2.6 2.35" result="skip"/>
      <feTurbulence type="fractalNoise" baseFrequency="{1.4 / k:.4f}" numOctaves="1" seed="11" result="g"/>
      <feColorMatrix in="g" type="matrix" values="0 0 0 0 0  0 0 0 0 0  0 0 0 0 0  0 0 0 -9 7.1" result="specks"/>
      <feComposite in="skip" in2="specks" operator="arithmetic" k1="1" result="mask"/>
      <feComposite in="d" in2="mask" operator="in"/>
    </filter>"""


def space(grid, maskable=False):
    """Model space: the workbench's black, with its grid as lines or dots."""
    shape = f'<rect width="{S}" height="{S}"/>' if maskable else f'<rect width="{S}" height="{S}" rx="{R}"/>'
    if grid == "dots":
        dots = "".join(f"M{x} {y}h0" for x in range(GRID // 2, S, GRID) for y in range(GRID // 2, S, GRID))
        marks = f'<path d="{dots}" stroke="{WHITE}" stroke-opacity="0.42" stroke-width="9" stroke-linecap="round"/>'
    else:
        minor = "".join(f"M{x} 0V{S}M0 {x}H{S}" for x in range(GRID // 2, S, GRID) if (x - GRID // 2) % (GRID * 4))
        major = "".join(f"M{x} 0V{S}M0 {x}H{S}" for x in range(GRID // 2, S, GRID * 4))
        marks = (
            f'<path d="{minor}" stroke="{GRID_GREY}" stroke-opacity="0.16" stroke-width="3" fill="none"/>'
            f'<path d="{major}" stroke="{GRID_GREY}" stroke-opacity="0.30" stroke-width="4" fill="none"/>'
        )
    return f"""<defs>
    <clipPath id="bc-tile">{shape}</clipPath>
    {chalk_filter("bc-chalk-b", 4.2)}
  </defs>
  <g clip-path="url(#bc-tile)">
    <rect width="{S}" height="{S}" fill="{INK}"/>
    {marks}
  </g>
  {"" if maskable else f'<rect x="4" y="4" width="{S - 8}" height="{S - 8}" rx="{R - 4}" fill="none" stroke="{EDGE}" stroke-width="8"/>'}"""


def big_b(fonts):
    """The handwritten B, centred on the tile by its own outline."""
    from fontTools.pens.boundsPen import BoundsPen

    font = TTFont(os.path.join(fonts, HAND))
    glyphs = font.getGlyphSet()
    g = glyphs[font.getBestCmap()[ord("B")]]
    bp = BoundsPen(glyphs)
    g.draw(bp)
    x0, y0, x1, y1 = bp.bounds
    k = 720 / (y1 - y0)  # the B stands 720 high on the 1024 tile
    tx = S / 2 - (x0 + x1) / 2 * k
    ty = S / 2 + (y0 + y1) / 2 * k
    pen = SVGPathPen(glyphs, num)
    g.draw(TransformPen(pen, (k, 0, 0, -k, tx, ty)))
    d = pen.getCommands()
    # The board shows through the chalk's streaks, but the centerlines pass
    # behind the letter, as a centerline passes behind a part: the B's
    # outline is knocked out of them first.
    return (
        f'<path d="{d}" fill="{INK}" stroke="{INK}" stroke-width="10" stroke-linejoin="round"/>'
        f'<path d="{d}" fill="{CHALK}" filter="url(#bc-chalk-b)"/>'
    )


def centerlines():
    """Centerlines through the tile's centre, behind the B: long-short dash,
    in the centerline layer's yellow, the short dash on the crossing as a
    CENTER linetype draws it."""
    lo, hi = 84, S - 84
    long, gap, short = 92, 22, 22
    period = long + gap + short + gap
    # Offset the pattern so a short dash sits on the centre.
    mid = long + gap + short / 2
    off = (mid - (S / 2 - lo)) % period
    return (
        f'<path d="M{S // 2} {lo}V{hi}M{lo} {S // 2}H{hi}" fill="none" stroke="{YELLOW}" stroke-width="18" '
        f'stroke-dasharray="{long} {gap} {short} {gap}" stroke-dashoffset="{num(-off)}"/>'
    )


def icon(fonts, grid, maskable=False):
    """The icon as an SVG document. A maskable one fills the square and
    keeps the B inside the safe circle launchers crop to."""
    b = centerlines() + "\n  " + big_b(fonts)
    if maskable:
        b = f'<g transform="translate({S * 0.1:.1f} {S * 0.1:.1f}) scale(0.8)">{b}</g>'
    return (
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {S} {S}" width="{S}" height="{S}" role="img" aria-label="BlackCAD">\n'
        f"  {space(grid, maskable)}\n  {b}\n</svg>\n"
    )


def word(path, text, x, baseline, cap_height, tracking=0.0):
    """`text` in the font at `path` as one SVG path: (d, advance)."""
    font = TTFont(path)
    glyphs = font.getGlyphSet()
    cmap = font.getBestCmap()
    cap = getattr(font["OS/2"], "sCapHeight", 0) or font["head"].unitsPerEm * 0.7
    k = cap_height / cap
    pen = SVGPathPen(glyphs, num)
    adv = 0.0
    for i, ch in enumerate(text):
        name = cmap[ord(ch)]
        glyphs[name].draw(TransformPen(pen, (k, 0, 0, -k, x + adv * k, baseline)))
        adv += glyphs[name].width + (tracking * cap if i < len(text) - 1 else 0)
    return pen.getCommands(), adv * k


ICON = 360  # the icon's side in the logo
GAP = 56  # icon to wordmark
CAP = 140  # cap height of CAD


def logo(fonts, dark, grid):
    hand = os.path.join(fonts, HAND)
    bold = os.path.join(fonts, BOLD)
    baseline = ICON / 2 + CAP / 2
    x = ICON + GAP
    # "Black" in chalk, a little taller than CAD, as on the welcome screen.
    d1, a1 = word(hand, "Black", x, baseline + 4, CAP * 1.16)
    x2 = x + a1 + CAP * 0.14
    d2, a2 = word(bold, "CAD", x2, baseline, CAP, tracking=0.06)
    stroke = CAP * 0.075
    width = x2 + a2 + stroke + 6
    chalk = CHALK if dark else SLATE
    fill, edge = (INK, WHITE) if dark else (WHITE, INK)
    k = ICON / S
    tile = icon(fonts, grid).split("\n", 1)[1].rsplit("</svg>", 1)[0]
    return f"""<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {width:.0f} {ICON}" width="{width:.0f}" height="{ICON}" role="img" aria-label="BlackCAD">
  <defs>
    {chalk_filter("bc-chalk", 1)}
  </defs>
  <g transform="scale({k:.6f})">{tile}</g>
  <path d="{d1}" fill="{chalk}" filter="url(#bc-chalk)"/>
  <path d="{d2}" fill="{fill}" stroke="{edge}" stroke-width="{stroke:.1f}" stroke-linejoin="round" paint-order="stroke fill"/>
</svg>
"""


def write(rel, text):
    path = os.path.join(ROOT, rel)
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w") as f:
        f.write(text)
    print("wrote", rel)


def png(svg_rel, png_rel, size):
    subprocess.run(
        ["rsvg-convert", "-w", str(size), "-h", str(size), "-o", os.path.join(ROOT, png_rel), os.path.join(ROOT, svg_rel)],
        check=True,
    )
    print("wrote", png_rel)


def main():
    if len(sys.argv) not in (2, 3):
        sys.exit(__doc__)
    fonts = sys.argv[1]
    grid = sys.argv[2] if len(sys.argv) == 3 else "lines"
    write("docs/images/blackcad-icon.svg", icon(fonts, grid))
    write("apps/web/icons/icon.svg", icon(fonts, grid))
    write("docs/images/blackcad-logo.svg", logo(fonts, dark=False, grid=grid))
    write("docs/images/blackcad-logo-dark.svg", logo(fonts, dark=True, grid=grid))
    if not shutil.which("rsvg-convert"):
        print("rsvg-convert not found: the PNGs were not made")
        return
    scratch = os.path.join(ROOT, "target", "brand")
    os.makedirs(scratch, exist_ok=True)
    with open(os.path.join(scratch, "maskable.svg"), "w") as f:
        f.write(icon(fonts, grid, maskable=True))
    png("apps/web/icons/icon.svg", "apps/web/icons/icon-192.png", 192)
    png("apps/web/icons/icon.svg", "apps/web/icons/icon-512.png", 512)
    png("target/brand/maskable.svg", "apps/web/icons/icon-maskable-512.png", 512)
    png("apps/web/icons/icon.svg", "apps/shell/icons/source-1024.png", 1024)


if __name__ == "__main__":
    main()
